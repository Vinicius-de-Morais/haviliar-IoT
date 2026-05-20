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
    controller::lora::LoraController,
    factory::{display_factory::DisplayFactory, lora_factory::LoraFactory},
    hal::{lora::PAYLOAD_LENGTH, peripheral_manager::PeripheralManagerStatic},
    protocol::{lora::LoraEnvelope, message_type::MessageType},
};
use log::*;
use esp_hal::{clock::CpuClock, gpio::{Input, InputConfig}, rng::Rng};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

const HEAP_SIZE: usize = 64 * 1024;
static mut HEAP: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];

const SYNC_INTERVAL_MS: u64 = 2000;
const RX_TIMEOUT_MS: u64 = 5000;
const METRICS_INTERVAL_MS: u64 = 10000;

static LORA: StaticCell<AsyncMutex<CriticalSectionRawMutex, LoraController>> = StaticCell::new();
static RNG: StaticCell<AsyncMutex<CriticalSectionRawMutex, Rng>> = StaticCell::new();

type SyncChannel = Channel<CriticalSectionRawMutex, LoraEnvelope, 8>;
static SYNC_CHANNEL: StaticCell<SyncChannel> = StaticCell::new();

static METRICS: StaticCell<AsyncMutex<CriticalSectionRawMutex, LoraMetrics>> = StaticCell::new();

#[derive(Debug, Clone, Copy)]
pub struct LoraMetrics {
    pub packets_sent: u32,
    pub packets_received: u32,
    pub packets_lost: u32,
    pub sync_count: u32,
    pub last_rssi: i16,
    pub avg_rssi: i32,
    pub peer_device_id: u32,
    pub session_start_ms: u32,
}

impl Default for LoraMetrics {
    fn default() -> Self {
        Self {
            packets_sent: 0,
            packets_received: 0,
            packets_lost: 0,
            sync_count: 0,
            last_rssi: 0,
            avg_rssi: 0,
            peer_device_id: 0,
            session_start_ms: 0,
        }
    }
}

#[embassy_executor::task]
async fn task_send(
    sync_channel: &'static SyncChannel,
    lora: &'static AsyncMutex<CriticalSectionRawMutex, LoraController>,
    metrics: &'static AsyncMutex<CriticalSectionRawMutex, LoraMetrics>,
) {
    let receiver = sync_channel.receiver();

    loop {
        let message = receiver.receive().await;

        let mut lora_ref = lora.lock().await;
        let res = lora_ref.send_message_envelope(&message).await;

        let mut metrics_ref = metrics.lock().await;
        if res.is_ok() {
            metrics_ref.packets_sent = metrics_ref.packets_sent.saturating_add(1);
            info!("Sent: seq={}, type={:?}", message.seq, message.msg_type);
        } else {
            error!("Send failed: {:?}", res.err());
        }
        drop(metrics_ref);

        drop(lora_ref);
    }
}

#[embassy_executor::task]
async fn task_receive(
    sync_channel: &'static SyncChannel,
    lora: &'static AsyncMutex<CriticalSectionRawMutex, LoraController>,
    metrics: &'static AsyncMutex<CriticalSectionRawMutex, LoraMetrics>,
) {
    let sync_sender = sync_channel.sender();
    let mut rssi_samples: u32 = 0;
    let mut total_rssi: i32 = 0;

    loop {
        let mut recv_buffer = [0u8; PAYLOAD_LENGTH];

        let mut lora_ref = lora.lock().await;
        let result = lora_ref.receive_message(&mut recv_buffer)
            .with_timeout(Duration::from_millis(RX_TIMEOUT_MS))
            .await;
        drop(lora_ref);

        match result {
            Ok(Ok((envelope, status))) => {
                let rssi = status.rssi;
                total_rssi = total_rssi.saturating_add(rssi as i32);
                rssi_samples = rssi_samples.saturating_add(1);

                let mut metrics_ref = metrics.lock().await;
                metrics_ref.packets_received = metrics_ref.packets_received.saturating_add(1);
                metrics_ref.last_rssi = rssi;
                metrics_ref.avg_rssi = if rssi_samples > 0 { total_rssi / rssi_samples as i32 } else { 0 };
                drop(metrics_ref);

                info!("Received: seq={}, type={:?}, rssi={}", envelope.seq, envelope.msg_type, rssi);
            }
            Ok(Err(e)) => {
                error!("Receive error: {:?}", e);
            }
            Err(_) => {}
        }

        Timer::after_millis(10).await;
    }
}

#[embassy_executor::task]
async fn task_sync(
    sync_channel: &'static SyncChannel,
    rng: &'static AsyncMutex<CriticalSectionRawMutex, Rng>,
    metrics: &'static AsyncMutex<CriticalSectionRawMutex, LoraMetrics>,
) {
    let sender = sync_channel.sender();
    let mut seq: u16 = 0;

    loop {
        Timer::after_millis(SYNC_INTERVAL_MS).await;

        let now = Instant::now();
        let timestamp_ms = core::cmp::min(now.as_millis(), u32::MAX as u64) as u32;
        let request_id = {
            let mut rng_guard = rng.lock().await;
            rng_guard.random()
        };

        let envelope = LoraEnvelope::new(
            MessageType::ContinuousPackage,
            seq,
            request_id,
            timestamp_ms,
            0,
            "sync".as_bytes().to_vec(),
        );

        let _ = sender.send(envelope).await;

        seq = seq.wrapping_add(1);

        let mut metrics_ref = metrics.lock().await;
        metrics_ref.sync_count = metrics_ref.sync_count.saturating_add(1);
        drop(metrics_ref);
    }
}

