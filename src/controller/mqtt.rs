use alloc::format;
use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, WithTimeout};
use log::{error, info};
use rust_mqtt::{client::{client::MqttClient, client_config::ClientConfig}, packet::v5::reason_codes::ReasonCode, utils::rng_generator::CountingRng};
pub struct MqttController<'a>{
    client: MqttClient<'a, TcpSocket<'a>, 5, CountingRng>,
    main_topic: &'static str,
}

// impl MqttController {
//     pub fn new(socket: TcpSocket<'static>, address: Ipv4Addr, main_topic: &'static str, cliend_id: &'static str) -> Self {
//         let mut write_buffer = [0u8; 256];
impl<'a> MqttController<'a> {
    pub async fn new(
        socket: TcpSocket<'a>,
        recv_buffer: &'a mut [u8],
        write_buffer: &'a mut [u8],
        main_topic: &'static str,
        client_id: &'static str,
        subscribe_topic: &'static str,
    ) -> Result<Self, ReasonCode> {
        let mut controller = Self::new_unconnected(socket, recv_buffer, write_buffer, main_topic, client_id);
        controller.connect(Some(subscribe_topic)).await?;
        Ok(controller)
    }

    pub fn new_unconnected(
        socket: TcpSocket<'a>,
        recv_buffer: &'a mut [u8],
        write_buffer: &'a mut [u8],
        main_topic: &'static str,
        client_id: &'static str,
    ) -> Self {
        let mut config: ClientConfig<'_, 5, CountingRng> = ClientConfig::new(
            rust_mqtt::client::client_config::MqttVersion::MQTTv5,
            CountingRng(20000),
        );
        config.add_max_subscribe_qos(rust_mqtt::packet::v5::publish_packet::QualityOfService::QoS1);
        config.add_client_id(client_id);
        let max_packet = core::cmp::min(recv_buffer.len(), write_buffer.len());
        config.max_packet_size = max_packet as u32;

        let client = MqttClient::<_, 5, _>::new(socket, write_buffer, max_packet, recv_buffer, max_packet, config);

        MqttController {
            client,
            main_topic,
        }
    }

    pub async fn connect(&mut self, subscribe_topic: Option<&str>) -> Result<(), ReasonCode> {
        match self.client.connect_to_broker().await {
            Ok(()) => {
                info!("MQTT connected!");
            }
            Err(mqtt_error) => {
                error!("MQTT connect error: {:?}", mqtt_error);
                return Err(mqtt_error);
            }
        }

        if let Some(topic) = subscribe_topic {
            match self.client.subscribe_to_topic(topic).await {
                Ok(()) => {
                    info!("Subscribed to topic '{}'", topic);
                }
                Err(mqtt_error) => {
                    error!("Subscribe error: {:?}", mqtt_error);
                    return Err(mqtt_error);
                }
            }
        }

        Ok(())
    }

    pub async fn receive_message(&mut self) -> Result<Option<(&str, &[u8])>, ReasonCode> {
        let _ = self.send_ping().await;

        match self.client.receive_message().with_timeout(Duration::from_secs(10)).await {
            Ok(result ) => {
                match result {
                    Ok((topic, payload)) => {
                        info!("Received message on topic '{}': {:?}", topic, payload);
                        Ok(Some((topic, payload)))
                    }
                    Err(mqtt_error) => {
                        error!("Receive message error: {:?}", mqtt_error);
                        return Err(mqtt_error);
                    }
                }
            }
            Err(e) => {
                info!("Timeout waiting for MQTT message: {:?}", e);
                Ok(None)
            }
        }
    }

    pub async fn publish_message(&mut self, subtopic: &str, payload: &[u8]) -> Result<(), ReasonCode> {
        let full_topic = format!("{}/{}", self.main_topic, subtopic);
        
        match self
            .client
            .send_message(&full_topic, payload, rust_mqtt::packet::v5::publish_packet::QualityOfService::QoS1, false)
            .await
        {
            Ok(()) => {
                info!("Published message to topic '{}': {:?}", full_topic, payload);
                Ok(())
            }
            Err(rust_mqtt::packet::v5::reason_codes::ReasonCode::NoMatchingSubscribers) => {
                info!("Published message to topic '{}' but no subscribers matched", full_topic);
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