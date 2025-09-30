use esp_idf_hal::{peripherals::Peripherals, spi::{SpiDriver, SpiDriverConfig}, units::Hertz};
use esp_idf_hal::spi::config::{Config};
use esp_idf_hal::spi::SpiDeviceDriver;
use crate::hal::lora::Lora;

pub struct LoraFactory;

impl LoraFactory {
    /// Create LoRa instance with proper lifetime management
    pub async fn create_with_driver<'d>(
        spi_driver: &'d mut SpiDeviceDriver<'d, SpiDriver<'d>>,
        dio1: esp_idf_hal::gpio::Gpio14,
        rst: esp_idf_hal::gpio::Gpio12,
    ) -> Result<Lora<'d, SpiDriver<'d>>, anyhow::Error> {
        Lora::new(spi_driver, dio1, rst).await
    }

    /// Create SPI driver (separate from LoRa creation for better lifetime management)
    pub fn create_spi_driver<'d>(
        peripherals: Peripherals,
    ) -> Result<(SpiDeviceDriver<'d, SpiDriver<'d>>, esp_idf_hal::gpio::Gpio14, esp_idf_hal::gpio::Gpio12), anyhow::Error> {
        let dio1 = peripherals.pins.gpio14;
        let rst = peripherals.pins.gpio12;

        let _spi = peripherals.spi2;
        let sclk = peripherals.pins.gpio9;
        let sdo = peripherals.pins.gpio11;
        let sdi = peripherals.pins.gpio10;
        
        let config = SpiDriverConfig::new();
        let driver = SpiDriver::new(_spi, sclk, sdo, Some(sdi), &config)
            .map_err(|e| anyhow::anyhow!("Failed to create SPI driver: {:?}", e))?;

        let cs = peripherals.pins.gpio8;
        let spi_config = Config::new().baudrate(Hertz(2_000_000));
        let spi_driver = SpiDeviceDriver::new(driver, Some(cs), &spi_config)
            .map_err(|e| anyhow::anyhow!("Failed to create SPI device driver: {:?}", e))?;

        Ok((spi_driver, dio1, rst))
    }
}