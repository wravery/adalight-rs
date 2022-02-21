use std::{mem, ptr};

use windows::{
    core::{Interface, Result},
    Win32::{
        Foundation::{E_FAIL, HINSTANCE, SIZE},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
                D3D11_BIND_FLAG, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_CREATE_DEVICE_SINGLETHREADED, D3D11_MAP_READ, D3D11_RESOURCE_MISC_FLAG,
                D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
            },
            Dxgi::{
                Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
                CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput1,
                IDXGIOutputDuplication, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_INVALID_CALL,
                DXGI_ERROR_UNSUPPORTED,
            },
        },
        System::SystemInformation::GetTickCount64,
    },
};

use crate::{
    gamma_correction::GammaLookup,
    pixel_buffer::PixelBuffer,
    settings::{OpcChannel, Settings},
};

struct DisplayResources {
    pub _adapter: IDXGIAdapter1,
    pub _device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub duplication: IDXGIOutputDuplication,
    pub staging: Option<ID3D11Texture2D>,
    pub acquired_frame: bool,
    pub bounds: SIZE,
}

struct PixelOffset {
    pub x: usize,
    pub y: usize,
}

const PIXEL_SAMPLES: usize = 16;
const OFFSET_ARRAY_SIZE: usize = PIXEL_SAMPLES * PIXEL_SAMPLES;

struct OffsetArray(Vec<PixelOffset>);

pub struct ScreenSamples<'a> {
    parameters: &'a Settings,
    gamma: &'a GammaLookup,
    factory: Option<IDXGIFactory1>,
    displays: Vec<DisplayResources>,
    pixel_offsets: Vec<Vec<OffsetArray>>,
    previous_colors: Vec<u32>,
    acquired_resources: bool,
    frame_count: usize,
    start_tick: u64,
    frame_rate: f64,
}

impl<'a> ScreenSamples<'a> {
    pub fn new(parameters: &'a Settings, gamma: &'a GammaLookup) -> Self {
        Self {
            parameters,
            gamma,
            factory: None,
            displays: Vec::new(),
            pixel_offsets: Vec::new(),
            previous_colors: Vec::new(),
            acquired_resources: false,
            frame_count: 0,
            start_tick: 0,
            frame_rate: 0.0,
        }
    }

