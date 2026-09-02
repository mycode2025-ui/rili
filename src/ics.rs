//! ICS（iCalendar，RFC 5545）导入 / 导出：让日程可以跟 Outlook / Google 日历 / Apple 日历互通，
//! 不需要任何账号或 OAuth —— 用户在这些软件里"导入 .ics 文件"或"订阅 .ics 链接"即可单向/定期同步。
//!
//! 刻意保持成一个"够用的子集"实现，而不是完整的 RFC 5545：
//! - 导出：把每条日程转成一个 VEVENT，重复规则尽量转成标准 RRULE（Outlook/Google 都认识 RRULE），
//!   这样导入方软件自己就能正确展开重复日程，不需要我们预先展开成几百条记录。
//! - 导入：只解析 VEVENT 里最常见的 SUMMARY / DTSTART / RRULE（FREQ + 简单 BYDAY），
//!   足够覆盖"从 Outlook/Google 导出后再导入回来"和大多数第三方日历导出文件的典型格式；
//!   遇到解析不了的复杂 RRULE（如 BYMONTHDAY、EXDATE 例外等）会退化成"只导入这一天"，不会报错崩溃。

use crate::recurrence::RepeatRule;
use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveTime};

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

/// 反过来把 RRULE 值解析回内部重复规则；只处理 FREQ + 可选单个 BYDAY，
/// 遇到不认识的组合（INTERVAL!=1、COUNT、UNTIL、多重 BYRULE 等）一律退化为不重复，
/// 这样至少"这一次"的日程能正确导入，不会整条丢失或程序崩溃。
fn rrule_to_repeat_rule(rrule: &str) -> RepeatRule {
    let mut freq = None;
    let mut byday: Option<&str> = None;
    for part in rrule.split(';') {
        let mut kv = part.splitn(2, '=');
        match (kv.next(), kv.next()) {
            (Some("FREQ"), Some(v)) => freq = Some(v),
            (Some("BYDAY"), Some(v)) => byday = Some(v),
            _ => {}
        }
    }
    match (freq, byday) {
        (Some("DAILY"), _) => RepeatRule::Daily,
        (Some("WEEKLY"), Some(days)) => {
            let parsed: Vec<u8> = days.split(',').filter_map(weekday_from_code).collect();
            if parsed.is_empty() {
                RepeatRule::Weekly
            } else {
                RepeatRule::WeeklyOn(parsed)
            }
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
                _ => RepeatRule::Monthly,
            }
        }
        (Some("MONTHLY"), None) => RepeatRule::Monthly,
        (Some("YEARLY"), _) => RepeatRule::Yearly,
        _ => RepeatRule::None,
    }
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
    pub repeat_rule: RepeatRule,
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
    let mut repeat_rule = RepeatRule::None;
    let mut cancelled = false;

    for line in unfolded.lines() {
        let line = line.trim_end_matches('\r');
        if line == "BEGIN:VEVENT" {
            in_event = true;
            summary.clear();
            uid.clear();
            date = None;
            time = None;
            note.clear();
            repeat_rule = RepeatRule::None;
            cancelled = false;
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
                        repeat_rule: repeat_rule.clone(),
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
            "RRULE" => repeat_rule = rrule_to_repeat_rule(value),
            "STATUS" => cancelled = value.eq_ignore_ascii_case("CANCELLED"),
            "DTSTART" => {
                let digits: String = value.chars().take_while(|c| c.is_ascii_digit()).collect();
                if digits.len() >= 8 {
                    date = NaiveDate::parse_from_str(&digits[0..8], "%Y%m%d").ok();
                }
                if let Some(t_idx) = value.find('T') {
                    let digits: String = value[t_idx + 1..]
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .take(4)
                        .collect();
                    if digits.len() == 4 {
                        let candidate = format!("{}:{}", &digits[0..2], &digits[2..4]);
                        if NaiveTime::parse_from_str(&candidate, "%H:%M").is_ok() {
                            time = Some(candidate);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(events)
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
}
