use embassy_net::{
    Config as EmbassyNetConfig, Runner, Stack, StackResources, dns::DnsQueryType, tcp::TcpSocket
};

use esp_hal::{rng::Rng, timer::timg::TimerGroup};
use esp_wifi::{
    wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState},
    EspWifiController,
};
use log::{error, info};
use static_cell::StaticCell;
use esp_hal::peripherals::{TIMG0};
use crate::hal::peripheral_manager::WifiPeripherals;

pub struct Wifi {
    pub ssid: &'static str,
    pub password: &'static str,
    wifi_controller: WifiController<'static>,
    stack: Stack<'static>,
    runner: Runner<'static, WifiDevice<'static>>,
}

static WIFI_CONTROLLER: StaticCell<EspWifiController<'static>> = StaticCell::new();
//static TIMER_GROUP_CELL: StaticCell<TimerGroup<TIMG0>> = StaticCell::new();
static STACK_RESOURCE_CELL: StaticCell<StackResources<3>> = StaticCell::new();

fn log_heap_info(context: &str) {
    let free = esp_alloc::HEAP.free();
    let used = esp_alloc::HEAP.used();
    let total = free + used;
    info!("{} - Heap: {}/{} bytes free ({:.1}% used)", 
          context, free, total, (used as f32 / total as f32) * 100.0);
}

impl Wifi {
    pub fn new(wifi_peripherals: WifiPeripherals) -> Wifi {
        
        log_heap_info("Before initializing WiFi controller");

        let ssid: &str = env!("SSID");
        let password: &str = env!("PASSWORD");
        info!("SSID: {} | PASSWORD {}", ssid, password);

        let timer_group = TimerGroup::new(wifi_peripherals.timg0);
        let mut rng = Rng::new(wifi_peripherals.rng);

        info!("Initializing WiFi controller...");
        let controller = 
            match esp_wifi::init(timer_group.timer1, rng) {
                Ok(ctrl) => ctrl,
                Err(e) => {
                    panic!("Failed to initialize WiFi controller: {:?}", e);
                }
            };
        info!("Initialize static...");
        let controller_static = WIFI_CONTROLLER.init(controller);

        info!("Creating WiFi interface...");
        let (wifi_controller, interface) = 
            match esp_wifi::wifi::new(controller_static, wifi_peripherals.wifi) {
                Ok(tuple) => tuple,
                Err(e) => {
                    panic!("Failed to create WiFi interface: {:?}", e);
                }
            };

        let seed = (rng.random() as u64) << 32 | rng.random() as u64;
        let embassy_net_config = EmbassyNetConfig::dhcpv4(Default::default());
        
        info!("Creating network stack...");
        let stack_resources  = StackResources::<3>::new();
        let stack_resources = STACK_RESOURCE_CELL.init(stack_resources);

        let (stack, runner) = embassy_net::new(
            interface.sta,
            embassy_net_config,
            stack_resources,
            seed
        );


        Wifi {
            ssid,
            password,
            wifi_controller,
            stack,
            runner,
        }
    }


    pub async fn connect(&mut self) -> Result<(), &'static str> {
        let client_config = ClientConfiguration {
            ssid: self.ssid.into(),
            password: self.password.into(),
            ..Default::default()
        };

        let config = Configuration::Client(client_config);

        self.wifi_controller.set_configuration(&config).map_err(|_| "Failed to set WiFi configuration")?;

        self.wifi_controller.start().map_err(|_| "Failed to start WiFi controller")?;

        //loop {
            match esp_wifi::wifi::wifi_state() {
                WifiState::StaStarted => {
                    self.wifi_controller.connect().map_err(|_| "Failed to connect to WiFi")?;
                }
                WifiState::StaConnected => {
                    info!("WiFi connected, obtaining IP address...");
                }
                WifiState::StaDisconnected => {
                    error!("WiFi disconnected, retrying...");
                    self.wifi_controller.connect().map_err(|_| "Failed to reconnect to WiFi")?;
                }
                _ => {}
            }
        //}

        Ok(())
    }

    pub fn get_controller(&self) -> &WifiController<'static> {
        &self.wifi_controller
    }

    pub fn get_stack(&mut self) -> &mut Stack<'static> {
        &mut self.stack
    }

    pub fn get_runner(&mut self) -> &mut Runner<'static, WifiDevice<'static>> {
        &mut self.runner
    }

    pub fn take_components(self) -> (WifiController<'static>, Runner<'static, WifiDevice<'static>>, Stack<'static>) {
        (self.wifi_controller, self.runner, self.stack)
    }
}