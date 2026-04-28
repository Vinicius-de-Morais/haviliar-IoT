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
    controller::lora::{self, LoraController},
    factory::{display_factory::DisplayFactory, lora_factory::LoraFactory},
    hal::{
        lora::{
            Lora, OutgoingMessage, PAYLOAD_LENGTH,
        },
        peripheral_manager::PeripheralManagerStatic,
    },
    protocol::{lora::MAX_APP_PAYLOAD, message_type::MessageType},
};
use log::*;
use esp_hal::clock::CpuClock;
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

static LORA_CONTROLLER: StaticCell<AsyncMutex<CriticalSectionRawMutex, LoraController>> = StaticCell::new();
//static DISPLAY: StaticCell<AsyncMutex<CriticalSectionRawMutex, Display<'static>>> = StaticCell::new();
type LoRaChannel = Channel<CriticalSectionRawMutex, OutgoingMessage, 1>;
static LORA_CHANNEL: StaticCell<LoRaChannel> = StaticCell::new();

#[embassy_executor::task]
async fn task_send(
    channel: &'static LoRaChannel,
    lora: &'static AsyncMutex<CriticalSectionRawMutex, LoraController>
    ) {
    
    let receiver = channel.receiver();
        
    loop {
        let mut message = receiver.receive().await;
        let payload = &mut message.payload[..message.len];

        lora.lock().await.send_message(
            haviliar_iot::protocol::message_type::MessageType::Counter,
            1, // Using first byte of payload as sequence for simplicity
            Instant::now().as_millis() as u32,
            0, // Elapsed time can be set to 0 for this example
            payload,
        ).await.unwrap_or_else(|e| error!("Failed to send LoRa message: {:?}", e));
    }
}

#[embassy_executor::task]
async fn task_receive(
    lora: &'static AsyncMutex<CriticalSectionRawMutex, LoraController>
    ) {
    
    loop{

        info!("Init to receive LoRa message");
        Timer::after_millis(100).await;

        let mut recv_buffer = [0u8; PAYLOAD_LENGTH];


        // it seems to be blocking the lora structure
        {
            let mut lora_ref  = lora.lock().await;
            match lora_ref.receive_message(&mut recv_buffer).await {
                Ok(_e) => info!("LoRa message received successfully"),
                Err(e) => error!("Failed to receive LoRa message: {:?}", e),
            }

            info!("Received LoRa message: {:?}", &recv_buffer[..core::cmp::min(4, recv_buffer.len())]);
        }
        
        Timer::after_millis(100).await;
    }

}

#[esp_hal_embassy::main]
async fn main(_spawner: Spawner) {
    // Initialize heap first before any logging
    // unsafe {
    //     esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
    //         HEAP.as_mut_ptr() as *mut u8,
    //         HEAP_SIZE,
    //         esp_alloc::MemoryCapability::Internal.into(),
    //     ));
    // }
    
    init_logger(log::LevelFilter::Info);
    
    info!("Initializing ESP32 Display...");

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

    info!("Initializing ESP32 LoRa...");

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
    let lora_controller_mutex = LORA_CONTROLLER.init(AsyncMutex::new(lora_controller));

    info!("Both display and LoRa initialized successfully!");

    if let Err(e) = display.show_message("LoRa + Display OK!") {
        error!("Failed to show initial message: {:?}", e);
    }
    
    let _ = _spawner.spawn(task_send(channel, lora_controller_mutex));
    let _ = _spawner.spawn(task_receive(lora_controller_mutex));
    
    // Main loop
    let mut counter = 0u32;
    let mut tx_seq: u16 = 0;
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

        
        let now = Instant::now();
        let timestamp_ms = core::cmp::min(now.as_millis(), u32::MAX as u64) as u32;
        let sender = channel.sender();

        // Usa somente o tamanho máximo de payload de aplicação suportado
        // pelo envelope CBOR dentro do frame LoRa.
        let payload = [counter as u8; MAX_APP_PAYLOAD];
        let mut lora_ref = lora_controller_mutex.lock().await;

        let _ = lora_ref.send_message(MessageType::Counter, tx_seq, timestamp_ms, 0 /*elapsed_ms*/, &payload).await;
        

        info!("Counter: {}", counter);
        counter += 1;

        Timer::after_millis(5000).await;
    }
}