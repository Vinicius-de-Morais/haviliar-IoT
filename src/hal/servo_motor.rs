use crate::hal::peripheral_manager::ServoPeripherals;
use esp_hal::ledc::channel::ChannelHW;
//use esp_hal::peripherals::{GPIO12, LEDC};
use esp_hal::ledc::Ledc;


pub struct ServoMotor {
    // pin: GPIO12<'static>,
    // ledc_pin: LEDC<'static>,
    ledc: Ledc<'static>,
    pub channel: esp_hal::ledc::channel::Channel<'static, esp_hal::ledc::HighSpeed>,
}

impl ServoMotor {
    pub fn new(peripherals: ServoPeripherals) -> Self {
        let pin = peripherals.pin;
        let ledc_pin = peripherals.ledc;

        let mut ledc = esp_hal::ledc::Ledc::new(ledc_pin);
        let channel = ledc.channel::<esp_hal::ledc::HighSpeed>(esp_hal::ledc::channel::Number::Channel0, pin);

        Self { 
            // pin, 
            // ledc_pin, 
            ledc, 
            channel }
    }

    pub fn set_angle(&mut self, angle: i16) {
        const MIN_DUTY: u32 = 819;  // 1ms
        const MAX_DUTY: u32 = 1638; // 2ms

        let duty = MIN_DUTY + ((angle as u32 + 90) * (MAX_DUTY - MIN_DUTY) / 180);
        self.channel.set_duty_hw(duty);
    }
}

