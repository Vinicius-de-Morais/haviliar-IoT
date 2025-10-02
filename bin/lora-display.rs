#![no_std]
#![no_main]
use anyhow::Result;
use core::fmt::Write;
use esp_idf_hal::{
    delay::FreeRtos, prelude::Peripherals
};
use esp_idf_sys as _;
use log::*;
use haviliar_iot::factory::display_factory::DisplayFactory;
use haviliar_iot::factory::lora_factory::LoraFactory;
use haviliar_iot::hal::peripheral_manager::PeripheralManager;

#[no_mangle]
fn main() -> Result<()> {
    // Setup do logger
    esp_idf_sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("Inicializando...");

    // Initialize peripheral manager - much cleaner approach!
    let peripherals = Peripherals::take().unwrap();
    let mut peripheral_manager = PeripheralManager::new(peripherals);

    // Reset display first
    // if let Err(e) = DisplayFactory::reset_display_from_manager(&mut peripheral_manager) {
    //     error!("Failed to reset display: {}", e);
    // }
    
    // Create display from manager
    let mut display = match DisplayFactory::create_from_manager(&mut peripheral_manager) {
        Ok(display) => display,
        Err(e) => {
            error!("Failed to create display: {}", e);
            return Err(anyhow::anyhow!("Display initialization failed"));
        }
    };

    // Create LoRa components from manager
    let (_lora_spi_driver, _lora_dio1, _lora_rst) = match LoraFactory::create_from_manager(&mut peripheral_manager) {
        Ok(components) => components,
        Err(e) => {
            error!("Failed to create LoRa components: {:?}", e);
            return Err(e);
        }
    };

    info!("Both display and LoRa initialized successfully!");

    if let Err(e) = display.show_message("Iniciando") {
        error!("Failed to show initial message: {:?}", e);
    }
    FreeRtos::delay_ms(2000);

    // Loop principal
    let mut counter = 0;
    loop {
        if let Err(e) = display.clear() {
            error!("Failed to clear display: {:?}", e);
            continue;
        }

        // Texto estático
        if let Err(e) = display.text_new_line("Display + LoRa OK!", 1) {
            error!("Failed to write text: {:?}", e);
        }
        if let Err(e) = display.text_new_line("Contador:", 2) {
            error!("Failed to write text: {:?}", e);
        }

        // Contador
        let mut counter_str = heapless::String::<10>::new();
        write!(&mut counter_str, "{}", counter).unwrap();
        
        if let Err(e) = display.text_new_line(&counter_str, 3) {
            error!("Failed to write counter: {:?}", e);
        }
        
        if let Err(e) = display.flush() {
            error!("Failed to flush display: {:?}", e);
        }        
        info!("Contador: {}", counter);
        counter += 1;
        
        FreeRtos::delay_ms(1000);
    }
}