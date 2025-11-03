use anyhow::Result;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use static_cell::StaticCell;
use core::result::Result::Ok;
use esp_hal::{gpio::{Input, InputConfig, Level, Output, OutputConfig}, spi::{master::Spi}, Async};
use lora_phy::{
    sx127x::{Sx127x, Sx1276, Config},
    iv::GenericSx127xInterfaceVariant,
    mod_params::{Bandwidth, CodingRate, ModulationParams, PacketParams, RadioError, SpreadingFactor},
    LoRa as LoRaPhy, RxMode,
};
use embassy_time::Delay as EmbassyDelay;
use log::*;
use core::result::Result::Err;
use esp_hal::peripherals::{GPIO14, GPIO26, GPIO18};
use core::default::Default;
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;

const LORA_FREQUENCY_IN_HZ: u32 = 903_900_000;

type LoRaInterface<'d> = GenericSx127xInterfaceVariant<
    Output<'d>,
    Input<'d>,
>;

pub type MutexCriticalSectionSpiAsync = Mutex<CriticalSectionRawMutex, esp_hal::spi::master::Spi<'static, Async>>;

pub static SPI_BUS: StaticCell<MutexCriticalSectionSpiAsync> =
    StaticCell::new();

pub struct Lora<'d> {
    driver: LoRaPhy<Sx127x<SpiDevice<'d, CriticalSectionRawMutex, Spi<'static, Async>, Output<'d>>, LoRaInterface<'d>, Sx1276>, EmbassyDelay>,
    modulation: ModulationParams,
    packet_params: PacketParams,
    //buffer: [u8; 256],
}

impl<'d> Lora<'d>
{
    pub async fn new(
        spi: Spi<'static, Async>,
        rst: GPIO14<'static>,
        dio1: GPIO26<'static>,
        nss: GPIO18<'static>,
    ) -> Result<Self> {

        info!("Entrou na criacao do lora");
        let delay = EmbassyDelay;

        let nss = Output::new(nss, Level::High, OutputConfig::default());
        let reset = Output::new(rst, Level::Low, OutputConfig::default());
        let dio1 = Input::new(dio1, InputConfig::default());

        // Initialize the static SPI bus
        info!("Creating bus");
        let spi_bus = SPI_BUS.init(Mutex::new(spi));
        let spi_device = embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice::new(spi_bus, nss);


        // Create interface
        info!("Creating Interface");
        let interface = GenericSx127xInterfaceVariant::new(
            reset,
            dio1,
            core::option::Option::None,
            core::option::Option::None,
        ).unwrap();

        // Configure radio
        let config = Config {
            chip: Sx1276,
            tcxo_used: false,
            tx_boost: false,
            rx_boost: false,
        };

        let sx127x = Sx127x::new(spi_device, interface, config);
        
        // Initialize LoRa driver
        let mut driver = match LoRaPhy::new(sx127x, false, delay).await {
            Ok(d) => d,
            Err(err) => {
                error!("LoRa initialization error: {:?}", err);
                return Err(anyhow::anyhow!("LoRa initialization failed: {:?}", err));
            }
        };

        let modulation = match driver.create_modulation_params(
            SpreadingFactor::_10,
            Bandwidth::_250KHz,
            CodingRate::_4_8,
            LORA_FREQUENCY_IN_HZ,
        ) {
            Ok(mp) => mp,
            Err(err) => {
                error!("Modulation params error: {:?}", err);
                return Err(anyhow::anyhow!("Failed to create modulation params: {:?}", err));
            }
        };

        let receiving_buffer = [0u8; 256];
        let packet_params = match driver.create_rx_packet_params(
            4, false, receiving_buffer.len() as u8, true, false, &modulation
        ) {
            Ok(pp) => pp,
            Err(err) => {
                error!("Packet params error: {:?}", err);
                return Err(anyhow::anyhow!("Failed to create packet params: {:?}", err));
            }
        };

        Ok(Lora { 
            driver, 
            modulation, 
            packet_params, 
            //buffer: receiving_buffer 
        })
    }

    pub async fn send(&mut self, payload: &[u8]) -> Result<(), RadioError> {
        let _ = self.driver.prepare_for_tx(&self.modulation, &mut self.packet_params, 17, payload).await;
        self.driver.tx().await
    }

    pub async fn receive(&mut self, buffer: &mut [u8]) -> Result<(u8, lora_phy::mod_params::PacketStatus), RadioError> {
        let _ = self.driver.prepare_for_rx(RxMode::Continuous, &self.modulation, &self.packet_params).await;
        self.driver.rx(&mut self.packet_params, buffer).await
    }
}