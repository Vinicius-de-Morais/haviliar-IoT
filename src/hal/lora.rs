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
use embassy_sync::mutex::Mutex as AsyncMutex;


const LORA_FREQUENCY_IN_HZ: u32 = 903_900_000;
pub const PAYLOAD_LENGTH: usize = 255;

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
    rx_packet_params: PacketParams,
    tx_packet_params: PacketParams,
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

        //let receiving_buffer = [0u8; PAYLOAD_LENGTH];
        let rx_packet_params = match driver.create_rx_packet_params(
            4, 
            false, 
            PAYLOAD_LENGTH as u8, 
            true, 
            false, 
            &modulation
        ) {
            Ok(pp) => pp,
            Err(err) => {
                error!("Packet params error: {:?}", err);
                return Err(anyhow::anyhow!("Failed to create packet params: {:?}", err));
            }
        };

        let tx_packet_params = match driver.create_tx_packet_params(
            4, 
            false, 
            true, 
            false, 
            &modulation
        ){
            Ok(pp) => pp,
            Err(err) => {
                error!("Packet params error: {:?}", err);
                return Err(anyhow::anyhow!("Failed to create packet params: {:?}", err));
            }
        };

        driver.init().await.unwrap();

        Ok(Lora { 
            driver, 
            modulation, 
            rx_packet_params, 
            tx_packet_params,
            //buffer: receiving_buffer 
        })
    }

    pub async fn send(&mut self, payload: &[u8]) -> Result<(), RadioError> {
        match self.driver.prepare_for_tx(&self.modulation, &mut self.tx_packet_params, 20, payload).await {
            Ok(()) => {
                self.driver.tx().await
            },
            Err(e) => {
                error!("Failed to prepare for TX: {:?}", e);
                return Err(e);
            }
        }
    }

    pub async fn receive(&mut self, buffer: &mut [u8]) -> Result<(u8, lora_phy::mod_params::PacketStatus), RadioError> {
        match self.driver.prepare_for_rx(RxMode::Continuous, &self.modulation, &self.rx_packet_params).await {
            Ok(()) => {
                self.driver.rx(&mut self.rx_packet_params, buffer).await
            },
            Err(e) => {
                error!("Failed to prepare for RX: {:?}", e);
                return Err(e);
            }
        }
    }

    pub async fn receive_from_mutex(
        lora: &'static AsyncMutex<CriticalSectionRawMutex, Lora<'static>>, 
        buffer: &mut [u8]
    ) -> Result<(u8, lora_phy::mod_params::PacketStatus), RadioError> {
        let mut lora_ref  = lora.lock().await;

        match lora_ref.receive(buffer).await {
            Ok((length, status)) => {
                let received_data = &buffer[..length as usize];

                let status_type_name = core::any::type_name_of_val(&status);
                info!("Received LoRa message (len {}): {:?}, status type: {}", length, received_data, status_type_name);

                return Ok((length, status));
            }
            Err(e) => {
                return Err(e);
            }
        }
    }


    pub async fn send_from_mutex(
        lora: &'static AsyncMutex<CriticalSectionRawMutex, Lora<'static>>, 
        payload: &mut [u8]
    ) -> Result<(), RadioError> {
        let mut lora_ref  = lora.lock().await;
            match lora_ref.send(&payload).await {
                Ok(()) => {
                    info!("LoRa message sent successfully from mutex");
                    return Ok(());
                }
                Err(e) => {
                    error!("Failed to send LoRa message from mutex: {:?}", e);
                    return Err(e);
                }
            }
    }
}