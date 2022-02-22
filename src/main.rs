mod gamma_correction;
mod opc_pool;
mod pixel_buffer;
mod screen_samples;
mod serial_port;
mod settings;

use std::fs;

use gamma_correction::GammaLookup;
use opc_pool::OpcPool;
use pixel_buffer::PixelBuffer;
use screen_samples::ScreenSamples;
use serial_port::SerialPort;
use settings::Settings;

fn main() {
    let config_json = fs::read_to_string("AdaLight.config.json").expect("read config file");
    let settings = Settings::from_str(&config_json);

    match settings {
        Ok(settings) => {
            let _serial = SerialPort::new(&settings);
            let gamma = GammaLookup::new();
            let mut samples = ScreenSamples::new(&settings, &gamma);
            let mut serial_buffer = PixelBuffer::new_serial_buffer(&settings);
            let mut port = SerialPort::new(&settings);
            let mut pool = OpcPool::new(&settings);

            let port_opened = port.open();
            let pool_opened = pool.open();

            if samples.is_empty() {
                samples
                    .create_resources()
                    .expect("create DXGI and D3D11 resources");
            }

            match samples.take_samples() {
                Ok(()) => {
                    println!("Got samples!");
                    if samples.render_serial(&mut serial_buffer) {
                        println!("Serial buffer: {}", serial_buffer.data().len());

                        if port_opened {
                            port.send(&serial_buffer);
                        }
                    }

                    for (i, server) in settings.servers.iter().enumerate() {
                        for channel in server.channels.iter() {
                            let mut pixels = if server.alpha_channel {
                                PixelBuffer::new_bob_buffer(channel)
                            } else {
                                PixelBuffer::new_opc_buffer(channel)
                            };

                            if samples.render_channel(channel, &mut pixels) {
                                println!("OPC buffer: {}", pixels.data().len());

                                if pool_opened {
                                    pool.send(i, &pixels);
                                }
                            }
                        }
                    }
                }
                Err(error) => eprintln!("Samples Error: {:?}", error),
            }

            println!("Settings: {settings:?}");
        }
        Err(error) => eprintln!("Settings Error: {:?}", error),
    }
}
