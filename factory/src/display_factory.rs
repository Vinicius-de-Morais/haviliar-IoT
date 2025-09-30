use std::sync::{Arc, Mutex};

use esp_idf_hal::{delay::FreeRtos, gpio::{AnyInputPin, AnyOutputPin, PinDriver}, i2c::I2cDriver, io::Error, peripherals::Peripherals, spi::{Spi, SpiDriver, SpiDriverConfig}, units::Hertz};
use esp_idf_hal::spi::config::{DriverConfig, Config};
use esp_idf_hal::spi::SpiDeviceDriver;
use hal::display::Display;

//use embedded_hal::spi::SpiDevice;

pub struct DisplayFactory;

impl DisplayFactory {
    pub async fn create<'d>(peripherals: Peripherals) -> Display {
        let mut rst_pin = PinDriver::output(peripherals.pins.gpio16).unwrap();
        
        // Reset manual do display
        rst_pin.set_low();
        FreeRtos::delay_ms(20);
        rst_pin.set_high();

        
        // Configuração do I2C
        let i2c = peripherals.i2c0;
        let sda = peripherals.pins.gpio4;
        let scl = peripherals.pins.gpio15;
        
        let config = esp_idf_hal::i2c::I2cConfig::new().baudrate(Hertz(400_000));
        let i2c_driver = I2cDriver::new(i2c, sda, scl, &config).unwrap();
        Display::new(i2c_driver).unwrap()
    }
}