//! 界面字体选择与运行时自定义字体注册。

use anyhow::{bail, Context, Result};
use slint::fontique_010::{fontique, shared_collection};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) const DEFAULT_FONT_FAMILY: &str = "Noto Sans SC";

pub(crate) fn family_from_choice(choice: &str) -> Option<&'static str> {
    match choice {
        "系统默认" => Some(""),
        "Noto Sans SC" => Some("Noto Sans SC"),
        "微软雅黑" => Some("Microsoft YaHei UI"),
        "等线" => Some("DengXian"),
        "思源宋体" => Some("Source Han Serif SC"),
        _ => None,
    }
}

pub(crate) fn choice_from_family(family: &str) -> String {
    match family {
        "" => "系统默认".to_string(),
        "Noto Sans SC" => "Noto Sans SC".to_string(),
        "Microsoft YaHei UI" | "Microsoft YaHei" => "微软雅黑".to_string(),
        "DengXian" => "等线".to_string(),
        "Source Han Serif SC" | "Source Han Serif CN" => "思源宋体".to_string(),
        custom => format!("自定义 · {custom}"),
    }
}

/// 将字体文件注册到 Slint 当前进程，并返回字体文件声明的首个家族名。
pub(crate) fn register_custom_font(path: &Path) -> Result<String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !matches!(
        extension.to_ascii_lowercase().as_str(),
        "ttf" | "otf" | "ttc"
    ) {
        bail!("仅支持 TTF、OTF 或 TTC 字体文件");
    }

    let bytes =
        std::fs::read(path).with_context(|| format!("无法读取字体文件：{}", path.display()))?;
    if bytes.len() > 128 * 1024 * 1024 {
        bail!("字体文件超过 128 MB");
    }

    let mut collection = shared_collection();
    let registered = collection.register_fonts(fontique::Blob::new(Arc::new(bytes)), None);
    let Some((family_id, _)) = registered.first() else {
        bail!("文件中没有可用字体");
    };
    collection
        .family_name(*family_id)
        .map(str::to_owned)
        .context("无法识别字体家族名称")
}

#[cfg(windows)]
pub(crate) fn pick_font_file() -> Result<Option<PathBuf>> {
    use core::ffi::c_void;

    #[repr(C)]
    struct OpenFileNameW {
        struct_size: u32,
        owner: *mut c_void,
        instance: *mut c_void,
        filter: *const u16,
        custom_filter: *mut u16,
        max_custom_filter: u32,
        filter_index: u32,
        file: *mut u16,
        max_file: u32,
        file_title: *mut u16,
        max_file_title: u32,
        initial_dir: *const u16,
        title: *const u16,
        flags: u32,
        file_offset: u16,
        file_extension: u16,
        default_extension: *const u16,
        custom_data: isize,
        hook: *mut c_void,
        template_name: *const u16,
        reserved: *mut c_void,
        reserved_value: u32,
        flags_ex: u32,
    }

    #[link(name = "comdlg32")]
    unsafe extern "system" {
        fn GetOpenFileNameW(dialog: *mut OpenFileNameW) -> i32;
        fn CommDlgExtendedError() -> u32;
    }

    const OFN_PATH_MUST_EXIST: u32 = 0x0000_0800;
    const OFN_FILE_MUST_EXIST: u32 = 0x0000_1000;
    const OFN_EXPLORER: u32 = 0x0008_0000;

    let mut file = vec![0u16; 32_768];
    let filter: Vec<u16> =
        "字体文件 (*.ttf;*.otf;*.ttc)\0*.ttf;*.otf;*.ttc\0所有文件 (*.*)\0*.*\0\0"
            .encode_utf16()
            .collect();
    let title: Vec<u16> = "选择界面字体\0".encode_utf16().collect();
    let mut dialog: OpenFileNameW = unsafe { std::mem::zeroed() };
    dialog.struct_size = std::mem::size_of::<OpenFileNameW>() as u32;
    dialog.filter = filter.as_ptr();
    dialog.filter_index = 1;
    dialog.file = file.as_mut_ptr();
    dialog.max_file = file.len() as u32;
    dialog.title = title.as_ptr();
    dialog.flags = OFN_EXPLORER | OFN_PATH_MUST_EXIST | OFN_FILE_MUST_EXIST;

    if unsafe { GetOpenFileNameW(&mut dialog) } != 0 {
        let length = file
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(file.len());
        return Ok(Some(PathBuf::from(String::from_utf16(&file[..length])?)));
    }
    let error = unsafe { CommDlgExtendedError() };
    if error == 0 {
        Ok(None)
    } else {
        bail!("字体文件选择框错误：0x{error:04X}")
    }
}

#[cfg(not(windows))]
pub(crate) fn pick_font_file() -> Result<Option<PathBuf>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_labels_map_to_expected_system_families() {
        assert_eq!(family_from_choice("系统默认"), Some(""));
        assert_eq!(family_from_choice("微软雅黑"), Some("Microsoft YaHei UI"));
        assert_eq!(family_from_choice("未知"), None);
        assert_eq!(choice_from_family("DengXian"), "等线");
        assert_eq!(choice_from_family("Example Font"), "自定义 · Example Font");
    }
}
