use esp_idf_hal::{
    gpio::{Gpio4, Gpio8, Gpio9, Gpio10, Gpio11, Gpio12, Gpio14, Gpio15, Gpio16},
    i2c::I2C0,
    peripherals::Peripherals,
    spi::SPI2,
};

/// Centralized peripheral manager that owns all hardware peripherals
/// and provides controlled access to them
pub struct PeripheralManager {
    // Display peripherals
    pub display_i2c: Option<I2C0>,
    pub display_sda: Option<Gpio4>,
    pub display_scl: Option<Gpio15>,
    pub display_rst: Option<Gpio16>,
    
    // LoRa peripherals
    pub lora_spi: Option<SPI2>,
    pub lora_sclk: Option<Gpio9>,
    pub lora_sdo: Option<Gpio11>,
    pub lora_sdi: Option<Gpio10>,
    pub lora_cs: Option<Gpio8>,
    pub lora_dio1: Option<Gpio14>,
    pub lora_rst: Option<Gpio12>,
}

impl PeripheralManager {
    /// Initialize the peripheral manager with all ESP32 peripherals
    pub fn new(peripherals: Peripherals) -> Self {
        Self {
            // Display peripherals
            display_i2c: Some(peripherals.i2c0),
            display_sda: Some(peripherals.pins.gpio4),
            display_scl: Some(peripherals.pins.gpio15),
            display_rst: Some(peripherals.pins.gpio16),
            
            // LoRa peripherals
            lora_spi: Some(peripherals.spi2),
            lora_sclk: Some(peripherals.pins.gpio9),
            lora_sdo: Some(peripherals.pins.gpio11),
            lora_sdi: Some(peripherals.pins.gpio10),
            lora_cs: Some(peripherals.pins.gpio8),
            lora_dio1: Some(peripherals.pins.gpio14),
            lora_rst: Some(peripherals.pins.gpio12),
        }
    }

    /// Take display peripherals (can only be called once)
    pub fn take_display_peripherals(&mut self) -> Option<(I2C0, Gpio4, Gpio15, Gpio16)> {
        if let (Some(i2c), Some(sda), Some(scl), Some(rst)) = (
            self.display_i2c.take(),
            self.display_sda.take(),
            self.display_scl.take(),
            self.display_rst.take(),
        ) {
            Some((i2c, sda, scl, rst))
        } else {
            None
        }
    }

    /// Take LoRa peripherals (can only be called once)
    pub fn take_lora_peripherals(&mut self) -> Option<(SPI2, Gpio9, Gpio11, Gpio10, Gpio8, Gpio14, Gpio12)> {
        if let (Some(spi), Some(sclk), Some(sdo), Some(sdi), Some(cs), Some(dio1), Some(rst)) = (
            self.lora_spi.take(),
            self.lora_sclk.take(),
            self.lora_sdo.take(),
            self.lora_sdi.take(),
            self.lora_cs.take(),
            self.lora_dio1.take(),
            self.lora_rst.take(),
        ) {
            Some((spi, sclk, sdo, sdi, cs, dio1, rst))
        } else {
            None
        }
    }

    /// Check if display peripherals are available
    pub fn has_display_peripherals(&self) -> bool {
        self.display_i2c.is_some() && 
        self.display_sda.is_some() && 
        self.display_scl.is_some() && 
        self.display_rst.is_some()
    }

    /// Check if LoRa peripherals are available
    pub fn has_lora_peripherals(&self) -> bool {
        self.lora_spi.is_some() && 
        self.lora_sclk.is_some() && 
        self.lora_sdo.is_some() && 
        self.lora_sdi.is_some() && 
        self.lora_cs.is_some() && 
        self.lora_dio1.is_some() && 
        self.lora_rst.is_some()
    }

    /// Take a specific peripheral if available
    pub fn take_gpio4(&mut self) -> Option<Gpio4> {
        self.display_sda.take()
    }

    pub fn take_gpio15(&mut self) -> Option<Gpio15> {
        self.display_scl.take()
    }

    pub fn take_gpio16(&mut self) -> Option<Gpio16> {
        self.display_rst.take()
    }

    pub fn take_i2c0(&mut self) -> Option<I2C0> {
        self.display_i2c.take()
    }

    // Add similar methods for LoRa peripherals as needed
    pub fn take_spi2(&mut self) -> Option<SPI2> {
        self.lora_spi.take()
    }

    pub fn take_gpio14(&mut self) -> Option<Gpio14> {
        self.lora_dio1.take()
    }

    pub fn take_gpio12(&mut self) -> Option<Gpio12> {
        self.lora_rst.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peripheral_manager_lifecycle() {
        // This is a conceptual test - you'd need to mock Peripherals for actual testing
        // let peripherals = Peripherals::take().unwrap();
        // let mut manager = PeripheralManager::new(peripherals);
        
        // assert!(manager.has_display_peripherals());
        // assert!(manager.has_lora_peripherals());
        
        // let display_periphs = manager.take_display_peripherals();
        // assert!(display_periphs.is_some());
        // assert!(!manager.has_display_peripherals());
        
        // let lora_periphs = manager.take_lora_peripherals();
        // assert!(lora_periphs.is_some());
        // assert!(!manager.has_lora_peripherals());
    }
}
