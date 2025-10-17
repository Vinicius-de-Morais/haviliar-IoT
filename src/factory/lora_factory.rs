use anyhow::Result;
use log::info;
use core::{default::Default, result::Result::Ok};
use esp_hal::{
    spi::{master::{Config as SpiConfig, Spi}, Mode}, time::Rate, Async
};
use crate::hal::{lora::Lora, peripheral_manager::LoRaPeripherals};
use crate::hal::peripheral_manager::PeripheralManager;
use esp_hal::peripherals::{GPIO12, GPIO14, GPIO8};

// static SPI_BUS: StaticCell<Mutex<CriticalSectionRawMutex, esp_hal::spi::master::Spi<'static, Async>>> =
//     StaticCell::new();

pub struct LoraFactory;

impl LoraFactory {
    pub fn create_from_manager<'d>(
        manager: &mut PeripheralManager,
    ) -> Result<(Spi<'static, Async>, GPIO14<'d>, GPIO12<'d>, GPIO8<'d>)> {
        let peripherals = manager
            .take_lora_peripherals()
            .ok_or_else(|| anyhow::anyhow!("LoRa peripherals not available"))?;
        
        // Configure SPI
        let spi_config = SpiConfig::default();
            //.with_frequency(2_000_000_u32.Hz());  // 2 MHz
            
        let spi = Spi::new(peripherals.spi, spi_config)
            .map_err(|e| anyhow::anyhow!("Failed to create SPI: {:?}", e))?
            .with_mosi(peripherals.mosi)
            .with_miso(peripherals.miso)
            .with_sck(peripherals.sclk)
            .into_async();

        Ok((spi, peripherals.dio1, peripherals.rst, peripherals.cs))
    }

    pub async fn create_lora_with_spi<'d>(
        // spi: Spi<'static, Async>,
        // dio1: GPIO14<'d>,
        // rst: GPIO12<'d>,
        // nss: GPIO8<'d>,
        peripherals: LoRaPeripherals,
    ) -> Result<Lora<'d>>
    {
        
        // Configure SPI
        let spi_config = SpiConfig::default()
            .with_frequency(Rate::from_khz(100))
            .with_mode(Mode::_0);
        
        let spi = peripherals.spi;
        let mosi = peripherals.mosi;
        let miso = peripherals.miso;
        let sclk = peripherals.sclk;

        let spi = Spi::new(spi, spi_config)
        .unwrap()
        .with_mosi(mosi)
        .with_miso(miso)
        .with_sck(sclk)
        .into_async();

        info!("Pego os perifericos do lora");

    
        Lora::new(spi, peripherals.rst, peripherals.dio1, peripherals.cs).await
    }
}