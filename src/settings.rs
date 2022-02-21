use regex::Regex;

use serde::Deserialize;
use serde_json::Result;

/// This struct contains the 2D coordinates corresponding to each pixel in the
/// LED strand, in the order that they're connected (i.e. the first element
/// here belongs to the first LED in the strand, second element is the second
/// LED, and so forth). Each pair in this array consists of an X and Y
/// coordinate specified in the grid units given for that display where
/// `{ 0, 0 }` is the top-left corner of the display.
#[derive(Debug)]
pub struct LedPosition {
    pub x: usize,
    pub y: usize,
}

#[doc(hidden)]
#[derive(Deserialize)]
struct JsonLedPosition {
    pub x: usize,
    pub y: usize,
}

impl From<JsonLedPosition> for LedPosition {
    fn from(json: JsonLedPosition) -> Self {
        Self {
            x: json.x,
            y: json.y,
        }
    }
}

/// This struct contains details for each display that the software will
/// process. The horizontalCount is the number LEDs accross the top of the
/// AdaLight board, and the verticalCount is the number of LEDs up and down
/// the sides. These counts are used to figure out how big a block of pixels
/// should be to sample the edge around each LED.  If you have screen(s)
/// attached that are not among those being "Adalighted," you only need to
/// include them in this list if they show up before the "Adalighted"
/// display(s) in the system's display enumeration. If you have multiple
/// displays this might require some trial and error to figure out the precise
/// order relative to your setup. To leave a gap in the list and include another
/// display after that, just include an entry for the skipped display with
/// `{ 0, 0 }` for the horizontalCount and verticalCount.
#[derive(Debug)]
pub struct DisplayConfiguration {
    pub horizontal_count: usize,
    pub vertical_count: usize,
    pub positions: Vec<LedPosition>,
}

#[doc(hidden)]
#[derive(Deserialize)]
#[allow(non_snake_case)]
struct JsonDisplayConfiguration {
    pub horizontalCount: usize,
    pub verticalCount: usize,
    pub positions: Vec<JsonLedPosition>,
}

impl From<JsonDisplayConfiguration> for DisplayConfiguration {
    fn from(json: JsonDisplayConfiguration) -> Self {
        Self {
            horizontal_count: json.horizontalCount,
            vertical_count: json.verticalCount,
            positions: json
                .positions
                .into_iter()
                .map(|position| position.into())
                .collect(),
        }
    }
}

/// Each range of pixels for an OPC (Open Pixel Controller) server is represented
/// by a channel and a pixelCount. Ranges are contiguous starting at 0 for each
/// channel, so to leave a gap in the channel you would create a range of pixels
/// which don't map to any LEDs. The pixels sampled from the display will be
/// interpolated with an even distribution over the range of pixels in the order
/// that they appear in displayIndex. The 2-dimensional vector `displayIndex[i][j]`
/// maps to the display at index `i` and the sub-pixel at index `j`. That way we don't
/// need to re-define the displays or get new samples separately for OPC, we can
/// just take samples and then re-render those samples to both the AdaLight over
/// a serial port and the OPC server over TCP/IP.
#[derive(Debug)]
pub struct OpcPixelRange {
    pub pixel_count: usize,
    pub display_index: Vec<Vec<usize>>,
    #[doc(hidden)]
    sample_count: usize,
    #[doc(hidden)]
    kernel_radius: usize,
    #[doc(hidden)]
    kernel_weights: Vec<f64>,
}

impl OpcPixelRange {
    /// Get the numer of display samples that should be used to drive this range
    /// of OPC pixels.
    pub fn get_sample_count(&self) -> usize {
        self.sample_count
    }

    /// When performing a Guassian blur over a range of OPC pixels, get the radius
    /// of the blur kernel.
    pub fn get_kernel_radius(&self) -> usize {
        self.kernel_radius
    }

    /// Get the weights of the elements in the blur kernel.
    pub fn get_kernel_weights(&self) -> &[f64] {
        &self.kernel_weights
    }
}

#[doc(hidden)]
#[derive(Deserialize)]
#[allow(non_snake_case)]
struct JsonOpcPixelRange {
    pub pixelCount: usize,
    pub displayIndex: Vec<Vec<usize>>,
}

