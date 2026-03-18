use core::fmt::Write;

use heapless::String;
use minicbor::bytes::ByteSlice;
use minicbor::encode::write::Cursor;
use minicbor::{Decode, Encode};

pub const PROTOCOL_VERSION: u8 = 1;
pub const MSG_TYPE_COUNTER: u8 = 1;
pub const MSG_TYPE_RESPONSE_TIME: u8 = 2;

#[derive(Debug, Encode, Decode)]
pub struct LoraEnvelope<'a> {
    #[n(0)]
    pub version: u8,
    #[n(1)]
    pub msg_type: u8,
    #[n(2)]
    pub seq: u16,
    #[n(3)]
    pub timestamp_ms: u32,
    #[n(4)]
    pub payload: &'a ByteSlice,
}

pub struct OutgoingFrame<const N: usize> {
    pub payload: [u8; N],
    pub len: usize,
}

pub fn encode_envelope<const N: usize>(msg: &LoraEnvelope) -> Option<OutgoingFrame<N>> {
    if N < 3 {
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

pub fn decode_envelope<'a>(received: &'a [u8]) -> Option<LoraEnvelope<'a>> {
    if received.len() < 2 {
        return None;
    }

    let declared_len = u16::from_le_bytes([received[0], received[1]]) as usize;
    if declared_len == 0 || declared_len + 2 > received.len() {
        return None;
    }

    let cbor_payload = &received[2..(2 + declared_len)];
    minicbor::decode::<LoraEnvelope>(cbor_payload).ok()
}

pub fn build_response_time_reply<const N: usize>(
    seq: u16,
    elapsed_ms: u64,
    timestamp_ms: u32,
) -> Option<OutgoingFrame<N>> {
    let mut msg = String::<96>::new();

    let _ = write!(
        &mut msg,
        "time elapse since last response: {} ms",
        elapsed_ms
    );

    let cbor = LoraEnvelope {
        version: PROTOCOL_VERSION,
        msg_type: MSG_TYPE_RESPONSE_TIME,
        seq,
        timestamp_ms,
        payload: msg.as_bytes().into(),
    };

    encode_envelope(&cbor)
}

pub fn build_counter_message<const N: usize>(
    seq: u16,
    counter: u32,
    timestamp_ms: u32,
) -> Option<OutgoingFrame<N>> {
    let mut msg = String::<32>::new();
    let _ = write!(&mut msg, "counter: {}", counter);

    let cbor = LoraEnvelope {
        version: PROTOCOL_VERSION,
        msg_type: MSG_TYPE_COUNTER,
        seq,
        timestamp_ms,
        payload: msg.as_bytes().into(),
    };

    encode_envelope(&cbor)
}

pub fn decode_payload_utf8<'a>(
    message: &'a LoraEnvelope<'a>,
) -> core::result::Result<&'a str, &'a [u8]> {
    let payload = message.payload.as_ref();
    core::str::from_utf8(payload).map_err(|_| payload)
}

pub fn decode_legacy_counter(received: &[u8]) -> Option<u32> {
    if received.len() < 4 {
        return None;
    }

    Some(u32::from_le_bytes([
        received[0],
        received[1],
        received[2],
        received[3],
    ]))
}
