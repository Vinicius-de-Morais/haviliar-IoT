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
    factory::lora_factory::LoraFactory,
    controller::mqtt::MqttController,
    hal::{
        lora::{
            Lora, OutgoingMessage,
            PAYLOAD_LENGTH,
        },
        peripheral_manager::PeripheralManagerStatic,
        wifi::Wifi,
    },
    protocol::{lora::LoraEnvelope, message_type::MessageType},
};
use log::*;
use esp_wifi::wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

const HEAP_SIZE: usize = 64 * 1024;
static mut HEAP: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];

// Poll curto para permitir alternar entre RX LoRa e fila de requests vindos do MQTT.
const LORA_RX_POLL_MS: u64 = 250;

struct GatewayConfig {
    broker_ip: embassy_net::Ipv4Address,
    broker_port: u16,
    main_topic: &'static str,
    client_id: &'static str,
    status_subtopic: &'static str,
    forward_ack_timeout_ms: u64,
}

const GATEWAY_CONFIG: GatewayConfig = GatewayConfig {
    broker_ip: embassy_net::Ipv4Address::new(192, 168, 1, 21),
    broker_port: 1883,
    main_topic: "esp32-haviliar",
    client_id: "esp32-lora-gateway-dev",
    status_subtopic: "lora/open",
    forward_ack_timeout_ms: 5_000,
};

struct ForwardRequest {
    request_id: u16,
    expected_ack_seq: u16,
    frame: OutgoingMessage,
    ack_timeout_ms: u64,
}

#[derive(Clone, Copy, Debug)]
enum ForwardResultKind {
    AckReceived,
    AckTimeout,
    SendError,
}

#[derive(Clone, Copy, Debug)]
struct ForwardResult {
    request_id: u16,
    seq: u16,
    kind: ForwardResultKind,
    elapsed_ms: u32,
}

struct PendingForward {
    request: ForwardRequest,
    sent_at: Instant,
}

type ForwardToLoraChannel = Channel<CriticalSectionRawMutex, ForwardRequest, 8>;
type LoraToMqttChannel = Channel<CriticalSectionRawMutex, ForwardResult, 8>;

static FORWARD_TO_LORA_CHANNEL: StaticCell<ForwardToLoraChannel> = StaticCell::new();
static LORA_TO_MQTT_CHANNEL: StaticCell<LoraToMqttChannel> = StaticCell::new();
static RX_BUFFER_CELL: StaticCell<[u8; 4096]> = StaticCell::new();
static TX_BUFFER_CELL: StaticCell<[u8; 4096]> = StaticCell::new();
static SOCKET_CELL: StaticCell<TcpSocket<'static>> = StaticCell::new();
static MQTT_CLIENT_CELL: StaticCell<Mutex<CriticalSectionRawMutex, MqttController<'static>>> =
    StaticCell::new();

fn encode_forward_frame(seq: u16, timestamp_ms: u32, payload: &[u8]) -> Option<OutgoingMessage> {
    let envelope = LoraEnvelope::new(MessageType::Reply, seq, timestamp_ms, 0, payload.into());
    OutgoingMessage::new(&envelope)
}

