//! ICS（iCalendar，RFC 5545）导入 / 导出：让日程可以跟 Outlook / Google 日历 / Apple 日历互通，
//! 不需要任何账号或 OAuth —— 用户在这些软件里"导入 .ics 文件"或"订阅 .ics 链接"即可单向/定期同步。
//!
//! 刻意保持成一个"够用的子集"实现，而不是完整的 RFC 5545：
//! - 导出：把每条日程转成一个 VEVENT，重复规则尽量转成标准 RRULE（Outlook/Google 都认识 RRULE），
//!   这样导入方软件自己就能正确展开重复日程，不需要我们预先展开成几百条记录。
//! - 导入：只解析 VEVENT 里最常见的 SUMMARY / DTSTART / RRULE（FREQ + 简单 BYDAY），
//!   足够覆盖"从 Outlook/Google 导出后再导入回来"和大多数第三方日历导出文件的典型格式；
//!   无法无损表达的复杂 RRULE 会明确报错，避免把长期会议静默变成单次会议。

use crate::recurrence::RepeatRule;
use anyhow::{Context, Result};
use chrono::{LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

/// 星期几从我们内部编码（0=周一..6=周日）转成 RFC 5545 的两字母代码。
fn weekday_code(w: u8) -> &'static str {
    ["MO", "TU", "WE", "TH", "FR", "SA", "SU"][(w % 7) as usize]
}

fn weekday_from_code(code: &str) -> Option<u8> {
    match code {
        "MO" => Some(0),
        "TU" => Some(1),
        "WE" => Some(2),
        "TH" => Some(3),
        "FR" => Some(4),
        "SA" => Some(5),
        "SU" => Some(6),
        _ => None,
    }
}

/// 把内部重复规则转成 RRULE 的值（不含 "RRULE:" 前缀），`None` 表示不重复、不需要输出 RRULE 行。
fn repeat_rule_to_rrule(rule: &RepeatRule) -> Option<String> {
    match rule {
        RepeatRule::None => None,
        RepeatRule::Daily => Some("FREQ=DAILY".to_string()),
        RepeatRule::Weekly => Some("FREQ=WEEKLY".to_string()),
        RepeatRule::WeeklyOn(days) => {
            let byday = days
                .iter()
                .map(|d| weekday_code(*d))
                .collect::<Vec<_>>()
                .join(",");
            Some(format!("FREQ=WEEKLY;BYDAY={byday}"))
        }
        RepeatRule::Monthly => Some("FREQ=MONTHLY".to_string()),
        RepeatRule::MonthlyNth(n, w) => Some(format!("FREQ=MONTHLY;BYDAY={n}{}", weekday_code(*w))),
        RepeatRule::Yearly => Some("FREQ=YEARLY".to_string()),
    }
}

