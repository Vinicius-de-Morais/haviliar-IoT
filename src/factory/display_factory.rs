use esp_idf_hal::{delay::FreeRtos, gpio::{Gpio15, Gpio16, Gpio4, PinDriver}, i2c::{I2cDriver, I2C0}, peripherals::Peripherals, units::{Hertz}};
use crate::hal::display::Display;

pub struct DisplayFactory;

impl DisplayFactory {
    pub fn create<'d>(peripherals: Peripherals) -> Display<'d> {        
        // Configuração do I2C com velocidade mais baixa para maior confiabilidade
        let i2c = peripherals.i2c0;
        let sda = peripherals.pins.gpio4;
        let scl = peripherals.pins.gpio15;
        
        let config = esp_idf_hal::i2c::I2cConfig::new()
            .baudrate(Hertz(100_000));
            //.timeout(esp_idf_hal::i2c::APBTickType::from(MilliSeconds(1000)));
        let i2c_driver = I2cDriver::new(i2c, sda, scl, &config).unwrap();
        Display::new(i2c_driver).unwrap()
    }

    pub fn create_from_i2c<'d>(i2c: I2cDriver<'d>) -> Display<'d> {        
        Display::new(i2c).unwrap()
    }

    pub fn create_i2c_driver<'d>(i2c: I2C0, sda: Gpio4, scl: Gpio15 ) -> I2cDriver<'d> {
        let config = esp_idf_hal::i2c::I2cConfig::new()
            .baudrate(Hertz(100_000));
            //.timeout(esp_idf_hal::i2c::APBTickType::from(MilliSeconds(1000)));
        I2cDriver::new(i2c, sda, scl, &config).unwrap()
    }

    pub fn reset_display<'d, P>(mut rst_pin: PinDriver<'d, P, esp_idf_hal::gpio::Output>)
    where
        P: esp_idf_hal::gpio::Pin,
    {
        // Reset manual do display com timing mais longo
        rst_pin.set_low().unwrap();
        FreeRtos::delay_ms(50);  // Increased delay
        rst_pin.set_high().unwrap();
        FreeRtos::delay_ms(50);  // Add delay after reset
    }

    pub fn get_peripherals(peripherals: Peripherals) -> (I2C0, Gpio4, Gpio15, Gpio16) {
        let i2c = peripherals.i2c0;
        let sda = peripherals.pins.gpio4;
        let scl = peripherals.pins.gpio15;
        let rst_pin = peripherals.pins.gpio16;
        (i2c, sda, scl, rst_pin)
    }
}