fn result_kind_to_str(kind: ForwardResultKind) -> &'static str {
    match kind {
        ForwardResultKind::AckReceived => "ack_received",
        ForwardResultKind::AckTimeout => "ack_timeout",
        ForwardResultKind::SendError => "send_error",
    }
}

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
    mut lora: Lora<'static>,
    forward_channel: &'static ForwardToLoraChannel,
    result_channel: &'static LoraToMqttChannel,
) {
    let forward_rx = forward_channel.receiver();
    let result_tx = result_channel.sender();
    let mut pending_forward: Option<PendingForward> = None;

    loop {
        let mut recv_buffer = [0u8; PAYLOAD_LENGTH];
        let rx_result = lora
            .receive(&mut recv_buffer)
            .with_timeout(Duration::from_millis(LORA_RX_POLL_MS))
            .await;

        match rx_result {
            Ok(Ok((len, _status))) => {
                let len_usize = len as usize;
                if len_usize > 0 {
                    let received_payload = &recv_buffer[..len_usize];

                    // if let Some(decoded) = decode_protocol_message(received_payload) {
                    //     match decode_protocol_payload_utf8(&decoded) {
                    //         Ok(text) => {
                    //             info!(
                    //                 "LoRa RX: seq={}, ts={}, payload='{}'",
                    //                 decoded.seq, decoded.timestamp_ms, text
                    //             );
                    //         }
                    //         Err(_) => {
                    //             info!(
                    //                 "LoRa RX: seq={}, ts={}, payload(bin)",
                    //                 decoded.seq, decoded.timestamp_ms
                    //             );
                    //         }
                    //     }

                    //     if let Some(ref pending) = pending_forward {
                    //         if decoded.seq == pending.request.expected_ack_seq {
                    //             let elapsed_ms =
                    //                 (Instant::now() - pending.sent_at).as_millis().min(u32::MAX as u64)
                    //                     as u32;

                    //             let result = ForwardResult {
                    //                 request_id: pending.request.request_id,
                    //                 seq: pending.request.expected_ack_seq,
                    //                 kind: ForwardResultKind::AckReceived,
                    //                 elapsed_ms,
                    //             };

                    //             if result_tx.try_send(result).is_err() {
                    //                 error!("Fila LORA->MQTT cheia: nao foi possivel publicar AckReceived");
                    //             }

                    //             pending_forward = None;
                    //         }
                    //     }
                    // }
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

        if let Some(ref pending) = pending_forward {
            let elapsed_ms_u64 = (Instant::now() - pending.sent_at).as_millis();
            if elapsed_ms_u64 >= pending.request.ack_timeout_ms {
                let result = ForwardResult {
                    request_id: pending.request.request_id,
                    seq: pending.request.expected_ack_seq,
                    kind: ForwardResultKind::AckTimeout,
                    elapsed_ms: elapsed_ms_u64.min(u32::MAX as u64) as u32,
                };

                if result_tx.try_send(result).is_err() {
                    error!("Fila LORA->MQTT cheia: nao foi possivel publicar AckTimeout");
                }

                pending_forward = None;
            }
        }

        if pending_forward.is_none() {
            if let Ok(request) = forward_rx.try_receive() {
                let payload = &request.frame.payload[..request.frame.len];
                match lora.send(payload).await {
                    Ok(()) => {
                        info!(
                            "Forward enviado para LoRa: request_id={}, seq={}",
                            request.request_id, request.expected_ack_seq
                        );
                        pending_forward = Some(PendingForward {
                            request,
                            sent_at: Instant::now(),
                        });
                    }
                    Err(e) => {
                        error!(
                            "Falha no forward para LoRa: request_id={}, erro={:?}",
                            request.request_id, e
                        );

                        let result = ForwardResult {
                            request_id: request.request_id,
                            seq: request.expected_ack_seq,
                            kind: ForwardResultKind::SendError,
                            elapsed_ms: 0,
                        };

                        if result_tx.try_send(result).is_err() {
                            error!("Fila LORA->MQTT cheia: nao foi possivel publicar SendError");
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
    sender: Sender<'static, CriticalSectionRawMutex, ForwardRequest, 8>,
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
                if let Some(frame) = encode_forward_frame(seq, timestamp_ms, payload_copy.as_slice()) {
                    let request = ForwardRequest {
                        request_id,
                        expected_ack_seq: seq,
                        frame,
                        ack_timeout_ms: GATEWAY_CONFIG.forward_ack_timeout_ms,
                    };

                    drop(mqtt_controller);
                    sender.send(request).await;

                    info!(
                        "MQTT->LoRa enfileirado: request_id={}, seq={}, bytes={}",
                        request_id,
                        seq,
                        payload_copy.len()
                    );

                    request_id = request_id.wrapping_add(1);
                    seq = seq.wrapping_add(1);
                } else {
                    error!("Falha ao codificar payload MQTT para forward LoRa");
                }
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
    receiver: Receiver<'static, CriticalSectionRawMutex, ForwardResult, 8>,
) {
    let mut payload = heapless::String::<128>::new();

    loop {
        let result = receiver.receive().await;
        payload.clear();
        let _ = write!(
            &mut payload,
            "request_id={},seq={},status={},elapsed_ms={}",
            result.request_id,
            result.seq,
            result_kind_to_str(result.kind),
            result.elapsed_ms
        );

        let mut mqtt_controller = mqtt_controller_mutex.lock().await;
        match mqtt_controller
            .publish_message(GATEWAY_CONFIG.status_subtopic, payload.as_bytes())
            .await
        {
            Ok(()) => {}
            Err(e) => error!("Falha ao publicar status no MQTT: {:?}", e),
        }

        info!(
            "LoRa->MQTT resultado: request_id={}, seq={}, status={:?}, elapsed={} ms",
            result.request_id, result.seq, result.kind, result.elapsed_ms
        );
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

    let forward_channel = FORWARD_TO_LORA_CHANNEL.init(Channel::new());
    let result_channel = LORA_TO_MQTT_CHANNEL.init(Channel::new());

    let _ = spawner.spawn(task_lora_gateway(lora, forward_channel, result_channel));
    let _ = spawner.spawn(task_mqtt_ingress(mqtt_controller_mutex, forward_channel.sender()));
    let _ = spawner.spawn(task_mqtt_egress(mqtt_controller_mutex, result_channel.receiver()));

    loop {
        Timer::after_secs(60).await;
    }
}