use std::{mem, ptr};

use windows::Win32::{
    Devices::Communication::{
        GetCommState, SetCommState, SetCommTimeouts, COMMTIMEOUTS, DCB, NOPARITY, ONESTOPBIT,
    },
    Foundation::{
        CloseHandle, GetLastError, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, HANDLE,
        INVALID_HANDLE_VALUE, PWSTR,
    },
    Storage::FileSystem::{
        CreateFileW, ReadFile, WriteFile, FILE_ACCESS_FLAGS, FILE_ATTRIBUTE_NORMAL,
        FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
    },
    System::{
        SystemServices::{GENERIC_READ, GENERIC_WRITE},
        Threading::CreateEventW,
        WindowsProgramming::CBR_115200,
        IO::{CancelIo, GetOverlappedResult, OVERLAPPED},
    },
};

use crate::{pixel_buffer::PixelBuffer, settings::Settings};

const COOKIE: [u8; 4] = [b'A', b'd', b'a', b'\n'];

struct PortResources {
    pub port_handle: HANDLE,
    pub configuration: DCB,
    pub port_number: u8,
    pub wait_handle: HANDLE,
    pub buffer: [u8; COOKIE.len()],
    pub cb: usize,
    pub overlapped: OVERLAPPED,
}

impl Default for PortResources {
    fn default() -> Self {
        Self {
            port_handle: INVALID_HANDLE_VALUE,
            configuration: DCB {
                DCBlength: std::mem::size_of::<DCB>() as u32,
                ..Default::default()
            },
            port_number: 0,
            wait_handle: INVALID_HANDLE_VALUE,
            buffer: [0_u8; 4],
            cb: 0,
            overlapped: Default::default(),
        }
    }
}

impl Drop for PortResources {
    fn drop(&mut self) {
        if INVALID_HANDLE_VALUE != self.port_handle {
            unsafe {
                CancelIo(self.port_handle);
                SetCommState(self.port_handle, &self.configuration);
                CloseHandle(self.port_handle);
            }
            self.port_handle = INVALID_HANDLE_VALUE;
        }

        if INVALID_HANDLE_VALUE != self.wait_handle {
            unsafe {
                CloseHandle(self.wait_handle);
            }
            self.wait_handle = INVALID_HANDLE_VALUE;
        }
    }
}

pub struct SerialPort<'a> {
    parameters: &'a Settings,
    port_handle: HANDLE,
    port_number: u8,
}

impl<'a> SerialPort<'a> {
    pub fn new(settings: &'a Settings) -> Self {
        Self {
            parameters: settings,
            port_handle: INVALID_HANDLE_VALUE,
            port_number: 0,
        }
    }

    pub fn open(&mut self) -> bool {
        if INVALID_HANDLE_VALUE == self.port_handle {
            let mut pending_ports: Vec<Option<PortResources>> = Vec::new();

            for port_number in 0_u8..=255 {
                for port in pending_ports.iter_mut().filter_map(Some) {
                    if let Some(resources) = port {
                        let mut cb = 0_u32;
                        unsafe {
                            if GetOverlappedResult(
                                resources.port_handle,
                                &resources.overlapped,
                                &mut cb,
                                false,
                            )
                            .as_bool()
                            {
                                if cb as usize == COOKIE.len() && resources.buffer == COOKIE {
                                    self.port_number = resources.port_number;
                                    break;
                                }
                            } else if GetLastError() == ERROR_IO_INCOMPLETE {
                                continue;
                            }

                            *port = None;
                        }
                    }
                }

                if self.port_number != 0 {
                    pending_ports.clear();
                    break;
                }

                let port_number = port_number + 1;
                let (port_handle, configuration) = self.get_port(port_number, true);
                if INVALID_HANDLE_VALUE == port_handle {
                    continue;
                }

                unsafe {
                    let wait_handle = CreateEventW(ptr::null(), true, false, PWSTR::default());
                    let mut port = PortResources {
                        port_number,
                        port_handle,
                        configuration,
                        wait_handle,
                        overlapped: OVERLAPPED {
                            hEvent: wait_handle,
                            ..Default::default()
                        },
                        ..Default::default()
                    };

                    if !ReadFile(
                        port.port_handle,
                        mem::transmute(port.buffer.as_mut_ptr()),
                        port.buffer.len() as u32,
                        ptr::null_mut(),
                        &mut port.overlapped,
                    )
                    .as_bool()
                        && ERROR_IO_PENDING != GetLastError()
                    {
                        continue;
                    }

                    pending_ports.push(Some(port));
                }
            }

            for port in pending_ports.iter_mut().filter_map(Some) {
                if let Some(resources) = port {
                    let mut cb = 0_u32;
                    unsafe {
                        if GetOverlappedResult(
                            resources.port_handle,
                            &resources.overlapped,
                            &mut cb,
                            true,
                        )
                        .as_bool()
                            && cb as usize == COOKIE.len()
                            && resources.buffer == COOKIE
                        {
                            self.port_number = resources.port_number;
                            break;
                        }

                        *port = None;
                    }
                }
            }

            if self.port_number != 0 {
                self.port_handle = self.get_port(self.port_number, false).0;
            }
        }

        INVALID_HANDLE_VALUE != self.port_handle
    }