impl From<JsonOpcPixelRange> for OpcPixelRange {
    fn from(json: JsonOpcPixelRange) -> Self {
        let mut pixel_range = Self {
            pixel_count: json.pixelCount,
            display_index: json.displayIndex,
            sample_count: 0,
            kernel_radius: 0,
            kernel_weights: vec![],
        };

        for display in pixel_range.display_index.iter() {
            pixel_range.sample_count += display.len();
        }

        // Build the 1 dimensional Gaussian kernel for this range. The standard deviation term plugged into
        // the Gaussian function is 1/3 of the radius since the curve approaches 0 beyond 3 standard deviations.
        if pixel_range.sample_count > 1 && pixel_range.pixel_count >= 3 * pixel_range.sample_count {
            pixel_range.kernel_radius = pixel_range.pixel_count / (2 * pixel_range.sample_count);
            let samples = (2 * pixel_range.kernel_radius) + 1;
            let denominator = (pixel_range.kernel_radius * pixel_range.kernel_radius) as f64 / 4.5;
            let mut total = 1.0;

            // The midpoint is always 1.
            pixel_range.kernel_weights.resize(samples, 0.0);
            pixel_range.kernel_weights[pixel_range.kernel_radius] = 1.0;

            // We only need to compute the first half, the second half is a mirror of those values.
            for x in 0..pixel_range.kernel_radius {
                let diff = x as f64 - pixel_range.kernel_radius as f64;
                let weight = (-(diff * diff) / denominator).exp();

                // Set the weight on both sides of the curve.
                total += 2.0 * weight;
                pixel_range.kernel_weights[x] = weight;
                pixel_range.kernel_weights[samples - x - 1] = weight;
            }

            // Normalize the weights so the area under the curve is 1.
            pixel_range.kernel_weights = pixel_range
                .kernel_weights
                .into_iter()
                .map(|weight| weight / total)
                .collect();
        }

        pixel_range
    }
}

/// Each channel can have multiple ranges. They cannot overlap, but if they
/// don't cover the whole range of pixels on the channel we'll just send smaller
/// buffers and we won't set the pixels on the remainder.
#[derive(Debug)]
pub struct OpcChannel {
    pub channel: u8,
    pub pixels: Vec<OpcPixelRange>,
    #[cfg(test)]
    total_sample_count: usize,
    #[doc(hidden)]
    total_pixel_count: usize,
}

impl OpcChannel {
    /// Get the count of display samples for this channel.
    #[cfg(test)]
    pub fn get_total_sample_count(&self) -> usize {
        self.total_sample_count
    }

    /// Get the total number of OPC pixels that the samples should be spread across.
    pub fn get_total_pixel_count(&self) -> usize {
        self.total_pixel_count
    }
}

impl From<JsonOpcChannel> for OpcChannel {
    fn from(json: JsonOpcChannel) -> Self {
        let mut channel = Self {
            channel: json.channel,
            pixels: json.pixels.into_iter().map(|pixel| pixel.into()).collect(),
            #[cfg(test)]
            total_sample_count: 0,
            total_pixel_count: 0,
        };

        for pixel_range in channel.pixels.iter() {
            #[cfg(test)]
            {
                channel.total_sample_count += pixel_range.sample_count;
            }
            channel.total_pixel_count += pixel_range.pixel_count;
        }

        channel
    }
}

#[doc(hidden)]
#[derive(Deserialize)]
#[allow(non_snake_case)]
struct JsonOpcChannel {
    pub channel: u8,
    pub pixels: Vec<JsonOpcPixelRange>,
}

/// OPC server configuration includes the hostname, port (as a string for getaddrinfo)
/// and a collection of sub-channels and pixel ranges mapped to portions of the AdaLight
/// display.
#[derive(Debug)]
pub struct OpcServer {
    pub host: String,
    pub port: String,
    pub alpha_channel: bool,
    pub channels: Vec<OpcChannel>,
}

#[doc(hidden)]
#[derive(Deserialize)]
#[allow(non_snake_case)]
struct JsonOpcServer {
    pub host: String,
    pub port: String,
    pub alphaChannel: bool,
    pub channels: Vec<JsonOpcChannel>,
}

