//! 应用数据目录：本地优先，所有数据都保存在用户本机的 SQLite 文件里，
//! 不上传任何云端，避免依赖自建后端服务。

use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::PathBuf;

pub fn data_dir() -> Result<PathBuf> {
    // Native smoke runs use disposable data, never the user's live calendars.
    if std::env::var_os("TIMEHUB_NATIVE_SMOKE").is_some() {
        if let Some(path) = std::env::var_os("TIMEHUB_SMOKE_DATA_DIR") {
            let path = PathBuf::from(path);
            anyhow::ensure!(path.is_absolute(), "冒烟测试数据目录必须是绝对路径");
            std::fs::create_dir_all(&path)?;
            return Ok(path);
        }
    }
    let dirs = ProjectDirs::from("dev", "rili", "rili").context("无法确定用户数据目录")?;
    let data_dir = dirs.data_dir();
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("创建应用数据目录失败: {}", data_dir.display()))?;
    Ok(data_dir.to_path_buf())
}

pub fn db_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("rili.db"))
}

pub fn log_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("timehub.log"))
}
