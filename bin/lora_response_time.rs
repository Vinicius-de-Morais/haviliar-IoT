#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::{fmt::Write, mem::MaybeUninit};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::{raw::CriticalSectionRawMutex}, channel::{Channel}, mutex::Mutex as AsyncMutex};
use embassy_time::{Duration, Instant, Timer, WithTimeout};
use esp_backtrace as _;
use esp_println::logger::init_logger;
use haviliar_iot::{
    controller::lora::LoraController, factory::{display_factory::DisplayFactory, lora_factory::LoraFactory}, hal::{lora::{OutgoingMessage, PAYLOAD_LENGTH}, peripheral_manager::PeripheralManagerStatic}, protocol::{lora::LoraEnvelope, message_type::MessageType}
};
use log::*;
use esp_hal::{clock::CpuClock, rng::Rng};
use minicbor::decode::info;
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

const HEAP_SIZE: usize = 64 * 1024; // 64KB heap
static mut HEAP: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];

const FLOOR_RX_TIMEOUT_MS: u64 = 5000;
const CEIL_RX_TIMEOUT_MS: u64 = 6000;

static LORA: StaticCell<AsyncMutex<CriticalSectionRawMutex, LoraController>> = StaticCell::new();
static RNG: StaticCell<AsyncMutex<CriticalSectionRawMutex, Rng>> = StaticCell::new();
//static DISPLAY: StaticCell<AsyncMutex<CriticalSectionRawMutex, Display<'static>>> = StaticCell::new();

type LoRaChannel = Channel<CriticalSectionRawMutex, LoraEnvelope<'static>, 1>;
type SentAckChannel = Channel<CriticalSectionRawMutex, (), 1>;
static LORA_CHANNEL: StaticCell<LoRaChannel> = StaticCell::new();
static SENT_ACK_CHANNEL: StaticCell<SentAckChannel> = StaticCell::new();

static PACKAGE_LOST_COUNTER: StaticCell<AsyncMutex<CriticalSectionRawMutex, u32>> = StaticCell::new();
static CURRENT_RSSI: StaticCell<AsyncMutex<CriticalSectionRawMutex, u16>> = StaticCell::new();
static ELAPSED_TIME: StaticCell<AsyncMutex<CriticalSectionRawMutex, u32>> = StaticCell::new();

#[embassy_executor::task]
async fn task_send(
    channel: &'static LoRaChannel,
    sent_ack_channel: &'static SentAckChannel,
    lora: &'static AsyncMutex<CriticalSectionRawMutex, LoraController>
    ) {
    
    let receiver = channel.receiver();
    let ack_sender = sent_ack_channel.sender();
        
    loop {
        let message = receiver.receive().await;

        let mut lora_ref = lora.lock().await;

        let res = lora_ref.send_message_envelope(&message).await;

        match res {
            Ok(_) => {
                //info!("LoRa reply sent successfully");
                ack_sender.send(()).await;
            }
            Err(e) => error!("Failed to send LoRa reply: {:?}", e),
        }

        drop(lora_ref);
    }
}

