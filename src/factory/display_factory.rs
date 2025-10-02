use esp_idf_hal::{
    delay::FreeRtos, 
    gpio::{Gpio15, Gpio16, Gpio4, Output, PinDriver}, 
    i2c::{I2cDriver, I2C0}, 
    peripherals::Peripherals, 
    units::Hertz
};
use crate::hal::display::Display;
use crate::hal::peripheral_manager::{PeripheralManager, DisplayPeripherals};
use log::info;

pub struct DisplayFactory;

impl DisplayFactory {
    /// Create display directly from peripherals (legacy method)
    // pub fn create<'d>(peripherals: Peripherals) -> Display<'d> {        
    //     // Configuração do I2C com velocidade mais baixa para maior confiabilidade
    //     let i2c = peripherals.i2c0;
    //     let sda = peripherals.pins.gpio4;
    //     let scl = peripherals.pins.gpio15;
        
    //     let config = esp_idf_hal::i2c::I2cConfig::new()
    //         .baudrate(Hertz(100_000)) // Try standard 100kHz first
    //         .scl_enable_pullup(true)
    //         .sda_enable_pullup(true);
    //     let mut i2c_driver = I2cDriver::new(i2c, sda, scl, &config).unwrap();
        
    //     // Add I2C scan to check if device is present
    //     info!("Scanning I2C bus for devices...");
    //     for addr in 0x08..0x78 {
    //         let mut dummy = [0u8; 1];
    //         if i2c_driver.read(addr, &mut dummy, esp_idf_hal::delay::BLOCK).is_ok() {
    //             info!("Found I2C device at address: 0x{:02X}", addr);
    //         }
    //     }
        
    //     Display::new(i2c_driver).unwrap()
    // }

    /// Create display from peripheral manager (preferred method)
    pub fn create_from_manager<'d>(manager: &mut PeripheralManager) -> Result<Display<'d>, &'static str> {
        let peripherals = manager
            .take_display_peripherals()
            .ok_or("Display peripherals not available")?;
        
        info!("Configuring display reset pin...");
        // First reset the display using the reset pin

        let mut rst_pin = Self::create_rst(peripherals.rst);
        
        info!("Configuring I2C driver...");
        // Try different I2C configurations
        let config = esp_idf_hal::i2c::I2cConfig::new()
            .baudrate(Hertz(100_000)) // Standard speed
            .scl_enable_pullup(true)
            .sda_enable_pullup(true);
        
        let mut i2c_driver = I2cDriver::new(
            peripherals.i2c, 
            peripherals.sda, 
            peripherals.scl, 
            &config
        ).map_err(|_| "Failed to create I2C driver")?;
        
        // Add a delay before creating display
        FreeRtos::delay_ms(100);
        
        info!("Creating display instance...");
        Display::new(i2c_driver, rst_pin)
            .map_err(|_| "Failed to create display")
    }

    /// Create I2C driver from components with improved configuration
    pub fn create_i2c_driver<'d>(i2c: I2C0, sda: Gpio4, scl: Gpio15) -> I2cDriver<'d> {
        let config = esp_idf_hal::i2c::I2cConfig::new()
            .baudrate(Hertz(100_000)) // Standard speed
            .scl_enable_pullup(true)
            .sda_enable_pullup(true);
        I2cDriver::new(i2c, sda, scl, &config).unwrap()
    }

    pub fn get_peripherals(peripherals: Peripherals) -> (I2C0, Gpio4, Gpio15, Gpio16) {
        let i2c = peripherals.i2c0;
        let sda = peripherals.pins.gpio4;
        let scl = peripherals.pins.gpio15;
        let rst_pin = peripherals.pins.gpio16;
        (i2c, sda, scl, rst_pin)
    }

    pub fn create_rst<'d>(rst: Gpio16) -> PinDriver<'d, Gpio16, Output> {
        let mut rst_pin = PinDriver::output(rst)
            .map_err(|_| "Failed to configure reset pin").unwrap();

        info!("Performing display reset sequence...");
        let _ = rst_pin.set_low().map_err(|_| "Failed to set reset pin low");
        FreeRtos::delay_ms(200); // Longer reset pulse
        let _ = rst_pin.set_high().map_err(|_| "Failed to set reset pin high");
        FreeRtos::delay_ms(500); // Much longer startup delay

        rst_pin
    }
}