use anyhow::Result;
use log::info;
use core::default::Default;
use esp_hal::{
    spi::{master::{Config as SpiConfig, Spi}, Mode}, time::Rate
};
use crate::hal::{lora::Lora, peripheral_manager::LoRaPeripherals};

pub struct LoraFactory;

impl LoraFactory {
    pub async fn create_from_manager<'d>(
        peripherals: LoRaPeripherals,
    ) -> Result<Lora<'d>>
    {
        
        //Configure SPI
        let spi_config = SpiConfig::default()
            .with_frequency(Rate::from_khz(100))
            .with_mode(Mode::_0);
        
        let spi = peripherals.spi;
        let mosi = peripherals.mosi;
        let miso = peripherals.miso;
        let sclk = peripherals.sclk;
        
        info!("Pego os perifericos do lora");

        
        let spi = Spi::new(spi, spi_config)
        .unwrap()
        .with_mosi(mosi)
        .with_miso(miso)
        .with_sck(sclk)
        .into_async();
    
        info!("SPI initialized");

        Lora::new(spi, peripherals.rst, peripherals.dio1, peripherals.nss).await
    }
}