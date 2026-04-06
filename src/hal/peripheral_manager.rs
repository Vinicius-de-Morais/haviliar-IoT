use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use esp_hal::peripherals::{Peripherals, I2C0, SPI2, GPIO0, GPIO17, GPIO9, GPIO10, GPIO11, GPIO8, GPIO12, GPIO21, GPIO18, GPIO36, GPIO47, GPIO13, GPIO14, TIMG0, TIMG1, RNG, WIFI, LEDC};
use esp_hal::timer::timg::TimerGroup;
use core::cell::RefCell;
use core::option::Option;
use core::option::Option::Some;
use core::sync::atomic::{AtomicBool, Ordering};
use core::cell::UnsafeCell;

/// Structured container for display peripherals
pub struct DisplayPeripherals{
    pub i2c: I2C0<'static>,
    pub sda: GPIO17<'static>,
    pub scl: GPIO18<'static>,
    pub rst: GPIO21<'static>,
    pub vext: GPIO36<'static>,
}

/// Structured container for LoRa peripherals
pub struct LoRaPeripherals {
    pub spi: SPI2<'static>,
    pub sck: GPIO9<'static>,
    pub mosi: GPIO10<'static>,
    pub miso: GPIO11<'static>,
    pub cs: GPIO8<'static>,
    pub dio: GPIO14<'static>,
    pub rst: GPIO12<'static>,
    pub busy: GPIO13<'static>,
}

pub struct WifiPeripherals {
    pub timg0: TIMG0<'static>,
    pub rng: RNG<'static>,
    pub wifi: WIFI<'static>,
}

pub struct ServoPeripherals {
    //pub pin: GPIO17<'static>,
    pub pin: GPIO47<'static>,
    pub ledc: LEDC<'static>,
}

pub struct ButtonPeripherals {
    pub prg: GPIO0<'static>,
}

/// Centralized peripheral manager
pub struct PeripheralManager {
    display_peripherals: Mutex<CriticalSectionRawMutex, RefCell<Option<DisplayPeripherals>>>,
    lora_peripherals: Mutex<CriticalSectionRawMutex, RefCell<Option<LoRaPeripherals>>>,
    wifi_peripherals: Mutex<CriticalSectionRawMutex, RefCell<Option<WifiPeripherals>>>,
    time_peripherals: Mutex<CriticalSectionRawMutex, RefCell<Option<TIMG1<'static>>>>,
    servo_peripherals: Mutex<CriticalSectionRawMutex, RefCell<Option<ServoPeripherals>>>,
    button_peripherals: Mutex<CriticalSectionRawMutex, RefCell<Option<ButtonPeripherals>>>,
}

impl PeripheralManager {
    pub fn new(peripherals: Peripherals) -> Self {
        let display_peripherals = DisplayPeripherals {
            i2c: peripherals.I2C0,
            sda: peripherals.GPIO17,
            scl: peripherals.GPIO18,
            rst: peripherals.GPIO21,
            vext: peripherals.GPIO36,
        };
        
        let lora_peripherals = LoRaPeripherals {
            spi: peripherals.SPI2,
            sck: peripherals.GPIO9,
            mosi: peripherals.GPIO10,
            miso: peripherals.GPIO11,
            cs: peripherals.GPIO8,
            dio: peripherals.GPIO14,
            rst: peripherals.GPIO12,
            busy: peripherals.GPIO13,
        };

        let wifi_peripherals = WifiPeripherals {
            timg0: peripherals.TIMG0,
            rng: peripherals.RNG,
            wifi: peripherals.WIFI,
        };

        let servo_peripherals = ServoPeripherals {
            pin: peripherals.GPIO47,
            ledc: peripherals.LEDC,
        };

        let button_peripherals = ButtonPeripherals {
            prg: peripherals.GPIO0,
        };
        
        Self {
            display_peripherals: Mutex::new(RefCell::new(Some(display_peripherals))),
            lora_peripherals: Mutex::new(RefCell::new(Some(lora_peripherals))),
            wifi_peripherals: Mutex::new(RefCell::new(Some(wifi_peripherals))),
            time_peripherals: Mutex::new(RefCell::new(Some(peripherals.TIMG1))),
            servo_peripherals: Mutex::new(RefCell::new(Some(servo_peripherals))),
            button_peripherals: Mutex::new(RefCell::new(Some(button_peripherals))),
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

    pub fn take_wifi_peripherals(&self) -> Option<WifiPeripherals> {
        self.wifi_peripherals.lock(|cell| {
            cell.borrow_mut().take()
        })
    }

    pub fn take_servo_peripherals(&self) -> Option<ServoPeripherals> {
        self.servo_peripherals.lock(|cell| {
            cell.borrow_mut().take()
        })
    }

    pub fn take_button_peripherals(&self) -> Option<ButtonPeripherals> {
        self.button_peripherals.lock(|cell| {
            cell.borrow_mut().take()
        })
    }

    pub fn take_time_peripherals(&self) -> Option<TIMG1> {
        self.time_peripherals.lock(|cell| {
            cell.borrow_mut().take()
        })
    }

    pub fn time_per(&self) -> TimerGroup<'_, TIMG1> {
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