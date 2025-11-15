#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::{fmt::Write, task::Context};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::{CriticalSectionMutex, Mutex, raw::CriticalSectionRawMutex}, channel::{Channel, Sender}, mutex::Mutex as AsyncMutex};
use embassy_time::Timer;
use esp_backtrace as _;
use esp_println::logger::init_logger;
use haviliar_iot::{
    factory::{display_factory::DisplayFactory, lora_factory::LoraFactory},
    hal::{display::Display, lora::{Lora, PAYLOAD_LENGTH}, peripheral_manager::PeripheralManagerStatic, wifi::Wifi},
};
use log::*;
use esp_hal::clock::CpuClock;
use static_cell::StaticCell;

// peripherals-related imports
use esp_alloc as _;
use esp_hal::{
    i2c::master::{Config, I2c},
    rng::Rng,
    timer::timg::TimerGroup,
};

use esp_wifi::{
    EspWifiController, init, wifi::{self, ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState}
};

// embassy related imports
use embassy_net::{
    tcp::TcpSocket,
    Runner,
    {dns::DnsQueryType, Config as EmbassyNetConfig, StackResources},
};
use embassy_time::{Duration};

// MQTT related imports
use rust_mqtt::{
    client::{client::MqttClient, client_config::ClientConfig},
    packet::v5::reason_codes::ReasonCode,
    utils::rng_generator::CountingRng,
};

esp_bootloader_esp_idf::esp_app_desc!();

//static WIFI: StaticCell<AsyncMutex<CriticalSectionRawMutex, Wifi>> = StaticCell::new();
static WIFI: StaticCell<Wifi> = StaticCell::new();
static WIFI_CONTROLLER: StaticCell<WifiController<'static>> = StaticCell::new();
static WIFI_RUNNER: StaticCell<Runner<'static, WifiDevice<'static>>> = StaticCell::new();