impl From<JsonOpcServer> for OpcServer {
    fn from(json: JsonOpcServer) -> Self {
        Self {
            host: json.host,
            port: json.port,
            alpha_channel: json.alphaChannel,
            channels: json
                .channels
                .into_iter()
                .map(|channel| channel.into())
                .collect(),
        }
    }
}

fn strip_comments(json: &str) -> String {
    #[derive(Debug)]
    enum State {
        Parsed,
        QuotedString,
        CommentBlock,
    }

    let mut state = State::Parsed;
    let mut output = Vec::new();
    let start_token = Regex::new(r#"(?:"|(?:/[/*]))"#).expect("build regex");
    let quoted = Regex::new(r#"(?m)^(?:[^"]|(?:\\"))*"#).expect("build regex");
    let end_block = Regex::new(r#"(?:\*/)"#).expect("build regex");
    let empty_line = Regex::new(r#"(?m)^\s*$"#).expect("build regex");

    for mut line in json.lines() {
        let mut content = String::new();

        loop {
            if line.is_empty() {
                break;
            }

            match state {
                State::Parsed => match start_token.find(line) {
                    Some(mat) if mat.as_str() == r#"""# => {
                        let start_quote = mat.end();
                        content.push_str(&line[..start_quote]);
                        line = &line[start_quote..];
                        state = State::QuotedString;
                    }
                    Some(mat) => {
                        let start_block = mat.start();
                        content.push_str(&line[..start_block]);

                        match mat.as_str() {
                            r#"/*"# => {
                                let start_block = mat.end();
                                line = &line[start_block..];
                                state = State::CommentBlock;
                            }
                            r#"//"# => break,
                            _ => unreachable!(),
                        }
                    }
                    None => {
                        content.push_str(line);
                        break;
                    }
                },
                State::QuotedString => match quoted.find(line) {
                    Some(mat) => {
                        let mut end_quote = mat.end();

                        if end_quote < line.len() {
                            end_quote += 1;
                            state = State::Parsed;
                        }

                        content.push_str(&line[..end_quote]);
                        line = &line[end_quote..];
                    }
                    None => break,
                },
                State::CommentBlock => match end_block.find(line) {
                    Some(mat) => {
                        let end_block = mat.end();
                        line = &line[end_block..];
                        state = State::Parsed;
                    }
                    None => break,
                },
            }
        }

        if !empty_line.is_match(&content) {
            output.push(content);
        }
    }

    output.join("\n")
}

/// Access all of the settings for AdaLight.
#[derive(Debug)]
pub struct Settings {
    /// Minimum LED brightness; some users prefer a small amount of backlighting
    /// at all times, regardless of screen content. Higher values are brighter,
    /// or set to 0 to disable this feature.
    pub min_brightness: u8,

    /// LED transition speed; it's sometimes distracting if LEDs instantaneously
    /// track screen contents (such as during bright flashing sequences), so this
    /// feature enables a gradual fade to each new LED state. Higher numbers yield
    /// slower transitions (max of 0.5), or set to 0 to disable this feature
    /// (immediate transition of all LEDs).
    pub fade: f64,

    /// Serial device timeout (in milliseconds), for locating Arduino device
    /// running the corresponding LEDstream code.
    pub timeout: u32,

    /// Cap the refresh rate at 30 FPS. If the update takes longer the FPS
    /// will actually be lower.
    pub fps_max: u32,

    /// Timer frequency (in milliseconds) when we're throttled, e.g. when a UAC prompt
    /// is displayed. If this value is higher, we'll use less CPU when we can't sample
    /// the display, but it will take longer to resume sampling again.
    pub throttle_timer: u32,

    /// Set of displays that should be sampled to drive the LED display.
    pub displays: Vec<DisplayConfiguration>,

    /// Set of OPC (Open Pixel Controller) servers and channels which should also be
    /// driven by the display samples.
    pub servers: Vec<OpcServer>,

    #[doc(hidden)]
    min_brightness_color: u32,
    #[doc(hidden)]
    total_led_count: usize,
    #[doc(hidden)]
    weight: f64,
    #[doc(hidden)]
    delay: u32,
}

impl Settings {
    /// Strip any JSON comments for backwards compatibility and parse the settings
    /// from a configuration file.
    pub fn from_str(json: &str) -> Result<Self> {
        let json = strip_comments(json);
        let json: JsonSettings = serde_json::from_str(&json)?;
        Ok(json.into())
    }

    /// Determine the color of an LED at its minimum brightness.
    pub fn get_min_brightness_color(&self) -> u32 {
        self.min_brightness_color
    }

    /// Get the count of all LEDs across all displays.
    pub fn get_total_led_count(&self) -> usize {
        self.total_led_count
    }

    /// Get the inverse of the fade value (1.0 - fade).
    pub fn get_weight(&self) -> f64 {
        self.weight
    }

    /// Get the delay in milliseconds per frame to limit the maximum frames-per-second.
    pub fn get_delay(&self) -> u32 {
        self.delay
    }
}

#[doc(hidden)]
#[derive(Deserialize)]
#[allow(non_snake_case)]
struct JsonSettings {
    pub minBrightness: u8,
    pub fade: f64,
    pub timeout: u32,
    pub fpsMax: u32,
    pub throttleTimer: u32,
    pub displays: Vec<JsonDisplayConfiguration>,
    pub servers: Vec<JsonOpcServer>,
}

impl From<JsonSettings> for Settings {
    fn from(json: JsonSettings) -> Self {
        let mut settings = Self {
            min_brightness: json.minBrightness,
            fade: json.fade,
            timeout: json.timeout,
            fps_max: json.fpsMax,
            throttle_timer: json.throttleTimer,
            displays: json
                .displays
                .into_iter()
                .map(|display| display.into())
                .collect(),
            servers: json
                .servers
                .into_iter()
                .map(|server| server.into())
                .collect(),
            min_brightness_color: 0,
            total_led_count: 0,
            weight: 0.0,
            delay: 0,
        };

        let min_brightness_channel = u32::from(settings.min_brightness / 3) & 0xFF;
        settings.min_brightness_color = (min_brightness_channel << 24) // red
            | (min_brightness_channel << 16) // green
            | (min_brightness_channel << 8) // blue
            | 0xFF; // alpha

        for display in settings.displays.iter() {
            settings.total_led_count += display.positions.len();
        }

        settings.weight = 1.0 - settings.fade;
        settings.delay = 1000 / settings.fps_max;

        settings
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_led_position() {
        let led_position: JsonLedPosition =
            serde_json::from_str(r#"{ "x": 3, "y": 4 }"#).expect("parse the JsonLedPosition");
        let led_position: LedPosition = led_position.into();
        assert_eq!(led_position.x, 3);
        assert_eq!(led_position.y, 4);
    }

    #[test]
    fn parse_display_configuration() {
        let display_configuration: JsonDisplayConfiguration =
            serde_json::from_str(r#"
{
    "horizontalCount": 10,
    "verticalCount": 5,
    "positions": [
        { "x": 3, "y": 4 }, { "x": 2, "y": 4 }, { "x": 1, "y": 4 },
        { "x": 0, "y": 4 }, { "x": 0, "y": 3 }, { "x": 0, "y": 2 }, { "x": 0, "y": 1 },
        { "x": 0, "y": 0 }, { "x": 1, "y": 0 }, { "x": 2, "y": 0 }, { "x": 3, "y": 0 }, { "x": 4, "y": 0 },
        { "x": 5, "y": 0 }, { "x": 6, "y": 0 }, { "x": 7, "y": 0 }, { "x": 8, "y": 0 }, { "x": 9, "y": 0 },
        { "x": 9, "y": 1 }, { "x": 9, "y": 2 }, { "x": 9, "y": 3 }, { "x": 9, "y": 4 },
        { "x": 8, "y": 4 }, { "x": 7, "y": 4 }, { "x": 6, "y": 4 }
    ]
}"#).expect("parse the JsonDisplayConfiguration");
        let display_configuration: DisplayConfiguration = display_configuration.into();
        assert_eq!(display_configuration.horizontal_count, 10);
        assert_eq!(display_configuration.vertical_count, 5);
        assert_eq!(display_configuration.positions.len(), 24);
    }

    #[test]
    fn parse_opc_pixel_range() {
        let opc_pixel_range: JsonOpcPixelRange = serde_json::from_str(
            r#"
{
    "pixelCount": 64,
    "displayIndex": [ [ 16, 15, 14, 13, 12, 11, 10, 9 ] ]
}"#,
        )
        .expect("parse the JsonOpcPixelRange");
        let opc_pixel_range: OpcPixelRange = opc_pixel_range.into();
        assert_eq!(opc_pixel_range.pixel_count, 64);
        assert_eq!(opc_pixel_range.display_index.len(), 1);
        let expected: Vec<usize> = (9..=16).into_iter().rev().collect();
        assert_eq!(opc_pixel_range.display_index[0], expected);
        assert_eq!(opc_pixel_range.get_sample_count(), 8);
        assert_eq!(opc_pixel_range.get_kernel_radius(), 4);
        let kernel_weights = opc_pixel_range.get_kernel_weights();
        assert_eq!(kernel_weights.len(), 9);
        let total = kernel_weights
            .iter()
            .copied()
            .reduce(|total, weight| total + weight)
            .expect("sum the weights");
        assert!((1.0 - total).abs() < 2.0 * f64::EPSILON);
    }

    #[test]
    fn parse_opc_channel() {
        let opc_channel: JsonOpcChannel = serde_json::from_str(
            r#"
{
    "channel": 2,
    "pixels": [
        {
            "pixelCount": 64,
            "displayIndex": [ [ 16, 15, 14, 13, 12, 11, 10, 9 ] ]
        },
        {
            "pixelCount": 4,
            "displayIndex": []
        },
        {
            "pixelCount": 19,
            "displayIndex": [ [ 8, 7 ] ]
        },
        {
            "pixelCount": 29,
            "displayIndex": [ [ 7, 6, 5, 4, 3 ] ]
        }
    ]
}"#,
        )
        .expect("parse the JsonOpcChannel");
        let opc_channel: OpcChannel = opc_channel.into();
        assert_eq!(opc_channel.channel, 2);
        assert_eq!(opc_channel.pixels.len(), 4);
        assert_eq!(opc_channel.get_total_sample_count(), 15);
        assert_eq!(opc_channel.get_total_pixel_count(), 116);
    }

    #[test]
    fn parse_opc_server() {
        let opc_server: JsonOpcServer = serde_json::from_str(
            r#"
{
    "host": "192.168.1.14",
    "port": "80",
    "alphaChannel": false,

    "channels": [
        {
            "channel": 2,
            "pixels": [
                {
                    "pixelCount": 64,
                    "displayIndex": [ [ 16, 15, 14, 13, 12, 11, 10, 9 ] ]
                },
                {
                    "pixelCount": 4,
                    "displayIndex": []
                },
                {
                    "pixelCount": 19,
                    "displayIndex": [ [ 8, 7 ] ]
                },

                {
                    "pixelCount": 29,
                    "displayIndex": [ [ 7, 6, 5, 4, 3 ] ]
                }
            ]
        }
    ]
}"#,
        )
        .expect("parse the JsonOpcServer");
        let opc_server: OpcServer = opc_server.into();
        assert_eq!(&opc_server.host, "192.168.1.14");
        assert_eq!(&opc_server.port, "80");
        assert!(!opc_server.alpha_channel);
        assert_eq!(opc_server.channels.len(), 1);
    }

    #[test]
    fn parse_settings() {
        let settings = Settings::from_str(r#"
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
        ).expect("parse the sample");
        assert_eq!(settings.min_brightness, 64);
        assert_eq!(settings.fade, 0.0);
        assert_eq!(settings.timeout, 5000);
        assert_eq!(settings.fps_max, 30);
        assert_eq!(settings.throttle_timer, 3000);
        assert_eq!(settings.displays.len(), 1);
        assert_eq!(settings.servers.len(), 1);
        assert_eq!(settings.get_min_brightness_color(), 0x151515FF);
        assert_eq!(settings.get_total_led_count(), 24);
        assert_eq!(settings.get_weight(), 1.0);
        assert_eq!(settings.get_delay(), 33);
    }
}