/// 反过来把 RRULE 值解析回内部重复规则，并保留 UNTIL 截止日。
/// COUNT 和非 1 的 INTERVAL 暂不展开，安全退化为单次事件，避免制造无限重复。
fn parse_rrule(rrule: &str) -> Result<(RepeatRule, Option<NaiveDate>)> {
    let mut freq = None;
    let mut byday: Option<&str> = None;
    let mut until = None;
    let mut unsupported = false;
    for part in rrule.split(';') {
        let mut kv = part.splitn(2, '=');
        match (kv.next(), kv.next()) {
            (Some("FREQ"), Some(v)) => freq = Some(v),
            (Some("BYDAY"), Some(v)) => byday = Some(v),
            (Some("UNTIL"), Some(v)) => {
                until = Some(parse_rrule_until(v).context("日历 RRULE 的 UNTIL 日期无效")?)
            }
            (Some("COUNT"), Some(_)) => unsupported = true,
            (Some("INTERVAL"), Some(v)) if v != "1" => unsupported = true,
            (Some("INTERVAL"), Some("1")) => {}
            (Some("WKST"), Some("MO" | "TU" | "WE" | "TH" | "FR" | "SA" | "SU")) => {}
            _ => unsupported = true,
        }
    }
    if unsupported {
        anyhow::bail!("暂不支持此日历重复规则：{rrule}");
    }
    let rule = match (freq, byday) {
        (Some("DAILY"), None) => RepeatRule::Daily,
        (Some("WEEKLY"), Some(days)) => {
            let parsed: Option<Vec<u8>> = days.split(',').map(weekday_from_code).collect();
            let parsed = parsed.context("日历 RRULE 的 BYDAY 无效")?;
            anyhow::ensure!(!parsed.is_empty(), "日历 RRULE 的 BYDAY 为空");
            RepeatRule::WeeklyOn(parsed)
        }
        (Some("WEEKLY"), None) => RepeatRule::Weekly,
        (Some("MONTHLY"), Some(spec)) => {
            // 形如 "3FR"：前面数字是第几个，后两位是星期几代码。
            let digits_end = spec
                .find(|c: char| !c.is_ascii_digit() && c != '-')
                .unwrap_or(0);
            let (n_part, code_part) = spec.split_at(digits_end);
            match (n_part.parse::<u8>().ok(), weekday_from_code(code_part)) {
                (Some(n), Some(w)) if (1..=5).contains(&n) => RepeatRule::MonthlyNth(n, w),
                _ => anyhow::bail!("暂不支持此日历重复规则：{rrule}"),
            }
        }
        (Some("MONTHLY"), None) => RepeatRule::Monthly,
        (Some("YEARLY"), None) => RepeatRule::Yearly,
        _ => RepeatRule::None,
    };
    anyhow::ensure!(rule != RepeatRule::None, "暂不支持此日历重复规则：{rrule}");
    Ok((rule, until))
}

fn parse_rrule_until(value: &str) -> Option<NaiveDate> {
    let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 14 && value.ends_with('Z') {
        let utc = NaiveDateTime::parse_from_str(&digits[..14], "%Y%m%d%H%M%S").ok()?;
        return Some(utc.and_utc().with_timezone(&chrono::Local).date_naive());
    }
    (digits.len() >= 8)
        .then(|| NaiveDate::parse_from_str(&digits[..8], "%Y%m%d").ok())
        .flatten()
}

/// 一条要导出的日程（已经是"基准记录"，不是展开后的每次发生；重复规则由 RRULE 表达）。
pub struct ExportEvent<'a> {
    pub uid: String,
    pub title: &'a str,
    pub date: NaiveDate,
    pub time: Option<&'a str>,
    pub note: &'a str,
    pub repeat_rule: &'a RepeatRule,
}

/// 生成一份完整的 .ics 文件内容（VCALENDAR，含若干 VEVENT）。
/// `calendar_name` 用作 `X-WR-CALNAME`，多数日历软件会拿它当作订阅后显示的日历名称。
pub fn export_ics(calendar_name: &str, events: &[ExportEvent]) -> String {
    let mut out = String::new();
    out.push_str("BEGIN:VCALENDAR\r\n");
    out.push_str("VERSION:2.0\r\n");
    out.push_str("PRODID:-//rili//本地优先日历//ZH\r\n");
    out.push_str("CALSCALE:GREGORIAN\r\n");
    out.push_str(&format!("X-WR-CALNAME:{}\r\n", escape_text(calendar_name)));
    let now_stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    for ev in events {
        out.push_str("BEGIN:VEVENT\r\n");
        out.push_str(&format!("UID:{}\r\n", ev.uid));
        out.push_str(&format!("DTSTAMP:{now_stamp}\r\n"));
        match ev.time {
            Some(t) => {
                let time = NaiveTime::parse_from_str(t, "%H:%M")
                    .unwrap_or_else(|_| NaiveTime::from_hms_opt(9, 0, 0).unwrap_or(NaiveTime::MIN));
                out.push_str(&format!(
                    "DTSTART:{}\r\n",
                    ev.date.and_time(time).format("%Y%m%dT%H%M%S")
                ));
            }
            None => {
                // 全天事件：VALUE=DATE，不带时间部分。
                out.push_str(&format!(
                    "DTSTART;VALUE=DATE:{}\r\n",
                    ev.date.format("%Y%m%d")
                ));
            }
        }
        if let Some(rrule) = repeat_rule_to_rrule(ev.repeat_rule) {
            out.push_str(&format!("RRULE:{rrule}\r\n"));
        }
        out.push_str(&format!("SUMMARY:{}\r\n", escape_text(ev.title)));
        if !ev.note.is_empty() {
            out.push_str(&format!("DESCRIPTION:{}\r\n", escape_text(ev.note)));
        }
        out.push_str("END:VEVENT\r\n");
    }
    out.push_str("END:VCALENDAR\r\n");
    out
}

