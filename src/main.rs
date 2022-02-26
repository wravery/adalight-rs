#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod gamma_correction;
mod hidden_window;
mod opc_pool;
mod pixel_buffer;
mod screen_samples;
mod serial_port;
mod settings;
mod update_timer;

use std::fs;

use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{DispatchMessageA, GetMessageA, TranslateMessage, MSG},
};

use {hidden_window::HiddenWindow, settings::Settings, update_timer::UpdateTimer};

fn main() {
    let config_json = fs::read_to_string("AdaLight.config.json").expect("read config file");
    let settings = Settings::from_str(&config_json);

    match settings {
        Ok(settings) => {
            let timer = UpdateTimer::new(settings);
            let _hidden_window = HiddenWindow::new(timer);
            let mut msg = MSG::default();

            unsafe {
                loop {
                    match GetMessageA(&mut msg, HWND::default(), 0, 0).0 {
                        -1 => {
                            HiddenWindow::display_last_error();
                            break;
                        }
                        0 => break,
                        _ => {
                            TranslateMessage(&msg);
                            DispatchMessageA(&msg);
                        }
                    }
                }
            }
        }
        Err(error) => eprintln!("Settings Error: {:?}", error),
    }
}
