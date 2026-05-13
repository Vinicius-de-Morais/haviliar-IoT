#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::fmt::Write;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use embassy_executor::Spawner;
use embassy_net::{Runner, Stack, tcp::TcpSocket};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Receiver, Sender},
};
use embassy_time::{Duration, Instant, Timer, WithTimeout};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_println::logger::init_logger;
use haviliar_iot::{
    controller::{lora::LoraController, mqtt::MqttController}, factory::lora_factory::LoraFactory, hal::{
        lora::PAYLOAD_LENGTH, peripheral_manager::PeripheralManagerStatic, servo_motor::ServoMotor, wifi::Wifi
    }, protocol::{lora::LoraEnvelope, message_type::MessageType}
};
use log::*;
use esp_wifi::wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

const HEAP_SIZE: usize = 64 * 1024;
static mut HEAP: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];

const LORA_RX_POLL_MS: u64 = 5000;

struct GatewayConfig {
    broker_ip: embassy_net::Ipv4Address,
    broker_port: u16,
    main_topic: &'static str,
    client_id: &'static str,
    status_subtopic: &'static str,
}

const GATEWAY_CONFIG: GatewayConfig = GatewayConfig {
    broker_ip: embassy_net::Ipv4Address::new(10, 43, 53, 199),
    broker_port: 1883,
    main_topic: "esp32-haviliar",
    client_id: "esp32-lora-gateway-dev",
    status_subtopic: "lora/open",
};

type ForwardToLoraChannel = Channel<CriticalSectionRawMutex, LoraEnvelope, 8>;
type LoraToMqttChannel = Channel<CriticalSectionRawMutex, LoraEnvelope, 8>;

static FORWARD_TO_LORA_CHANNEL: StaticCell<ForwardToLoraChannel> = StaticCell::new();
static LORA_TO_MQTT_CHANNEL: StaticCell<LoraToMqttChannel> = StaticCell::new();
static STACK_CELL: StaticCell<Stack<'static>> = StaticCell::new();

fn wifi_is_connected() -> bool {
    matches!(esp_wifi::wifi::wifi_state(), WifiState::StaConnected)
}

fn has_ip(stack: &Stack<'_>) -> bool {
    stack.config_v4().is_some()
}

#[embassy_executor::task]
async fn task_wifi_manager(mut controller: WifiController<'static>, ssid: &'static str, password: &'static str) {
    loop {
        if wifi_is_connected() {
            controller.wait_for_event(WifiEvent::StaDisconnected).await;
            warn!("WiFi disconnected, waiting before reconnect...");
            Timer::after(Duration::from_secs(2)).await;
            continue;
        }

        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: ssid.try_into().unwrap(),
                password: password.try_into().unwrap(),
                ..Default::default()
            });
            if let Err(e) = controller.set_configuration(&client_config) {
                error!("Falha ao configurar WiFi: {:?}", e);
                Timer::after(Duration::from_secs(5)).await;
                continue;
            }
            if let Err(e) = controller.start_async().await {
                error!("Falha ao iniciar WiFi: {:?}", e);
                Timer::after(Duration::from_secs(5)).await;
                continue;
            }
            info!("WiFi iniciado");
        }

        match controller.connect_async().await {
            Ok(_) => info!("WiFi conectado"),
            Err(e) => {
                error!("Falha na conexao WiFi: {:?}", e);
            }
        }

        Timer::after(Duration::from_millis(500)).await;
    }
}

#[embassy_executor::task]
async fn task_net(mut runner: Runner<'static, WifiDevice<'static>>) {
    let _ = runner.run().await;
}

