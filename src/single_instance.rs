//! GUI 单实例保护。命令行子命令不受影响。

use anyhow::{Context, Result};
#[cfg(windows)]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
#[cfg(windows)]
use std::thread::JoinHandle;

const MUTEX_NAME: &str = "Local\\TimeHub.Desktop.SingleInstance\0";
const ACTIVATION_EVENT_NAME: &str = "Local\\TimeHub.Desktop.Activate\0";

#[cfg(windows)]
pub struct Guard {
    mutex: *mut core::ffi::c_void,
    activation_event: *mut core::ffi::c_void,
    stop: Arc<AtomicBool>,
    listener: Option<JoinHandle<()>>,
}

#[cfg(not(windows))]
pub struct Guard;

#[cfg(windows)]
impl Drop for Guard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        unsafe {
            SetEvent(self.activation_event);
        }
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
        unsafe {
            CloseHandle(self.activation_event);
            CloseHandle(self.mutex);
        }
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateMutexW(
        attributes: *const core::ffi::c_void,
        initial_owner: i32,
        name: *const u16,
    ) -> *mut core::ffi::c_void;
    fn CreateEventW(
        attributes: *const core::ffi::c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> *mut core::ffi::c_void;
    fn GetLastError() -> u32;
    fn SetEvent(event: *mut core::ffi::c_void) -> i32;
    fn WaitForSingleObject(handle: *mut core::ffi::c_void, milliseconds: u32) -> u32;
    fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
}

/// 获取 GUI 单实例锁。`None` 表示已有 TimeHub GUI 正在运行。
#[cfg(windows)]
pub fn acquire() -> Result<Option<Guard>> {
    const ERROR_ALREADY_EXISTS: u32 = 183;
    let name: Vec<u16> = MUTEX_NAME.encode_utf16().collect();
    let handle = unsafe { CreateMutexW(core::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(std::io::Error::last_os_error()).context("无法创建 TimeHub 单实例锁");
    }
    let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

    let event_name: Vec<u16> = ACTIVATION_EVENT_NAME.encode_utf16().collect();
    let activation_event = unsafe { CreateEventW(core::ptr::null(), 0, 0, event_name.as_ptr()) };
    if activation_event.is_null() {
        unsafe {
            CloseHandle(handle);
        }
        return Err(std::io::Error::last_os_error()).context("无法创建 TimeHub 窗口激活事件");
    }

    if already_exists {
        unsafe {
            SetEvent(activation_event);
            CloseHandle(activation_event);
            CloseHandle(handle);
        }
        Ok(None)
    } else {
        Ok(Some(Guard {
            mutex: handle,
            activation_event,
            stop: Arc::new(AtomicBool::new(false)),
            listener: None,
        }))
    }
}

#[cfg(windows)]
impl Guard {
    /// 第二次启动应用时，在后台线程收到激活信号并调用回调。
    pub fn on_activate(&mut self, callback: impl Fn() + Send + 'static) {
        const WAIT_OBJECT_0: u32 = 0;
        const INFINITE: u32 = u32::MAX;

        let event = self.activation_event as usize;
        let stop = self.stop.clone();
        self.listener = Some(std::thread::spawn(move || loop {
            let result = unsafe { WaitForSingleObject(event as *mut core::ffi::c_void, INFINITE) };
            if result != WAIT_OBJECT_0 || stop.load(Ordering::Acquire) {
                break;
            }
            callback();
        }));
    }
}

#[cfg(not(windows))]
pub fn acquire() -> Result<Option<Guard>> {
    Ok(Some(Guard))
}

#[cfg(not(windows))]
impl Guard {
    pub fn on_activate(&mut self, _callback: impl Fn() + Send + 'static) {}
}
