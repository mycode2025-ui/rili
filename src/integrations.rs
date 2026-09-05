//! 外部能力适配层。
//!
//! 这里统一承载 ICS URL 订阅和需要专用账号的 CalDAV 同步。OAuth/云同步等能力也应通过适配层进入，
//! 不把网络协议、OAuth 或第三方 SDK 代码散落到 GUI 和数据库模块中。

use crate::{db, ics, secret_store};
use anyhow::{Context, Result};
use quick_xml::events::Event as XmlEvent;
use quick_xml::Reader;
use serde::Serialize;
use std::thread;
use std::time::Duration;

const HTTP_TIMEOUT_SECS: u64 = 12;
const SUBSCRIPTION_REFRESH_SECS: u64 = 30 * 60;

/// Subscription URLs may contain bearer tokens, paths or userinfo. Never show
/// complete URLs in notifications, persistent error messages or copied details.
pub fn safe_error(message: &str) -> String {
    message
        .split_whitespace()
        .map(|part| {
            if part.contains("https://") || part.contains("http://") {
                "[服务器地址已隐藏]"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

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
    let content = if subscription.source_type == "caldav" {
        fetch_caldav(subscription)?
    } else {
        http_agent()
            .get(&subscription.url)
            .call()
            .with_context(|| format!("拉取 ICS 订阅失败：{}", subscription.url))?
            .into_string()
            .context("读取 ICS 订阅响应失败")?
    };
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
        let note = meeting_note(&event);
        let event_id = db::upsert_subscribed_event(
            conn,
            db::SubscribedEvent {
                subscription_id: subscription.id,
                calendar_id: subscription.calendar_id,
                external_uid: &uid,
                title: &event.title,
                date: event.date,
                time: event.time.as_deref(),
                duration_minutes: event.duration_minutes,
                note: &note,
                repeat_rule: &repeat_rule,
            },
        )?;
        db::set_subscribed_event_repeat_until(conn, event_id, event.repeat_until)?;
        imported += 1;
    }
    Ok(SubscriptionSyncResult {
        imported_events: imported,
        removed_cancelled_events: removed_cancelled,
    })
}

fn caldav_query() -> String {
    // 部分 CalDAV 服务（包括钉钉）不会为无时间范围的 REPORT 返回事件。
    // 覆盖过去两年到未来三年，既能导入近期历史，也能包含长期会议安排。
    let today = chrono::Utc::now().date_naive();
    let start = today - chrono::Duration::days(366 * 2);
    let end = today + chrono::Duration::days(366 * 3);
    format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<cal:calendar-query xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
  <d:prop><d:getetag/><cal:calendar-data/></d:prop>
  <cal:filter><cal:comp-filter name="VCALENDAR"><cal:comp-filter name="VEVENT"><cal:time-range start="{}T000000Z" end="{}T000000Z"/></cal:comp-filter></cal:comp-filter></cal:filter>
</cal:calendar-query>"#,
        start.format("%Y%m%d"),
        end.format("%Y%m%d")
    )
}

const CALDAV_DISCOVERY: &str = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
  <d:prop><d:resourcetype/><d:current-user-principal/><cal:calendar-home-set/></d:prop>
</d:propfind>"#;

#[derive(Default)]
struct DavDiscovery {
    principal: Option<String>,
    calendar_home: Option<String>,
    calendars: Vec<String>,
}

fn fetch_caldav(subscription: &db::Subscription) -> Result<String> {
    let password = secret_store::unprotect(&subscription.secret)?;
    let calendar_urls = discover_calendar_urls(subscription, &password)?;
    let query = caldav_query();
    let mut calendars = Vec::new();
    for calendar_url in calendar_urls {
        let xml = caldav_request(
            "REPORT",
            &calendar_url,
            &subscription.username,
            &password,
            "1",
            &query,
        )?;
        calendars.extend(extract_calendar_data(&xml)?);
    }
    Ok(calendars.join("\r\n"))
}

fn caldav_request(
    method: &str,
    url: &str,
    username: &str,
    password: &str,
    depth: &str,
    body: &str,
) -> Result<String> {
    use base64::Engine;

    let credentials =
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    http_agent()
        .request(method, url)
        .set("Depth", depth)
        .set("Content-Type", "application/xml; charset=utf-8")
        .set("Authorization", &format!("Basic {credentials}"))
        .send_string(body)
        .map_err(|error| match error {
            ureq::Error::Status(401, _) | ureq::Error::Status(403, _) => {
                anyhow::anyhow!("CalDAV 用户名或专用密码无效")
            }
            other => anyhow::anyhow!("连接 CalDAV 服务器失败：{other}"),
        })?
        .into_string()
        .context("读取 CalDAV 响应失败")
}

fn discover_calendar_urls(subscription: &db::Subscription, password: &str) -> Result<Vec<String>> {
    let discovery_url = dingtalk_user_discovery_url(&subscription.url, &subscription.username)?
        .unwrap_or_else(|| subscription.url.clone());

    let root_xml = caldav_request(
        "PROPFIND",
        &discovery_url,
        &subscription.username,
        password,
        "0",
        CALDAV_DISCOVERY,
    )?;
    let root = parse_dav_discovery(&root_xml)?;
    if !root.calendars.is_empty() {
        return root
            .calendars
            .iter()
            .map(|calendar| resolve_dav_href(&discovery_url, calendar))
            .collect();
    }

    let calendar_home = if let Some(home) = root.calendar_home {
        resolve_dav_href(&discovery_url, &home)?
    } else if let Some(principal) = root.principal {
        let principal_url = resolve_dav_href(&discovery_url, &principal)?;
        let principal_xml = caldav_request(
            "PROPFIND",
            &principal_url,
            &subscription.username,
            password,
            "0",
            CALDAV_DISCOVERY,
        )?;
        let principal = parse_dav_discovery(&principal_xml)?;
        let home = principal
            .calendar_home
            .context("服务器未返回 CalDAV 日历主目录")?;
        resolve_dav_href(&principal_url, &home)?
    } else {
        anyhow::bail!("服务器未返回 CalDAV 用户或日历地址")
    };

    let home_xml = caldav_request(
        "PROPFIND",
        &calendar_home,
        &subscription.username,
        password,
        "1",
        CALDAV_DISCOVERY,
    )?;
    let home = parse_dav_discovery(&home_xml)?;
    anyhow::ensure!(
        !home.calendars.is_empty(),
        "账号中未发现可同步的 CalDAV 日历"
    );
    home.calendars
        .iter()
        .map(|calendar| resolve_dav_href(&calendar_home, calendar))
        .collect()
}

fn dingtalk_user_discovery_url(server_url: &str, username: &str) -> Result<Option<String>> {
    let mut url = url::Url::parse(server_url).context("CalDAV 服务器地址无效")?;
    if url.host_str() != Some("calendar.dingtalk.com") {
        return Ok(None);
    }

    let username = username.trim();
    anyhow::ensure!(!username.is_empty(), "钉钉 CalDAV 用户名不能为空");
    url.set_query(None);
    url.set_fragment(None);
    url.set_path("/dav");
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("钉钉 CalDAV 服务器地址无效"))?
        .push("users")
        .push(username)
        .push("");
    Ok(Some(url.to_string()))
}

