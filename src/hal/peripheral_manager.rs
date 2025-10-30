use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use esp_hal::peripherals::{Peripherals, I2C0, SPI2, GPIO4, GPIO5, GPIO19, GPIO27, GPIO26, GPIO18, GPIO14, GPIO15, GPIO16, TIMG0};
use esp_hal::timer::timg::TimerGroup;
use core::cell::RefCell;
use core::option::Option;
use core::option::Option::Some;
use core::sync::atomic::{AtomicBool, Ordering};
use core::cell::UnsafeCell;

/// Structured container for display peripherals
pub struct DisplayPeripherals{
    pub i2c: I2C0<'static>,
    pub sda: GPIO4<'static>,
    pub scl: GPIO15<'static>,
    pub rst: GPIO16<'static>,
}

/// Structured container for LoRa peripherals
pub struct LoRaPeripherals {
    pub spi: SPI2<'static>,
    pub sclk: GPIO5<'static>,
    pub mosi: GPIO27<'static>,
    pub miso: GPIO19<'static>,
    pub nss: GPIO18<'static>,
    pub dio1: GPIO26<'static>,
    pub rst: GPIO14<'static>,
}

/// Centralized peripheral manager
pub struct PeripheralManager {
    display_peripherals: Mutex<CriticalSectionRawMutex, RefCell<Option<DisplayPeripherals>>>,
    lora_peripherals: Mutex<CriticalSectionRawMutex, RefCell<Option<LoRaPeripherals>>>,
    time_peripherals: Mutex<CriticalSectionRawMutex, RefCell<Option<TIMG0<'static>>>>,
}

impl PeripheralManager {
    pub fn new(peripherals: Peripherals) -> Self {
        let display_peripherals = DisplayPeripherals {
            i2c: peripherals.I2C0,
            sda: peripherals.GPIO4,
            scl: peripherals.GPIO15,
            rst: peripherals.GPIO16,
        };
        
        let lora_peripherals = LoRaPeripherals {
            spi: peripherals.SPI2,
            sclk: peripherals.GPIO5,
            mosi: peripherals.GPIO27,
            miso: peripherals.GPIO19,
            nss: peripherals.GPIO18,
            dio1: peripherals.GPIO26,
            rst: peripherals.GPIO14,
        };
        
        Self {
            display_peripherals: Mutex::new(RefCell::new(Some(display_peripherals))),
            lora_peripherals: Mutex::new(RefCell::new(Some(lora_peripherals))),
            time_peripherals: Mutex::new(RefCell::new(Some(peripherals.TIMG0))),
        }
    }

    pub fn take_display_peripherals(&self) -> Option<DisplayPeripherals> {
        self.display_peripherals.lock(|cell| {
            cell.borrow_mut().take()
        })
    }

    pub fn take_lora_peripherals(&self) -> Option<LoRaPeripherals> {
        self.lora_peripherals.lock(|cell| {
            cell.borrow_mut().take()
        })
    }

    pub fn take_time_peripherals(&self) -> Option<TIMG0> {
        self.time_peripherals.lock(|cell| {
            cell.borrow_mut().take()
        })
    }

    pub fn time_per(&self) -> TimerGroup<'_, TIMG0> {
        let time_per = self.take_time_peripherals().unwrap();
        
        TimerGroup::new(time_per)
    }

    
}


// Add this static variable
static PERIPHERAL_MANAGER: PeripheralManagerStatic = PeripheralManagerStatic::new();

// Add this struct for static management
pub struct PeripheralManagerStatic {
    initialized: AtomicBool,
    manager: UnsafeCell<Option<PeripheralManager>>,
}

// Safety: We ensure single-threaded access through atomic flags
unsafe impl Sync for PeripheralManagerStatic {}

impl PeripheralManagerStatic {
    const fn new() -> Self {
        Self {
            initialized: AtomicBool::new(false),
            manager: UnsafeCell::new(None),
        }
    }

    pub fn init(peripherals: Peripherals) -> &'static mut PeripheralManager {
        if PERIPHERAL_MANAGER.initialized.swap(true, Ordering::Acquire) {
            panic!("PeripheralManager already initialized");
        }

        let manager_ref = unsafe { &mut *PERIPHERAL_MANAGER.manager.get() };
        *manager_ref = Some(PeripheralManager::new(peripherals));
        manager_ref.as_mut().unwrap()
    }

    pub fn get() -> &'static mut PeripheralManager {
        if !PERIPHERAL_MANAGER.initialized.load(Ordering::Acquire) {
            panic!("PeripheralManager not initialized");
        }
        
        let manager_ref = unsafe { &mut *PERIPHERAL_MANAGER.manager.get() };
        manager_ref.as_mut().unwrap()
    }
}