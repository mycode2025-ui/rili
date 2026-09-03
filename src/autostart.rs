//! Windows 当前用户开机启动项。
//!
//! 使用 HKCU 的 `Run` 键，不需要管理员权限。注册表中的可执行文件路径始终
//! 使用双引号包裹，避免安装目录含空格时启动失败。

use anyhow::{bail, Context, Result};
use std::path::Path;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "TimeHub";

fn startup_command(executable: &Path) -> String {
    format!(r#""{}""#, executable.display())
}

#[cfg(windows)]
fn registry_command() -> std::process::Command {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = std::process::Command::new("reg.exe");
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// 返回系统中是否存在 TimeHub 的当前用户登录启动项。
#[cfg(windows)]
pub fn is_enabled() -> bool {
    registry_command()
        .args(["QUERY", RUN_KEY, "/v", VALUE_NAME])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(not(windows))]
pub fn is_enabled() -> bool {
    false
}

/// 开启或关闭当前用户登录时自动启动 TimeHub。
#[cfg(windows)]
pub fn set_enabled(enabled: bool) -> Result<()> {
    let output = if enabled {
        let executable = std::env::current_exe().context("无法确定 TimeHub 程序路径")?;
        registry_command()
            .args([
                "ADD",
                RUN_KEY,
                "/v",
                VALUE_NAME,
                "/t",
                "REG_SZ",
                "/d",
                &startup_command(&executable),
                "/f",
            ])
            .output()
            .context("无法写入 Windows 开机启动项")?
    } else {
        if !is_enabled() {
            return Ok(());
        }
        registry_command()
            .args(["DELETE", RUN_KEY, "/v", VALUE_NAME, "/f"])
            .output()
            .context("无法删除 Windows 开机启动项")?
    };

    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if detail.is_empty() {
            bail!(
                "Windows 注册表操作失败（退出码 {:?}）",
                output.status.code()
            );
        }
        bail!("Windows 注册表操作失败：{detail}");
    }
}

#[cfg(not(windows))]
pub fn set_enabled(_enabled: bool) -> Result<()> {
    bail!("当前平台暂不支持开机启动设置")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_command_quotes_paths_with_spaces() {
        let command = startup_command(Path::new(r"C:\Program Files\TimeHub\rili.exe"));
        assert_eq!(command, r#""C:\Program Files\TimeHub\rili.exe""#);
    }
}
