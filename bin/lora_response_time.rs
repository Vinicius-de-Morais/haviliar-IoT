#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::{fmt::Write, mem::MaybeUninit};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::{raw::CriticalSectionRawMutex}, channel::{Channel}, mutex::Mutex as AsyncMutex};
use embassy_time::{Instant, Timer};
use esp_backtrace as _;
use esp_println::logger::init_logger;
use haviliar_iot::{
    factory::{display_factory::DisplayFactory, lora_factory::LoraFactory},
    hal::{lora::{Lora, PAYLOAD_LENGTH}, peripheral_manager::PeripheralManagerStatic},
};
use log::*;
use esp_hal::clock::CpuClock;
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

const HEAP_SIZE: usize = 64 * 1024; // 64KB heap
static mut HEAP: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];

static LORA: StaticCell<AsyncMutex<CriticalSectionRawMutex, Lora<'static>>> = StaticCell::new();
//static DISPLAY: StaticCell<AsyncMutex<CriticalSectionRawMutex, Display<'static>>> = StaticCell::new();
struct OutgoingMessage {
    payload: [u8; PAYLOAD_LENGTH],
    len: usize,
}

type LoRaChannel = Channel<CriticalSectionRawMutex, OutgoingMessage, 32>;
static LORA_CHANNEL: StaticCell<LoRaChannel> = StaticCell::new();

fn build_reply_message(elapsed_ms: u64) -> OutgoingMessage {
    let mut payload = [0u8; PAYLOAD_LENGTH];
    let mut msg = heapless::String::<96>::new();

    let _ = write!(
        &mut msg,
        "time elapse since last response: {} ms",
        elapsed_ms
    );

    let bytes = msg.as_bytes();
    let len = core::cmp::min(bytes.len(), PAYLOAD_LENGTH);
    payload[..len].copy_from_slice(&bytes[..len]);

    OutgoingMessage { payload, len }
}

#[embassy_executor::task]
async fn task_send(
    channel: &'static LoRaChannel, 
    lora: &'static AsyncMutex<CriticalSectionRawMutex, Lora<'static>>
    ) {
    
    let receiver = channel.receiver();
        
    loop {
        let mut message = receiver.receive().await;
        let payload = &mut message.payload[..message.len];

        match Lora::send_from_mutex(lora, payload).await {
            Ok(_) => info!("LoRa reply sent successfully"),
            Err(e) => error!("Failed to send LoRa reply: {:?}", e),
        }
    }
}

#[embassy_executor::task]
async fn task_receive(
    tx_channel: &'static LoRaChannel,
    lora: &'static AsyncMutex<CriticalSectionRawMutex, Lora<'static>>
    ) {
    let tx_sender = tx_channel.sender();
    let mut last_response_at: Option<Instant> = None;
    
    loop{
        info!("Waiting for LoRa response...");

        let mut recv_buffer = [0u8; PAYLOAD_LENGTH];

        let result = Lora::receive_from_mutex(lora, &mut recv_buffer).await;

        match result {
            Ok((len, status)) => {
                let len_usize = len as usize;
                if len_usize > 0 {
                    info!("Received LoRa message of length {}: {:?}", len, &recv_buffer[..len_usize]);
                }
                info!("LoRa packet RSSI: {:?}", status.rssi);

                let now = Instant::now();
                let elapsed_ms = if let Some(last) = last_response_at {
                    (now - last).as_millis()
                } else {
                    0
                };

                let reply = build_reply_message(elapsed_ms);
                info!("Built reply message: {:?}", &reply.payload[..reply.len]);
                tx_sender.send(reply).await;
                last_response_at = Some(now);
            }
            Err(e) => {
                error!("Failed to receive LoRa message: {:?}", e);
                Timer::after_millis(100).await;
            }
        }
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

    let channel = LORA_CHANNEL.init(Channel::new());
    let lora = LORA.init(AsyncMutex::new(lora));

    info!("Both display and LoRa initialized successfully!");

    if let Err(e) = display.show_message("LoRa + Display OK!") {
        error!("Failed to show initial message: {:?}", e);
    }
    
    let _ = _spawner.spawn(task_send(channel, lora));
    let _ = _spawner.spawn(task_receive(channel, lora));
    
    // Main loop
    let mut counter = 0u32;
    loop {
        if let Err(e) = display.clear() {
            error!("Failed to clear display: {:?}", e);
            continue;
        }

        // Static text
        if let Err(e) = display.text_new_line("LoRa + Display OK!", 1) {
            error!("Failed to write text: {:?}", e);
        }
        
        if let Err(e) = display.text_new_line("Contador:", 2) {
            error!("Failed to write text: {:?}", e);
        }

        // Counter
        let mut counter_str = heapless::String::<10>::new();
        write!(&mut counter_str, "{}", counter).unwrap();
        
        if let Err(e) = display.text_new_line(&counter_str, 3) {
            error!("Failed to write counter: {:?}", e);
        }
        
        if let Err(e) = display.flush() {
            error!("Failed to flush display: {:?}", e);
        }

        
        counter += 1;

        Timer::after_millis(1000).await;
    }
}