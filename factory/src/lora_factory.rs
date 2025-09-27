use std::sync::{Arc, Mutex};

use esp_idf_hal::{io::Error, peripherals::Peripherals, spi::{Spi, SpiDriver, SpiDriverConfig}, units::Hertz};
use esp_idf_hal::spi::config::{DriverConfig, Config};
use esp_idf_hal::spi::SpiDeviceDriver;
use hal::lora::Lora;
//use embedded_hal::spi::SpiDevice;

struct LoraFactory;

impl LoraFactory {
    pub async fn create<'d>(pheripherals: Arc<Mutex<Peripherals>>) -> Result<Lora<'d, SpiDriver<'d>>, Box<dyn std::error::Error>> {

        let peripherals = pheripherals.lock().unwrap();

        let dio1 = peripherals.pins.gpio4;
        let rst = peripherals.pins.gpio5;

        let _spi = peripherals.spi2;
        let sclk = peripherals.pins.gpio18;
        let sdo = peripherals.pins.gpio23;
        let sdi = peripherals.pins.gpio19;
        let config = SpiDriverConfig::new();
        let driver = SpiDriver::new(
            _spi,   
            sclk,
            sdo,
            Some(sdi), // MISO
            &config,
        ).unwrap();

        let cs = peripherals.pins.gpio5;
        let spi_config= Config::new().baudrate(Hertz(2_000_000));
        let mut spi_driver = SpiDeviceDriver::new(driver, Some(cs), &spi_config).unwrap();

        Lora::new(&mut spi_driver, dio1, rst)
    }
}