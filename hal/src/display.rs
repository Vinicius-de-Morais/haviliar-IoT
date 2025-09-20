use anyhow::Result;
use core::fmt::Write;
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Alignment, Text},
};
use esp_idf_hal::{
    i2c::{I2cConfig, I2cDriver}, // Import I2cConfig directly
    prelude::*,
    units::Hertz,
};
use esp_idf_sys as _;
use log::*;
// Import the specific types needed for the struct definition
use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};

pub struct display {
    display: Ssd1306<
        // Provide the I2cDriver as a generic argument here
        I2CDisplayInterface<I2cDriver<'static>>,
        DisplaySize128x64,
        BufferedGraphicsMode<DisplaySize128x64>,
    >,
}

impl display {
    pub fn new(peripherals: Peripherals) -> Result<Self> {
        // I2C Configuration
        let i2c = peripherals.i2c0;
        let sda = peripherals.pins.gpio4;
        let scl = peripherals.pins.gpio15;

        let config = I2cConfig::new().baudrate(Hertz(400_000));
        let i2c_driver = I2cDriver::new(i2c, sda, scl, &config)?;

        // Display Interface
        let interface = I2CDisplayInterface::new(i2c_driver);
        let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();

        display.init().map_err(|e| anyhow::anyhow!("Failed to initialize OLED: {:?}", e))?;
        info!("OLED initialized successfully!");

        // Clear the display
        display.clear(BinaryColor::Off).unwrap();
        display.flush().unwrap();

        Ok(display { display })
    }

    pub fn show_message(&mut self, message: &str) {
        // Clear the display before writing new text to avoid overlap
        self.display.clear(BinaryColor::Off).unwrap();

        let text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

        Text::with_alignment(
            message,
            self.display.bounding_box().center(), // Simplified positioning to the center
            text_style,
            Alignment::Center,
        )
        .draw(&mut self.display)
        .unwrap();

        self.display.flush().unwrap();
    }
}