fn resolve_dav_href(base: &str, href: &str) -> Result<String> {
    Ok(url::Url::parse(base)
        .context("CalDAV 服务器地址无效")?
        .join(href)
        .context("CalDAV 返回了无效的日历地址")?
        .to_string())
}

fn parse_dav_discovery(xml: &str) -> Result<DavDiscovery> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut result = DavDiscovery::default();
    let mut in_response = false;
    let mut response_href = None;
    let mut response_is_calendar = false;
    let mut special_href: Option<&str> = None;
    let mut href_target: Option<&str> = None;
    let mut href = String::new();
    loop {
        match reader.read_event()? {
            XmlEvent::Start(element) => match element.local_name().as_ref() {
                b"response" => {
                    in_response = true;
                    response_href = None;
                    response_is_calendar = false;
                }
                b"current-user-principal" => special_href = Some("principal"),
                b"calendar-home-set" => special_href = Some("home"),
                b"calendar" if in_response => response_is_calendar = true,
                b"href" => {
                    href_target = special_href.or(if in_response && response_href.is_none() {
                        Some("response")
                    } else {
                        None
                    });
                    href.clear();
                }
                _ => {}
            },
            XmlEvent::End(element) => match element.local_name().as_ref() {
                b"href" => {
                    match href_target.take() {
                        Some("principal") => result.principal = Some(href.clone()),
                        Some("home") => result.calendar_home = Some(href.clone()),
                        Some("response") => response_href = Some(href.clone()),
                        _ => {}
                    }
                    href.clear();
                }
                b"current-user-principal" | b"calendar-home-set" => special_href = None,
                b"response" => {
                    if response_is_calendar {
                        if let Some(href) = response_href.take() {
                            result.calendars.push(href);
                        }
                    }
                    in_response = false;
                }
                _ => {}
            },
            XmlEvent::Empty(element)
                if in_response && element.local_name().as_ref() == b"calendar" =>
            {
                response_is_calendar = true;
            }
            XmlEvent::Text(text) if href_target.is_some() => {
                href.push_str(&quick_xml::escape::unescape(&text.decode()?)?);
            }
            XmlEvent::GeneralRef(reference) if href_target.is_some() => {
                let escaped = format!("&{};", reference.decode()?);
                href.push_str(&quick_xml::escape::unescape(&escaped)?);
            }
            XmlEvent::Eof => break,
            _ => {}
        }
    }
    Ok(result)
}

