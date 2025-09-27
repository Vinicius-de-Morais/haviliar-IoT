use std::borrow::Borrow;

use anyhow::Result;
//use esp_idf_hal::delay::{Delay, Ets};
use esp_idf_hal::gpio::{AnyOutputPin, Gpio14, Gpio26, Input, Level, Output, Pin, PinDriver, Pull};
use esp_idf_hal::peripheral::Peripheral;
use esp_idf_hal::spi::{SpiDeviceDriver};
use lora_phy::sx127x::{Sx127x, Sx1276, Config};
use lora_phy::iv::{GenericSx127xInterfaceVariant};
use lora_phy::mod_params::{Bandwidth, CodingRate, ModulationParams, PacketParams, RadioError, SpreadingFactor};
use lora_phy::{LoRa, DelayNs};
use embassy_time::Delay;
use esp_idf_hal::gpio::AnyInputPin;
use esp_idf_hal::spi::SpiDriver as hal_SpiDriver;

const LORA_FREQUENCY_IN_HZ: u32 = 903_900_000;

// type GenericInterface<'d> = GenericSx127xInterfaceVariant<PinDriver<'d, AnyInputPin, Output>, PinDriver<'d, AnyInputPin, Input>>;
// // type SpiDriverType<'d, T> = SpiDeviceDriver<'d, T>;
// // type LoRaType<'d, T> = LoRa<Sx127x<SpiDriverType<'d, T>, GenericInterface<'d>, Sx1276>, Delay>;

// type SpiDriverType<'d, T> = SpiDeviceDriver<'d, T>;
// type LoRaType<'d, T> = LoRa<Sx127x<SpiDriverType<'d, T>, GenericInterface<'d>, Sx1276>, Delay>;
// pub struct Lora<'d, T> 
// where
//     // T: Borrow<SpiDriver<'d>> + 'd,
//     T: Borrow<esp_idf_hal::spi::SpiDriver<'d>> + 'd,
//     &'d mut SpiDeviceDriver<'d, T>: embedded_hal_async::spi::SpiDevice,
// {
//     driver: LoRaType<'d, T>,
//     modulation: ModulationParams,
//     packet_params: PacketParams,
// }


type GenericInterface<'d> =
    GenericSx127xInterfaceVariant<
        PinDriver<'d, AnyInputPin, Output>,
        PinDriver<'d, AnyInputPin, Input>
    >;

// use a mutable reference — this matches the `impl for &mut T`
type SpiDriverType<'d, T> = &'d mut SpiDeviceDriver<'d, T>;
type LoRaType<'d, T> = LoRa<Sx127x<SpiDriverType<'d, T>, GenericInterface<'d>, Sx1276>, Delay>;

pub struct Lora<'d, T>
where
    T: Borrow<hal_SpiDriver<'d>> + 'd,
    SpiDeviceDriver<'d, T>: embedded_hal_async::spi::SpiDevice,
{
    driver: LoRaType<'d, T>,
    modulation: ModulationParams,
    packet_params: PacketParams,
}

impl<'d, T> Lora<'d, T>
where
    T: Borrow<hal_SpiDriver<'d>> + 'd,
    SpiDeviceDriver<'d, T>: embedded_hal_async::spi::SpiDevice,
{
    pub async fn new(
        spi: &'d mut SpiDeviceDriver<'d, T>,
        dio0: impl Peripheral<P = AnyInputPin> + 'd,
        rst: impl Peripheral<P = AnyOutputPin> + 'd,
    ) -> Result<Self> {
        let mut delay = Delay;

        let interface = GenericSx127xInterfaceVariant::new(
            PinDriver::output(rst).unwrap(),
            PinDriver::input(dio0).unwrap(),
            None,
            None
        ).unwrap();

        let config = Config {
            chip: Sx1276,
            tcxo_used: false,
            tx_boost: false,
            rx_boost: false,
        };

        let sx127x = Sx127x::new(spi, interface, config);
        let mut driver = {
            match lora_phy::LoRa::new(sx127x, false, &mut delay).await {
                Ok(d) => { d }
                Err(err) => { panic!("Radio error = {:?}", err); }
            }
        };   


        
        let modulation =  {
            match driver.create_modulation_params(
                SpreadingFactor::_10,
                Bandwidth::_250KHz,
                CodingRate::_4_8,
                LORA_FREQUENCY_IN_HZ,
            ) {
                Ok(mp) => mp,
                Err(err) => {
                    panic!("Radio error = {:?}", err);
                }
            }
        };

        let packet_params =  {
            match lora.create_rx_packet_params(4, false, receiving_buffer.len() as u8, true, false, &mdltn_params) {
                Ok(pp) => pp,
                Err(err) => {
                    info!("Radio error = {}", err);
                    return;
                }
            }
        };

        Ok(Lora { driver, modulation, packet_params })
    }

    pub fn send(&mut self, payload: &[u8]) -> Result<(), RadioError> {
-        self.driver.prepare_for_tx(&self.modulation, &self.packet_params, 17, payload)?;
        self.driver.tx()
    }

    pub fn receive(&mut self, buffer: &mut [u8], timeout_ms: u32) -> Result<(usize, lora_phy::mod_params::PacketStatus), RadioError> {
        let mut delay = Delay;
        self.driver.prepare_for_rx(&self.modulation, &self.packet_params, timeout_ms)?;
        self.driver.rx(buffer, &mut delay)
    }
}