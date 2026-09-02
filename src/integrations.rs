//! 外部能力适配层。
//!
//! 这里先放不依赖账号后台的 ICS URL 订阅。Outlook/CalDAV/微信/云同步等能力都应通过适配层进入，
//! 不把网络协议、OAuth 或第三方 SDK 代码散落到 GUI 和数据库模块中。

use crate::{db, ics};
use anyhow::{Context, Result};
use serde::Serialize;
use std::thread;
use std::time::Duration;

const HTTP_TIMEOUT_SECS: u64 = 12;
const SUBSCRIPTION_REFRESH_SECS: u64 = 30 * 60;

fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .timeout_read(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .timeout_write(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub subscriptions: usize,
    pub imported_events: usize,
    pub removed_cancelled_events: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct SubscriptionSyncResult {
    pub imported_events: usize,
    pub removed_cancelled_events: usize,
}

fn fallback_uid(event: &ics::ImportedEvent) -> String {
    format!(
        "fallback:{}:{}:{}:{}",
        event.title,
        event.date,
        event.time.as_deref().unwrap_or(""),
        event.note
    )
}

fn sync_subscription_inner(
    conn: &rusqlite::Connection,
    subscription: &db::Subscription,
) -> Result<SubscriptionSyncResult> {
    let content = http_agent()
        .get(&subscription.url)
        .call()
        .with_context(|| format!("拉取 ICS 订阅失败：{}", subscription.url))?
        .into_string()
        .context("读取 ICS 订阅响应失败")?;
    let events = ics::parse_ics(&content).context("解析 ICS 订阅失败")?;
    let mut imported = 0;
    let mut removed_cancelled = 0;
    for event in events {
        let uid = if event.uid.trim().is_empty() {
            fallback_uid(&event)
        } else {
            event.uid.clone()
        };
        if event.cancelled {
            if db::delete_subscribed_event(conn, subscription.id, &uid)? {
                removed_cancelled += 1;
            }
            continue;
        }
        let repeat_rule = event.repeat_rule.to_string();
        db::upsert_subscribed_event(
            conn,
            subscription.id,
            subscription.calendar_id,
            &uid,
            &event.title,
            event.date,
            event.time.as_deref(),
            &event.note,
            &repeat_rule,
        )?;
        imported += 1;
    }
    Ok(SubscriptionSyncResult {
        imported_events: imported,
        removed_cancelled_events: removed_cancelled,
    })
}

pub fn sync_ics_subscription_with_result(
    conn: &rusqlite::Connection,
    subscription: &db::Subscription,
) -> Result<SubscriptionSyncResult> {
    if !subscription.enabled {
        anyhow::bail!("订阅已停用：{}", subscription.name);
    }
    match sync_subscription_inner(conn, subscription) {
        Ok(result) => {
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            db::set_subscription_result(conn, subscription.id, Some(&timestamp), None)?;
            Ok(result)
        }
        Err(error) => {
            let message = error.to_string();
            let _ = db::set_subscription_result(conn, subscription.id, None, Some(&message));
            Err(error)
        }
    }
}

#[allow(dead_code)]
pub fn sync_ics_subscription(
    conn: &rusqlite::Connection,
    subscription: &db::Subscription,
) -> Result<usize> {
    Ok(sync_ics_subscription_with_result(conn, subscription)?.imported_events)
}

pub fn sync_all_ics(conn: &rusqlite::Connection) -> Result<SyncReport> {
    let subscriptions = db::list_subscriptions(conn)?;
    let mut report = SyncReport {
        subscriptions: 0,
        imported_events: 0,
        removed_cancelled_events: 0,
        errors: Vec::new(),
    };
    for subscription in subscriptions
        .iter()
        .filter(|subscription| subscription.enabled)
    {
        report.subscriptions += 1;
        match sync_ics_subscription_with_result(conn, subscription) {
            Ok(result) => {
                report.imported_events += result.imported_events;
                report.removed_cancelled_events += result.removed_cancelled_events;
            }
            Err(error) => report
                .errors
                .push(format!("{}：{error}", subscription.name)),
        }
    }
    Ok(report)
}

/// 后台订阅刷新：启动时尝试一次，之后每 30 分钟执行；网络错误只写入订阅状态，不影响 GUI。
pub fn spawn() {
    thread::spawn(|| loop {
        match db::open() {
            Ok(conn) => {
                let local_only = db::get_setting(&conn, "local_only", "0")
                    .unwrap_or_else(|_| "0".to_string())
                    == "1";
                if !local_only {
                    if let Err(error) = sync_all_ics(&conn) {
                        eprintln!("ICS 订阅刷新失败（不影响其他功能）：{error}");
                    }
                }
            }
            Err(error) => eprintln!("ICS 订阅线程打开数据库失败：{error}"),
        }
        thread::sleep(Duration::from_secs(SUBSCRIPTION_REFRESH_SECS));
    });
}
