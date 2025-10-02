use anyhow::Result;
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Alignment, Text},
};
use esp_idf_hal::{delay::FreeRtos, gpio::{Gpio16, Output, PinDriver}, i2c::I2cDriver, peripheral::Peripheral, prelude::Peripherals};
use esp_idf_sys as _;
use log::*;
use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};

pub struct Display<'d> {
    rst_pin: PinDriver<'d, Gpio16, Output>,
    display: Ssd1306<
        I2CInterface<I2cDriver<'d>>,
        DisplaySize128x64,
        BufferedGraphicsMode<DisplaySize128x64>,
    >,
    text_style: MonoTextStyle<'d, BinaryColor>,
}

impl<'d> Display<'d> {
    pub fn new(i2c_driver: I2cDriver<'d>, rst_pin: PinDriver<'d, Gpio16, Output>) -> Result<Self> {

        let interface = I2CDisplayInterface::new(i2c_driver);
        let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();

        display.init().map_err(|e| anyhow::anyhow!("Failed to initialize OLED: {:?}", e))?;
        info!("OLED initialized successfully!");

        display.clear(BinaryColor::Off).map_err(|e| anyhow::anyhow!("Failed to clear display: {:?}", e))?;
        display.flush().map_err(|e| anyhow::anyhow!("Failed to flush display: {:?}", e))?;

        
        let text_style = Display::get_text_style();
        Ok(Display { display , text_style, rst_pin})
    }

    pub fn show_message(&mut self, message: &str) -> Result<()> {
        self.clear()?;

        Text::with_alignment(
            message,
            self.display.bounding_box().center(),
            self.text_style,
            Alignment::Center,
        )
        .draw(&mut self.display)
        .map_err(|e| anyhow::anyhow!("Failed to draw text: {:?}", e))?;

        self.display.flush().map_err(|e| anyhow::anyhow!("Failed to flush display: {:?}", e))?;
        Ok(())
    }

    pub fn text_no_clear(&mut self, message: &str, x: i32, y: i32) -> Result<()> {        
        Text::new(message, Point::new(x, y), self.text_style)
            .draw(&mut self.display).map_err(|e| anyhow::anyhow!("Failed to draw text: {:?}", e))?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.display.flush().map_err(|e| anyhow::anyhow!("Failed to flush display: {:?}", e))?;
        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        self.display.clear(BinaryColor::Off).map_err(|e| anyhow::anyhow!("Failed to clear display: {:?}", e))?;
        Ok(())
    }

    pub fn text_clear(&mut self, message: &str, x: i32, y: i32) -> Result<()> {
        self.clear()?;

        Text::new(message, Point::new(x, y), self.text_style)
            .draw(&mut self.display).map_err(|e| anyhow::anyhow!("Failed to draw text: {:?}", e))?;

        self.display.flush().map_err(|e| anyhow::anyhow!("Failed to flush display: {:?}", e))?;
        Ok(())
    }

    pub fn get_text_style() -> MonoTextStyle<'static, BinaryColor> {
        MonoTextStyle::new(&FONT_6X10, BinaryColor::On)
    }
    
    // TODO: Control in the struct the current line to avoid overwriting
    pub fn text_new_line(&mut self, message: &str, line: u8) -> Result<()> {
        let y = 10 * (line as i32); // Assuming FONT_6X10 height is 10 pixels
        self.text_no_clear(message, 0, y)?;
        Ok(())
    }
}