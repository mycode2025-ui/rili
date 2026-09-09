//! 后台提醒扫描线程：周期性检查未来/近期的日程发生，按多级提醒偏移量
//! （提前 N 分钟）触发系统通知；同一条提醒只发一次（见 `db::mark_reminder_sent`）。

use crate::db;
use anyhow::Result;
use chrono::{Duration, Local, NaiveDate, NaiveTime};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration as StdDuration;

const CHECK_INTERVAL_SECS: u64 = 20;
/// 超过这个时长还没发出去的提醒视为“已错过”，不再补发（避免长时间未启动后集中轰炸）。
const STALE_AFTER_HOURS: i64 = 24;
const MAX_PENDING_NOTIFICATIONS: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationStyle {
    Quiet,
    Standard,
    Strong,
}

impl NotificationStyle {
    pub fn from_setting(value: &str) -> Self {
        match value.trim() {
            "quiet" => Self::Quiet,
            "strong" => Self::Strong,
            _ => Self::Standard,
        }
    }

    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Quiet => "quiet",
            Self::Standard => "standard",
            Self::Strong => "strong",
        }
    }

    pub fn effect_level(self) -> i32 {
        match self {
            Self::Quiet => 0,
            Self::Standard => 1,
            Self::Strong => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReminderAlert {
    pub message: String,
    pub style: NotificationStyle,
}

type GuiNotifier = Arc<dyn Fn(ReminderAlert) -> bool + Send + Sync + 'static>;

fn pending_notifications() -> &'static Mutex<VecDeque<ReminderAlert>> {
    static PENDING: OnceLock<Mutex<VecDeque<ReminderAlert>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn gui_notifier() -> &'static Mutex<Option<GuiNotifier>> {
    static NOTIFIER: OnceLock<Mutex<Option<GuiNotifier>>> = OnceLock::new();
    NOTIFIER.get_or_init(|| Mutex::new(None))
}

/// Connect due reminders to the application's always-on-top corner popup.
/// Reminders that become due before the Slint window is ready are queued.
pub fn install_gui_notifier(notifier: impl Fn(ReminderAlert) -> bool + Send + Sync + 'static) {
    let notifier: GuiNotifier = Arc::new(notifier);
    if let Ok(mut slot) = gui_notifier().lock() {
        *slot = Some(notifier.clone());
    }
    loop {
        let message = pending_notifications()
            .lock()
            .ok()
            .and_then(|mut pending| pending.pop_front());
        let Some(message) = message else {
            break;
        };
        if !notifier(message.clone()) {
            if let Ok(mut pending) = pending_notifications().lock() {
                pending.push_front(message);
            }
            break;
        }
    }
}

fn dispatch_gui_notification(alert: ReminderAlert) {
    let notifier = gui_notifier()
        .lock()
        .ok()
        .and_then(|notifier| notifier.clone());
    if let Some(notifier) = notifier {
        if notifier(alert.clone()) {
            return;
        }
    }
    if let Ok(mut pending) = pending_notifications().lock() {
        while pending.len() >= MAX_PENDING_NOTIFICATIONS {
            pending.pop_front();
        }
        pending.push_back(alert);
    }
}

/// 启动一个后台线程持续扫描提醒；线程随进程退出而结束，不需要手动停止。
pub fn spawn() {
    thread::spawn(|| loop {
        if let Err(e) = tick() {
            crate::error_reporter::report("提醒检查失败", &e);
        }
        thread::sleep(StdDuration::from_secs(CHECK_INTERVAL_SECS));
    });
}

fn tick() -> Result<()> {
    let conn = db::open()?;
    if db::get_setting(&conn, "notifications_enabled", "1")? == "0" {
        return Ok(());
    }
    let style =
        NotificationStyle::from_setting(&db::get_setting(&conn, "notification_style", "standard")?);
    let now = Local::now().naive_local();
    let today = now.date();
    let range_start = today - Duration::days(10);
    let range_end = today + Duration::days(10);

    for occ in db::list_event_occurrences(&conn, range_start, range_end)? {
        let offsets: Vec<i64> = occ
            .event
            .reminder_offsets
            .split(',')
            .filter_map(|s| s.trim().parse::<i64>().ok())
            .collect();
        if offsets.is_empty() {
            continue;
        }
        let Ok(occ_date) = NaiveDate::parse_from_str(&occ.occurrence_date, "%Y-%m-%d") else {
            continue;
        };
        let event_time = occ
            .event
            .time
            .as_deref()
            .and_then(|t| NaiveTime::parse_from_str(t, "%H:%M").ok())
            .unwrap_or_else(|| NaiveTime::from_hms_opt(9, 0, 0).unwrap_or(NaiveTime::MIN));
        let event_datetime = occ_date.and_time(event_time);

        for offset in offsets {
            let remind_at = event_datetime - Duration::minutes(offset);
            if remind_at > now || remind_at < now - Duration::hours(STALE_AFTER_HOURS) {
                continue;
            }
            if db::mark_reminder_sent(&conn, occ.event.id, &occ.occurrence_date, offset)? {
                fire_notification(&occ.event.title, offset, event_datetime, style);
            }
        }
    }
    Ok(())
}

pub fn send_test_notification(style: NotificationStyle) -> Result<()> {
    if style == NotificationStyle::Quiet {
        return Ok(());
    }
    try_toast("TimeHub 示例提醒", "系统通知通道工作正常")
}

fn format_offset(offset_minutes: i64) -> String {
    if offset_minutes == 0 {
        "现在开始".to_string()
    } else if offset_minutes % 1440 == 0 {
        format!("{} 天后开始", offset_minutes / 1440)
    } else if offset_minutes % 60 == 0 {
        format!("{} 小时后开始", offset_minutes / 60)
    } else {
        format!("{offset_minutes} 分钟后开始")
    }
}

fn fire_notification(
    title: &str,
    offset_minutes: i64,
    event_datetime: chrono::NaiveDateTime,
    style: NotificationStyle,
) {
    let body = format!(
        "{}  {}",
        event_datetime.format("%Y-%m-%d %H:%M"),
        format_offset(offset_minutes)
    );
    // Always show the application's own corner popup. An unpackaged Win32 app can
    // receive Ok from the Windows toast API while Windows still suppresses the
    // banner because no registered Start-menu identity exists.
    dispatch_gui_notification(ReminderAlert {
        message: format!("日程提醒：{title} · {body}"),
        style,
    });
    if style != NotificationStyle::Quiet {
        if let Err(e) = try_toast(title, &body) {
            // The in-app popup above is the reliable delivery path. Keep the native
            // toast failure in the log without replacing the actual reminder popup.
            crate::error_reporter::record("Windows 系统通知发送失败，已使用应用内提醒", &e);
        }
    }
}

#[cfg(windows)]
fn try_toast(title: &str, body: &str) -> Result<()> {
    use winrt_toast::{Text, Toast, ToastManager};
    let manager = ToastManager::new("dev.rili.rili");
    let mut toast = Toast::new();
    toast.text1(title).text2(Text::new(body));
    manager.show(&toast).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn try_toast(_title: &str, _body: &str) -> Result<()> {
    anyhow::bail!("当前平台暂不支持系统通知")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn reminder_offset_labels_are_stable() {
        assert_eq!(format_offset(0), "现在开始");
        assert_eq!(format_offset(10), "10 分钟后开始");
        assert_eq!(format_offset(60), "1 小时后开始");
        assert_eq!(format_offset(1440), "1 天后开始");
    }

    #[test]
    fn due_reminder_is_always_forwarded_to_the_gui_popup() {
        if let Ok(mut notifier) = gui_notifier().lock() {
            *notifier = None;
        }
        if let Ok(mut pending) = pending_notifications().lock() {
            pending.clear();
        }

        let (sender, receiver) = mpsc::channel();
        install_gui_notifier(move |message| sender.send(message).is_ok());
        dispatch_gui_notification(ReminderAlert {
            message: "日程提醒：测试 · 2026-09-07 12:46  现在开始".to_string(),
            style: NotificationStyle::Strong,
        });

        let alert = receiver.recv().unwrap();
        assert_eq!(alert.style, NotificationStyle::Strong);
        assert!(alert.message.contains("日程提醒：测试"));
        assert!(alert.message.contains("2026-09-07 12:46"));
        assert!(alert.message.contains("现在开始"));

        if let Ok(mut notifier) = gui_notifier().lock() {
            *notifier = None;
        }
    }

    #[test]
    fn notification_styles_reject_unknown_persisted_values() {
        assert_eq!(
            NotificationStyle::from_setting("quiet"),
            NotificationStyle::Quiet
        );
        assert_eq!(
            NotificationStyle::from_setting("strong"),
            NotificationStyle::Strong
        );
        assert_eq!(
            NotificationStyle::from_setting("broken"),
            NotificationStyle::Standard
        );
        assert_eq!(NotificationStyle::Strong.as_setting(), "strong");
        assert_eq!(NotificationStyle::Strong.effect_level(), 2);
    }
}
