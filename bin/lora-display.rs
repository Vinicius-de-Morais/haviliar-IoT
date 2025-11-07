#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::{fmt::Write, task::Context};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::{CriticalSectionMutex, Mutex, raw::CriticalSectionRawMutex}, channel::{Channel, Sender}, mutex::Mutex as AsyncMutex};
use embassy_time::Timer;
use esp_backtrace as _;
use esp_println::logger::init_logger;
use haviliar_iot::{
    factory::{display_factory::DisplayFactory, lora_factory::LoraFactory},
    hal::{display::Display, lora::{Lora, PAYLOAD_LENGTH}, peripheral_manager::PeripheralManagerStatic},
};
use log::*;
use esp_hal::clock::CpuClock;
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

static LORA: StaticCell<AsyncMutex<CriticalSectionRawMutex, Lora<'static>>> = StaticCell::new();
//static DISPLAY: StaticCell<AsyncMutex<CriticalSectionRawMutex, Display<'static>>> = StaticCell::new();
static LORA_CHANNEL: StaticCell<Channel<CriticalSectionRawMutex, [u8; PAYLOAD_LENGTH], 10>> = StaticCell::new();

#[embassy_executor::task]
async fn task_send(
    channel: &'static Channel<CriticalSectionRawMutex, [u8; PAYLOAD_LENGTH], 10>, 
    lora: &'static AsyncMutex<CriticalSectionRawMutex, Lora<'static>>
    ) {
    
    let receiver = channel.receiver();//channel.dyn_receiver();
        
    loop {
        info!("receiver.empty(): {}", receiver.is_empty());
        info!("receiver.len(): {}", receiver.len());

        match receiver.try_receive() {
            Ok(mut message) => {

                info!("Received message from Channel to send via LoRa: {:?}", &message[..4]);

                Lora::send_from_mutex(lora, &mut message).await;
            }
            Err(e) => {
                error!("Failed to receive from Channel: {:?}", e);
            }
        }

        Timer::after_millis(100).await;
    }
}

#[embassy_executor::task]
async fn task_receive(
    lora: &'static AsyncMutex<CriticalSectionRawMutex, Lora<'static>>
    ) {
    
    loop{
        Timer::after_millis(100).await;

        let mut recv_buffer = [0u8; PAYLOAD_LENGTH];


        //TODO: The code below is not working as expected. Fix it.
        // it seems to be blocking the lora structure
        // {
        //     Lora::receive_from_mutex(lora, &mut recv_buffer).await;
        // }
        
        Timer::after_millis(100).await;
    }

}

#[esp_hal_embassy::main]
async fn main(_spawner: Spawner) {
    init_logger(log::LevelFilter::Info);
    
    info!("haviliar_iot::init_heap() called");
    
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

    let channel = LORA_CHANNEL.init(Channel::new());
    let lora = LORA.init(AsyncMutex::new(lora));

    info!("Both display and LoRa initialized successfully!");

    if let Err(e) = display.show_message("LoRa + Display OK!") {
        error!("Failed to show initial message: {:?}", e);
    }
    
    let _ = _spawner.spawn(task_send(channel, lora));
    let _ = _spawner.spawn(task_receive(lora));
    
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

        
        let mut msg = [0u8; PAYLOAD_LENGTH];
        msg[0..4].copy_from_slice(&counter.to_le_bytes());
        let sender = channel.sender();
        
        match sender.try_send(msg) {
            Ok(_) => info!("Sent message to LoRa task"),
            Err(e) => error!("Failed to send message to LoRa task: {:?}", e),
        }
        
        //let _ = sender.send(msg);
        
        info!("Counter: {}", counter);
        counter += 1;

        Timer::after_millis(5000).await;
    }
}