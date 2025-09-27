#![no_std]
#![no_main]

use anyhow::Result;
use core::fmt::Write;
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Alignment, Text},
};
use esp_idf_hal::{
    delay::FreeRtos,
    gpio::PinDriver,
    i2c::I2cDriver,
    prelude::*,
    units::Hertz,
};
use esp_idf_sys as _;
use log::*;
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306};
use hal::display::Display;

// Configurações do TTGO LoRa32
const OLED_SDA: u8 = 4;
const OLED_SCL: u8 = 15;
const OLED_RST: u8 = 16;
const I2C_ADDRESS: u8 = 0x3C;

#[no_mangle]
fn main() -> Result<()> {
    // Setup do logger
    esp_idf_sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("Inicializando...");

    // Configuração dos periféricos
    let peripherals = Peripherals::take().unwrap();

    // Configuração do pino de reset
    let mut rst_pin = PinDriver::output(peripherals.pins.gpio16)?;
    
    // Reset manual do display
    rst_pin.set_low()?;
    FreeRtos::delay_ms(20);
    rst_pin.set_high()?;

    
    // Configuração do I2C
    let i2c = peripherals.i2c0;
    let sda = peripherals.pins.gpio4;
    let scl = peripherals.pins.gpio15;
    
    let config = esp_idf_hal::i2c::I2cConfig::new().baudrate(Hertz(400_000));
    let i2c_driver = I2cDriver::new(i2c, sda, scl, &config)?;
    let mut display = Display::new(i2c_driver).unwrap();

    display.show_message("Iniciando");
    FreeRtos::delay_ms(2000);

    // Loop principal
    let mut counter = 0;
    loop {
        display.clear();

        // // Texto estático
        display.text_new_line("Display OK!", 1);
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