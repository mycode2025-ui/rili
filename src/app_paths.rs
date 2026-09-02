//! 应用数据目录：本地优先，所有数据都保存在用户本机的 SQLite 文件里，
//! 不上传任何云端，避免依赖自建后端服务。

use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::PathBuf;

pub fn db_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("dev", "rili", "rili").context("无法确定用户数据目录")?;
    let data_dir = dirs.data_dir();
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("创建应用数据目录失败: {}", data_dir.display()))?;
    Ok(data_dir.join("rili.db"))
}
