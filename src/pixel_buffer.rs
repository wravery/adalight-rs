use crate::settings::{OpcChannel, Settings};

struct Header(Vec<u8>);

pub struct PixelBuffer {
    pub buffer: Vec<u8>,
    alpha_channel: bool,
    offset: Header,
    position: usize,
}

impl PixelBuffer {
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

    pub fn clear(&mut self) {
        self.position = self.offset.0.len();
    }

    pub fn data(&self) -> &[u8] {
        &self.buffer
    }
}
