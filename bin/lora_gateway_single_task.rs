#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::{fmt::Write, mem::MaybeUninit};

use embassy_executor::Spawner;
use embassy_net::{Runner, tcp::TcpSocket};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Receiver, Sender},
    mutex::Mutex,
};
use embassy_time::{Duration, Instant, Timer, WithTimeout};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_println::logger::init_logger;
use haviliar_iot::{
    controller::{lora::LoraController, mqtt::MqttController}, factory::lora_factory::LoraFactory, hal::{
        lora::PAYLOAD_LENGTH, peripheral_manager::PeripheralManagerStatic, servo_motor::{self, ServoMotor}, wifi::Wifi
    }, protocol::{lora::LoraEnvelope, message_type::MessageType}
};
use log::*;
use esp_wifi::wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState};
use minicbor::decode::info;
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

const HEAP_SIZE: usize = 64 * 1024;
static mut HEAP: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];

// Poll curto para permitir alternar entre RX LoRa e fila de requests vindos do MQTT.
const LORA_RX_POLL_MS: u64 = 5000;

struct GatewayConfig {
    broker_ip: embassy_net::Ipv4Address,
    broker_port: u16,
    main_topic: &'static str,
    client_id: &'static str,
    status_subtopic: &'static str,
    forward_ack_timeout_ms: u64,
}

const GATEWAY_CONFIG: GatewayConfig = GatewayConfig {
    broker_ip: embassy_net::Ipv4Address::new(10, 43, 53, 199),
    broker_port: 1883,
    main_topic: "esp32-haviliar",
    client_id: "esp32-lora-gateway-dev",
    status_subtopic: "lora/open",
    forward_ack_timeout_ms: 5_000,
};


type ForwardToLoraChannel = Channel<CriticalSectionRawMutex, LoraEnvelope, 8>;
type LoraToMqttChannel = Channel<CriticalSectionRawMutex, LoraEnvelope, 8>;

static FORWARD_TO_LORA_CHANNEL: StaticCell<ForwardToLoraChannel> = StaticCell::new();
static LORA_TO_MQTT_CHANNEL: StaticCell<LoraToMqttChannel> = StaticCell::new();
static RX_BUFFER_CELL: StaticCell<[u8; 4096]> = StaticCell::new();
static TX_BUFFER_CELL: StaticCell<[u8; 4096]> = StaticCell::new();
static SOCKET_CELL: StaticCell<TcpSocket<'static>> = StaticCell::new();
static MQTT_CLIENT_CELL: StaticCell<Mutex<CriticalSectionRawMutex, MqttController<'static>>> =
    StaticCell::new();

#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>, ssid: &'static str, password: &'static str) {
    loop {
        if let WifiState::StaConnected = esp_wifi::wifi::wifi_state() {
            controller.wait_for_event(WifiEvent::StaDisconnected).await;
            Timer::after(Duration::from_secs(5)).await;
            continue;
        }

        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: ssid.try_into().unwrap(),
                password: password.try_into().unwrap(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            controller.start_async().await.unwrap();
            info!("WiFi iniciado");
        }

        match controller.connect_async().await {
            Ok(_) => info!("WiFi conectado"),
            Err(e) => {
                error!("Falha na conexao WiFi: {:?}", e);
                Timer::after(Duration::from_secs(5)).await;
            }
        }
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await;
}