fn escape_text(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(',', "\\,")
        .replace(';', "\\;")
        .replace('\n', "\\n")
}

fn unescape_text(s: &str) -> String {
    s.replace("\\n", "\n")
        .replace("\\;", ";")
        .replace("\\,", ",")
        .replace("\\\\", "\\")
}

/// 解析结果：一条待导入的日程。
#[derive(Debug, Clone)]
pub struct ImportedEvent {
    pub uid: String,
    pub title: String,
    pub date: NaiveDate,
    pub time: Option<String>,
    pub note: String,
    /// 会议地点（若来源提供 LOCATION）。
    pub location: String,
    /// 在线会议入口（若来源提供 URL）。
    pub url: String,
    /// 由 DTSTART/DTEND 或 DURATION 推导出的时长；缺失时保持 60 分钟。
    pub duration_minutes: i64,
    pub repeat_rule: RepeatRule,
    /// RRULE 的 UNTIL（含当天）；用于阻止已结束的外部周期事件无限延伸。
    pub repeat_until: Option<NaiveDate>,
    /// EXDATE 中明确排除的发生日期（已换算到本地日期）。
    pub exception_dates: Vec<NaiveDate>,
    /// RFC 5545 的 STATUS:CANCELLED，或 Outlook 导出时使用的“已取消:”标题。
    pub cancelled: bool,
}

fn title_marks_cancelled(title: &str) -> bool {
    let trimmed = title.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    trimmed.starts_with("已取消:")
        || trimmed.starts_with("已取消：")
        || lower.starts_with("canceled:")
        || lower.starts_with("cancelled:")
}

