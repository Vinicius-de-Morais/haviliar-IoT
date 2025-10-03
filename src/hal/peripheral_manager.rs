use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use esp_idf_hal::{
    gpio::{Gpio4, Gpio8, Gpio9, Gpio10, Gpio11, Gpio12, Gpio14, Gpio15, Gpio16},
    i2c::I2C0,
    peripherals::Peripherals,
    spi::SPI2,
};
use log::warn;
use core::cell::RefCell;

/// Structured container for display peripherals
pub struct DisplayPeripherals {
    pub i2c: I2C0,
    pub sda: Gpio4,
    pub scl: Gpio15,
    pub rst: Gpio16,
}

/// Structured container for LoRa peripherals
pub struct LoRaPeripherals {
    pub spi: SPI2,
    pub sclk: Gpio9,
    pub sdo: Gpio11,
    pub sdi: Gpio10, 
    pub cs: Gpio8,
    pub dio1: Gpio14,
    pub rst: Gpio12,
}

/// Centralized peripheral manager that owns all hardware peripherals
/// and provides controlled access to them through functional groups
pub struct PeripheralManager {
    // Display peripherals wrapped in mutex with interior mutability
    display_peripherals: Mutex<CriticalSectionRawMutex, RefCell<Option<DisplayPeripherals>>>,
    
    // LoRa peripherals wrapped in mutex with interior mutability
    lora_peripherals: Mutex<CriticalSectionRawMutex, RefCell<Option<LoRaPeripherals>>>,
}

impl PeripheralManager {
    /// Initialize the peripheral manager with all ESP32 peripherals
    pub fn new(peripherals: Peripherals) -> Self {
        // Create display peripherals
        let display_peripherals = DisplayPeripherals {
            i2c: peripherals.i2c0,
            sda: peripherals.pins.gpio4,
            scl: peripherals.pins.gpio15,
            rst: peripherals.pins.gpio16,
        };
        
        // Create LoRa peripherals
        let lora_peripherals = LoRaPeripherals {
            spi: peripherals.spi2,
            sclk: peripherals.pins.gpio9,
            sdo: peripherals.pins.gpio11,
            sdi: peripherals.pins.gpio10,
            cs: peripherals.pins.gpio8,
            dio1: peripherals.pins.gpio14,
            rst: peripherals.pins.gpio12,
        };
        
        Self {
            display_peripherals: Mutex::new(RefCell::new(Some(display_peripherals))),
            lora_peripherals: Mutex::new(RefCell::new(Some(lora_peripherals))),
        }
    }

    /// Get display peripherals (thread-safe)
    /// Returns None if already taken, or provides temporary access while holding the lock
    pub fn with_display_peripherals<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut DisplayPeripherals) -> R,
    {
        self.display_peripherals.lock(|cell| {
            if let Some(ref mut peripherals) = *cell.borrow_mut() {
                Some(f(peripherals))
            } else {
                None
            }
        })
    }

    /// Get LoRa peripherals (thread-safe)
    /// Returns None if already taken, or provides temporary access while holding the lock
    pub fn with_lora_peripherals<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut LoRaPeripherals) -> R,
    {
        self.lora_peripherals.lock(|cell| {
            if let Some(ref mut peripherals) = *cell.borrow_mut() {
                Some(f(peripherals))
            } else {
                None
            }
        })
    }

    /// Take display peripherals (legacy method, maintains backward compatibility)
    /// Returns a structured container with all required peripherals
    pub fn take_display_peripherals(&self) -> Option<DisplayPeripherals> {
        self.display_peripherals.lock(|cell| {
            cell.borrow_mut().take()
        })
    }

    /// Take LoRa peripherals (legacy method, maintains backward compatibility)
    /// Returns a structured container with all required peripherals
    pub fn take_lora_peripherals(&self) -> Option<LoRaPeripherals> {
        self.lora_peripherals.lock(|cell| {
            cell.borrow_mut().take()
        })
    }

    /// Check if display peripherals are available
    pub fn has_display_peripherals(&self) -> bool {
        self.display_peripherals.lock(|cell| cell.borrow().is_some())
    }

    /// Check if LoRa peripherals are available
    pub fn has_lora_peripherals(&self) -> bool {
        self.lora_peripherals.lock(|cell| cell.borrow().is_some())
    }
}