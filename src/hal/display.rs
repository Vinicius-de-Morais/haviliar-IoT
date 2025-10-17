use anyhow::Result;
use core::result::Result::Ok;
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Alignment, Text},
};
use esp_hal::{
    gpio::{Output},
    i2c::master::{I2c, Config},
    delay::Delay,
};
use ssd1306::{
    mode::BufferedGraphicsMode, 
    prelude::*, 
    Ssd1306
};
use log::*;
use core::default::Default;
use esp_hal::peripherals::{GPIO4, GPIO15, GPIO16};

pub struct Display<'d> {
    rst_pin: Output<'d>,
    display: Ssd1306<
        I2CInterface<I2c<'d, esp_hal::Blocking>>,
        DisplaySize128x64,
        BufferedGraphicsMode<DisplaySize128x64>,
    >,
    text_style: MonoTextStyle<'d, BinaryColor>,
}

impl<'d> Display<'d> {
    pub fn new(
        i2c_peripheral: esp_hal::peripherals::I2C0<'d>,
        sda: GPIO4<'d>,
        scl: GPIO15<'d>,
        rst: GPIO16<'d>,
    ) -> Result<Self> {
        // Configure I2C  
        let config = Config::default();//.with_frequency(100_000_u32.Hz());
        let mut i2c = I2c::new(i2c_peripheral, config)?;
        i2c = i2c.with_sda(sda);
        i2c = i2c.with_scl(scl);

        // Configure reset pin
        let mut rst_pin = Output::new(rst, esp_hal::gpio::Level::Low, esp_hal::gpio::OutputConfig::default());
        
        // Reset sequence
        rst_pin.set_low();
        Delay::new().delay_millis(100);
        rst_pin.set_high();
        Delay::new().delay_millis(100);

        // Initialize display
        let interface = I2CInterface::new(i2c, 0x3C, 0x40);
        let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();

        display.init().map_err(|e| anyhow::anyhow!("Failed to initialize OLED: {:?}", e))?;
        display.clear(BinaryColor::Off).map_err(|e| anyhow::anyhow!("Failed to clear display: {:?}", e))?;
        display.flush().map_err(|e| anyhow::anyhow!("Failed to flush display: {:?}", e))?;

        info!("OLED initialized successfully!");
        
        let text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
        
        Ok(Display { 
            rst_pin,
            display, 
            text_style,
        })
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

        self.flush()
    }

    pub fn text_new_line(&mut self, message: &str, line: u8) -> Result<()> {
        let y = 10 * (line as i32);
        self.text_no_clear(message, 0, y)
    }

    pub fn text_no_clear(&mut self, message: &str, x: i32, y: i32) -> Result<()> {
        Text::new(message, Point::new(x, y), self.text_style)
            .draw(&mut self.display)
            .map_err(|e| anyhow::anyhow!("Failed to draw text: {:?}", e))?;
        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        self.display.clear(BinaryColor::Off)
            .map_err(|e| anyhow::anyhow!("Failed to clear display: {:?}", e))?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.display.flush()
            .map_err(|e| anyhow::anyhow!("Failed to flush display: {:?}", e))?;
        Ok(())
    }
}