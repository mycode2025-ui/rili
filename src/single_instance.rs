//! GUI 单实例保护。命令行子命令不受影响。

use anyhow::{Context, Result};

const MUTEX_NAME: &str = "Local\\TimeHub.Desktop.SingleInstance\0";

#[cfg(windows)]
pub struct Guard(*mut core::ffi::c_void);

#[cfg(not(windows))]
pub struct Guard;

#[cfg(windows)]
impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
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
    fn GetLastError() -> u32;
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
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            CloseHandle(handle);
        }
        Ok(None)
    } else {
        Ok(Some(Guard(handle)))
    }
}

#[cfg(not(windows))]
pub fn acquire() -> Result<Option<Guard>> {
    Ok(Some(Guard))
}
