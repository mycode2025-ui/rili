//! 本地自然语言时间解析器。
//!
//! 它不是云端大模型，而是一个可解释、离线、可确认的轻量解析器，覆盖常见的中文日期、时间和提醒表达。
//! 未来接入 AI 服务时，只需要把服务结果转换成同一个 `Draft`，不会让模型直接写库。

use anyhow::{bail, Result};
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Draft {
    pub title: String,
    pub date: String,
    pub time: String,
    pub reminder: String,
    pub explanation: String,
}

pub fn parse(input: &str, today: NaiveDate) -> Result<Draft> {
    let input = input.trim();
    if input.is_empty() {
        bail!("请输入一句日程描述");
    }
    let date = find_date(input, today).unwrap_or(today);
    let time = find_time(input).unwrap_or_default();
    let reminder = find_reminder(input);
    let title = clean_title(input, date, &time, &reminder);
    if title.is_empty() {
        bail!("没有识别出日程标题，请补充要做的事情");
    }
    let mut explanation = format!("识别日期：{}", date);
    if !time.is_empty() {
        explanation.push_str(&format!("，时间：{time}"));
    }
    if !reminder.is_empty() {
        explanation.push_str(&format!("，提醒：{reminder} 分钟前"));
    }
    Ok(Draft {
        title,
        date: date.to_string(),
        time,
        reminder,
        explanation,
    })
}

fn find_date(input: &str, today: NaiveDate) -> Option<NaiveDate> {
    for (keyword, offset) in [("大后天", 3), ("后天", 2), ("明天", 1), ("今天", 0)] {
        if input.contains(keyword) {
            return Some(today + Duration::days(offset));
        }
    }
    if let Some(index) = input.find("下周") {
        let weekday = input[index + "下周".len()..]
            .chars()
            .next()
            .and_then(parse_weekday);
        let target = weekday.unwrap_or(Weekday::Mon);
        let current = today.weekday().num_days_from_monday() as i64;
        let target = target.num_days_from_monday() as i64;
        let offset = 7 + target - current;
        return Some(today + Duration::days(offset));
    }
    for candidate in input.as_bytes().windows(10) {
        if candidate[4] == b'-'
            && candidate[7] == b'-'
            && candidate.iter().enumerate().all(|(index, byte)| {
                (index == 4 || index == 7) && *byte == b'-'
                    || (index != 4 && index != 7 && byte.is_ascii_digit())
            })
        {
            if let Ok(candidate) = std::str::from_utf8(candidate) {
                if let Ok(date) = NaiveDate::parse_from_str(candidate, "%Y-%m-%d") {
                    return Some(date);
                }
            }
        }
    }
    if let Some(month_index) = input.find('月') {
        let month = last_ascii_digits(&input[..month_index])?
            .parse::<u32>()
            .ok()?;
        let after_month = &input[month_index + '月'.len_utf8()..];
        let day = first_ascii_digits(after_month)?.parse::<u32>().ok()?;
        let year = input[..month_index]
            .rfind('年')
            .and_then(|year_index| last_ascii_digits(&input[..year_index]))
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(today.year());
        return NaiveDate::from_ymd_opt(year, month, day);
    }
    None
}

fn find_time(input: &str) -> Option<String> {
    for candidate in input.as_bytes().windows(5) {
        if candidate[2] == b':'
            && candidate[0].is_ascii_digit()
            && candidate[1].is_ascii_digit()
            && candidate[3].is_ascii_digit()
            && candidate[4].is_ascii_digit()
        {
            let hour = u32::from(candidate[0] - b'0') * 10 + u32::from(candidate[1] - b'0');
            let minute = u32::from(candidate[3] - b'0') * 10 + u32::from(candidate[4] - b'0');
            if hour < 24 && minute < 60 {
                return Some(format!("{hour:02}:{minute:02}"));
            }
        }
    }
    for marker in ["下午", "晚上", "上午", "早上", "凌晨"] {
        let Some(index) = input.find(marker) else {
            continue;
        };
        let after = &input[index + marker.len()..];
        let number = first_chinese_number(after)?;
        let mut hour = number;
        if matches!(marker, "下午" | "晚上") && hour < 12 {
            hour += 12;
        }
        if marker == "凌晨" && hour == 12 {
            hour = 0;
        }
        let minute = if let Some(fen_index) = after.find('分') {
            first_chinese_number(&after[..fen_index]).unwrap_or(0)
        } else {
            0
        };
        if hour < 24 && minute < 60 {
            return Some(format!("{hour:02}:{minute:02}"));
        }
    }
    None
}

fn find_reminder(input: &str) -> String {
    let Some(index) = input.find("提前") else {
        return String::new();
    };
    let text = &input[index + "提前".len()..];
    if text.contains("一天") || text.contains("1天") || text.contains("一日") {
        return "1440".to_string();
    }
    if text.contains("半小时") {
        return "30".to_string();
    }
    if text.contains("一小时") || text.contains("1小时") {
        return "60".to_string();
    }
    first_chinese_number(text)
        .filter(|_| text.contains("分钟") || text.contains("分"))
        .map(|minutes| minutes.to_string())
        .unwrap_or_default()
}