#[embassy_executor::task]
async fn task_receive(
    tx_channel: &'static LoRaChannel,
    sent_ack_channel: &'static SentAckChannel,
    lora: &'static AsyncMutex<CriticalSectionRawMutex, LoraController>,
    rng: &'static AsyncMutex<CriticalSectionRawMutex, Rng>,
    package_lost_counter: &'static AsyncMutex<CriticalSectionRawMutex, u32>,
    current_rssi: &'static AsyncMutex<CriticalSectionRawMutex, u16>,
    elapsed_time: &'static AsyncMutex<CriticalSectionRawMutex, u32>,
    ) {
    let tx_sender = tx_channel.sender();
    let ack_receiver = sent_ack_channel.receiver();
    let mut last_response_at: Option<Instant> = None;
    let mut last_sent_seq: Option<u16> = None;
    let mut lost_packets: u32 = 0;
    let mut response_samples: u32 = 0;
    let mut total_response_ms: u64 = 0;
    
    let random_timeout_ms = {
        let mut rng_guard = rng.lock().await;
        let span_ms = CEIL_RX_TIMEOUT_MS.saturating_sub(FLOOR_RX_TIMEOUT_MS).saturating_add(1);
        let rand_ms = if span_ms > 0 {
            (rng_guard.random() as u64) % span_ms
        } else {
            0
        };
        FLOOR_RX_TIMEOUT_MS.saturating_add(rand_ms)
    };
    
    loop {
        let loop_started_at = Instant::now();
        let mut recv_buffer = [0u8; PAYLOAD_LENGTH];
        let receive_started_at = Instant::now();

        let mut lora_ref = lora.lock().await;

        let result = lora_ref.receive_message(&mut recv_buffer)
            .with_timeout(Duration::from_millis(random_timeout_ms))
            .await;

        drop(lora_ref);

        match result {
            Ok(Ok((envelope, status))) => {
                let receive_wait_ms = (Instant::now() - receive_started_at).as_millis();

                let now = Instant::now();
                let had_prev = last_response_at.is_some();
                let elapsed_ms = if let Some(last) = last_response_at {
                    (now - last).as_millis()
                } else {
                    0
                };
                let timestamp_ms = core::cmp::min(now.as_millis(), u32::MAX as u64) as u32;

                if had_prev {
                    total_response_ms = total_response_ms.saturating_add(elapsed_ms);
                    response_samples = response_samples.saturating_add(1);
                }

                let mut rssi_guard = current_rssi.lock().await;
                *rssi_guard = status.rssi as u16;

                let now = Instant::now();
                let elapsed_ms = if let Some(last) = last_response_at {
                    (now - last).as_millis()
                } else {
                    0
                };
                let mut elapsed_time_guard = elapsed_time.lock().await;
                *elapsed_time_guard = elapsed_ms as u32;

                let lora_envelope = LoraEnvelope::new(
                    MessageType::Reply, 
                    0, 
                    timestamp_ms, 
                    elapsed_ms as u32, 
                    "".as_bytes().into()
                );

                tx_sender.send(lora_envelope).await;

                ack_receiver.receive().await;

                last_response_at = Some(Instant::now());
                last_sent_seq = Some(envelope.seq);
            }
            Ok(Err(e)) => {
                error!("Failed to receive LoRa message: {:?}", e);
                Timer::after_millis(100).await;
            }
            Err(_) => {                
                if let (Some(last_at), Some(seq)) = (last_response_at, last_sent_seq) {

                    let now = Instant::now();
                    let elapsed_ms = (now - last_at).as_millis();
                    let timestamp_ms = core::cmp::min(now.as_millis(), u32::MAX as u64) as u32;

                    lost_packets = lost_packets.saturating_add(1);
                    let mut counter_guard = package_lost_counter.lock().await;
                    *counter_guard = counter_guard.saturating_add(1);

                   let lora_envelope = LoraEnvelope::new(
                        MessageType::Reply, 
                        seq, 
                        timestamp_ms, 
                        elapsed_ms as u32, 
                        "".as_bytes().into()
                    );

                    tx_sender.send(lora_envelope).await;

                    ack_receiver.receive().await;
                }
            }
        }

        let _avg_ms = if response_samples > 0 {
            total_response_ms / response_samples as u64
        } else {
            0
        };

        let _ = lost_packets;
    }
}

