mod gamma_correction;
mod pixel_buffer;
mod screen_samples;
mod serial_port;
mod settings;

use gamma_correction::GammaLookup;
use pixel_buffer::PixelBuffer;
use screen_samples::ScreenSamples;
use serial_port::SerialPort;
use settings::Settings;

fn main() {
    let settings = Settings::from_str(
        r#"
{
    /*
     * Minimum LED brightness; some users prefer a small amount of backlighting
     * at all times, regardless of screen content. Higher values are brighter,
     * or set to 0 to disable this feature.
     */
    "minBrightness": 64,

    /*
     * LED transition speed; it's sometimes distracting if LEDs instantaneously
     * track screen contents (such as during bright flashing sequences), so this
     * feature enables a gradual fade to each new LED state. Higher numbers yield
     * slower transitions (max of 0.5), or set to 0 to disable this feature
     * (immediate transition of all LEDs).
     */
    "fade": 0,

    /*
     * Serial device timeout (in milliseconds), for locating Arduino device
     * running the corresponding LEDstream code.
     */
    "timeout": 5000, // 5 seconds

    /*
     * Cap the refresh rate at 30 FPS. If the update takes longer the FPS
     * will actually be lower.
     */
    "fpsMax": 30,

    /*
     * Timer frequency (in milliseconds) when we're throttled, e.g. when a UAC prompt
     * is displayed. If this value is higher, we'll use less CPU when we can't sample
     * the display, but it will take longer to resume sampling again.
     */
    "throttleTimer": 3000, // 3 seconds

    /*
     * This array contains details for each display that the software will
     * process. The horizontalCount is the number LEDs accross the top of the
     * AdaLight board, and the verticalCount is the number of LEDs up and down
     * the sides. These counts are used to figure out how big a block of pixels
     * should be to sample the edge around each LED.  If you have screen(s)
     * attached that are not among those being "Adalighted," you only need to
     * include them in this list if they show up before the "Adalighted"
     * display(s) in the system's display enumeration. If you have multiple
     * displays this might require some trial and error to figure out the precise
     * order relative to your setup. To leave a gap in the list and include another
     * display after that, just include an entry for the skipped display with
     * { 0, 0 } for the horizontalCount and verticalCount.
     */
    "displays": [
        {
            "horizontalCount": 10,
            "verticalCount": 5,

            "positions": [
                // Bottom edge, left half
                { "x": 3, "y": 4 }, { "x": 2, "y": 4 }, { "x": 1, "y": 4 },
                // Left edge
                { "x": 0, "y": 4 }, { "x": 0, "y": 3 }, { "x": 0, "y": 2 }, { "x": 0, "y": 1 },
                // Top edge
                { "x": 0, "y": 0 }, { "x": 1, "y": 0 }, { "x": 2, "y": 0 }, { "x": 3, "y": 0 }, { "x": 4, "y": 0 },
                { "x": 5, "y": 0 }, { "x": 6, "y": 0 }, { "x": 7, "y": 0 }, { "x": 8, "y": 0 }, { "x": 9, "y": 0 },
                // Right edge
                { "x": 9, "y": 1 }, { "x": 9, "y": 2 }, { "x": 9, "y": 3 }, { "x": 9, "y": 4 },
                // Bottom edge, right half
                { "x": 8, "y": 4 }, { "x": 7, "y": 4 }, { "x": 6, "y": 4 }
            ]
        }
    ],

    /*
     * OPC server configuration includes the hostname, port (as a string for getaddrinfo)
     * and a collection of sub-channels and pixel ranges mapped to portions of the AdaLight
     * display. Each range of pixels for an OPC (Open Pixel Controller) server is represented
     * by a channel and a pixelCount. Ranges are contiguous starting at 0 for each channel,
     * so to leave a gap in the channel you would create a range of pixels which don't map to
     * any LEDs. The pixels sampled from the display will be interpolated with an even
     * distribution over the range of pixels in the order that they appear in displayIndex.
     * The 2-dimensional array displayIndex[i][j] maps to the display at index i and the
     * sub-pixel at index j. That way we don't need to re-define the displays or get new
     * samples separately for OPC, we can just take samples and then re-render those samples
     * to both the AdaLight over a serial port and the OPC server over TCP/IP.
     */
    "servers": [
        {
            "host": "192.168.1.14",
            "port": "80",
            "alphaChannel": false,

            "channels": [
                {
                    "channel": 2,

                    "pixels": [
                        // The top edge is not proportional to the display in the OPC strip,
                        // the first 83 pixels go from the top right to the top left. There's
                        // also a 4 pixel gap between the first 64 pixels and the last 19,
                        // so we need to divide that into 3 ranges.
                        {
                            "pixelCount": 64,

                            // Top edge (right to left)
                            "displayIndex": [ [ 16, 15, 14, 13, 12, 11, 10, 9 ] ]
                        },
                        {
                            "pixelCount": 4,

                            // Top edge (gap)
                            "displayIndex": []
                        },
                        {
                            "pixelCount": 19,

                            // Top edge (right to left)
                            "displayIndex": [ [ 8, 7 ] ]
                        },

                        // The left edge continues down from the top left corner and reaches
                        // the bottom with 29 pixels. Note the overlap between these edges on the
                        // display, both ranges of pixels end up using the origin (0, 0) in the
                        // top-left corner of the display.
                        {
                            "pixelCount": 29,

                            // Left edge (top to bottom)
                            "displayIndex": [ [ 7, 6, 5, 4, 3 ] ]
                        }
                    ]
                }
            ]
        }
    ]
}"#,
    );

    match settings {
        Ok(settings) => {
            let _serial = SerialPort::new(&settings);
            let gamma = GammaLookup::new();
            let mut samples = ScreenSamples::new(&settings, &gamma);
            let mut serial_buffer = PixelBuffer::new_serial_buffer(&settings);
            let opc_channel = &settings.servers[0].channels[0];
            let mut opc_buffer = PixelBuffer::new_opc_buffer(opc_channel);
            let mut bob_buffer = PixelBuffer::new_bob_buffer(opc_channel);
            let mut port = SerialPort::new(&settings);

            let port_opened = port.open();

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
                    if samples.render_channel(opc_channel, &mut opc_buffer) {
                        println!("OPC buffer: {}", opc_buffer.data().len());
                    }
                    if samples.render_channel(opc_channel, &mut bob_buffer) {
                        println!("BOB buffer: {}", bob_buffer.data().len());
                    }
                }
                Err(error) => eprintln!("Samples Error: {:?}", error),
            }

            println!("Settings: {settings:?}");
        }
        Err(error) => eprintln!("Settings Error: {:?}", error),
    }
}
