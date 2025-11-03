#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::fmt::Write;
use embassy_executor::Spawner;
use embassy_time::{Timer};
use esp_backtrace as _;
use esp_println::logger::init_logger;
use haviliar_iot::{
    factory::display_factory::DisplayFactory,
    hal::peripheral_manager::{PeripheralManagerStatic},
};
use log::*;
use esp_hal::{clock::CpuClock};

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal_embassy::main]
async fn main(_spawner: Spawner) {
    init_logger(log::LevelFilter::Info);

    haviliar_iot::init_heap(); // Initialize the heap
    info!("haviliar_iot::init_heap() called");
    
    info!("Initializing ESP32 with embassy...");

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

    // Show initial message
    if let Err(e) = display.show_message("Iniciando") {
        error!("Failed to show initial message: {:?}", e);
    }
    
    // Main loop
    let mut counter = 0u32;
    loop {
        if let Err(e) = display.clear() {
            error!("Failed to clear display: {:?}", e);
            continue;
        }

        // Static text
        if let Err(e) = display.text_new_line("Display OK! Embassy", 1) {
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
        
        Timer::after_millis(1000).await;
    }
}