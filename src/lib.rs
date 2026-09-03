//! TimeHub 的可复用核心逻辑。
//!
//! GUI 装配保留在二进制入口中；数据库、导入导出、日期算法和提醒逻辑放在
//! 库目标里，既便于独立测试，也避免 `cargo test` 为整套 Slint GUI 生成测试程序。

pub mod almanac;
pub mod app_paths;
pub mod autostart;
pub mod cli;
pub mod date_calc;
pub mod db;
pub mod holidays;
pub mod ics;
pub mod integrations;
pub mod lunar;
pub mod natural;
pub mod recurrence;
pub mod reminders;
pub mod share;
pub mod sync;
pub mod weather;
pub mod window_policy;
