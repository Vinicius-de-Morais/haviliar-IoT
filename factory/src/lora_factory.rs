use std::sync::{Arc, Mutex};

use esp_idf_hal::{gpio::{AnyInputPin, AnyOutputPin}, io::Error, peripherals::Peripherals, spi::{Spi, SpiDriver, SpiDriverConfig}, units::Hertz};
use esp_idf_hal::spi::config::{DriverConfig, Config};
use esp_idf_hal::spi::SpiDeviceDriver;
use hal::lora::Lora;

//use embedded_hal::spi::SpiDevice;

pub struct LoraFactory;

impl LoraFactory {
    pub async fn create<'d>(peripherals: Peripherals) -> Result<Lora<'d, SpiDriver<'d>>, anyhow::Error> {

        //let peripherals = pheripherals.lock().unwrap(); pheripherals: Arc<Mutex<Peripherals>>

        let dio1  = peripherals.pins.gpio14;
        let rst = peripherals.pins.gpio12;

        let _spi = peripherals.spi2;
        let sclk = peripherals.pins.gpio9;
        let sdo = peripherals.pins.gpio11;
        let sdi = peripherals.pins.gpio10;
        let config = SpiDriverConfig::new();
        let driver = SpiDriver::new(
            _spi,   
            sclk,
            sdo,
            Some(sdi), 
            &config,
        ).unwrap();

        let cs = peripherals.pins.gpio8;
        let spi_config= Config::new().baudrate(Hertz(2_000_000));
        let spi_driver = Box::new(SpiDeviceDriver::new(driver, Some(cs), &spi_config).unwrap());

        Lora::new(Box::leak(spi_driver), dio1, rst).await
    }
}