    pub fn send(&mut self, buffer: &PixelBuffer) -> bool {
        if INVALID_HANDLE_VALUE == self.port_handle {
            return false;
        }

        let mut cb_written = 0_u32;

        unsafe {
            if !WriteFile(
                self.port_handle,
                mem::transmute(buffer.buffer.as_ptr()),
                buffer.buffer.len() as u32,
                &mut cb_written,
                ptr::null_mut(),
            )
            .as_bool()
                || cb_written as usize != buffer.buffer.len()
            {
                self.close();
                return false;
            }
        }

        true
    }

    pub fn close(&mut self) {
        if INVALID_HANDLE_VALUE != self.port_handle {
            unsafe {
                CloseHandle(self.port_handle);
            }
            self.port_handle = INVALID_HANDLE_VALUE;
        }
    }

    fn get_port(&self, port_number: u8, read_test: bool) -> (HANDLE, DCB) {
        let port_name = format!("COM{port_number}");
        let (desired_access, flags_and_attributes) = if read_test {
            (FILE_ACCESS_FLAGS(GENERIC_READ), FILE_FLAG_OVERLAPPED)
        } else {
            (FILE_ACCESS_FLAGS(GENERIC_WRITE), FILE_ATTRIBUTE_NORMAL)
        };
        unsafe {
            let mut port_handle = CreateFileW(
                port_name,
                desired_access,
                Default::default(),
                ptr::null(),
                OPEN_EXISTING,
                flags_and_attributes,
                HANDLE::default(),
            );
            let mut configuration = DCB {
                DCBlength: std::mem::size_of::<DCB>() as u32,
                ..Default::default()
            };

            if INVALID_HANDLE_VALUE != port_handle {
                if GetCommState(port_handle, &mut configuration).as_bool() {
                    let reconfigured = DCB {
                        BaudRate: CBR_115200,
                        ByteSize: 8,
                        StopBits: ONESTOPBIT,
                        Parity: NOPARITY,
                        ..configuration
                    };
                    let timeouts = COMMTIMEOUTS {
                        ReadTotalTimeoutConstant: self.parameters.timeout,
                        WriteTotalTimeoutConstant: self.parameters.get_delay(),
                        ..Default::default()
                    };

                    if SetCommState(port_handle, &reconfigured).as_bool()
                        && SetCommTimeouts(port_handle, &timeouts).as_bool()
                    {
                        return (port_handle, configuration);
                    }

                    SetCommState(port_handle, &configuration);
                }

                CloseHandle(port_handle);
                port_handle = INVALID_HANDLE_VALUE;
            }

            (port_handle, configuration)
        }
    }
}

impl<'a> Drop for SerialPort<'a> {
    fn drop(&mut self) {
        self.close();
    }
}
