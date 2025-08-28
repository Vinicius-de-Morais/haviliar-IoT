fn main() -> anyhow::Result<()> {
    // --- Inicializa logger ---
    esp_idf_svc::log::EspLogger::initialize_default();

    // --- Inicializa periféricos ---
    let peripherals = Peripherals::take().unwrap();

    // --- Conexão WiFi ---
    let sysloop = EspSysLoopStack::new()?;
    let mut wifi = EspWifi::new(peripherals.modem, sysloop.clone(), None)?;
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: "MinhaRede".into(),
        password: "MinhaSenha".into(),
        ..Default::default()
    }))?;
    wifi.start()?;
    wifi.connect()?;
    wifi.wait_netif_up()?;
    println!("✅ Conectado ao WiFi!");

    // --- Cliente MQTT ---
    let mqtt_config = MqttClientConfiguration::default();
    let mut mqtt_client = EspMqttClient::new(
        "mqtt://test.mosquitto.org:1883",
        &mqtt_config,
        move |event| {
            if let Event::Received(msg) = event {
                if let Some(topic) = msg.topic() {
                    println!("📩 MQTT recebeu no tópico {topic}: {:?}", msg.data());

                    // 👉 Aqui você poderia repassar pro LoRa
                    // lora.transmit(msg.data(), &mut delay).unwrap();
                }
            }
        },
    )?;

    mqtt_client.subscribe("esp32/lora/down", QoS::AtMostOnce)?;

    // --- Inicializa LoRa (via SPI) ---
    let spi = SpiDriver::new(
        peripherals.spi2,
        peripherals.pins.gpio18, // SCLK
        peripherals.pins.gpio23, // MOSI
        peripherals.pins.gpio19, // MISO
        &SpiConfig::new(),
    )?;
    let nss = PinDriver::output(peripherals.pins.gpio5)?;
    let reset = PinDriver::output(peripherals.pins.gpio14)?;
    let dio0 = PinDriver::input(peripherals.pins.gpio26)?;
    let mut delay = Ets;
    let mut lora = LoRa::new(spi, nss, reset, dio0, 915, &mut delay)?;
    println!("✅ LoRa inicializado!");

    // --- Loop principal ---
    loop {
        // Recebe do LoRa e publica no MQTT
        if let Ok(packet) = lora.receive(&mut delay) {
            println!("📡 LoRa recebeu: {:?}", packet);
            mqtt_client.publish(
                "esp32/lora/up",
                QoS::AtMostOnce,
                false,
                &packet.payload,
            )?;
        }

        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}
