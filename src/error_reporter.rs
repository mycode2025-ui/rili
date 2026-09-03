//! 统一错误记录：GUI 前台错误可直接调用 `record` 后显示通知；后台线程调用
//! `report`。GUI 初始化后会直接唤醒 Slint 事件循环并显示在屏幕右下角；
//! 初始化前的错误暂存在有界队列中。所有错误同时写入带轮转的本地日志。

use chrono::Local;
use std::collections::VecDeque;
use std::fmt::Display;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

const MAX_PENDING_ERRORS: usize = 20;
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
type GuiNotifier = Arc<dyn Fn(String) + Send + Sync + 'static>;

fn pending() -> &'static Mutex<VecDeque<String>> {
    static PENDING: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn gui_notifier() -> &'static Mutex<Option<GuiNotifier>> {
    static NOTIFIER: OnceLock<Mutex<Option<GuiNotifier>>> = OnceLock::new();
    NOTIFIER.get_or_init(|| Mutex::new(None))
}

fn log_lock() -> &'static Mutex<()> {
    static LOG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOG_LOCK.get_or_init(|| Mutex::new(()))
}

fn write_log(message: &str) {
    let Ok(_guard) = log_lock().lock() else {
        return;
    };
    let Ok(path) = crate::app_paths::log_path() else {
        return;
    };
    if std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() >= MAX_LOG_BYTES) {
        let backup = path.with_extension("log.1");
        let _ = std::fs::remove_file(&backup);
        let _ = std::fs::rename(&path, backup);
    }
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
    let notifier = gui_notifier()
        .lock()
        .ok()
        .and_then(|notifier| notifier.clone());
    if let Some(notifier) = notifier {
        notifier(message);
        return;
    }
    if let Ok(mut queue) = pending().lock() {
        while queue.len() >= MAX_PENDING_ERRORS {
            queue.pop_front();
        }
        queue.push_back(message);
    }
}

/// Connect background reports directly to the GUI event loop. Errors raised
/// before the window exists remain queued and are delivered after installation.
pub fn install_gui_notifier(notifier: impl Fn(String) + Send + Sync + 'static) {
    let notifier: GuiNotifier = Arc::new(notifier);
    if let Ok(mut slot) = gui_notifier().lock() {
        *slot = Some(notifier.clone());
    }
    while let Some(message) = take_pending() {
        notifier(message);
    }
}

pub fn take_pending() -> Option<String> {
    pending().lock().ok()?.pop_front()
}

#[cfg(test)]
mod tests {
    use super::{gui_notifier, install_gui_notifier, report, take_pending};
    use std::sync::mpsc;

    #[test]
    fn background_error_is_queued_for_the_gui() {
        if let Ok(mut notifier) = gui_notifier().lock() {
            *notifier = None;
        }
        while take_pending().is_some() {}
        report("测试错误", &"示例");
        assert_eq!(take_pending().as_deref(), Some("测试错误：示例"));

        let (sender, receiver) = mpsc::channel();
        install_gui_notifier(move |message| {
            let _ = sender.send(message);
        });
        report("后台错误", &"立即通知");
        assert_eq!(receiver.recv().as_deref(), Ok("后台错误：立即通知"));
        if let Ok(mut notifier) = gui_notifier().lock() {
            *notifier = None;
        }
    }
}