/// 解析 .ics 文件内容，提取其中所有 VEVENT。先按 RFC 5545 规则"反折行"
/// （连续行如果以空格/Tab开头，说明是上一行的延续），再逐个 VEVENT 块解析。
pub fn parse_ics(content: &str) -> Result<Vec<ImportedEvent>> {
    let unfolded = unfold_lines(content);
    let mut events = Vec::new();
    let mut in_event = false;
    let mut summary = String::new();
    let mut uid = String::new();
    let mut date: Option<NaiveDate> = None;
    let mut time: Option<String> = None;
    let mut note = String::new();
    let mut location = String::new();
    let mut url = String::new();
    let mut repeat_rule = RepeatRule::None;
    let mut repeat_until = None;
    let mut exception_dates = Vec::new();
    let mut cancelled = false;
    let mut start: Option<NaiveDateTime> = None;
    let mut end: Option<NaiveDateTime> = None;
    let mut explicit_duration: Option<i64> = None;

    for line in unfolded.lines() {
        let line = line.trim_end_matches('\r');
        if line == "BEGIN:VEVENT" {
            in_event = true;
            summary.clear();
            uid.clear();
            date = None;
            time = None;
            note.clear();
            location.clear();
            url.clear();
            repeat_rule = RepeatRule::None;
            repeat_until = None;
            exception_dates.clear();
            cancelled = false;
            start = None;
            end = None;
            explicit_duration = None;
            continue;
        }
        if line == "END:VEVENT" {
            if in_event {
                if let Some(d) = date {
                    events.push(ImportedEvent {
                        uid: uid.clone(),
                        title: summary.clone(),
                        date: d,
                        time: time.clone(),
                        note: note.clone(),
                        location: location.clone(),
                        url: url.clone(),
                        duration_minutes: explicit_duration
                            .or_else(|| {
                                end.zip(start)
                                    .map(|(end, start)| (end - start).num_minutes())
                            })
                            .filter(|minutes| *minutes > 0)
                            .unwrap_or(60),
                        repeat_rule: repeat_rule.clone(),
                        repeat_until,
                        exception_dates: exception_dates.clone(),
                        cancelled: cancelled || title_marks_cancelled(&summary),
                    });
                }
            }
            in_event = false;
            continue;
        }
        if !in_event {
            continue;
        }
        let Some((key_part, value)) = line.split_once(':') else {
            continue;
        };
        // key 可能带参数，如 "DTSTART;VALUE=DATE" 或 "DTSTART;TZID=Asia/Shanghai"。
        let key = key_part.split(';').next().unwrap_or(key_part);
        match key {
            "UID" => uid = unescape_text(value),
            "SUMMARY" => summary = unescape_text(value),
            "DESCRIPTION" => note = unescape_text(value),
            "LOCATION" => location = unescape_text(value),
            "URL" => url = unescape_text(value),
            "DURATION" => explicit_duration = parse_duration_minutes(value),
            "RRULE" => {
                (repeat_rule, repeat_until) = parse_rrule(value)?;
            }
            "EXDATE" => {
                for excluded in value.split(',') {
                    let (excluded_date, _, _) =
                        parse_ics_datetime(key_part, excluded)?.context("日历 EXDATE 日期无效")?;
                    exception_dates.push(excluded_date);
                }
            }
            "RECURRENCE-ID" => anyhow::bail!("暂不支持日历单次重排覆盖，已保留原有同步数据"),
            "STATUS" => cancelled = value.eq_ignore_ascii_case("CANCELLED"),
            "DTSTART" => {
                if let Some((parsed_date, parsed_time, parsed_start)) =
                    parse_ics_datetime(key_part, value)?
                {
                    date = Some(parsed_date);
                    time = parsed_time;
                    start = parsed_start;
                }
            }
            "DTEND" => end = parse_ics_datetime(key_part, value)?.and_then(|(_, _, value)| value),
            _ => {}
        }
    }
    Ok(events)
}

type ParsedIcsDateTime = (NaiveDate, Option<String>, Option<NaiveDateTime>);

fn parse_ics_datetime(key: &str, value: &str) -> Result<Option<ParsedIcsDateTime>> {
    let digits: String = value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(14)
        .collect();
    if digits.len() < 8 {
        return Ok(None);
    }
    let Some(date) = NaiveDate::parse_from_str(&digits[0..8], "%Y%m%d").ok() else {
        return Ok(None);
    };
    if !value.contains('T') || digits.len() < 12 {
        return Ok(Some((date, None, None)));
    }
    let candidate = format!("{}:{}", &digits[8..10], &digits[10..12]);
    let Ok(parsed_time) = NaiveTime::parse_from_str(&candidate, "%H:%M") else {
        return Ok(Some((date, None, None)));
    };
    let start = date.and_time(parsed_time);
    let local = if value.ends_with('Z') {
        start.and_utc().with_timezone(&chrono::Local).naive_local()
    } else if let Some(tzid) = key.split(';').find_map(|part| part.strip_prefix("TZID=")) {
        let name = tzid.trim_matches('"');
        let canonical = match name {
            "China Standard Time" => "Asia/Shanghai",
            "Eastern Standard Time" => "America/New_York",
            "Pacific Standard Time" => "America/Los_Angeles",
            "Central European Standard Time" => "Europe/Berlin",
            _ => name,
        };
        let timezone: chrono_tz::Tz = canonical
            .parse()
            .map_err(|_| anyhow::anyhow!("不支持的日历时区：{name}"))?;
        let zoned = match timezone.from_local_datetime(&start) {
            LocalResult::Single(value) => value,
            _ => anyhow::bail!("日历时间处于夏令时切换的歧义区间：{name} {start}"),
        };
        zoned.with_timezone(&chrono::Local).naive_local()
    } else {
        start
    };
    Ok(Some((
        local.date(),
        Some(local.format("%H:%M").to_string()),
        Some(local),
    )))
}

