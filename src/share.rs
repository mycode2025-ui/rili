//! 日程分享包适配层。
//!
//! 输出的是渠道无关的 JSON + ICS 载荷。微信小程序、网页或其他客户端可以把它上传/解析，
//! 但本地核心不直接依赖微信 SDK，也不会在没有 AppID/后台的情况下伪造分享成功。

use crate::{db, ics, recurrence};
use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareEvent {
    pub source_id: i64,
    pub title: String,
    pub date: String,
    pub time: Option<String>,
    pub note: String,
    pub repeat_rule: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareBundle {
    pub format_version: u32,
    pub generated_at: String,
    pub title: String,
    pub events: Vec<ShareEvent>,
    pub ics: String,
}

pub fn export_range(
    conn: &rusqlite::Connection,
    start: NaiveDate,
    end: NaiveDate,
    title: &str,
) -> Result<ShareBundle> {
    let events = db::list_events(conn, start, end)?;
    let share_events: Vec<ShareEvent> = events
        .iter()
        .map(|event| ShareEvent {
            source_id: event.id,
            title: event.title.clone(),
            date: event.date.clone(),
            time: event.time.clone(),
            note: event.note.clone(),
            repeat_rule: event.repeat_rule.clone(),
            category: event.category.clone(),
        })
        .collect();
    let parsed_rules: Vec<recurrence::RepeatRule> = events
        .iter()
        .map(|event| recurrence::RepeatRule::parse(&event.repeat_rule))
        .collect();
    let export_events: Vec<ics::ExportEvent<'_>> = events
        .iter()
        .zip(parsed_rules.iter())
        .filter_map(|(event, repeat_rule)| {
            let date = NaiveDate::parse_from_str(&event.date, "%Y-%m-%d").ok()?;
            Some(ics::ExportEvent {
                uid: format!("rili-share-{}", event.id),
                title: &event.title,
                date,
                time: event.time.as_deref(),
                note: &event.note,
                repeat_rule,
            })
        })
        .collect();
    Ok(ShareBundle {
        format_version: 1,
        generated_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        title: title.to_string(),
        events: share_events,
        ics: ics::export_ics(title, &export_events),
    })
}

pub fn import_bundle(
    conn: &rusqlite::Connection,
    bundle: &ShareBundle,
    calendar_id: i64,
) -> Result<usize> {
    anyhow::ensure!(
        bundle.format_version == 1,
        "不支持的分享包版本：{}",
        bundle.format_version
    );
    let events = ics::parse_ics(&bundle.ics).context("解析分享包中的 ICS 失败")?;
    let default_reminder = db::default_event_reminder(conn)?;
    let mut count = 0;
    for event in events {
        if event.cancelled {
            continue;
        }
        let repeat_rule = event.repeat_rule.to_string();
        let created = db::create_event(
            conn,
            db::NewEvent {
                title: &event.title,
                date: event.date,
                time: event.time.as_deref(),
                note: &event.note,
                repeat_rule: &repeat_rule,
                reminder_offsets: &default_reminder,
                category: "event",
                calendar_id,
            },
        )?;
        db::set_event_source_kind(conn, created.id, "shared")?;
        count += 1;
    }
    Ok(count)
}
