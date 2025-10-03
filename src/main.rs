#![no_std]
#![no_main]

pub mod hal;
pub mod factory;
use anyhow::{Ok, Result};
use esp_idf_sys as _;
use log::*;

#[no_mangle]
fn main() -> Result<()> {
    info!("Error: Wrong binary executed. This binary is not intended to be run directly.");
    Ok(())
}