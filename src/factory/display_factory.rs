use esp_idf_hal::{delay::FreeRtos, gpio::{Gpio15, Gpio16, Gpio4, PinDriver}, i2c::{I2cDriver, I2C0}, peripherals::Peripherals, units::{Hertz}};
use crate::hal::display::Display;
use crate::hal::peripheral_manager::PeripheralManager;

pub struct DisplayFactory;

impl DisplayFactory {
    /// Create display directly from peripherals (legacy method)
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

    /// Create display from peripheral manager (preferred method)
    pub fn create_from_manager<'d>(manager: &mut PeripheralManager) -> Result<Display<'d>, &'static str> {
        let (i2c, sda, scl, _rst) = manager
            .take_display_peripherals()
            .ok_or("Display peripherals not available")?;
        
        let config = esp_idf_hal::i2c::I2cConfig::new()
            .baudrate(Hertz(100_000));
        
        let i2c_driver = I2cDriver::new(i2c, sda, scl, &config)
            .map_err(|_| "Failed to create I2C driver")?;
        
        Display::new(i2c_driver)
            .map_err(|_| "Failed to create display")
    }

    /// Create display from I2C driver
    pub fn create_from_i2c<'d>(i2c: I2cDriver<'d>) -> Display<'d> {        
        Display::new(i2c).unwrap()
    }

    /// Create I2C driver from components
    pub fn create_i2c_driver<'d>(i2c: I2C0, sda: Gpio4, scl: Gpio15 ) -> I2cDriver<'d> {
        let config = esp_idf_hal::i2c::I2cConfig::new()
            .baudrate(Hertz(100_000));
            //.timeout(esp_idf_hal::i2c::APBTickType::from(MilliSeconds(1000)));
        I2cDriver::new(i2c, sda, scl, &config).unwrap()
    }

    /// Reset display using peripheral manager
    pub fn reset_display_from_manager(manager: &mut PeripheralManager) -> Result<(), &'static str> {
        let rst_pin = manager.take_gpio16()
            .ok_or("Reset pin not available")?;
        
        let mut rst_pin = PinDriver::output(rst_pin)
            .map_err(|_| "Failed to configure reset pin")?;

        // Reset manual do display com timing mais longo
        rst_pin.set_low().map_err(|_| "Failed to set reset pin low")?;
        FreeRtos::delay_ms(50);
        rst_pin.set_high().map_err(|_| "Failed to set reset pin high")?;
        FreeRtos::delay_ms(50);
        
        Ok(())
    }

    pub fn get_peripherals(peripherals: Peripherals) -> (I2C0, Gpio4, Gpio15, Gpio16) {
        let i2c = peripherals.i2c0;
        let sda = peripherals.pins.gpio4;
        let scl = peripherals.pins.gpio15;
        let rst_pin = peripherals.pins.gpio16;
        (i2c, sda, scl, rst_pin)
    }
}