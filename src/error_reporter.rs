//! 统一错误记录：GUI 前台错误可直接调用 `record` 后显示通知；后台线程调用
//! `report`，主事件循环会从队列取出并显示在屏幕右下角。

use chrono::Local;
use std::collections::VecDeque;
use std::fmt::Display;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

const MAX_PENDING_ERRORS: usize = 20;

fn pending() -> &'static Mutex<VecDeque<String>> {
    static PENDING: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn write_log(message: &str) {
    let Ok(path) = crate::app_paths::log_path() else {
        return;
    };
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(
        file,
        "{}  {}",
        Local::now().format("%Y-%m-%d %H:%M:%S"),
        message.replace(['\r', '\n'], " ")
    );
}

pub fn record(context: &str, error: &impl Display) -> String {
    let message = format!("{context}：{error}");
    eprintln!("{message}");
    write_log(&message);
    message
}

pub fn report(context: &str, error: &impl Display) {
    let message = record(context, error);
    if let Ok(mut queue) = pending().lock() {
        while queue.len() >= MAX_PENDING_ERRORS {
            queue.pop_front();
        }
        queue.push_back(message);
    }
}

pub fn take_pending() -> Option<String> {
    pending().lock().ok()?.pop_front()
}

#[cfg(test)]
mod tests {
    use super::{report, take_pending};

    #[test]
    fn background_error_is_queued_for_the_gui() {
        while take_pending().is_some() {}
        report("测试错误", &"示例");
        assert_eq!(take_pending().as_deref(), Some("测试错误：示例"));
    }
}
