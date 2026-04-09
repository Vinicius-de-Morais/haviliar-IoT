use log::{error, info};
use lora_phy::mod_params::{PacketStatus, RadioError};

use crate::{hal::lora::{Lora, PAYLOAD_LENGTH}, protocol::{lora::{LoraEnvelope, LoraParser}, message_type}};



pub struct LoraController {
    lora: Lora<'static>,
}

impl LoraController {
    pub fn new(lora: Lora<'static>) -> Self {
        Self { lora }
    }

    pub async fn send_message(&mut self, 
        msg_type: message_type::MessageType,
        sequence: u16,
        timestamp_ms: u32,
        elapsed_ms: u32,
        payload: &[u8]
    ) -> Result<(), RadioError> {
        let frame = LoraParser::encode_envelope::<PAYLOAD_LENGTH>(msg_type, sequence, timestamp_ms, elapsed_ms, payload);

        match frame {
            Some(mut outgoing) => {
                let payload = &mut outgoing.payload[..outgoing.len];
                self.lora.send(payload).await
            }
            None => {
                error!("Failed to encode LoRa message: payload too large");
                Err(RadioError::Irq)
            },
        }
    }

    pub async fn send_message_envelope(&mut self, envelope: &LoraEnvelope<'_>) -> Result<(), RadioError> {
        let frame = envelope.into_outgoing();

        match frame {
            Some(mut outgoing) => {
                let payload = &mut outgoing.payload[..outgoing.len];
                self.lora.send(payload).await
            }
            None => {
                error!("Failed to encode LoRa message: payload too large");
                Err(RadioError::Irq)
             },
        }
    }

    pub async fn receive_message<'a>(
        &mut self, 
        recv_buffer: &'a mut [u8]
    ) -> Result<(LoraEnvelope<'a>, PacketStatus), RadioError> {

        match self.lora.receive(recv_buffer).await {
            Ok((len, status)) => {
                let len_usize = len as usize;

                if len_usize > 0 {
                    let received_payload = &recv_buffer[..len_usize];
                    if let Some(decoded) = LoraParser::decode_envelope(received_payload) {
                        match LoraParser::decode_payload_utf8(&decoded) {
                            Ok(text) => {
                                info!(
                                    "Received CBOR message: v={}, type={:?}, seq={}, ts={}, payload='{}'",
                                    decoded.version,
                                    decoded.msg_type,
                                    decoded.seq,
                                    decoded.timestamp_ms,
                                    text
                                );

                                return Ok((decoded, status));
                            },
                            Err(_) => {
                                info!(
                                    "Received CBOR message: v={}, type={:?}, seq={}, ts={}, payload(bytes)={:?}",
                                    decoded.version,
                                    decoded.msg_type,
                                    decoded.seq,
                                    decoded.timestamp_ms,
                                    decoded.payload.as_ref()
                                );

                                return Ok((decoded,status));
                            },
                        }
                    } else {
                        error!("Failed to decode LoRa message: invalid envelope");
                        return Err(RadioError::Irq);
                    }
                }

                return Err(RadioError::Irq);
            }
            Err(e) => {
                error!("Failed to receive LoRa message: {:?}", e);
                return Err(e);
            }
        }
    }
}