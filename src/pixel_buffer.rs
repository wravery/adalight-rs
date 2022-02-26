use crate::settings::{OpcChannel, Settings};

/// Each message uses the same header every time it is sent.
struct Header(Vec<u8>);

/// Representation of a fixed size message buffer for either a [crate::serial_port::SerialPort]
/// or [crate::opc_pool::OpcPool].
pub struct PixelBuffer {
    pub buffer: Vec<u8>,
    alpha_channel: bool,
    offset: Header,
    position: usize,
}

impl PixelBuffer {
    /// Allocate a new [PixelBuffer] for the Arduino listening on a [crate::serial_port::SerialPort].
    pub fn new_serial_buffer(settings: &Settings) -> Self {
        let led_count = (settings.get_total_led_count() - 1) as u16;
        let led_count_high = ((led_count & 0xFF00) >> 8) as u8;
        let led_count_low = (led_count & 0xFF) as u8;
        let led_count_checksum = led_count_high ^ led_count_low ^ 0x55;
        let offset = Header(vec![
            b'A',
            b'd',
            b'a',
            led_count_high,
            led_count_low,
            led_count_checksum,
        ]);
        let position = offset.0.len();
        let buffer_size = position + (3 * settings.get_total_led_count());
        let mut buffer = Vec::new();
        buffer.reserve_exact(buffer_size);
        buffer.extend_from_slice(&offset.0);
        buffer.resize(buffer_size, 0_u8);

        Self {
            buffer,
            alpha_channel: false,
            offset,
            position,
        }
    }

    /// Allocate a new [PixelBuffer] to send to an [crate::opc_pool::OpcPool] which
    /// implements the standard OPC protocol and does not support the `alphaChannel`.
    pub fn new_opc_buffer(opc_channel: &OpcChannel) -> Self {
        let channel = opc_channel.channel;
        let command = 0_u8;
        let opc_data_size = (3 * opc_channel.get_total_pixel_count()) as u16;
        let length_high = ((opc_data_size & 0xFF00) >> 8) as u8;
        let length_low = (opc_data_size & 0xFF) as u8;
        let offset = Header(vec![channel, command, length_high, length_low]);
        let position = offset.0.len();
        let buffer_size = position + (3 * opc_channel.get_total_pixel_count());
        let mut buffer = Vec::new();
        buffer.reserve_exact(buffer_size);
        buffer.extend_from_slice(&offset.0);
        buffer.resize(buffer_size, 0_u8);

        Self {
            buffer,
            alpha_channel: false,
            offset,
            position,
        }
    }

    /// Allocate a new [PixelBuffer] to send to an [crate::opc_pool::OpcPool] which
    /// implements the `BobLight` OPC protocol extension and supports the `alphaChannel`.
    /// # WARNING
    /// This has not been tested against any real server implementations.
    pub fn new_bob_buffer(opc_channel: &OpcChannel) -> Self {
        let channel = opc_channel.channel;
        let command = 255_u8;
        let opc_data_size = (3 * opc_channel.get_total_pixel_count()) as u16;
        let length_high = ((opc_data_size & 0xFF00) >> 8) as u8;
        let length_low = (opc_data_size & 0xFF) as u8;
        let system_id = 0xB0B_u16;
        let system_id_high = ((system_id & 0xFF00) >> 8) as u8;
        let system_id_low = (system_id & 0xFF) as u8;
        let offset = Header(vec![
            channel,
            command,
            length_high,
            length_low,
            system_id_high,
            system_id_low,
        ]);
        let position = offset.0.len();
        let buffer_size = position + (4 * opc_channel.get_total_pixel_count());
        let mut buffer = Vec::new();
        buffer.reserve_exact(buffer_size);
        buffer.extend_from_slice(&offset.0);
        buffer.resize(buffer_size, 0_u8);

        Self {
            buffer,
            alpha_channel: true,
            offset,
            position,
        }
    }

    /// Add an RGBA pixel to the [PixelBuffer].
    pub fn add(&mut self, rgba_pixel: u32) {
        self.buffer[self.position] = ((rgba_pixel & 0xFF000000) >> 24) as u8;
        self.position += 1;
        self.buffer[self.position] = ((rgba_pixel & 0xFF0000) >> 16) as u8;
        self.position += 1;
        self.buffer[self.position] = ((rgba_pixel & 0xFF00) >> 8) as u8;
        self.position += 1;

        if self.alpha_channel {
            self.buffer[self.position] = (rgba_pixel & 0xFF) as u8;
            self.position += 1;
        }
    }

    /// Reset the buffer position to the start of the pixel data in the [PixelBuffer].
    pub fn clear(&mut self) {
        let buffer_size = self.buffer.len();
        if buffer_size > self.offset.0.len() {
            self.position = self.offset.0.len();
            self.buffer.resize(self.offset.0.len(), 0_u8);
            self.buffer.resize(buffer_size, 0_u8);
        }
    }

    /// Get a [u8] slice for the full [PixelBuffer] buffer, including the [Header] at
    /// the beginning.
    pub fn data(&self) -> &[u8] {
        &self.buffer
    }
}