/// 解析会议导出常见的 ISO 8601 时长，如 PT45M、PT1H30M。
fn parse_duration_minutes(value: &str) -> Option<i64> {
    let value = value.trim().strip_prefix('P')?;
    let time = value.strip_prefix('T')?;
    let mut number = String::new();
    let mut minutes = 0i64;
    for ch in time.chars() {
        if ch.is_ascii_digit() {
            number.push(ch);
            continue;
        }
        let amount = number.parse::<i64>().ok()?;
        number.clear();
        match ch {
            'H' => minutes += amount * 60,
            'M' => minutes += amount,
            'S' => minutes += (amount > 0) as i64,
            _ => return None,
        }
    }
    (minutes > 0).then_some(minutes)
}

/// RFC 5545 行折叠的反向操作：延续行以单个空格或 Tab 开头，要拼接到上一行末尾（去掉这个前导空白）。
fn unfold_lines(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    for raw_line in content.split('\n') {
        let line = raw_line.trim_end_matches('\r');
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            out.push_str(&line[1..]);
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
        }
    }
    out
}

/// 读取 .ics 文件并解析；文件不存在/无法解码时返回明确的错误信息。
pub fn parse_ics_file(path: &std::path::Path) -> Result<Vec<ImportedEvent>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("读取 ICS 文件失败: {}", path.display()))?;
    parse_ics(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_recurrence_until_without_creating_an_infinite_series() {
        let content = concat!(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:bounded-weekly\r\n",
            "DTSTART;TZID=China Standard Time:20250904T100000\r\n",
            "RRULE:FREQ=WEEKLY;UNTIL=20250904T020000Z;INTERVAL=1;BYDAY=TH;WKST=MO\r\n",
            "SUMMARY:网络维护\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );
        let event = parse_ics(content).unwrap().remove(0);
        assert_eq!(event.repeat_rule, RepeatRule::WeeklyOn(vec![3]));
        assert_eq!(event.repeat_until, NaiveDate::from_ymd_opt(2025, 9, 4));
    }

    #[test]
    fn unsupported_recurrence_count_does_not_repeat_forever() {
        assert!(parse_rrule("FREQ=WEEKLY;COUNT=4;BYDAY=MO").is_err());
        assert!(parse_rrule("FREQ=WEEKLY;BYDAY=MO,XX").is_err());
        assert!(parse_rrule("FREQ=YEARLY;BYDAY=MO").is_err());
        assert!(parse_rrule("FREQ=DAILY;UNTIL=not-a-date").is_err());
    }

    #[test]
    fn recognizes_standard_and_outlook_cancelled_events() {
        let content = concat!(
            "BEGIN:VCALENDAR\r\n",
            "BEGIN:VEVENT\r\nUID:standard\r\nDTSTART:20260903T090000\r\n",
            "SUMMARY:普通标题\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:outlook\r\nDTSTART:20260904T090000\r\n",
            "SUMMARY:已取消: 项目周会\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:active\r\nDTSTART:20260905T090000\r\n",
            "SUMMARY:项目周会\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );

        let events = parse_ics(content).unwrap();
        assert_eq!(events.len(), 3);
        assert!(events[0].cancelled);
        assert!(events[1].cancelled);
        assert!(!events[2].cancelled);
    }

    #[test]
    fn malformed_or_non_ascii_time_never_panics() {
        let content = concat!(
            "BEGIN:VCALENDAR\r\n",
            "BEGIN:VEVENT\r\nUID:bad-time\r\nDTSTART:20260903T上午九点\r\n",
            "SUMMARY:中文时间\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:invalid-time\r\nDTSTART:20260904T996000\r\n",
            "SUMMARY:越界时间\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );

        let events = parse_ics(content).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].time, None);
        assert_eq!(events[1].time, None);
    }

    #[test]
    fn unfolds_and_unescapes_common_ics_text() {
        let content = concat!(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:folded\r\n",
            "DTSTART;VALUE=DATE:20260905\r\n",
            "SUMMARY:项目\\,周\r\n 会\r\n",
            "DESCRIPTION:第一行\\n第二行\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );
        let events = parse_ics(content).unwrap();
        assert_eq!(events[0].title, "项目,周会");
        assert_eq!(events[0].note, "第一行\n第二行");
    }

    #[test]
    fn imports_meeting_duration_location_and_join_url() {
        let content = concat!(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:ding-meeting-1\r\n",
            "DTSTART;TZID=Asia/Shanghai:20260905T093000\r\n",
            "DTEND;TZID=Asia/Shanghai:20260905T110000\r\n",
            "SUMMARY:钉钉项目会\r\nLOCATION:三楼会议室\r\n",
            "URL:https://meeting.dingtalk.com/example\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );
        let event = parse_ics(content).unwrap().remove(0);
        let source = NaiveDate::from_ymd_opt(2026, 9, 5)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        let expected = chrono_tz::Asia::Shanghai
            .from_local_datetime(&source)
            .single()
            .unwrap()
            .with_timezone(&chrono::Local);
        assert_eq!(event.date, expected.date_naive());
        assert_eq!(
            event.time.as_deref(),
            Some(expected.format("%H:%M").to_string().as_str())
        );
        assert_eq!(event.duration_minutes, 90);
        assert_eq!(event.location, "三楼会议室");
        assert_eq!(event.url, "https://meeting.dingtalk.com/example");
    }

    #[test]
    fn imports_explicit_meeting_duration() {
        let content = concat!(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:ding-meeting-2\r\n",
            "DTSTART:20260905T093000\r\nDURATION:PT1H15M\r\n",
            "SUMMARY:钉钉培训\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );
        assert_eq!(parse_ics(content).unwrap()[0].duration_minutes, 75);
    }

    #[test]
    fn converts_named_timezone_and_rejects_unknown_timezone() {
        let content = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:ny-meeting\r\nDTSTART;TZID=America/New_York:20260930T090000\r\nSUMMARY:纽约会议\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let event = &parse_ics(content).unwrap()[0];
        let source = NaiveDate::from_ymd_opt(2026, 9, 30)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let expected = chrono_tz::America::New_York
            .from_local_datetime(&source)
            .single()
            .unwrap()
            .with_timezone(&chrono::Local);
        assert_eq!(event.date, expected.date_naive());
        assert_eq!(
            event.time.as_deref(),
            Some(expected.format("%H:%M").to_string().as_str())
        );
        assert!(parse_ics(&content.replace("America/New_York", "Unknown/Zone")).is_err());
    }

    #[test]
    fn imports_exdates_and_rejects_unrepresentable_recurrence() {
        let content = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:weekly\r\nDTSTART;VALUE=DATE:20260930\r\nRRULE:FREQ=WEEKLY\r\nEXDATE;VALUE=DATE:20261007,20261014\r\nSUMMARY:例会\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let parsed = parse_ics(content).unwrap();
        assert_eq!(parsed[0].exception_dates.len(), 2);
        assert_eq!(
            parsed[0].exception_dates[0],
            NaiveDate::from_ymd_opt(2026, 10, 7).unwrap()
        );
        assert!(
            parse_ics(&content.replace("RRULE:FREQ=WEEKLY", "RRULE:FREQ=WEEKLY;INTERVAL=2"))
                .is_err()
        );
        assert!(
            parse_ics(&content.replace("EXDATE;VALUE=DATE", "RECURRENCE-ID;VALUE=DATE")).is_err()
        );
    }
}
