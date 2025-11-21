use core::net::Ipv4Addr;

use alloc::format;
use embassy_net::tcp::{TcpSocket, client};
use embassy_time::{Duration, WithTimeout};
use log::{error, info};
use rust_mqtt::{client::{client::MqttClient, client_config::ClientConfig}, packet::v5::reason_codes::ReasonCode, utils::rng_generator::CountingRng};
use static_cell::StaticCell;

static RECV_BUFFER_CELL: StaticCell<[u8; 256]> = StaticCell::new();
static WRITE_BUFFER_CELL: StaticCell<[u8; 256]> = StaticCell::new();

pub struct MqttController<'a>{
    //socket: &'a mut TcpSocket<'a>,
    //address: Ipv4Addr,
    client: MqttClient<'a, &'a mut TcpSocket<'a>, 5, CountingRng>,
    main_topic: &'static str,
    is_connected: bool,
}

// impl MqttController {
//     pub fn new(socket: TcpSocket<'static>, address: Ipv4Addr, main_topic: &'static str, cliend_id: &'static str) -> Self {
//         let mut write_buffer = [0u8; 256];
impl<'a> MqttController<'a> {
    pub async fn new(socket: &'a mut TcpSocket<'a>, main_topic: &'static str, cliend_id: &'static str) -> Result<Self, ReasonCode> {
        
        let recv_buffer = RECV_BUFFER_CELL.init([0u8; 256]);
        let write_buffer = WRITE_BUFFER_CELL.init([0u8; 256]);

        let mut config: ClientConfig<'_, 5, CountingRng> = ClientConfig::new(
            rust_mqtt::client::client_config::MqttVersion::MQTTv5,
            CountingRng(20000),
        );
        config.add_max_subscribe_qos(rust_mqtt::packet::v5::publish_packet::QualityOfService::QoS1);
        config.add_client_id(cliend_id);
        config.max_packet_size = 255;
        //config.keep_alive = 10;

        let mut client = MqttClient::<_, 5, _>::new(socket, write_buffer, 255, recv_buffer, 255, config);

        match client.connect_to_broker().await {
            Ok(()) => {
                info!("✓ MQTT connected!");
            }
            Err(mqtt_error) => {
                error!("MQTT connect error: {:?}", mqtt_error);
                return Err(mqtt_error);
            }
        }
    
        match client.subscribe_to_topic("esp32/open").await {
                Ok(()) => {
                    info!("✓ Subscribed to topic 'esp32/open' successfully!");
                }
                Err(mqtt_error) => {
                    error!("Subscribe error: {:?}", mqtt_error);
                    return Err(mqtt_error);
                }
            }

        Ok(
            MqttController {
                //socket,
                //address,
                client,
                main_topic,
                is_connected: true,
            }
        )
    }

    pub async fn receive_message(&mut self) -> Result<(&str, &[u8]), ReasonCode> {
        let _ = self.send_ping().await;

        match self.client.receive_message().with_timeout(Duration::from_secs(10)).await {
            Ok(result ) => {
                match result {
                    Ok((topic, payload)) => {
                        info!("Received message on topic '{}': {:?}", topic, payload);
                        Ok((topic, payload))
                    }
                    Err(mqtt_error) => {
                        error!("Receive message error: {:?}", mqtt_error);
                        return Err(mqtt_error);
                    }
                }
            }
            Err(e) => {
                error!("Timeout: {:?}", e);
                Err(ReasonCode::MaximumConnectTime)
            }
        }
    }

    pub async fn publish_message(&mut self, subtopic: &str, payload: &[u8]) -> Result<(), ReasonCode> {
        let full_topic = format!("{}/{}", self.main_topic, subtopic);
        
        match self.client.send_message(&full_topic, payload, rust_mqtt::packet::v5::publish_packet::QualityOfService::QoS1, false).await {
            Ok(()) => {
                info!("Published message to topic '{}': {:?}", full_topic, payload);
                Ok(())
            }
            Err(mqtt_error) => {
                error!("Publish message error: {:?}", mqtt_error);
                return Err(mqtt_error);
            }
        }
    }

    pub async fn send_ping(&mut self) -> Result<(), ReasonCode> {
        match self.client.send_ping().await {
            Ok(()) => {
                info!("Ping sent successfully");
                Ok(())
            }
            Err(mqtt_error) => {
                error!("Ping error: {:?}", mqtt_error);
                Err(mqtt_error)
            }
        }
    }

    // pub async fn resolve_dns(){
    //     let address = match stack
    //         .dns_query("test.mosquitto.org", DnsQueryType::A)
    //         .await
    //         .map(|a| a[0])
    //     {
    //         Ok(address) => {
    //             info!("DNS resolved to: {:?}", address);
    //             address
    //         },
    //         Err(e) => {
    //             error!("DNS lookup error: {:?}", e);
    //             Timer::after(Duration::from_secs(5)).await;
    //             continue;
    //         }
    //     };
    // }
}