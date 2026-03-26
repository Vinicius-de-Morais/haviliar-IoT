use minicbor::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub enum MessageType {
    #[n(0)]
    Counter = 0,

    #[n(1)]
    ResponseTime = 1,

    #[n(2)]
    ContinuousPackage = 2,
}