use std::{cell::RefCell, mem, ptr, rc::Rc};

use windows::{
    core::Error,
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, PSTR, PWSTR, WPARAM},
        System::{
            Diagnostics::Debug::{
                FormatMessageW, FORMAT_MESSAGE_ALLOCATE_BUFFER, FORMAT_MESSAGE_FROM_SYSTEM,
            },
            LibraryLoader::GetModuleHandleA,
            Memory::LocalFree,
            RemoteDesktop::{
                WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
                NOTIFY_FOR_THIS_SESSION,
            },
        },
        UI::WindowsAndMessaging::{
            self, CreateWindowExA, DefWindowProcA, DestroyWindow, GetSystemMetrics, MessageBoxW,
            PostQuitMessage, RegisterClassExA, GWLP_USERDATA, HMENU, HWND_DESKTOP, MB_ICONERROR,
            SM_REMOTESESSION, WINDOW_LONG_PTR_INDEX, WNDCLASSEXA,
        },
    },
};

use crate::update_timer::UpdateTimer;

struct WindowState {
    pub connected_to_console: bool,
    pub timer: UpdateTimer,
}

impl WindowState {
    pub fn new(timer: UpdateTimer) -> Self {
        Self {
            connected_to_console: unsafe { GetSystemMetrics(SM_REMOTESESSION) } == 0,
            timer,
        }
    }
}

pub struct HiddenWindow(HWND);

impl HiddenWindow {
    pub fn new(timer: UpdateTimer) -> Self {
        let h_wnd = unsafe {
            let class_name = Self::get_window_class();
            let exe_instance = GetModuleHandleA(PSTR::default());
            let window_class = WNDCLASSEXA {
                cbSize: mem::size_of::<WNDCLASSEXA>() as u32,
                lpfnWndProc: Some(Self::window_proc),
                hInstance: exe_instance,
                lpszClassName: PSTR(class_name.as_ptr()),
                ..Default::default()
            };
            if RegisterClassExA(&window_class) == 0 {
                Self::display_last_error();
                Default::default()
            } else {
                let h_wnd = CreateWindowExA(
                    Default::default(),
                    PSTR(class_name.as_ptr()),
                    PSTR::default(),
                    Default::default(),
                    0,
                    0,
                    0,
                    0,
                    HWND_DESKTOP,
                    HMENU::default(),
                    exe_instance,
                    ptr::null(),
                );
                let state = Box::new(Rc::new(RefCell::new(Some(WindowState::new(timer)))));
                Self::set_window_long(h_wnd, GWLP_USERDATA, Box::into_raw(state) as isize);
                Self::attach_to_console(h_wnd);
                h_wnd
            }
        };

        Self(h_wnd)
    }

    fn get_window_class() -> Vec<u8> {
        "AdaLightListener"
            .bytes()
            .chain(std::iter::once(0))
            .collect()
    }

    pub unsafe fn display_last_error() {
        let mut error = PWSTR::default();
        FormatMessageW(
            FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM,
            ptr::null(),
            Error::from_win32().code().0 as u32,
            0,
            PWSTR(mem::transmute(&mut (error.0))),
            0,
            ptr::null(),
        );
        MessageBoxW(HWND_DESKTOP, error, PWSTR::default(), MB_ICONERROR);
        LocalFree(error.0 as isize);
    }

    fn set_window_state(
        h_wnd: HWND,
        state: Option<Rc<RefCell<WindowState>>>,
    ) -> Option<Rc<RefCell<WindowState>>> {
        unsafe {
            match Self::set_window_long(
                h_wnd,
                WindowsAndMessaging::GWLP_USERDATA,
                match state {
                    Some(state) => Box::into_raw(Box::new(state)) as isize,
                    None => 0_isize,
                },
            ) {
                0 => None,
                ptr => {
                    let state: Box<Rc<RefCell<WindowState>>> = Box::from_raw(mem::transmute(ptr));
                    Some((*state).clone())
                }
            }
        }
    }

