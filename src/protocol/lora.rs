use minicbor::bytes::{ByteSlice, ByteVec};
use minicbor::encode::write::Cursor;
use minicbor::{Decode, Encode};

use crate::hal::lora::PAYLOAD_LENGTH;
use crate::protocol::message_type::MessageType;

pub const PROTOCOL_VERSION: u8 = 1;
/// Tamanho máximo de payload de aplicação que garante que o envelope CBOR
/// completo caiba dentro de um frame LoRa de PAYLOAD_LENGTH bytes.
///
/// Cálculo (pior caso):
/// - 2 bytes reservados para o prefixo de tamanho do CBOR
/// - ~24 bytes de overhead do envelope (mapa, chaves e campos escalares)
/// - restante para os dados do payload
///
/// 255 (PAYLOAD_LENGTH) - 2 (prefixo) - 24 (overhead) = 229 bytes úteis.
pub const MAX_APP_PAYLOAD: usize = 229;

#[derive(Debug, Encode, Decode)]
pub struct LoraEnvelope {
    #[n(0)]
    pub version: u8,
    #[n(1)]
    pub msg_type: MessageType,
    #[n(2)]
    pub seq: u16,
    #[n(3)]
    pub timestamp_ms: u32,    
    #[n(4)]
    pub elapsed_ms: u32,
    #[n(5)]
    pub payload: ByteVec,
}

impl LoraEnvelope {
    
    pub fn new(
        msg_type: MessageType,
        seq: u16,
        timestamp_ms: u32,
        elapsed_ms: u32,
        payload:  impl Into<ByteVec>,
    ) -> Self {
        LoraEnvelope {
            version: PROTOCOL_VERSION,
            msg_type,
            timestamp_ms,
            elapsed_ms,
            seq,
            payload: payload.into(),
        }
    }

    pub fn new_version(
        version: u8,
        msg_type: MessageType,
        seq: u16,
        timestamp_ms: u32,
        elapsed_ms: u32,
        payload: impl Into<ByteVec>,
    ) -> Self {
        LoraEnvelope {
            version,
            msg_type,
            timestamp_ms,
            elapsed_ms,
            seq,
            payload: payload.into(),
        }
    }

    pub fn into_outgoing(&self) -> Option<OutgoingFrame<PAYLOAD_LENGTH>> {
        OutgoingFrame::new(self)
    }
}

pub struct OutgoingFrame<const N: usize> {
    pub payload: [u8; N],
    pub len: usize,
}

impl<const N: usize> OutgoingFrame<N> {
    pub fn new(msg: &LoraEnvelope) -> Option<Self> {
        if N < 3 {
            return None;
        }

        // Garante que o payload de aplicação não excede o limite calculado
        // para que o envelope CBOR inteiro caiba no frame.
        if msg.payload.len() > MAX_APP_PAYLOAD {
            return None;
        }

        // Reserve first 2 bytes for encoded CBOR length to support fixed-size RF payloads.
        let mut payload = [0u8; N];
        let mut cursor = Cursor::new(&mut payload[2..]);
        minicbor::encode(msg, &mut cursor).ok()?;
        let cbor_len = cursor.position();

        if cbor_len > u16::MAX as usize || cbor_len + 2 > N {
            return None;
        }

        let len_prefix = (cbor_len as u16).to_le_bytes();
        payload[0..2].copy_from_slice(&len_prefix);

        Some(OutgoingFrame {
            payload,
            len: cbor_len + 2,
        })
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.payload[..self.len]
    }
}

pub struct LoraParser;

impl LoraParser {
    pub fn decode_envelope(received: &[u8]) -> Option<LoraEnvelope> {
        if received.len() < 2 {
            return None;
        }
        let declared_len = u16::from_le_bytes([received[0], received[1]]) as usize;
        if declared_len == 0 || declared_len + 2 > received.len() {
            return None;
        }
        let cbor_payload: &[u8] = &received[2..(2 + declared_len)];
        minicbor::decode::<LoraEnvelope>(cbor_payload).ok()
    }

    pub fn decode_payload_utf8(
        message: &LoraEnvelope,
    ) -> core::result::Result<&str, &[u8]> {
        let payload = message.payload.as_ref();
        core::str::from_utf8(payload).map_err(|_| payload)
    }

    pub fn encode_envelope<const N: usize>(
        msg_type: MessageType,
        seq: u16,
        timestamp_ms: u32,
        elapsed_ms: u32,
        payload: impl Into<ByteVec>,
    ) -> Option<OutgoingFrame<N>> {
        let envelope = LoraEnvelope::new(
            msg_type,
            seq,
            timestamp_ms,
            elapsed_ms,
            payload.into(),
        );
        OutgoingFrame::new(&envelope)
    }
}