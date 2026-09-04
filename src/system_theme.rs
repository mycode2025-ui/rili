//! Native operating-system appearance detection.

/// Returns whether Windows is configured to use dark colors for applications.
///
/// Windows exposes separate preferences for the shell and applications. TimeHub
/// follows `AppsUseLightTheme`, which is the value used for normal application
/// windows in the Personalization settings page.
#[cfg(target_os = "windows")]
pub fn apps_use_dark_mode() -> bool {
    use std::ffi::c_void;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            hkey: isize,
            sub_key: *const u16,
            value: *const u16,
            flags: u32,
            value_type: *mut u32,
            data: *mut c_void,
            data_size: *mut u32,
        ) -> i32;
    }

    const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as isize;
    const RRF_RT_REG_DWORD: u32 = 0x0000_0010;
    let sub_key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect();
    let value_name: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();
    let mut value = 1u32;
    let mut value_size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            sub_key.as_ptr(),
            value_name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut value_size,
        )
    };
    status == 0 && value == 0
}

#[cfg(not(target_os = "windows"))]
pub fn apps_use_dark_mode() -> bool {
    false
}
