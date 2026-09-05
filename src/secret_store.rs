//! 外部日历专用密码的本机保护。
//!
//! Windows 使用当前登录用户的 DPAPI；密文离开该用户配置文件后无法直接解密。

use anyhow::Result;

#[cfg(windows)]
pub fn protect(secret: &str) -> Result<String> {
    use base64::Engine;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let mut bytes = secret.as_bytes().to_vec();
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        anyhow::bail!(
            "Windows 无法保护 CalDAV 密码：{}",
            std::io::Error::last_os_error()
        );
    }
    let protected = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let encoded = base64::engine::general_purpose::STANDARD.encode(protected);
    unsafe {
        let _ = LocalFree(output.pbData.cast());
    }
    Ok(format!("dpapi:{encoded}"))
}

#[cfg(windows)]
pub fn unprotect(protected: &str) -> Result<String> {
    use base64::Engine;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let encoded = protected
        .strip_prefix("dpapi:")
        .ok_or_else(|| anyhow::anyhow!("CalDAV 密码不是受支持的安全格式"))?;
    let mut bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        anyhow::bail!(
            "Windows 无法解密 CalDAV 密码：{}",
            std::io::Error::last_os_error()
        );
    }
    let plain = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let plain = plain.to_vec();
    unsafe {
        let _ = LocalFree(output.pbData.cast());
    }
    Ok(String::from_utf8(plain)?)
}

#[cfg(not(windows))]
pub fn protect(_secret: &str) -> Result<String> {
    anyhow::bail!("当前平台尚未配置 CalDAV 密码安全存储")
}

#[cfg(not(windows))]
pub fn unprotect(_protected: &str) -> Result<String> {
    anyhow::bail!("当前平台尚未配置 CalDAV 密码安全存储")
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn dpapi_round_trip_preserves_unicode_secret() {
        let secret = "钉钉-CalDAV-密钥-123";
        let protected = super::protect(secret).unwrap();
        assert_ne!(protected, secret);
        assert_eq!(super::unprotect(&protected).unwrap(), secret);
    }
}
