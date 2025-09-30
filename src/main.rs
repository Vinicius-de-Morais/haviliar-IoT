#![no_std]
#![no_main]

use anyhow::Result;
use core::fmt::Write;
use esp_idf_hal::{
    delay::FreeRtos, prelude::Peripherals,
};
use esp_idf_sys as _;
use log::*;
use factory::display_factory::DisplayFactory;

// Configurações do TTGO LoRa32
const OLED_SDA: u8 = 4;
const OLED_SCL: u8 = 15;
const OLED_RST: u8 = 16;
const I2C_ADDRESS: u8 = 0x3C;

#[no_mangle]
async fn main() -> Result<()> {
    // Setup do logger
    esp_idf_sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("Inicializando...");

    // Configuração dos periféricos
    let peripherals = Peripherals::take().unwrap();

    let mut display = DisplayFactory::create(peripherals).await;

    display.show_message("Iniciando");
    FreeRtos::delay_ms(2000);

    // Loop principal
    let mut counter = 0;
    loop {
        display.clear();

        // // Texto estático
        display.text_new_line("Display OK! Com Factory", 1);
        display.text_new_line("Contador:", 2);    

        // // Contador
        let mut counter_str = heapless::String::<10>::new();
        write!(&mut counter_str, "{}", counter).unwrap();
        
        display.text_new_line(&counter_str, 3);
        
        display.flush();        
        info!("Contador: {}", counter);
        counter += 1;
        
        FreeRtos::delay_ms(1000);
    }
}