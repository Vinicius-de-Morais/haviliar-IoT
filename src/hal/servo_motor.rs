use crate::hal::peripheral_manager::ServoPeripherals;
use esp_hal::ledc::channel::ChannelHW;
//use esp_hal::peripherals::{GPIO12, LEDC};
use esp_hal::gpio::DriveMode;
use esp_hal::ledc::channel::ChannelIFace;
use esp_hal::ledc::timer::TimerIFace;
use esp_hal::ledc::{HighSpeed, Ledc, channel, timer};
use esp_hal::time::Rate;
use static_cell::StaticCell;
use embedded_hal::pwm::SetDutyCycle;

pub struct ServoMotor {
    // pin: GPIO12<'static>,
    // ledc_pin: LEDC<'static>,
    ledc: Ledc<'static>,
    pub channel: esp_hal::ledc::channel::Channel<'static, esp_hal::ledc::HighSpeed>,
    max_duty_cycle: u32,
}

static HSTIMER0: StaticCell<esp_hal::ledc::timer::Timer<'static, esp_hal::ledc::HighSpeed>> = StaticCell::new();

impl ServoMotor {
    pub fn new(peripherals: ServoPeripherals) -> Self {
        let pin = peripherals.pin;
        let ledc_pin = peripherals.ledc;

        let mut ledc = Ledc::new(ledc_pin);

        let mut hstimer0 = ledc.timer::<HighSpeed>(timer::Number::Timer0);
        hstimer0
            .configure(timer::config::Config {
                duty: timer::config::Duty::Duty12Bit,
                clock_source: timer::HSClockSource::APBClk,
                frequency: Rate::from_hz(50),
            })
            .unwrap();

        let hstimer0 = HSTIMER0.init(hstimer0);

        let mut channel = ledc.channel(channel::Number::Channel0, pin);
        channel
            .configure(channel::config::Config {
                timer: hstimer0,
                duty_pct: 10,
                pin_config: channel::config::PinConfig::PushPull,
            })
            .unwrap();     
        
        let max_duty_cycle = channel.max_duty_cycle() as u32;

        Self { 
            // pin, 
            // ledc_pin, 
            ledc, 
            channel,
            max_duty_cycle
        }
    }
    
    pub fn set_angle(&mut self, deg: u32) -> Result<(), esp_hal::ledc::channel::Error> {
        let min_duty = (25 * self.max_duty_cycle) / 1000;
        let max_duty = (125 * self.max_duty_cycle) / 1000;
        let duty_gap = max_duty - min_duty;

        let duty = (min_duty + ((deg * duty_gap) / 180) ) as u16;

        self.channel.set_duty_cycle(duty)
    }

    pub fn open(&mut self) -> Result<(), esp_hal::ledc::channel::Error> {
        self.set_angle(45)
    }

    pub fn close(&mut self) -> Result<(), esp_hal::ledc::channel::Error> {
        self.set_angle(0)
    }
}

