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
        lora::{
            PAYLOAD_LENGTH,
        },
        peripheral_manager::PeripheralManagerStatic,
        wifi::Wifi,
    }, protocol::{lora::LoraEnvelope, message_type::MessageType}
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
                // aq significa que recebemos uma mensagem LoRa
                // oq significa que teremos que abrir o servomotor e enviar um ACK via lora.
                // pq se a mensagem foi recebida via lora, significa que o dispositivo final nao tem conectividade com o broker MQTT, 
                // entao o ACK tem que ser enviado via LoRa mesmo.

                // Mas eu preciso ainda verificar se a mensagem recebida é um ACK de um forward que eu enviei, ou se é uma nova mensagem vinda de um dispositivo final.
                // Se for um ACK, eu preciso atualizar o pending_forward e enviar o resultado para MQTT
                // Se for uma nova mensagem, eu preciso enviar um ACK de volta para o dispositivo final preencher alguma variavel de "pending ACK".
                // quando o pending_ack for igual sequence da mensagem recebida, eu sei que o dispositivo final recebeu o ACK e pode processar a mensagem normalmente, e entao eu posso enviar o resultado para MQTT.
                    
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
        if pending_forward.is_none() {
            if let Ok(request) = forward_rx.try_receive() {
                // aq significa que recebemos um novo request do MQTT para enviar via LoRa
                // entao eu preciso enviar esse request via LoRa e marcar ele como pending_forward, para esperar o ACK.
                // se o envio falhar, eu preciso enviar o resultado de falha para MQTT imediatamente, sem esperar o ACK.

                match lora.send_message_envelope(&request).await {
                    Ok(()) => {
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

    let forward_channel = FORWARD_TO_LORA_CHANNEL.init(Channel::new());
    let result_channel = LORA_TO_MQTT_CHANNEL.init(Channel::new());

    let _ = spawner.spawn(task_lora_gateway(lora_controller, forward_channel, result_channel));
    let _ = spawner.spawn(task_mqtt_ingress(mqtt_controller_mutex, forward_channel.sender()));
    let _ = spawner.spawn(task_mqtt_egress(mqtt_controller_mutex, result_channel.receiver()));

    loop {
        Timer::after_secs(60).await;
    }
}