#[esp_hal_embassy::main]
async fn main(_spawner: Spawner) {
    // Initialize heap first before any logging
    unsafe {
        esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
            HEAP.as_mut_ptr() as *mut u8,
            HEAP_SIZE,
            esp_alloc::MemoryCapability::Internal.into(),
        ));
    }
    
    init_logger(log::LevelFilter::Info);
    
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let peripheral_manager = PeripheralManagerStatic::init(peripherals);

    let time_per =  peripheral_manager.time_per();
    esp_hal_embassy::init(time_per.timer0);

    // Create display
    let display_peripherals = peripheral_manager.take_display_peripherals().unwrap();
    let mut display = match DisplayFactory::create_from_peripherals(display_peripherals) {
        Ok(display) => display,
        Err(e) => {
            error!("Failed to create display: {}", e);
            panic!("Display initialization failed");
        }
    };

    // Create LoRa
    let lora_peripherals = peripheral_manager.take_lora_peripherals().unwrap();

    //  Setup ESP32
    let lora = match LoraFactory::create_from_manager(lora_peripherals).await {
        Ok(lora) => lora,
        Err(e) => {
            error!("Failed to initialize LoRa: {:?}", e);
            panic!("LoRa initialization failed");
        }
    };
    let lora_controller = LoraController::new(lora);
    
    let channel = LORA_CHANNEL.init(Channel::new());
    let sent_ack_channel = SENT_ACK_CHANNEL.init(Channel::new());
    let lora = LORA.init(AsyncMutex::new(lora_controller));

    let wifi_peripherals = peripheral_manager.take_wifi_peripherals().unwrap();
    let rng = RNG.init(AsyncMutex::new(Rng::new(wifi_peripherals.rng)));

    let package_lost_counter = PACKAGE_LOST_COUNTER.init(AsyncMutex::new(0));
    let current_rssi = CURRENT_RSSI.init(AsyncMutex::new(0));
    let elapsed_time = ELAPSED_TIME.init(AsyncMutex::new(0));

    //info!("Both display and LoRa initialized successfully!");

    if let Err(e) = display.show_message("LoRa + Display OK!") {
        error!("Failed to show initial message: {:?}", e);
    }
        
    let _ = _spawner.spawn(task_send(channel, sent_ack_channel, lora));
    let _ = _spawner.spawn(task_receive(channel, sent_ack_channel, lora, rng, package_lost_counter, current_rssi, elapsed_time));
    
    // Main loop
    let mut _counter = 0u32;
    loop {
        if let Err(e) = display.clear() {
            error!("Failed to clear display: {:?}", e);
            continue;
        }

        // // Static text
        // if let Err(e) = display.text_new_line("LoRa + Display OK!", 1) {
        //     error!("Failed to write text: {:?}", e);
        // }
        
        // if let Err(e) = display.text_new_line("Contador:", 2) {
        //     error!("Failed to write text: {:?}", e);
        // }

        // Counter
        // let mut counter_str = heapless::String::<10>::new();
        // write!(&mut counter_str, "{}", counter).unwrap();
        
        // if let Err(e) = display.text_new_line(&counter_str, 3) {
        //     error!("Failed to write counter: {:?}", e);
        // }

        display.text_new_line("Last Response at: ", 1).ok();
        let guard = elapsed_time.lock().await;
        let elapsed_time = *guard;
        drop(guard);
        let mut elapsed_time_str = heapless::String::<10>::new();
        write!(&mut elapsed_time_str, "{}", elapsed_time).unwrap();
        display.text_new_line(&elapsed_time_str, 2).ok();

        
        display.text_new_line("Package Lost Counter: ", 4).ok();

        // Package lost counter        let lost_count = {
        let guard = package_lost_counter.lock().await;
        let lost_count = *guard;
        drop(guard);

        let mut lost_str = heapless::String::<10>::new();
        write!(&mut lost_str, "{}", lost_count).unwrap();
        
        if let Err(e) = display.text_new_line(&lost_str, 5) {
            error!("Failed to write counter: {:?}", e);
        }

        // Current RSSI
        let rssi_value = {
            let guard = current_rssi.lock().await;
            let rssi = *guard as i16; // Convert back to signed
            drop(guard);
            rssi
        };

        let mut rssi_str = heapless::String::<16>::new();
        if write!(&mut rssi_str, "RSSI: {}", rssi_value).is_err() {
            rssi_str.clear();
            let _ = rssi_str.push_str("RSSI: ERR");
        }
        
        display.text_new_line(&rssi_str, 6).ok();


        if let Err(e) = display.flush() {
            error!("Failed to flush display: {:?}", e);
        }
        
        _counter += 1;

        Timer::after_millis(1000).await;
    }
}