    pub fn create_resources(&mut self) -> Result<()> {
        if self.acquired_resources {
            return Ok(());
        }

        let display_len = self.parameters.displays.len();
        self.displays.reserve(display_len);
        let factory = self.get_factory()?;

        for i in 0..(display_len as u32) {
            unsafe {
                match factory.EnumAdapters1(i) {
                    Ok(ref adapter) => {
                        for j in 0..(display_len as u32) {
                            match adapter.EnumOutputs(j) {
                                Ok(output) => {
                                    let output: IDXGIOutput1 = output.cast()?;
                                    let output_description = match output.GetDesc() {
                                        Ok(description) => description,
                                        Err(_) => continue,
                                    };
                                    if !output_description.AttachedToDesktop.as_bool() {
                                        continue;
                                    }
                                    let mut device = None;
                                    let mut context = None;
                                    if D3D11CreateDevice(
                                        adapter,
                                        D3D_DRIVER_TYPE_UNKNOWN,
                                        HINSTANCE::default(),
                                        D3D11_CREATE_DEVICE_SINGLETHREADED
                                            | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                                        ptr::null(),
                                        0,
                                        D3D11_SDK_VERSION,
                                        &mut device,
                                        ptr::null_mut(),
                                        &mut context,
                                    )
                                    .is_err()
                                    {
                                        continue;
                                    }
                                    let (device, context) = match (device, context) {
                                        (Some(device), Some(context)) => (device, context),
                                        _ => continue,
                                    };
                                    let duplication = match output.DuplicateOutput(&device) {
                                        Ok(duplication) => duplication,
                                        Err(_) => continue,
                                    };
                                    let mut duplication_description = Default::default();
                                    duplication.GetDesc(&mut duplication_description);
                                    let use_map_desktop_surface = duplication_description
                                        .DesktopImageInSystemMemory
                                        .as_bool();
                                    let bounds = &output_description.DesktopCoordinates;
                                    let width = bounds.right - bounds.left;
                                    let height = bounds.bottom - bounds.top;
                                    let mut staging = None;

                                    if !use_map_desktop_surface {
                                        let texture_description = D3D11_TEXTURE2D_DESC {
                                            Width: width as u32,
                                            Height: height as u32,
                                            MipLevels: 1,
                                            ArraySize: 1,
                                            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                                            SampleDesc: DXGI_SAMPLE_DESC {
                                                Count: 1,
                                                Quality: 0,
                                            },
                                            Usage: D3D11_USAGE_STAGING,
                                            BindFlags: D3D11_BIND_FLAG(0),
                                            CPUAccessFlags: D3D11_CPU_ACCESS_READ,
                                            MiscFlags: D3D11_RESOURCE_MISC_FLAG(0),
                                        };
                                        staging =
                                            Some(device.CreateTexture2D(
                                                &texture_description,
                                                ptr::null(),
                                            )?);
                                    }

                                    self.displays.push(DisplayResources {
                                        _adapter: adapter.clone(),
                                        _device: device,
                                        context,
                                        duplication,
                                        staging,
                                        acquired_frame: false,
                                        bounds: SIZE {
                                            cx: width,
                                            cy: height,
                                        },
                                    })
                                }
                                Err(_) => break,
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        }

        if self.displays.is_empty() {
            E_FAIL.ok()?;
        }

        self.pixel_offsets
            .resize_with(self.displays.len(), Vec::new);

        for (i, display) in self.parameters.displays.iter().enumerate() {
            let bounds = &self.displays[i].bounds;
            let range_x = bounds.cx as f64 / display.horizontal_count as f64;
            let step_x = range_x / PIXEL_SAMPLES as f64;
            let range_y = bounds.cy as f64 / display.vertical_count as f64;
            let step_y = range_y / PIXEL_SAMPLES as f64;
            self.pixel_offsets[i].resize_with(display.positions.len(), || {
                let mut offsets = Vec::new();
                offsets.resize_with(OFFSET_ARRAY_SIZE, || PixelOffset { x: 0, y: 0 });
                OffsetArray(offsets)
            });
            for (j, led) in display.positions.iter().enumerate() {
                let mut x = [0_usize; PIXEL_SAMPLES];
                let mut y = [0_usize; PIXEL_SAMPLES];
                let start_x = (range_x * led.x as f64) + (step_x / 2.0);
                let start_y = (range_y * led.y as f64) + (step_y / 2.0);
                for i in 0..PIXEL_SAMPLES {
                    x[i] = (start_x + (step_x * (i as f64))) as usize;
                    y[i] = (start_y + (step_y * (i as f64))) as usize;
                }
                for (row, y) in y.iter().enumerate() {
                    for (col, x) in x.iter().enumerate() {
                        let pixel_index = (row * PIXEL_SAMPLES) + col;
                        self.pixel_offsets[i][j].0[pixel_index] = PixelOffset { x: *x, y: *y };
                    }
                }
            }
        }

        self.previous_colors = Vec::new();
        self.previous_colors.resize(
            self.parameters.get_total_led_count(),
            self.parameters.get_min_brightness_color(),
        );

        self.acquired_resources = true;
        self.start_tick = unsafe { GetTickCount64() };

        Ok(())
    }

    pub fn free_resources(&mut self) {
        if !self.acquired_resources {
            return;
        }

        for device in self
            .displays
            .iter_mut()
            .filter(|device| device.staging.is_some())
        {
            unsafe {
                if device.acquired_frame {
                    let _ = device.duplication.ReleaseFrame();
                    device.acquired_frame = false;
                }
            }
        }

        self.displays.clear();
        self.pixel_offsets.clear();

        if self.start_tick != 0 {
            self.frame_rate = (self.frame_count * 1000) as f64
                / (unsafe { GetTickCount64() } - self.start_tick) as f64;
            self.frame_count = 0;
            self.start_tick = 0;

            dbg!(format!("Frame Rate: {}", self.frame_rate));
        }

        self.acquired_resources = false;
    }

    pub fn take_samples(&mut self) -> Result<()> {
        if !self.acquired_resources {
            E_FAIL.ok()?;
        }

        // Take a screenshot for all of the devices that require a staging texture.
        for device in self
            .displays
            .iter_mut()
            .filter(|device| device.staging.is_some())
        {
            unsafe {
                if device.acquired_frame {
                    device.duplication.ReleaseFrame()?;
                    device.acquired_frame = false;
                }

                let mut info = Default::default();
                let mut resource = None;
                match device.duplication.AcquireNextFrame(
                    self.parameters.get_delay(),
                    &mut info,
                    &mut resource,
                ) {
                    Ok(()) => {
                        if let (Some(staging), Some(screen_texture)) =
                            (device.staging.clone(), resource)
                        {
                            let screen_texture: ID3D11Texture2D = screen_texture.cast()?;
                            device.acquired_frame = true;
                            device.context.CopyResource(staging, screen_texture);
                        }
                    }
                    Err(error) => match error.code() {
                        DXGI_ERROR_ACCESS_LOST | DXGI_ERROR_INVALID_CALL => {
                            // Recreate the duplication interface if this fails with with an expected
                            // error that invalidates the duplication interface or that might allow us
                            // to switch to MapDesktopSurface.
                            self.free_resources();
                            return Err(error);
                        }
                        _ => (),
                    },
                };
            }
        }

        let mut previous_color = self.previous_colors.iter_mut();

        for (i, device) in self.displays.iter_mut().enumerate() {
            let display = &self.parameters.displays[i];
            for j in 0..display.positions.len() {
                let offsets = &self.pixel_offsets[i][j];
                let (pixels, pitch) = if let Some(staging) = &device.staging {
                    unsafe {
                        let staging_map = match device.context.Map(staging, 0, D3D11_MAP_READ, 0) {
                            Ok(map) => map,
                            Err(_) => continue,
                        };
                        let pixels: *const u8 = mem::transmute(staging_map.pData);
                        let pitch = staging_map.RowPitch as usize;
                        (pixels, pitch)
                    }
                } else {
                    unsafe {
                        let desktop_map = match device.duplication.MapDesktopSurface() {
                            Ok(map) => map,
                            Err(error) => match error.code() {
                                DXGI_ERROR_ACCESS_LOST
                                | DXGI_ERROR_UNSUPPORTED
                                | DXGI_ERROR_INVALID_CALL => {
                                    // Recreate the duplication interface if this fails with with an expected
                                    // error that invalidates the duplication interface or requires that we
                                    // switch to AcquireNextFrame.
                                    self.free_resources();
                                    return Err(error);
                                }
                                _ => continue,
                            },
                        };
                        let pixels: *const u8 = mem::transmute(desktop_map.pBits);
                        let pitch = desktop_map.Pitch as usize;
                        (pixels, pitch)
                    }
                };

                let previous_color = previous_color.next().unwrap();

                let divisor = OFFSET_ARRAY_SIZE as f64;
                let (r, g, b) = offsets
                    .0
                    .iter()
                    .map(|offset| {
                        let byte_offset = (offset.y * pitch) + (offset.x * mem::size_of::<u32>());
                        let pixels =
                            ptr::slice_from_raw_parts(pixels, byte_offset + mem::size_of::<u32>());
                        unsafe {
                            (
                                (*pixels)[byte_offset + 2] as f64,
                                (*pixels)[byte_offset + 1] as f64,
                                (*pixels)[byte_offset] as f64,
                            )
                        }
                    })
                    .reduce(|total, rgb| (total.0 + rgb.0, total.1 + rgb.1, total.2 + rgb.2))
                    .unwrap();
                let (mut r, mut g, mut b) = (r / divisor, g / divisor, b / divisor);

                // Average in the previous color if fading is enabled.
                if self.parameters.fade.abs() > f64::EPSILON {
                    r = r * self.parameters.get_weight()
                        + ((*previous_color & 0xFF000000) >> 24) as f64 * self.parameters.fade;
                    g = g * self.parameters.get_weight()
                        + ((*previous_color & 0xFF0000) >> 16) as f64 * self.parameters.fade;
                    b = b * self.parameters.get_weight()
                        + ((*previous_color & 0xFF00) >> 8) as f64 * self.parameters.fade;
                }

                let min_brightness = self.parameters.min_brightness as f64;
                let sum = r + b + g;

                // Boost pixels that fall below the minimum brightness.
                if sum < min_brightness {
                    if sum.abs() < f64::EPSILON {
                        // Spread equally to R, G, and B.
                        let value = sum / 3.0;

                        r = value;
                        g = value;
                        b = value;
                    } else {
                        // Spread the "brightness deficit" back into R, G, and B in proportion
                        // to their individual contribition to that deficit.  Rather than simply
                        // boosting all pixels at the low end, this allows deep (but saturated)
                        // colors to stay saturated...they don't "pink out."
                        let deficit = min_brightness - sum;
                        let sum_2 = sum * 2.0;

                        r = (deficit * (sum - r)) / sum_2;
                        g = (deficit * (sum - g)) / sum_2;
                        b = (deficit * (sum - b)) / sum_2;
                    }
                }

                let (r, g, b, a) = (
                    r as u32 & 0xFF << 24,
                    g as u32 & 0xFF << 16,
                    b as u32 & 0xFF << 8,
                    0xFF_u32,
                );
                *previous_color = r | g | b | a;
            }
        }

        self.frame_count += 1;

        Ok(())
    }

    pub fn render_serial(&self, serial: &mut PixelBuffer) -> bool {
        serial.clear();

        if !self.acquired_resources {
            return false;
        }

        for pixel in self.previous_colors.iter() {
            let (r, g, b) = (
                self.gamma.red(((*pixel & 0xFF000000) >> 24) as u8),
                self.gamma.green(((*pixel & 0xFF0000) >> 16) as u8),
                self.gamma.blue(((*pixel & 0xFF00) >> 8) as u8),
            );
            let (r, g, b, a) = (
                r as u32 & 0xFF << 24,
                g as u32 & 0xFF << 16,
                b as u32 & 0xFF << 8,
                0xFF_u32,
            );

            // Write the gamma corrected values to the serial data.
            serial.add(r | g | b | a);
        }

        true
    }

    pub fn render_channel(&self, channel: &OpcChannel, pixels: &mut PixelBuffer) -> bool {
        pixels.clear();

        if !self.acquired_resources {
            return false;
        }

        for range in channel.pixels.iter() {
            let mut sampled_pixels = Vec::new();
            sampled_pixels.resize(range.pixel_count, 0_u32);

            // Start with sampled pixels, which tends to make very abrupt transitions when the pixel count
            // is higher than the sample count.
            for (pixel_index, sample) in sampled_pixels.iter_mut().enumerate() {
                let mut pixel_color = 0_u32;
                let mut display = 0_usize;
                let mut pixel_offset = pixel_index * range.get_sample_count() / range.pixel_count;
                let mut previous_color_index = 0_usize;

                loop {
                    if display >= range.display_index.len()
                        || pixel_offset < range.display_index[display].len()
                    {
                        break;
                    }

                    pixel_offset -= range.display_index.len();
                    previous_color_index += self.pixel_offsets[display].len();
                    display += 1;
                }

                if display < range.display_index.len() {
                    previous_color_index += range.display_index[display][pixel_offset];
                    pixel_color = self.previous_colors[previous_color_index];
                }

                *sample = pixel_color;
            }

            // Write the pixel value to the message buffer, optionally blurring with the Gaussian kernel.
            for pixel_index in 0..range.pixel_count {
                let kernel_radius = range.get_kernel_radius();
                let mut pixel_color = sampled_pixels[pixel_index];

                if pixel_index >= kernel_radius && pixel_index + kernel_radius < range.pixel_count {
                    let (mut r, mut g, mut b, mut a) = (0.0, 0.0, 0.0, 0.0);

                    for (x, weight) in range.get_kernel_weights().iter().enumerate() {
                        let sample = sampled_pixels[x + pixel_index - kernel_radius];
                        r += ((sample & 0xFF000000) >> 24) as f64 * weight;
                        g += ((sample & 0xFF0000) >> 16) as f64 * weight;
                        b += ((sample & 0xFF00) >> 8) as f64 * weight;
                        a += (sample & 0xFF) as f64 * weight;
                    }

                    let (r, g, b, a) = (
                        (r as u32).clamp(0, 255) << 24,
                        (g as u32).clamp(0, 255) << 16,
                        (b as u32).clamp(0, 255) << 8,
                        (a as u32).clamp(0, 255),
                    );

                    pixel_color = r | g | b | a;
                }

                pixels.add(pixel_color);
            }
        }

        true
    }

    pub fn is_empty(&self) -> bool {
        !self.acquired_resources
    }

    fn get_factory(&mut self) -> Result<IDXGIFactory1> {
        if self.factory.is_none() {
            self.factory = Some(unsafe { CreateDXGIFactory1() }?);
        }

        Ok(self.factory.as_ref().unwrap().clone())
    }
}
