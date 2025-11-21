#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::{CriticalSectionRawMutex, RawMutex}, channel::{Channel, Receiver}, mutex::Mutex};
use embassy_time::Timer;
use esp_backtrace as _;
use esp_println::logger::init_logger;
use haviliar_iot::{
    controller::mqtt::{self, MqttController}, hal::{peripheral_manager::PeripheralManagerStatic, servo_motor::ServoMotor, wifi::Wifi}
};
use log::*;
use esp_hal::{clock::CpuClock};
use static_cell::StaticCell;
use esp_alloc as _;
use esp_wifi::{
    wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState}
};
use embassy_net::{
    Runner, dns::Socket, tcp::TcpSocket
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
static RX_BUFFER_CELL: StaticCell<[u8; 4096]> = StaticCell::new();
static TX_BUFFER_CELL: StaticCell<[u8; 4096]> = StaticCell::new();
static SOCKET_CELL: StaticCell<TcpSocket<'static>> = StaticCell::new();
static MQTT_CLIENT_CELL: StaticCell<Mutex<CriticalSectionRawMutex, MqttController>> = StaticCell::new();

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
    servo.close();

    loop {
        match receiver.try_receive() {
            Ok(message) => {
                let angle = message as u32;
                //info!("Servo requested angle: {}", angle);
                servo.open();
                Timer::after(Duration::from_secs(3)).await;
                servo.close();
            }
            Err(e) => {
                //error!("Failed to receive from Channel: {:?}", e);
            }
        }
        Timer::after(Duration::from_millis(10)).await;
    }
}

#[embassy_executor::task]
async fn watch_mqtt_messages(mqtt_controller_mutex: &'static Mutex<CriticalSectionRawMutex, MqttController<'static>>, sender: embassy_sync::channel::Sender<'static, CriticalSectionRawMutex, i16, 4>) {
    loop {

        let mut mqtt_controller = mqtt_controller_mutex.lock().await;
        match mqtt_controller.receive_message().await {
                Ok((topic, payload)) => {
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
                    //break;
                }
        }
        Timer::after(Duration::from_secs(1)).await;
    }
}

#[embassy_executor::task]
async fn send_ping_task(mqtt_controller: &'static Mutex<CriticalSectionRawMutex, MqttController<'static>>) {
    loop {
        {
            let mut controller = mqtt_controller.lock().await;
            match controller.send_ping().await {
                Ok(()) => {
                    info!("Ping sent successfully");
                }
                Err(mqtt_error) => {
                    error!("Ping error: {:?}", mqtt_error);
                }
            };
        }
        Timer::after(Duration::from_secs(30)).await;
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

    // INICIAR SERVO MOTOR
    info!("Initializing Servo Motor...");
    let servo_peripherals = peripheral_manager.take_servo_peripherals().unwrap();
    let servo_motor = ServoMotor::new(servo_peripherals);

    // Spawn a task que controla o servo
    let channel = SERVO_CHANNEL.init(Channel::new());
    let sender = channel.sender();
    let receiver = channel.receiver();
    let _ = _spawner.spawn(servo_task(receiver, servo_motor));

    loop {
        if stack.config_v4().is_none() {
            error!("Lost IP address, waiting...");
            Timer::after(Duration::from_secs(5)).await;
            continue;
        }

        break;
    }

    let config = stack.config_v4().unwrap();
    info!("Current IP: {} | Gateway: {:?}", config.address, config.gateway);

    let rx_buffer = RX_BUFFER_CELL.init([0; 4096]);
    let tx_buffer = TX_BUFFER_CELL.init([0; 4096]);

    let mut socket = TcpSocket::new(stack, rx_buffer, tx_buffer);
    socket.set_timeout(Some(embassy_time::Duration::from_secs(60))); 
    socket.set_keep_alive(Some(embassy_time::Duration::from_secs(30)));

    let socket = SOCKET_CELL.init(socket);

    let address = embassy_net::Ipv4Address::new(192, 168, 1, 21);
    let remote_endpoint = (address, 1883);

    loop {
        let connection = socket.connect(remote_endpoint).await;
        if let Err(e) = connection {
            error!("TCP connect error: {:?}", e);
            error!("Waiting 10 seconds before retry...");
            Timer::after(Duration::from_secs(1)).await;
            continue;
        }
        info!("TCP connected successfully!");
        break;
    }

    let mqtt_controller = match MqttController::new(socket, "esp32/haviliar", "esp32-haviliar").await {
        Ok(controller) => controller,
        Err(e) => {
            error!("Failed to create MQTT controller: {:?}", e);
            return;
        }
    };

    let mqtt_controller_mutex = MQTT_CLIENT_CELL.init(Mutex::new(mqtt_controller));
    let _ = _spawner.spawn(send_ping_task(mqtt_controller_mutex));
    let _ = _spawner.spawn(watch_mqtt_messages(mqtt_controller_mutex, sender));

    // Main loop
    loop {

        Timer::after(Duration::from_secs(10)).await;
    }
}