#[embassy_executor::task]
async fn task_lora_gateway(
    mut lora: LoraController,
    forward_channel: &'static ForwardToLoraChannel,
    result_channel: &'static LoraToMqttChannel,
    mut servo_motor: ServoMotor
) {
    let forward_rx = forward_channel.receiver();
    let result_tx = result_channel.sender();
    let mut pending_forward: Option<LoraEnvelope> = None;

    loop {
        let mut recv_buffer = [0u8; PAYLOAD_LENGTH];
        let rx_result = lora
            .receive_message(&mut recv_buffer)
            .with_timeout(Duration::from_millis(LORA_RX_POLL_MS))
            .await;

        match rx_result {
            Ok(Ok((envelope, _status))) => {

                match envelope.msg_type {
                    MessageType::Ack => {
                        // aq significa que recebemos um ACK de um forward que enviamos anteriormente, entao precisamos verificar se o seq do ACK corresponde ao pending_forward, e se sim, enviar o resultado para MQTT e limpar o pending_forward.
                        if let Some(pending) = &pending_forward {
                            if pending.seq == envelope.seq {
                                // ACK corresponde ao pending_forward, podemos considerar o forward como bem sucedido e enviar o resultado para MQTT.
                                let result = LoraEnvelope::new(MessageType::Reply, pending.seq, envelope.timestamp_ms, 0, b"LoRa forward ACK received".as_slice().to_vec());

                                lora.send_message_envelope(&result).await.ok(); // confirmar o reply

                                result_tx.send(result).await;
                                pending_forward = None;
                            } else {
                                // ACK recebido, mas seq nao corresponde ao pending_forward. Pode ser um ACK atrasado ou fora de ordem. Ignorar.
                                warn!("ACK recebido com seq {} mas pending_forward tem seq {}", envelope.seq, pending.seq);
                            }
                        } else {
                            // ACK recebido mas nao temos nenhum forward pendente. Pode ser um ACK atrasado ou fora de ordem. Ignorar.
                            warn!("ACK recebido com seq {} mas nao temos nenhum forward pendente", envelope.seq);
                        }
                    }
                    MessageType::Open => {
                        // aq significa que recebemos uma nova mensagem vinda de um dispositivo final, entao precisamos enviar um ACK de volta para o dispositivo final preencher a variavel de "pending ACK", e entao podemos processar a mensagem normalmente e enviar o resultado para MQTT.
                        servo_motor.open().ok(); // abrir o servo motor para simular o processamento da mensagem recebida

                        let ack = LoraEnvelope::new(MessageType::Ack, envelope.seq, envelope.timestamp_ms, 0, b"ACK".as_slice().to_vec());
                        lora.send_message_envelope(&ack).await.ok(); // enviar ACK para dispositivo final

                        pending_forward = Some(ack); // marcar a mensagem recebida como pending_forward para esperar o ACK do dispositivo final

                    }
                    MessageType::Reply => {
                        pending_forward = None;
                    }
                    _ => {
                    }
                }
                    
            }
            Ok(Err(e)) => {
                error!("Erro de radio ao receber LoRa: {:?}", e);
                Timer::after_millis(25).await;
            }
            Err(_) => {
                // Timeout curto esperado para alternar com fila de forward.
            }
        }

        // por enquanto vou so receber novos requests enquanto nao tiver um forward pendente, pq o protocolo atual é "enviar um forward e esperar o ACK antes de enviar outro".
        match pending_forward {
            Some(ref pending) => {
                let payload_copy = pending.payload.clone();
                
                info!("Reenviando mensagem pendente para LoRa: seq={}, bytes={}", pending.seq, payload_copy.len());

                lora.send_message(pending.msg_type, pending.seq, pending.timestamp_ms, pending.elapsed_ms, payload_copy.as_slice()).await.ok();
            } 
            None =>  {
                if let Ok(request) = forward_rx.try_receive() {

                    match lora.send_message_envelope(&request).await {
                        Ok(()) => {
                            info!("LoRa forward enviado: seq={}, bytes={}", request.seq, request.payload.len());
                            pending_forward = Some(request);
                        }
                        Err(e) => {
                            error!("Falha ao enviar mensagem LoRa: {:?}", e);
                            let result = LoraEnvelope::new(MessageType::Reply, request.seq, request.timestamp_ms, 0, b"LoRa send failed".as_slice().to_vec());
                            result_tx.send(result).await;
                        }
                    }
                }
            }
        }
    }
}

#[embassy_executor::task]
async fn task_mqtt_ingress(
    mqtt_controller_mutex: &'static Mutex<CriticalSectionRawMutex, MqttController<'static>>,
    sender: Sender<'static, CriticalSectionRawMutex, LoraEnvelope, 8>,
) {
    let mut request_id: u16 = 0;
    let mut seq: u16 = 1;

    loop {
        let mut mqtt_controller = mqtt_controller_mutex.lock().await;
        
        match mqtt_controller.receive_message().await {
            Ok((_topic, payload)) => {
                let mut payload_copy = heapless::Vec::<u8, PAYLOAD_LENGTH>::new();
                if payload_copy.extend_from_slice(payload).is_err() {
                    error!("Payload MQTT maior que o limite LoRa ({} bytes)", PAYLOAD_LENGTH);
                    drop(mqtt_controller);
                    Timer::after_millis(100).await;
                    continue;
                }

                let now = Instant::now();
                let timestamp_ms = now.as_millis().min(u32::MAX as u64) as u32;
                
                let envelope = LoraEnvelope::new(MessageType::Open, seq, timestamp_ms, 0, payload_copy.clone().to_vec());
                sender.send(envelope).await;
                
                info!(
                    "MQTT->LoRa enfileirado: request_id={}, seq={}, bytes={}",
                    request_id,
                    seq,
                    payload_copy.len()
                );
                
                request_id = request_id.wrapping_add(1);
                seq = seq.wrapping_add(1);
                
                drop(mqtt_controller);
            }
            Err(e) => {
                error!("Erro ao receber mensagem MQTT: {:?}", e);
                drop(mqtt_controller);
            }
        }

        Timer::after_millis(100).await;
    }
}