fn clean_title(input: &str, date: NaiveDate, time: &str, reminder: &str) -> String {
    let mut title = input.to_string();
    for keyword in [
        "提醒我",
        "提醒",
        "帮我",
        "请",
        "安排",
        "创建",
        "新建",
        "今天",
        "明天",
        "后天",
        "大后天",
    ] {
        title = title.replace(keyword, "");
    }
    if let Some(index) = title.find("下周") {
        let after = &title[index + "下周".len()..];
        let end = after
            .chars()
            .next()
            .map(|character| index + "下周".len() + character.len_utf8())
            .unwrap_or(title.len());
        title.replace_range(index..end, "");
    }
    title = title.replace(&date.to_string(), "");
    if !time.is_empty() {
        title = title.replace(time, "");
    }
    for marker in ["下午", "晚上", "上午", "早上", "凌晨"] {
        if let Some(index) = title.find(marker) {
            let after = &title[index + marker.len()..];
            if let Some(point_index) = after.find('点') {
                let mut end = index + marker.len() + point_index + '点'.len_utf8();
                let after_point = &title[end..];
                if let Some(minute_index) = after_point.find('分') {
                    end += minute_index + '分'.len_utf8();
                }
                title.replace_range(index..end, "");
            }
        }
    }
    if let Some(index) = title.find("提前") {
        title.truncate(index);
    }
    if !reminder.is_empty() {
        title = title.replace(reminder, "");
    }
    title
        .trim_matches(|c: char| " ，,。；;：:的".contains(c))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_weekday(value: char) -> Option<Weekday> {
    match value {
        '一' | '1' => Some(Weekday::Mon),
        '二' | '2' => Some(Weekday::Tue),
        '三' | '3' => Some(Weekday::Wed),
        '四' | '4' => Some(Weekday::Thu),
        '五' | '5' => Some(Weekday::Fri),
        '六' | '6' => Some(Weekday::Sat),
        '日' | '天' | '7' => Some(Weekday::Sun),
        _ => None,
    }
}

fn first_chinese_number(value: &str) -> Option<u32> {
    let mut chars = value
        .chars()
        .skip_while(|c| !c.is_ascii_digit() && !is_cn_digit(*c));
    let first = chars.next()?;
    if first.is_ascii_digit() {
        let mut digits = String::from(first);
        digits.extend(chars.take_while(|c| c.is_ascii_digit()));
        return digits.parse().ok();
    }
    let mut word = String::from(first);
    word.extend(chars.take_while(|c| is_cn_digit(*c) || *c == '十'));
    chinese_number(&word)
}

fn chinese_number(value: &str) -> Option<u32> {
    if value == "十" {
        return Some(10);
    }
    if let Some(index) = value.find('十') {
        let left = &value[..index];
        let right = &value[index + '十'.len_utf8()..];
        let tens = if left.is_empty() {
            1
        } else {
            cn_digit(left.chars().next()?)?
        };
        let ones = if right.is_empty() {
            0
        } else {
            cn_digit(right.chars().next()?)?
        };
        return Some(tens * 10 + ones);
    }
    if value.chars().count() == 1 {
        return cn_digit(value.chars().next()?);
    }
    None
}

fn cn_digit(value: char) -> Option<u32> {
    match value {
        '零' | '〇' => Some(0),
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    }
}

fn is_cn_digit(value: char) -> bool {
    cn_digit(value).is_some()
}

fn last_ascii_digits(value: &str) -> Option<String> {
    let digits: String = value
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    (!digits.is_empty()).then_some(digits)
}

fn first_ascii_digits(value: &str) -> Option<String> {
    let digits: String = value
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    (!digits.is_empty()).then_some(digits)
}

#[cfg(test)]
mod tests {
    use super::parse;
    use chrono::NaiveDate;

    #[test]
    fn parses_common_chinese_sentence() {
        let draft = parse(
            "明天下午两点和供应商开会，提前一天提醒我",
            NaiveDate::from_ymd_opt(2026, 8, 31).unwrap(),
        )
        .unwrap();
        assert_eq!(draft.date, "2026-09-01");
        assert_eq!(draft.time, "14:00");
        assert_eq!(draft.reminder, "1440");
        assert!(draft.title.contains("供应商"));
        assert!(!draft.title.contains("下午"));
    }

    #[test]
    fn arbitrary_short_unicode_never_panics() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        for input in ["a", "测试", "😀", "9", "abc"] {
            let result = std::panic::catch_unwind(|| parse(input, today));
            assert!(result.is_ok(), "parser panicked for {input:?}");
        }
    }
}