#[embassy_executor::task]
async fn task_metrics_report(
    sync_channel: &'static SyncChannel,
    metrics: &'static AsyncMutex<CriticalSectionRawMutex, LoraMetrics>,
) {
    let sender = sync_channel.sender();

    loop {
        Timer::after_millis(METRICS_INTERVAL_MS).await;

        let (packets_sent, packets_received, packets_lost, sync_count) = {
            let metrics_ref = metrics.lock().await;
            (
                metrics_ref.packets_sent,
                metrics_ref.packets_received,
                metrics_ref.packets_lost,
                metrics_ref.sync_count,
            )
        };

        let now = Instant::now();
        let timestamp_ms = core::cmp::min(now.as_millis(), u32::MAX as u64) as u32;

        let mut payload = heapless::String::<32>::new();
        write!(&mut payload, "M|s:{}|r:{}|l:{}|c:{}",
            packets_sent, packets_received, packets_lost, sync_count).ok();

        let envelope = LoraEnvelope::new(
            MessageType::Metrics,
            0,
            0,
            timestamp_ms,
            0,
            payload.into_bytes().to_vec(),
        );

        let _ = sender.send(envelope).await;
    }
}

#[esp_hal_embassy::main]
async fn main(_spawner: Spawner) {
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

    let time_per = peripheral_manager.time_per();
    esp_hal_embassy::init(time_per.timer0);

    let display_peripherals = peripheral_manager.take_display_peripherals().unwrap();
    let mut display = match DisplayFactory::create_from_peripherals(display_peripherals) {
        Ok(display) => display,
        Err(e) => {
            error!("Failed to create display: {}", e);
            panic!("Display initialization failed");
        }
    };

    let button_peripherals = peripheral_manager.take_button_peripherals().unwrap();
    let prg_button = Input::new(button_peripherals.prg, InputConfig::default());

    let lora_peripherals = peripheral_manager.take_lora_peripherals().unwrap();
    let lora = match LoraFactory::create_from_manager(lora_peripherals).await {
        Ok(lora) => lora,
        Err(e) => {
            error!("Failed to initialize LoRa: {:?}", e);
            panic!("LoRa initialization failed");
        }
    };

    let wifi_peripherals = peripheral_manager.take_wifi_peripherals().unwrap();

    let lora_controller = LoraController::new(lora);
    let channel = SYNC_CHANNEL.init(Channel::new());
    let lora = LORA.init(AsyncMutex::new(lora_controller));
    let metrics = METRICS.init(AsyncMutex::new(LoraMetrics::default()));
    let rng = RNG.init(AsyncMutex::new(Rng::new(wifi_peripherals.rng)));

    if let Err(e) = display.show_message("LoRa Metrics\nReady") {
        error!("Failed to show initial message: {:?}", e);
    }

    let _ = _spawner.spawn(task_send(channel, lora, metrics));
    let _ = _spawner.spawn(task_receive(channel, lora, metrics));
    //let _ = _spawner.spawn(task_sync(channel, rng, metrics));
    let _ = _spawner.spawn(task_metrics_report(channel, metrics));

    let mut button_was_pressed = false;

    loop {
        if prg_button.is_low() && !button_was_pressed {
            button_was_pressed = true;

            info!("PRG button pressed, sending sync message");

            let _ = channel.sender().send(LoraEnvelope::new(
                MessageType::Metrics,
                0,
                {
                    let mut rng_guard = rng.lock().await;
                    rng_guard.random()
                },
                core::cmp::min(Instant::now().as_millis(), u32::MAX as u64) as u32,
                0,
                "sync".as_bytes().to_vec(),
            )).await;
        } else if prg_button.is_high() {
            button_was_pressed = false;
        }

        if let Err(e) = display.clear() {
            error!("Failed to clear display: {:?}", e);
            Timer::after_millis(100).await;
            continue;
        }

        let (sent, received, lost, last_rssi) = {
            let metrics_ref = metrics.lock().await;
            (
                metrics_ref.packets_sent,
                metrics_ref.packets_received,
                metrics_ref.packets_lost,
                metrics_ref.last_rssi,
            )
        };

        display.text_no_clear("Snd:", 0, 12).ok();
        let mut buf = heapless::String::<12>::new();
        write!(&mut buf, "{}", sent).ok();
        display.text_no_clear(&buf, 40, 12).ok();

        display.text_no_clear("Rcv:", 0, 24).ok();
        let mut buf = heapless::String::<12>::new();
        write!(&mut buf, "{}", received).ok();
        display.text_no_clear(&buf, 40, 24).ok();

        display.text_no_clear("Lst:", 0, 36).ok();
        let mut buf = heapless::String::<12>::new();
        write!(&mut buf, "{}", lost).ok();
        display.text_no_clear(&buf, 40, 36).ok();

        display.text_no_clear("RSSI:", 0, 48).ok();
        let mut buf = heapless::String::<12>::new();
        write!(&mut buf, "{}", last_rssi).ok();
        display.text_no_clear(&buf, 40, 48).ok();

        display.text_no_clear("PRG:Send", 0, 60).ok();

        if let Err(e) = display.flush() {
            error!("Failed to flush display: {:?}", e);
        }

        Timer::after_millis(500).await;
    }
}