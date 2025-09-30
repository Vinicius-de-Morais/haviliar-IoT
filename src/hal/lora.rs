use core::borrow::Borrow;
use anyhow::Result;
use esp_idf_hal::gpio::{Input, Output, PinDriver};
use esp_idf_hal::peripheral::Peripheral;
use esp_idf_hal::spi::{SpiDeviceDriver};
use lora_phy::sx127x::{Sx127x, Sx1276, Config};
use lora_phy::iv::{GenericSx127xInterfaceVariant};
use lora_phy::mod_params::{Bandwidth, CodingRate, ModulationParams, PacketParams, RadioError, SpreadingFactor};
use lora_phy::{LoRa, RxMode};
use embassy_time::Delay;
use esp_idf_hal::spi::SpiDriver as hal_SpiDriver;
use super::spi_adapter::AsyncSpiAdapter;
use log::*;


const LORA_FREQUENCY_IN_HZ: u32 = 903_900_000;

type GenericInterface<'d> =
    GenericSx127xInterfaceVariant<
        PinDriver<'d, esp_idf_hal::gpio::Gpio12, Output>,
        PinDriver<'d, esp_idf_hal::gpio::Gpio14, Input>
    >;

// Modified to use our adapter
type SpiDriverType<'d, T> = AsyncSpiAdapter<'d, T>;
type LoRaType<'d, T> = LoRa<Sx127x<SpiDriverType<'d, T>, GenericInterface<'d>, Sx1276>, Delay>;

pub struct Lora<'d, T>
where
    T: Borrow<hal_SpiDriver<'d>> + 'd,
{
    driver: LoRaType<'d, T>,
    modulation: ModulationParams,
    packet_params: PacketParams,
    buffer: [u8; 256],
}

impl<'d, T> Lora<'d, T>
where
    T: Borrow<hal_SpiDriver<'d>> + 'd,
{
    pub async fn new(
        spi: &'d mut SpiDeviceDriver<'d, T>,
        dio1: impl Peripheral<P = esp_idf_hal::gpio::Gpio14> + 'd,
        rst: impl Peripheral<P = esp_idf_hal::gpio::Gpio12> + 'd,
    ) -> Result<Self> {
        let delay = Delay;
        
        // Create our adapter
        let spi_adapter = AsyncSpiAdapter::new(spi);

        let mut reset = PinDriver::output(rst).unwrap();
        reset.set_high().unwrap();
        let dio1 = PinDriver::input(dio1).unwrap();

        let interface = GenericSx127xInterfaceVariant::new(
            reset,
            dio1,
            None,
            None
        ).unwrap();

        let config = Config {
            chip: Sx1276,
            tcxo_used: false,
            tx_boost: false,
            rx_boost: false,
        };

        let sx127x = Sx127x::new(spi_adapter, interface, config);
        let mut driver = {
            match lora_phy::LoRa::new(sx127x, false, delay).await {
                Ok(d) => { d }
                Err(err) => { info!("Radio error = {:?}", err); panic!("Radio error = {:?}", err);}
            }
        };   

        let receiving_buffer = [0u8; 256];
        
        let modulation =  {
            match driver.create_modulation_params(
                SpreadingFactor::_10,
                Bandwidth::_250KHz,
                CodingRate::_4_8,
                LORA_FREQUENCY_IN_HZ,
            ) {
                Ok(mp) => mp,
                Err(err) => {
                    info!("Radio error = {:?}", err);
                    panic!("Radio error = {:?}", err);
                }
            }
        };

        let packet_params =  {
            match driver.create_rx_packet_params(4, false, receiving_buffer.len() as u8, true, false, &modulation) {
                Ok(pp) => pp,
                Err(err) => {
                    info!("Radio error = {:?}", err);
                    panic!("Radio error = {:?}", err);
                }
            }
        };

        Ok(Lora { driver, modulation, packet_params, buffer: receiving_buffer })
    }

    // Rest of the methods remain the same
    pub async fn send(&mut self, payload: &[u8]) -> Result<(), RadioError> {
        let _ = self.driver.prepare_for_tx(&self.modulation, & mut self.packet_params, 17, payload).await;
        self.driver.tx().await
    }

    pub async fn receive(&mut self, buffer: &mut [u8]) -> Result<(u8, lora_phy::mod_params::PacketStatus), RadioError> {
        let _ = self.driver.prepare_for_rx(RxMode::Continuous, &self.modulation, &self.packet_params).await;
        self.driver.rx(& mut self.packet_params, buffer).await
    }
}