fn extract_calendar_data(xml: &str) -> Result<Vec<String>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut in_calendar_data = false;
    let mut current = String::new();
    let mut calendars = Vec::new();
    loop {
        match reader.read_event()? {
            XmlEvent::Start(element) if element.local_name().as_ref() == b"calendar-data" => {
                in_calendar_data = true;
                current.clear();
            }
            XmlEvent::End(element) if element.local_name().as_ref() == b"calendar-data" => {
                in_calendar_data = false;
                if !current.trim().is_empty() {
                    calendars.push(current.clone());
                }
            }
            XmlEvent::Text(text) if in_calendar_data => {
                let decoded = text.decode()?;
                current.push_str(&quick_xml::escape::unescape(&decoded)?);
            }
            XmlEvent::CData(text) if in_calendar_data => {
                current.push_str(&String::from_utf8_lossy(text.as_ref()));
            }
            XmlEvent::GeneralRef(reference) if in_calendar_data => {
                let reference = reference.decode()?;
                let escaped = format!("&{reference};");
                current.push_str(&quick_xml::escape::unescape(&escaped)?);
            }
            XmlEvent::Eof => break,
            _ => {}
        }
    }
    Ok(calendars)
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
            let message = safe_error(&error.to_string());
            let _ = db::set_subscription_result(conn, subscription.id, None, Some(&message));
            Err(anyhow::anyhow!(message))
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

fn meeting_note(event: &ics::ImportedEvent) -> String {
    let mut parts = Vec::new();
    if !event.location.trim().is_empty() {
        parts.push(format!("地点：{}", event.location.trim()));
    }
    if !event.url.trim().is_empty() && !event.note.contains(event.url.trim()) {
        parts.push(format!("入会链接：{}", event.url.trim()));
    }
    if !event.note.trim().is_empty() {
        parts.push(event.note.trim().to_string());
    }
    parts.join("\n")
}

pub fn get_or_create_integration_calendar(
    conn: &rusqlite::Connection,
    name: &str,
    color: &str,
) -> Result<db::Calendar> {
    if let Some(calendar) = db::list_calendars(conn)?
        .into_iter()
        .find(|calendar| calendar.name == name)
    {
        Ok(calendar)
    } else {
        db::create_calendar(conn, name, color)
    }
}

pub fn get_or_create_dingtalk_calendar(conn: &rusqlite::Connection) -> Result<db::Calendar> {
    get_or_create_integration_calendar(conn, "钉钉会议", "#1677ff")
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
                        crate::error_reporter::report("外部日历刷新失败", &error);
                    }
                }
            }
            Err(error) => crate::error_reporter::report("外部日历无法打开数据库", &error),
        }
        thread::sleep(Duration::from_secs(SUBSCRIPTION_REFRESH_SECS));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dingtalk_discovers_all_calendars_from_the_user_collection() {
        assert_eq!(
            dingtalk_user_discovery_url("https://calendar.dingtalk.com/dav", "u_example")
                .unwrap()
                .as_deref(),
            Some("https://calendar.dingtalk.com/dav/users/u_example/")
        );
        assert_eq!(
            dingtalk_user_discovery_url("https://calendar.example/dav", "u_example").unwrap(),
            None
        );
    }

    #[test]
    fn extracts_namespaced_and_cdata_calendar_payloads() {
        let xml = concat!(
            "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\" ",
            "xmlns:c=\"urn:ietf:params:xml:ns:caldav\"><d:response><d:propstat>",
            "<d:prop><c:calendar-data>BEGIN:VCALENDAR&#13;&#10;BEGIN:VEVENT&#13;&#10;",
            "UID:one&#13;&#10;DTSTART:20260905T090000&#13;&#10;SUMMARY:周会&#13;&#10;",
            "END:VEVENT&#13;&#10;END:VCALENDAR</c:calendar-data></d:prop>",
            "</d:propstat></d:response><d:response><d:propstat><d:prop>",
            "<c:calendar-data><![CDATA[BEGIN:VCALENDAR\r\nEND:VCALENDAR]]>",
            "</c:calendar-data></d:prop></d:propstat></d:response></d:multistatus>"
        );
        let calendars = extract_calendar_data(xml).unwrap();
        assert_eq!(calendars.len(), 2);
        assert!(calendars[0].contains("UID:one\r\n"));
        assert!(calendars[1].contains("BEGIN:VCALENDAR"));
    }

    #[test]
    fn discovers_principal_home_and_calendar_collection() {
        let principal = parse_dav_discovery(
            r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
            <d:response><d:href>/dav/user/</d:href><d:propstat><d:prop>
            <d:current-user-principal><d:href>/dav/user/</d:href></d:current-user-principal>
            <c:calendar-home-set><d:href>/dav/user/calendars/</d:href></c:calendar-home-set>
            </d:prop></d:propstat></d:response></d:multistatus>"#,
        )
        .unwrap();
        assert_eq!(principal.principal.as_deref(), Some("/dav/user/"));
        assert_eq!(
            principal.calendar_home.as_deref(),
            Some("/dav/user/calendars/")
        );

        let calendars = parse_dav_discovery(
            r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
            <d:response><d:href>/dav/user/calendars/meetings/</d:href><d:propstat><d:prop>
            <d:resourcetype><d:collection/><c:calendar/></d:resourcetype>
            </d:prop></d:propstat></d:response></d:multistatus>"#,
        )
        .unwrap();
        assert_eq!(calendars.calendars, vec!["/dav/user/calendars/meetings/"]);
        assert_eq!(
            resolve_dav_href("https://calendar.example/dav/", &calendars.calendars[0]).unwrap(),
            "https://calendar.example/dav/user/calendars/meetings/"
        );
    }
}