    fn get_window_state(h_wnd: HWND) -> Option<Rc<RefCell<WindowState>>> {
        unsafe {
            let data = Self::get_window_long(h_wnd, WindowsAndMessaging::GWLP_USERDATA);
            match data {
                0 => None,
                _ => {
                    let raw: Box<Rc<RefCell<WindowState>>> = Box::from_raw(mem::transmute(data));
                    let state = (*raw).clone();
                    mem::forget(raw);
                    Some(state)
                }
            }
        }
    }

    fn attach_to_console(h_wnd: HWND) {
        if let Some(state) = Self::get_window_state(h_wnd) {
            let state = state.borrow();
            if state.connected_to_console {
                state.timer.resume();
                state.timer.start();
            }
        }
    }

    fn detach_from_console(h_wnd: HWND) {
        if let Some(state) = Self::get_window_state(h_wnd) {
            let state = state.borrow();
            if state.connected_to_console {
                state.timer.stop();
            }
        }
    }

    unsafe extern "system" fn window_proc(
        h_wnd: HWND,
        message: u32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        match message {
            WindowsAndMessaging::WM_CREATE => {
                WTSRegisterSessionNotification(h_wnd, NOTIFY_FOR_THIS_SESSION);
                Default::default()
            }
            WindowsAndMessaging::WM_DESTROY => {
                WTSUnRegisterSessionNotification(h_wnd);
                Self::detach_from_console(h_wnd);
                PostQuitMessage(0);
                Default::default()
            }
            WindowsAndMessaging::WM_WTSSESSION_CHANGE => {
                match w_param.0 as u32 {
                    WindowsAndMessaging::WTS_CONSOLE_CONNECT => {
                        if let Some(state) = Self::get_window_state(h_wnd) {
                            let mut state = state.borrow_mut();
                            state.connected_to_console = true;
                        }
                        Self::attach_to_console(h_wnd);
                    }
                    WindowsAndMessaging::WTS_CONSOLE_DISCONNECT => {
                        Self::detach_from_console(h_wnd);
                        if let Some(state) = Self::get_window_state(h_wnd) {
                            let mut state = state.borrow_mut();
                            state.connected_to_console = false;
                        }
                    }
                    WindowsAndMessaging::WTS_SESSION_LOCK => Self::detach_from_console(h_wnd),
                    WindowsAndMessaging::WTS_SESSION_UNLOCK => Self::attach_to_console(h_wnd),
                    _ => (),
                };
                Default::default()
            }
            WindowsAndMessaging::WM_DISPLAYCHANGE => {
                Self::detach_from_console(h_wnd);
                Self::attach_to_console(h_wnd);
                Default::default()
            }
            _ => DefWindowProcA(h_wnd, message, w_param, l_param),
        }
    }

    #[allow(non_snake_case)]
    #[cfg(target_pointer_width = "32")]
    unsafe fn set_window_long(window: HWND, index: WINDOW_LONG_PTR_INDEX, value: isize) -> isize {
        WindowsAndMessaging::SetWindowLongA(window, index, value as _) as _
    }

    #[allow(non_snake_case)]
    #[cfg(target_pointer_width = "64")]
    unsafe fn set_window_long(window: HWND, index: WINDOW_LONG_PTR_INDEX, value: isize) -> isize {
        WindowsAndMessaging::SetWindowLongPtrA(window, index, value)
    }

    #[allow(non_snake_case)]
    #[cfg(target_pointer_width = "32")]
    unsafe fn get_window_long(window: HWND, index: WINDOW_LONG_PTR_INDEX) -> isize {
        WindowsAndMessaging::GetWindowLongA(window, index) as _
    }

    #[allow(non_snake_case)]
    #[cfg(target_pointer_width = "64")]
    unsafe fn get_window_long(window: HWND, index: WINDOW_LONG_PTR_INDEX) -> isize {
        WindowsAndMessaging::GetWindowLongPtrA(window, index)
    }
}

impl Drop for HiddenWindow {
    fn drop(&mut self) {
        if self.0 != Default::default() {
            Self::set_window_state(self.0, None);
            if unsafe { DestroyWindow(self.0) }.as_bool() {
                self.0 = Default::default();
            }
        }
    }
}
