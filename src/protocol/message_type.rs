use minicbor::{Decode, Encode};

#[derive(Debug, Encode, Decode, Clone, Copy)]
pub enum MessageType {
    #[n(0)]
    Counter = 0,

    #[n(1)]
    ResponseTime = 1,

    #[n(2)]
    ContinuousPackage = 2,

    #[n(3)]
    Reply = 3,

    #[n(4)]
    Ack = 4,

    #[n(5)]
    Open = 5,
}