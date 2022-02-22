mod gamma_correction;
mod opc_pool;
mod pixel_buffer;
mod screen_samples;
mod serial_port;
mod settings;
mod update_timer;

use std::{fs, thread, time::Duration};

use {settings::Settings, update_timer::UpdateTimer};

fn main() {
    let config_json = fs::read_to_string("AdaLight.config.json").expect("read config file");
    let settings = Settings::from_str(&config_json);

    match settings {
        Ok(settings) => {
            let timer = UpdateTimer::new(settings);
            if timer.start() {
                thread::sleep(Duration::from_secs(30));
                timer.stop();
            }
        }
        Err(error) => eprintln!("Settings Error: {:?}", error),
    }
}