#[embassy_executor::task]
async fn task_mqtt_egress(
    mqtt_controller_mutex: &'static Mutex<CriticalSectionRawMutex, MqttController<'static>>,
    receiver: Receiver<'static, CriticalSectionRawMutex, LoraEnvelope, 8>,
) {
    let mut payload = heapless::String::<128>::new();

    loop {
        let result = receiver.receive().await;
        payload.clear();

        // aq significa que foi recebido um ack do foward que foi recebido anteriormente via MQTT, ou seja, 
        // o dispositivo final recebeu a solicitação e agora vamos notificar o mqtt disso

        let mut mqtt_controller = mqtt_controller_mutex.lock().await;
        match mqtt_controller
            .publish_message(GATEWAY_CONFIG.status_subtopic, payload.as_bytes())
            .await
        {
            Ok(()) => {}
            Err(e) => error!("Falha ao publicar status no MQTT: {:?}", e),
        }

        
    }
}

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    unsafe {
        esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
            HEAP.as_mut_ptr() as *mut u8,
            HEAP_SIZE,
            esp_alloc::MemoryCapability::Internal.into(),
        ));
    }

    init_logger(log::LevelFilter::Info);

    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let peripheral_manager = PeripheralManagerStatic::init(peripherals);

    let time_per = peripheral_manager.time_per();
    esp_hal_embassy::init(time_per.timer0);

    let wifi_peripherals = peripheral_manager.take_wifi_peripherals().unwrap();
    let wifi = Wifi::new(wifi_peripherals);
    let ssid = wifi.ssid;
    let password = wifi.password;
    let (wifi_controller, runner, stack) = wifi.take_components();

    let _ = spawner.spawn(connection(wifi_controller, ssid, password));
    let _ = spawner.spawn(net_task(runner));

    info!("Aguardando IP DHCP...");
    loop {
        if let Some(config) = stack.config_v4() {
            info!("IP obtido: {}", config.address);
            break;
        }
        Timer::after_millis(500).await;
    }

    let rx_buffer = RX_BUFFER_CELL.init([0; 4096]);
    let tx_buffer = TX_BUFFER_CELL.init([0; 4096]);

    let mut socket = TcpSocket::new(stack, rx_buffer, tx_buffer);
    socket.set_timeout(Some(Duration::from_secs(60)));
    socket.set_keep_alive(Some(Duration::from_secs(30)));

    let socket = SOCKET_CELL.init(socket);
    loop {
        match socket
            .connect((GATEWAY_CONFIG.broker_ip, GATEWAY_CONFIG.broker_port))
            .await
        {
            Ok(()) => {
                info!("TCP conectado ao broker MQTT");
                break;
            }
            Err(e) => {
                error!("Falha no TCP connect com broker MQTT: {:?}", e);
                Timer::after_secs(1).await;
            }
        }
    }

    let mqtt_controller =
        match MqttController::new(socket, GATEWAY_CONFIG.main_topic, GATEWAY_CONFIG.client_id)
            .await
        {
        Ok(controller) => controller,
        Err(e) => {
            error!("Falha ao inicializar MQTT: {:?}", e);
            return;
        }
    };
    let mqtt_controller_mutex = MQTT_CLIENT_CELL.init(Mutex::new(mqtt_controller));

    let lora_peripherals = peripheral_manager.take_lora_peripherals().unwrap();
    let lora = match LoraFactory::create_from_manager(lora_peripherals).await {
        Ok(lora) => lora,
        Err(e) => {
            error!("Falha ao inicializar LoRa: {:?}", e);
            panic!("LoRa initialization failed");
        }
    };
    let lora_controller = LoraController::new(lora);

    let servo_peripherals = peripheral_manager.take_servo_peripherals().unwrap();
    let servo_motor = ServoMotor::new(servo_peripherals);

    let forward_channel = FORWARD_TO_LORA_CHANNEL.init(Channel::new());
    let result_channel = LORA_TO_MQTT_CHANNEL.init(Channel::new());

    let _ = spawner.spawn(task_lora_gateway(lora_controller, forward_channel, result_channel, servo_motor));
    let _ = spawner.spawn(task_mqtt_ingress(mqtt_controller_mutex, forward_channel.sender()));
    let _ = spawner.spawn(task_mqtt_egress(mqtt_controller_mutex, result_channel.receiver()));

    loop {
        Timer::after_secs(60).await;
    }
}