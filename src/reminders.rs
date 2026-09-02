//! 后台提醒扫描线程：周期性检查未来/近期的日程发生，按多级提醒偏移量
//! （提前 N 分钟）触发系统通知；同一条提醒只发一次（见 `db::mark_reminder_sent`）。

use crate::db;
use anyhow::Result;
use chrono::{Duration, Local, NaiveDate, NaiveTime};
use std::thread;
use std::time::Duration as StdDuration;

const CHECK_INTERVAL_SECS: u64 = 20;
/// 超过这个时长还没发出去的提醒视为“已错过”，不再补发（避免长时间未启动后集中轰炸）。
const STALE_AFTER_HOURS: i64 = 24;

/// 启动一个后台线程持续扫描提醒；线程随进程退出而结束，不需要手动停止。
pub fn spawn() {
    thread::spawn(|| loop {
        if let Err(e) = tick() {
            eprintln!("提醒检查失败: {e}");
        }
        thread::sleep(StdDuration::from_secs(CHECK_INTERVAL_SECS));
    });
}

fn tick() -> Result<()> {
    let conn = db::open()?;
    if db::get_setting(&conn, "notifications_enabled", "1")? == "0" {
        return Ok(());
    }
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
            .unwrap_or_else(|| NaiveTime::from_hms_opt(9, 0, 0).unwrap());
        let event_datetime = occ_date.and_time(event_time);

        for offset in offsets {
            let remind_at = event_datetime - Duration::minutes(offset);
            if remind_at > now || remind_at < now - Duration::hours(STALE_AFTER_HOURS) {
                continue;
            }
            if db::mark_reminder_sent(&conn, occ.event.id, &occ.occurrence_date, offset)? {
                fire_notification(&occ.event.title, offset, event_datetime);
            }
        }
    }
    Ok(())
}

pub fn send_test_notification() -> Result<()> {
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

fn fire_notification(title: &str, offset_minutes: i64, event_datetime: chrono::NaiveDateTime) {
    let body = format!(
        "{}  {}",
        event_datetime.format("%Y-%m-%d %H:%M"),
        format_offset(offset_minutes)
    );
    if let Err(e) = try_toast(title, &body) {
        // 未打包的 Win32 应用发系统 Toast 通知有已知限制（需要 AUMID/开始菜单快捷方式），
        // 这里只记录日志、不让提醒线程因此崩溃；后续可以补充“应用内提醒列表”兜底展示。
        eprintln!("系统通知发送失败，已跳过（不影响后续提醒）: {e}");
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
