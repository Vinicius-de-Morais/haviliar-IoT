use std::borrow::Borrow;
use std::fmt::{self, Debug, Display};
use embedded_hal_async::spi::{ErrorType, SpiDevice, Operation};
use esp_idf_hal::spi::{SpiDeviceDriver, SpiDriver};
use esp_idf_hal::io::EspIOError;

// Custom error type that implements embedded_hal::spi::Error
#[derive(Debug)]
pub enum SpiAdapterError {
    IoError(EspIOError),
    TransferError(&'static str),
}

impl Display for SpiAdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "SPI I/O error: {:?}", e),
            Self::TransferError(msg) => write!(f, "SPI transfer error: {}", msg),
        }
    }
}

impl std::error::Error for SpiAdapterError {}

// Implement the embedded_hal::spi::Error trait
impl embedded_hal::spi::Error for SpiAdapterError {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}

pub struct AsyncSpiAdapter<'d, T> 
where
    T: Borrow<SpiDriver<'d>> + 'd,
{
    driver: &'d mut SpiDeviceDriver<'d, T>,
}

impl<'d, T> AsyncSpiAdapter<'d, T> 
where
    T: Borrow<SpiDriver<'d>> + 'd,
{
    pub fn new(driver: &'d mut SpiDeviceDriver<'d, T>) -> Self {
        Self { driver }
    }
    
    pub fn inner_mut(&mut self) -> &mut SpiDeviceDriver<'d, T> {
        self.driver
    }
}

impl<'d, T> ErrorType for AsyncSpiAdapter<'d, T>
where
    T: Borrow<SpiDriver<'d>> + 'd,
{
    type Error = SpiAdapterError;
}

impl<'d, T> SpiDevice for AsyncSpiAdapter<'d, T>
where
    T: Borrow<SpiDriver<'d>> + 'd,
{
    async fn transaction(&mut self, operations: &mut [embedded_hal::spi::Operation<'_, u8>]) -> Result<(), SpiAdapterError> {
        // Implement transactions using the synchronous API
        for op in operations {
            match op {
                Operation::Read(buffer) => {
                    self.driver.read(buffer)
                        .map_err(|e| SpiAdapterError::IoError(EspIOError(e)))?;
                }
                Operation::Write(buffer) => {
                    self.driver.write(buffer)
                        .map_err(|e| SpiAdapterError::IoError(EspIOError(e)))?;
                }
                Operation::Transfer(read, write) => {
                    // For transfer, we need to read and write the same amount of data
                    if read.len() == write.len() {
                        // Create a temporary buffer to hold the write data
                        let mut write_data = write.iter().copied().collect::<Vec<u8>>();

                        self.driver.transfer(&mut write_data, read)
                            .map_err(|e| SpiAdapterError::IoError(EspIOError(e)))?;
                    } else {
                        return Err(SpiAdapterError::TransferError(
                            "Read and write buffers must be the same length"
                        ));
                    }
                }
                Operation::TransferInPlace(items) => {
                    // Create a temporary buffer with the same content
                    // let mut write_data: &[u8];
                    // items.clone_into(write_data);
                    
                    // // Perform the transfer
                    // self.driver.transfer(write_data, items)
                    //     .map_err(|e| SpiAdapterError::IoError(EspIOError(e)))?;
                }
                Operation::DelayNs(_) => {
                    // No direct delay API in SpiDeviceDriver, but for small delays
                    // we can just continue since the operation overhead is enough
                }
            }
        }
        Ok(())
    }
}