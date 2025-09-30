#![no_std]
#![no_main]
use anyhow::Result;
use core::fmt::Write;
use esp_idf_hal::{
    delay::FreeRtos, gpio::PinDriver, prelude::Peripherals
};
use esp_idf_sys as _;
use log::*;
use haviliar_iot::factory::display_factory::DisplayFactory;

#[no_mangle]
fn main() -> Result<()> {
    // Setup do logger
    esp_idf_sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("Inicializando...");

    // Configuração dos periféricos
    let peripherals = Peripherals::take().unwrap();
    let (i2c, sda, scl, rst_pin) = DisplayFactory::get_peripherals(peripherals);
    let mut rst_pin = PinDriver::output(rst_pin).unwrap();

    // Reset manual do display com timing mais longo
    rst_pin.set_low().unwrap();
    FreeRtos::delay_ms(50);  // Increased delay
    rst_pin.set_high().unwrap();
    FreeRtos::delay_ms(50);  // Add delay after reset
    
    let i2c = DisplayFactory::create_i2c_driver(i2c, sda, scl);
    let mut display = DisplayFactory::create_from_i2c(i2c);

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

        // // Texto estático
        if let Err(e) = display.text_new_line("Display OK! Com Factory", 1) {
            error!("Failed to write text: {:?}", e);
        }
        if let Err(e) = display.text_new_line("Contador:", 2) {
            error!("Failed to write text: {:?}", e);
        }

        // // Contador
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