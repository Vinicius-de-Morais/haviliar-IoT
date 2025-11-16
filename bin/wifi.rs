#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::{raw::CriticalSectionRawMutex}, channel::{Channel, Receiver}};
use embassy_time::Timer;
use esp_backtrace as _;
use esp_println::logger::init_logger;
use haviliar_iot::{
    hal::{peripheral_manager::PeripheralManagerStatic, servo_motor::ServoMotor, wifi::Wifi},
};
use log::*;
use esp_hal::{clock::CpuClock};
use static_cell::StaticCell;
use esp_alloc as _;
use esp_wifi::{
    wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState}
};
use embassy_net::{
    tcp::TcpSocket,
    Runner
};
use embassy_time::{Duration};
use rust_mqtt::{
    client::{client::MqttClient, client_config::ClientConfig},
    utils::rng_generator::CountingRng,
};

esp_bootloader_esp_idf::esp_app_desc!();

//static WIFI: StaticCell<AsyncMutex<CriticalSectionRawMutex, Wifi>> = StaticCell::new();
//static WIFI: StaticCell<Wifi> = StaticCell::new();
//static WIFI_CONTROLLER: StaticCell<WifiController<'static>> = StaticCell::new();
//static WIFI_RUNNER: StaticCell<Runner<'static, WifiDevice<'static>>> = StaticCell::new();
static SERVO_CHANNEL: StaticCell<Channel<CriticalSectionRawMutex, i16, 4>> = StaticCell::new();


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

#[embassy_executor::task]
async fn servo_task(receiver: Receiver<'static, CriticalSectionRawMutex, i16, 4>, mut servo: ServoMotor) {
    
    info!("Servo task started");
    // Small async test sequence (await-able)
    info!("Running quick GPIO17 (LEDC) test...");
    for i in 0..3 {

        info!("Test cycle {}: MIN_DUTY", i);
        servo.set_angle(255);
        Timer::after(Duration::from_millis(400)).await;
        
        info!("Test cycle {}: MAX_DUTY", i);
        servo.set_angle(0);
        Timer::after(Duration::from_millis(400)).await;
    }
    info!("GPIO17 (LEDC) test finished. If servo doesn't move, check wiring/power.");


    loop {
        match receiver.try_receive() {
            Ok(message) => {
                let angle = message;
                info!("Servo requested angle: {}", angle);
        
                servo.set_angle(angle);
        
                info!("Servo set to angle: {}", angle);
                Timer::after(Duration::from_millis(10)).await;
            }
            Err(e) => {
                error!("Failed to receive from Channel: {:?}", e);
            }
        }
        Timer::after(Duration::from_millis(600)).await;
    }
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


    // INICIAR SERVO MOTOR
    info!("Initializing Servo Motor...");
    let servo_peripherals = peripheral_manager.take_servo_peripherals().unwrap();
    let servo_motor = ServoMotor::new(servo_peripherals);

    
    // Spawn a task que controla o servo
    let channel = SERVO_CHANNEL.init(Channel::new());
    let sender = channel.sender();
    let receiver = channel.receiver();
    let _ = _spawner.spawn(servo_task(receiver, servo_motor));

    // Main loop
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
        socket.set_timeout(Some(embassy_time::Duration::from_secs(60))); 
        socket.set_keep_alive(Some(embassy_time::Duration::from_secs(30)));


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
        let address = embassy_net::Ipv4Address::new(192, 168, 1, 21);

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
        config.max_packet_size = 255;
        config.keep_alive = 60; // Keep connection alive for 60 seconds
        
        let mut client =
            MqttClient::<_, 5, _>::new(socket, &mut write_buffer, 255, &mut recv_buffer, 255, config);

        info!("Connecting to MQTT broker...");
        info!("Attempting MQTT handshake with broker at {:?}...", remote_endpoint);
        
        match client.connect_to_broker().await {
            Ok(()) => {
                info!("✓ MQTT connected!");
            }
            Err(mqtt_error) => {
                error!("MQTT connect error: {:?}", mqtt_error);
                error!("Waiting 5s before retrying...");
                Timer::after(Duration::from_secs(5)).await;
                continue;
            }
        }
        
        info!("Publishing message to 'esp32/test'...");
        // match client
        //     .send_message(
        //         "esp32/test",
        //         b"Hello from ESP32!",
        //         rust_mqtt::packet::v5::publish_packet::QualityOfService::QoS1,
        //         true,
        //     )
        //     .await
        // {
        //     Ok(()) => {
        //         info!("✓ Message published successfully!");
        //     }
        //     Err(mqtt_error) => {
        //         error!("Publish error: {:?}", mqtt_error);
        //     }
        // }

        match client.subscribe_to_topic("esp32/open").await {
            Ok(()) => {
                info!("✓ Subscribed to topic 'esp32/test' successfully!");
            }
            Err(mqtt_error) => {
                error!("Subscribe error: {:?}", mqtt_error);
            }
        }

        // Process incoming messages for a short duration
        let start_time = embassy_time::Instant::now();
        let processing_duration = Duration::from_secs(10); // Process messages for 10 seconds
        loop {
            if embassy_time::Instant::now() - start_time > processing_duration {
                info!("Finished processing incoming messages.");
                break;
            }

            match client.receive_message().await {
                Ok((topic, payload)) => {
                    // info!(
                    //     "Received message on topic '{}': {}",
                    //     topic,
                    //     core::str::from_utf8(&payload).unwrap_or("<invalid utf8>")
                    // );
                    let msg = core::str::from_utf8(&payload).unwrap_or("<invalid utf8>");
                    info!("Received message on topic '{}': {}", topic, msg);

                    // Tentar parsear como inteiro (ângulo)
                    if let Ok(angle) = msg.trim().parse::<i16>() {
                        // limitar faixa (0..=180)
                        let angle = angle.clamp(0, 180);
                        info!("Setting servo angle to: {}", angle);
                        match sender.try_send(angle) {
                            Ok(_) => info!("Sent angle to servo task"),
                            Err(e) => error!("Failed to send angle to servo task: {:?}", e),
                        }                         
                    } else {
                        info!("Payload não é um inteiro válido para ângulo: '{}'", msg);
                    }
                }
                Err(mqtt_error) => {
                    error!("Receive message error: {:?}", mqtt_error);
                    break;
                }
            }
        }

        // Esperar um pouco antes de desconectar
        Timer::after(Duration::from_secs(3)).await;
        
        info!("Disconnecting and waiting 10s before next cycle...");
        Timer::after(Duration::from_secs(10)).await;
    }
}