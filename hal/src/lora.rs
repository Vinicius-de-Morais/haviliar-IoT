use anyhow::Result;
use esp_idf_hal::delay::Ets;
use esp_idf_hal::gpio::{Input, Output, Pin, PinDriver};
use esp_idf_hal::spi::{SpiDeviceDriver, SpiDriver};
use lora_phy::lora_traits::LoraRadio;
use lora_phy::sx127x::{Sx127x, LoRaModulationConfig, PacketParams};
use lora_phy::mod_params::RadioError;

pub struct Lora<'d> {
    driver: lora_phy::LoRa<Sx127x<'d, SpiDeviceDriver<'d, SpiDriver<'d>>, PinDriver<'d, impl Pin, Input>, PinDriver<'d, impl Pin, Output>>>,
    modulation: LoRaModulationConfig,
    packet_params: PacketParams,
}

impl<'d> Lora<'d> {
    pub fn new(
        spi: SpiDeviceDriver<'d, SpiDriver<'d>>,
        dio0: PinDriver<'d, impl Pin, Input>,
        rst: PinDriver<'d, impl Pin, Output>,
    ) -> Result<Self> {
        let mut delay = Ets;
        let sx127x = Sx127x::new(spi, dio0, rst);
        let mut driver = lora_phy::LoRa::new(sx127x, false, &mut delay)
           .map_err(|_| anyhow::anyhow!("Failed to create LoRa driver"))?;
        
        driver.init(&mut delay)
           .map_err(|_| anyhow::anyhow!("Failed to initialize radio"))?;
        
        let modulation = LoRaModulationConfig {
            spreading_factor: 7,
            bandwidth: 125_000,
            coding_rate: 4,
            low_data_rate_optimize: 0,
            frequency_in_hz: 915_000_000,
        };

        let packet_params = PacketParams {
            preamble_length: 8,
            implicit_header: false,
            payload_length: 0,
            crc_on: true,
            iq_inverted: false,
        };

        Ok(Lora { driver, modulation, packet_params })
    }

    pub fn send(&mut self, payload: &[u8]) -> Result<(), RadioError> {
        let mut delay = Ets;
        self.driver.prepare_for_tx(&self.modulation, &self.packet_params, 17, payload)?;
        self.driver.tx(&mut delay)
    }

    pub fn receive(&mut self, buffer: &mut [u8], timeout_ms: u32) -> Result<(usize, lora_phy::mod_params::PacketStatus), RadioError> {
        let mut delay = Ets;
        self.driver.prepare_for_rx(&self.modulation, &self.packet_params, timeout_ms)?;
        self.driver.rx(buffer, &mut delay)
    }
}