#[embassy_executor::task]
async fn task_lora_gateway(
    mut lora: LoraController,
    forward_channel: &'static ForwardToLoraChannel,
    result_channel: &'static LoraToMqttChannel,
    mut servo_motor: ServoMotor,
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
                        if let Some(pending) = &pending_forward {
                            if pending.seq == envelope.seq {
                                let result = LoraEnvelope::new(MessageType::Reply, pending.seq, envelope.timestamp_ms, 0, b"LoRa forward ACK received".as_slice().to_vec());
                                lora.send_message_envelope(&result).await.ok();
                                result_tx.send(result).await;
                                
                                pending_forward = Some(LoraEnvelope::new(MessageType::Reply, pending.seq, pending.request_id, envelope.timestamp_ms, 0, b"LoRa forward ACK received".as_slice().to_vec()));
                            } else {
                                warn!("ACK recebido com seq {} mas pending_forward tem seq {}", envelope.seq, pending.seq);
                            }
                        } else {
                            warn!("ACK recebido com seq {} mas nao temos nenhum forward pendente", envelope.seq);
                        }
                    }
                    MessageType::Open => {
                        if seen_seqs.contains(envelope.request_id) {
                            warn!("LoRa duplicata rejeitada: request_id={}", envelope.request_id);
                            let ack = LoraEnvelope::new(MessageType::Ack, envelope.seq, envelope.request_id, envelope.timestamp_ms, 0, b"DUP_ACK".as_slice().to_vec());
                            lora.send_message_envelope(&ack).await.ok();
                        } else {
                            seen_seqs.insert(envelope.request_id);
                            servo_motor.open().ok();

                            Timer::after(Duration::from_secs(5)).await;

                            servo_motor.close().ok();

                            let ack = LoraEnvelope::new(MessageType::Ack, envelope.seq, envelope.request_id, envelope.timestamp_ms, 0, b"ACK".as_slice().to_vec());
                            lora.send_message_envelope(&ack).await.ok();
                            pending_forward = Some(ack);
                        }
                        servo_motor.open().ok();
                        let ack = LoraEnvelope::new(MessageType::Ack, envelope.seq, envelope.timestamp_ms, 0, b"ACK".as_slice().to_vec());
                        lora.send_message_envelope(&ack).await.ok();
                        pending_forward = Some(ack);
                    }
                    MessageType::Reply => {
                        let result = LoraEnvelope::new(MessageType::Reply, envelope.seq, envelope.request_id, envelope.timestamp_ms, 0, b"LoRa forward Reply received".as_slice().to_vec());
                                lora.send_message_envelope(&result).await.ok();
                                result_tx.send(result).await;

                        pending_forward = None;
                    }
                    _ => {}
                }
            }
            Ok(Err(e)) => {
                error!("Erro de radio ao receber LoRa: {:?}", e);
                Timer::after_millis(25).await;
            }
            Err(_) => {
                // Timeout esperado para alternar com fila de forward.
            }
        }

        match pending_forward {
            Some(ref pending) => {
                let payload_copy = pending.payload.clone();
                info!("Reenviando mensagem pendente para LoRa: seq={}, bytes={}", pending.seq, payload_copy.len());
                lora.send_message(pending.msg_type, pending.seq, pending.timestamp_ms, pending.elapsed_ms, payload_copy.as_slice()).await.ok();
            }
            None => {
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
#[allow(static_mut_refs)]
async fn task_mqtt_ingress(
    stack: &'static Stack<'static>,
    sender: Sender<'static, CriticalSectionRawMutex, LoraEnvelope, 8>,
) {
    let mut request_id: u32 = 0;
    let mut seq: u16 = 1;
    static mut RX_BUF: [u8; 4096] = [0u8; 4096];
    static mut TX_BUF: [u8; 4096] = [0u8; 4096];
    static mut MQTT_CLIENT_RX: [u8; 1024] = [0u8; 1024];
    static mut MQTT_CLIENT_TX: [u8; 1024] = [0u8; 1024];
    let mut client: Option<MqttController<'static>> = None;

    loop {
        if !wifi_is_connected() || !has_ip(stack) {
            Timer::after(Duration::from_secs(2)).await;
            continue;
        }

        if client.is_none() {
            info!("MQTT ingress: conectando ao broker...");
            let rx_buf = unsafe { &mut RX_BUF };
            let tx_buf = unsafe { &mut TX_BUF };
            let mut socket = TcpSocket::new(*stack, rx_buf, tx_buf);
            socket.set_timeout(Some(Duration::from_secs(60)));
            if socket
                .connect((GATEWAY_CONFIG.broker_ip, GATEWAY_CONFIG.broker_port))
                .await
                .is_err()
            {
                error!("MQTT ingress: falha no TCP connect");
                Timer::after(Duration::from_secs(5)).await;
                continue;
            }
            let mqtt_rx = unsafe { &mut MQTT_CLIENT_RX };
            let mqtt_tx = unsafe { &mut MQTT_CLIENT_TX };
            match MqttController::new(
                socket,
                mqtt_rx,
                mqtt_tx,
                GATEWAY_CONFIG.main_topic,
                GATEWAY_CONFIG.client_id,
                GATEWAY_CONFIG.main_topic,
            )
            .await
            {
                Ok(c) => {
                    info!("MQTT ingress: conectado e pronto");
                    client = Some(c);
                }
                Err(e) => {
                    error!("MQTT ingress: falha ao conectar ao broker: {:?}", e);
                    Timer::after(Duration::from_secs(5)).await;
                    continue;
                }
            }
        }

        match client.as_mut().unwrap().receive_message().await {
            Ok(Some((_topic, payload))) => {
    sender: &Sender<'a, CriticalSectionRawMutex, LoraEnvelope, 8>,
    request_id: &mut u16,
    seq: &mut u16,
    rx_buf: &'a mut [u8],
    tx_buf: &'a mut [u8],
) {
    let mut socket = TcpSocket::new(*stack, rx_buf, tx_buf);
    socket.set_timeout(Some(Duration::from_secs(60)));

    info!("MQTT ingress: conectando ao broker...");
    if socket.connect((GATEWAY_CONFIG.broker_ip, GATEWAY_CONFIG.broker_port)).await.is_err() {
        error!("MQTT ingress: falha no TCP connect");
        return;
    }

    let mut client = match MqttController::new(socket, GATEWAY_CONFIG.main_topic, GATEWAY_CONFIG.client_id, GATEWAY_CONFIG.main_topic).await {
        Ok(c) => c,
        Err(e) => {
            error!("MQTT ingress: falha ao conectar ao broker: {:?}", e);
            return;
        }
    };

    info!("MQTT ingress: conectado e pronto");

    loop {
        match client.receive_message().await {
            Ok((_topic, payload)) => {
                let mut payload_copy = heapless::Vec::<u8, PAYLOAD_LENGTH>::new();
                if payload_copy.extend_from_slice(payload).is_err() {
                    error!("Payload MQTT maior que o limite LoRa ({} bytes)", PAYLOAD_LENGTH);
                    continue;
                }

                let now = Instant::now();
                let timestamp_ms = now.as_millis().min(u32::MAX as u64) as u32;

                let envelope = LoraEnvelope::new(MessageType::Open, seq, request_id, timestamp_ms, 0, payload_copy.clone().to_vec());
                let envelope = LoraEnvelope::new(MessageType::Open, *seq, timestamp_ms, 0, payload_copy.clone().to_vec());
                sender.send(envelope).await;

                info!(
                    "MQTT->LoRa enfileirado: request_id={}, seq={}, bytes={}",
                    request_id, seq, payload_copy.len()
                );

                request_id = request_id.wrapping_add(1);
                seq = seq.wrapping_add(1);
            }
            Ok(None) => {}
            Err(e) => {
                warn!("MQTT ingress: conexao perdida, reconectando... {:?}", e);
                client = None;
                Timer::after(Duration::from_secs(2)).await;
            }
        }

        Timer::after_millis(100).await;
    }
}

#[embassy_executor::task]
#[allow(static_mut_refs)]
async fn task_mqtt_ingress(
    stack: &'static Stack<'static>,
    sender: Sender<'static, CriticalSectionRawMutex, LoraEnvelope, 8>,
) {
    let mut request_id: u16 = 0;
    let mut seq: u16 = 1;
    static RX_BUF: StaticCell<[u8; 4096]> = StaticCell::new();
    static TX_BUF: StaticCell<[u8; 4096]> = StaticCell::new();
    let rx_buf = RX_BUF.init([0u8; 4096]);
    let tx_buf = TX_BUF.init([0u8; 4096]);

    loop {
        if !wifi_is_connected() || !has_ip(stack) {
            Timer::after(Duration::from_secs(2)).await;
            continue;
        }

        mqtt_ingress_session(stack, &sender, &mut request_id, &mut seq, rx_buf, tx_buf).await;

        Timer::after(Duration::from_secs(5)).await;
    }
}

#[embassy_executor::task]
async fn task_mqtt_egress(
    stack: &'static Stack<'static>,
    receiver: Receiver<'static, CriticalSectionRawMutex, LoraEnvelope, 8>,
) {
    static mut EGRESS_RX_BUF: [u8; 4096] = [0u8; 4096];
    static mut EGRESS_TX_BUF: [u8; 4096] = [0u8; 4096];
    static mut MQTT_CLIENT_RX_EGRESS: [u8; 1024] = [0u8; 1024];
    static mut MQTT_CLIENT_TX_EGRESS: [u8; 1024] = [0u8; 1024];
    let mut client: Option<MqttController<'static>> = None;

    loop {
        let result = receiver.receive().await;

        if !wifi_is_connected() || !has_ip(stack) {
            warn!("MQTT egress: WiFi indisponivel, aguardando...");
            continue;
        }

        if client.is_none() {
            info!("MQTT egress: conectando ao broker...");
            let rx_buf = unsafe { &mut EGRESS_RX_BUF };
            let tx_buf = unsafe { &mut EGRESS_TX_BUF };
            let mut socket = TcpSocket::new(*stack, rx_buf, tx_buf);
            socket.set_timeout(Some(Duration::from_secs(60)));
            if socket
                .connect((GATEWAY_CONFIG.broker_ip, GATEWAY_CONFIG.broker_port))
                .await
                .is_err()
            {
                error!("MQTT egress: falha no TCP connect");
                Timer::after(Duration::from_secs(5)).await;
                continue;
            }
            let mqtt_rx = unsafe { &mut MQTT_CLIENT_RX_EGRESS };
            let mqtt_tx = unsafe { &mut MQTT_CLIENT_TX_EGRESS };
            match MqttController::new(
                socket,
                mqtt_rx,
                mqtt_tx,
                GATEWAY_CONFIG.main_topic,
                GATEWAY_CONFIG.client_id,
                GATEWAY_CONFIG.main_topic,
            )
            .await
            {
                Ok(c) => {
                    client = Some(c);
                }
                Err(e) => {
                    error!("MQTT egress: falha ao conectar ao broker: {:?}", e);
                    Timer::after(Duration::from_secs(5)).await;
                    continue;
                }
            }
        }

        let mut payload = heapless::String::<128>::new();
        payload.clear();
        write!(payload, "ACK seq={}", result.seq).ok();

        match client
            .as_mut()
            .unwrap()
            .publish_message(GATEWAY_CONFIG.status_subtopic, payload.as_bytes())
            .await
        {
            Ok(()) => info!("MQTT egress: status publicado"),
            Err(e) => {
                error!("MQTT egress: falha ao publicar: {:?}", e);
                client = None;
                Timer::after(Duration::from_secs(2)).await;
            }
        }
    }
}

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    unsafe {
        esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
            addr_of_mut!(HEAP) as *mut u8,
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

    let stack = STACK_CELL.init(stack);

    let _ = spawner.spawn(task_wifi_manager(wifi_controller, ssid, password));
    let _ = spawner.spawn(task_net(runner));

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
    let _ = spawner.spawn(task_mqtt_ingress(stack, forward_channel.sender()));
    let _ = spawner.spawn(task_mqtt_egress(stack, result_channel.receiver()));

    info!("Gateway iniciado - LoRa ativo, WiFi/MQTT geridos independentemente");

    loop {
        Timer::after_secs(60).await;
    }
}
