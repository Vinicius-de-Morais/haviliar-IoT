#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::fmt::Write;
use embassy_executor::Spawner;
use embassy_sync::{mutex::Mutex as AsyncMutex, blocking_mutex::{CriticalSectionMutex, Mutex, raw::CriticalSectionRawMutex}, channel::Channel};
use embassy_time::Timer;
use esp_backtrace as _;
use esp_println::logger::init_logger;
use haviliar_iot::{
    factory::{display_factory::DisplayFactory, lora_factory::LoraFactory},
    hal::{display::Display, lora::Lora, peripheral_manager::PeripheralManagerStatic},
};
use log::*;
use esp_hal::clock::CpuClock;
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

static LORA: StaticCell<AsyncMutex<CriticalSectionRawMutex, Lora<'static>>> = StaticCell::new();
//static DISPLAY: StaticCell<AsyncMutex<CriticalSectionRawMutex, Display<'static>>> = StaticCell::new();
static LORA_CHANNEL: StaticCell<Channel<CriticalSectionRawMutex, [u8; 256], 10>> = StaticCell::new();

#[embassy_executor::task]
async fn task_send(
    channel: &'static Channel<CriticalSectionRawMutex, [u8; 256], 10>, 
    lora: &'static AsyncMutex<CriticalSectionRawMutex, Lora<'static>>
    ) {
    
        
    loop {
          info!("1234512");  
        let receiver = channel.receiver();
        if !receiver.is_empty() {
            info!("receiver is not empty");
            
            let message = receiver.receive().await;
            let mut lora_ref = lora.lock().await;
            if let Err(e) = lora_ref.send(&message).await {
                error!("Failed to send LoRa message: {:?}", e);
            } else {
                info!("LoRa message sent successfully");
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
        let mut recv_buffer = [0u8; 256];

        let mut lora_ref = lora.lock().await;
            match lora_ref.receive(&mut recv_buffer).await {
                Ok((length, status)) => {
                    let received_data = &recv_buffer[..length as usize];
                    // PacketStatus does not implement Debug, so avoid using {:?} on it.
                    // Log the length and the received bytes, and the runtime type name of status.
                    let status_type_name = core::any::type_name_of_val(&status);
                    info!("Received LoRa message (len {}): {:?}, status type: {}", length, received_data, status_type_name);
                }
                Err(e) => {
                    error!("Failed to receive LoRa message: {:?}", e);
                }
            }
        
        Timer::after_millis(7000).await;
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

        info!("Counter: {}", counter);
        counter += 1;

        let mut msg = [0u8; 256];
        msg[0..4].copy_from_slice(&counter.to_le_bytes());
        let _ = channel.send(msg);

        Timer::after_millis(5000).await;
    }
}