#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>, ssid: &'static str, password: &'static str) {
    info!("start connection task");
    debug!("Device capabilities: {:?}", controller.capabilities());
    loop {
        match esp_wifi::wifi::wifi_state() {
            WifiState::StaConnected => {
                // wait until we're no longer connected
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                Timer::after(Duration::from_millis(5000)).await;
                continue; // Skip to next iteration after disconnect
            }
            _ => {}
        }
        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: ssid.try_into().unwrap(),
                password: password.try_into().unwrap(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            info!("Starting wifi");
            controller.start_async().await.unwrap();
            info!("Wifi started!");
        }
        info!("About to connect...");

        match controller.connect_async().await {
            Ok(_) => {
                info!("Wifi connected!");
                // Wait here while connected, don't loop immediately
            },
            Err(e) => {
                error!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

// A background task, to process network events - when new packets, they need to processed, embassy-net, wraps smoltcp
#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}

use core::mem::MaybeUninit;

const HEAP_SIZE: usize = 72 * 1024; // 72KB heap
static mut HEAP: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];


#[esp_hal_embassy::main]
async fn main(_spawner: Spawner) {

    unsafe {
        esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
            HEAP.as_mut_ptr() as *mut u8,
            HEAP_SIZE,
            esp_alloc::MemoryCapability::Internal.into(),
        ));
    }
    
    info!("Heap initialized with {} bytes", HEAP_SIZE);

    init_logger(log::LevelFilter::Info);
        
    info!("Initializing ESP32 Wifi...");

    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let peripheral_manager = PeripheralManagerStatic::init(peripherals);

    info!("Taking Wifi Peripherals...");
    let wifi_peripherals = peripheral_manager.take_wifi_peripherals().unwrap();
    let wifi = Wifi::new(wifi_peripherals);
    
    let ssid = wifi.ssid;
    let password = wifi.password;
    
    let (wifi_controller, runner, stack) = wifi.take_components();

    info!("Spawning tasks...");

    let _ = _spawner.spawn(connection(wifi_controller, ssid, password));
    let _ = _spawner.spawn(net_task(runner));
    
    let time_per =  peripheral_manager.time_per();
    esp_hal_embassy::init(time_per.timer0);
    
    info!("Waiting to get IP address...");
    loop {
        if let Some(config) = stack.config_v4() {
            info!("Got IP: {}", config.address); //dhcp IP address
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];

    // Main loop
    let mut counter = 0u32;
    loop {
        // Verificar se ainda temos IP antes de tentar conectar
        if stack.config_v4().is_none() {
            error!("Lost IP address, waiting...");
            Timer::after(Duration::from_secs(5)).await;
            continue;
        }

        let config = stack.config_v4().unwrap();
        info!("Current IP: {} | Gateway: {:?}", config.address, config.gateway);

        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(embassy_time::Duration::from_secs(60))); // Aumentar timeout
        socket.set_keep_alive(Some(embassy_time::Duration::from_secs(30))); // Adicionar keep-alive

        // Alternativa: Usar DNS para broker público (apenas teste)
        // info!("Resolving DNS for test.mosquitto.org...");
        // let address = match stack
        //     .dns_query("test.mosquitto.org", DnsQueryType::A)
        //     .await
        //     .map(|a| a[0])
        // {
        //     Ok(address) => {
        //         info!("DNS resolved to: {:?}", address);
        //         address
        //     },
        //     Err(e) => {
        //         error!("DNS lookup error: {:?}", e);
        //         Timer::after(Duration::from_secs(5)).await;
        //         continue;
        //     }
        // };
        // Use WSL eth0 IP address - check with: ip addr show eth0
        let address = embassy_net::Ipv4Address::new(172, 24, 180, 80);

        let remote_endpoint = (address, 1883);
        info!("Connecting to {:?}...", remote_endpoint);
        
        // Adicionar delay antes de conectar
        Timer::after(Duration::from_millis(100)).await;
        
        let connection = socket.connect(remote_endpoint).await;
        if let Err(e) = connection {
            error!("TCP connect error: {:?}", e);
            error!("Waiting 10 seconds before retry...");
            Timer::after(Duration::from_secs(10)).await;
            continue;
        }
        info!("TCP connected successfully!");

        // Dar tempo para a conexão estabilizar
        Timer::after(Duration::from_millis(500)).await;

        // Use larger buffers for MQTT communication
        let mut recv_buffer = [0; 512];
        let mut write_buffer = [0; 512];

        let mut config: ClientConfig<'_, 5, CountingRng> = ClientConfig::new(
            rust_mqtt::client::client_config::MqttVersion::MQTTv5,
            CountingRng(20000),
        );
        config.add_max_subscribe_qos(rust_mqtt::packet::v5::publish_packet::QualityOfService::QoS1);
        config.add_client_id("esp32-haviliar");
        config.max_packet_size = 512;
        config.keep_alive = 60; // Keep connection alive for 60 seconds
        
        let mut client =
            MqttClient::<_, 5, _>::new(socket, &mut write_buffer, 512, &mut recv_buffer, 512, config);

        info!("Connecting to MQTT broker...");
        info!("Attempting MQTT handshake with broker at {:?}...", remote_endpoint);
        
        match client.connect_to_broker().await {
            Ok(()) => {
                info!("✓ MQTT connected!");
            }
            Err(mqtt_error) => {
                error!("MQTT connect error: {:?}", mqtt_error);
                error!("This could be due to:");
                error!("  1. Broker doesn't support MQTT v5");
                error!("  2. Firewall blocking the connection");
                error!("  3. Broker configuration issue");
                error!("Waiting 5s before retrying...");
                Timer::after(Duration::from_secs(5)).await;
                continue;
            }
        }
        
        info!("Publishing message to 'esp32/test'...");
        match client
            .send_message(
                "esp32/test",
                b"Hello from ESP32!",
                rust_mqtt::packet::v5::publish_packet::QualityOfService::QoS1,
                true,
            )
            .await
        {
            Ok(()) => {
                info!("✓ Message published successfully!");
            }
            Err(mqtt_error) => {
                error!("Publish error: {:?}", mqtt_error);
            }
        }

        // Esperar um pouco antes de desconectar
        Timer::after(Duration::from_secs(3)).await;
        
        info!("Disconnecting and waiting 10s before next cycle...");
        Timer::after(Duration::from_secs(10)).await;
    }
}