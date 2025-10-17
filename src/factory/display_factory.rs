
use anyhow::Result;
use crate::hal::{display::Display, peripheral_manager::{DisplayPeripherals, PeripheralManager}};

pub struct DisplayFactory;

impl DisplayFactory {
    pub fn create_from_manager<'d>(manager: &mut PeripheralManager) -> Result<Display<'d>> {
        let peripherals = manager.take_display_peripherals()
            .ok_or_else(|| anyhow::anyhow!("Display peripherals already taken"))?;
        
        Display::new(
            peripherals.i2c,
            peripherals.sda,
            peripherals.scl,
            peripherals.rst,
        )
    }

    pub fn create_from_peripherals<'d>(peripherals: DisplayPeripherals) -> Result<Display<'d>>{
        Display::new(
            peripherals.i2c,
            peripherals.sda,
            peripherals.scl,
            peripherals.rst,
        )
    }
}