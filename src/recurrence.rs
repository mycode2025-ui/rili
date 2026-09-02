//! 重复日程规则：只做"按同一条基准日期 + 规则"在查询区间内展开发生日期，
//! 不支持单次例外编辑（编辑/删除只作用于整条重复序列），这是刻意简化的 MVP 范围。
//!
//! 规则编码为字符串存进数据库，支持两类"复杂重复"：
//! - `weekly:1,3,5`   —— 每周固定在多个星期几重复（0=周一 .. 6=周日），如"每周一三五"
//! - `monthly-nth:3:4` —— 每月第 N 个星期几重复（N=1..=5，星期几同上），如"每月第三个周五"

use chrono::{Datelike, NaiveDate};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepeatRule {
    None,
    Daily,
    /// 简单每周重复：固定间隔 7 天，落在与基准日期相同的星期几。
    Weekly,
    /// 每周固定在多个星期几重复（0=周一 .. 6=周日）。
    WeeklyOn(Vec<u8>),
    /// 简单每月重复：固定在与基准日期相同的"日"。
    Monthly,
    /// 每月第 N 个星期几（nth: 1..=5，weekday: 0=周一 .. 6=周日）。
    MonthlyNth(u8, u8),
    Yearly,
}

/// 中文星期几简写，索引 0=周一 .. 6=周日，用于 UI 展示。
pub const WEEKDAY_LABELS: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];

impl RepeatRule {
    pub fn parse(s: &str) -> Self {
        let s = s.trim();
        if let Some(rest) = s.strip_prefix("weekly:") {
            let days: Vec<u8> = rest
                .split(',')
                .filter_map(|d| d.trim().parse::<u8>().ok())
                .filter(|d| *d < 7)
                .collect();
            if !days.is_empty() {
                return RepeatRule::WeeklyOn(days);
            }
        }
        if let Some(rest) = s.strip_prefix("monthly-nth:") {
            let mut parts = rest.split(':');
            if let (Some(n), Some(w)) = (
                parts.next().and_then(|v| v.parse::<u8>().ok()),
                parts.next().and_then(|v| v.parse::<u8>().ok()),
            ) {
                if (1..=5).contains(&n) && w < 7 {
                    return RepeatRule::MonthlyNth(n, w);
                }
            }
        }
        match s {
            "daily" => RepeatRule::Daily,
            "weekly" => RepeatRule::Weekly,
            "monthly" => RepeatRule::Monthly,
            "yearly" => RepeatRule::Yearly,
            _ => RepeatRule::None,
        }
    }
}

impl fmt::Display for RepeatRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RepeatRule::None => f.write_str("none"),
            RepeatRule::Daily => f.write_str("daily"),
            RepeatRule::Weekly => f.write_str("weekly"),
            RepeatRule::WeeklyOn(days) => {
                let joined = days
                    .iter()
                    .map(|d| d.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                write!(f, "weekly:{joined}")
            }
            RepeatRule::Monthly => f.write_str("monthly"),
            RepeatRule::MonthlyNth(n, w) => write!(f, "monthly-nth:{n}:{w}"),
            RepeatRule::Yearly => f.write_str("yearly"),
        }
    }
}

/// 面向 UI 的中文说明，例如"每周一/三/五重复""每月第3个周五重复"。
pub fn describe(rule: &RepeatRule) -> String {
    match rule {
        RepeatRule::None => String::new(),
        RepeatRule::Daily => "每天重复".to_string(),
        RepeatRule::Weekly => "每周重复".to_string(),
        RepeatRule::WeeklyOn(days) => {
            let names = days
                .iter()
                .map(|d| WEEKDAY_LABELS[*d as usize % 7])
                .collect::<Vec<_>>()
                .join("/");
            format!("每周{names}重复")
        }
        RepeatRule::Monthly => "每月重复".to_string(),
        RepeatRule::MonthlyNth(n, w) => {
            format!("每月第{n}个周{}重复", WEEKDAY_LABELS[*w as usize % 7])
        }
        RepeatRule::Yearly => "每年重复".to_string(),
    }
}

/// 展开某条日程在 [start, end] 区间内的所有发生日期（含首次基准日期本身）。
/// 为避免无限循环，最多展开 1000 次迭代（复杂规则按"跨度"而不是"发生次数"限制，见各分支注释）。
pub fn occurrences_in_range(
    base: NaiveDate,
    rule: RepeatRule,
    start: NaiveDate,
    end: NaiveDate,
) -> Vec<NaiveDate> {
    if end < base && !matches!(rule, RepeatRule::WeeklyOn(_) | RepeatRule::MonthlyNth(_, _)) {
        return Vec::new();
    }
    let mut out = Vec::new();
    match rule {
        RepeatRule::None => {
            if base >= start && base <= end {
                out.push(base);
            }
        }
        RepeatRule::Daily => {
            let mut d = base;
            for _ in 0..1000 {
                if d > end {
                    break;
                }
                if d >= start {
                    out.push(d);
                }
                let Some(next) = d.succ_opt() else {
                    break;
                };
                d = next;
            }
        }
        RepeatRule::Weekly => {
            let mut d = base;
            for _ in 0..1000 {
                if d > end {
                    break;
                }
                if d >= start {
                    out.push(d);
                }
                d += chrono::Duration::days(7);
            }
        }
        RepeatRule::WeeklyOn(days) => {
            // 按天遍历（而不是按发生次数），最多遍历约 20 年，找出星期几命中的那些天。
            let scan_start = base.max(start - chrono::Duration::days(7));
            let mut d = scan_start;
            for _ in 0..(365 * 20) {
                if d > end {
                    break;
                }
                if d >= base
                    && d >= start
                    && days.contains(&(d.weekday().num_days_from_monday() as u8))
                {
                    out.push(d);
                }
                let Some(next) = d.succ_opt() else {
                    break;
                };
                d = next;
            }
        }
        RepeatRule::Monthly => {
            for i in 0..1000 {
                let Some(d) = add_months(base, i) else {
                    continue;
                };
                if d > end {
                    break;
                }
                if d >= start {
                    out.push(d);
                }
            }
        }
        RepeatRule::MonthlyNth(nth, weekday) => {
            // 按月遍历（而不是按发生次数），最多遍历 240 个月（20 年）。
            let mut year = base.year();
            let mut month = base.month();
            for _ in 0..240 {
                if let Some(d) = nth_weekday_of_month(year, month, nth, weekday) {
                    if d > end {
                        break;
                    }
                    if d >= base && d >= start {
                        out.push(d);
                    }
                }
                if month == 12 {
                    month = 1;
                    year += 1;
                } else {
                    month += 1;
                }
                if NaiveDate::from_ymd_opt(year, month, 1)
                    .map(|d| d > end)
                    .unwrap_or(true)
                {
                    break;
                }
            }
        }
        RepeatRule::Yearly => {
            for i in 0..1000 {
                let Some(d) = add_years(base, i) else {
                    continue;
                };
                if d > end {
                    break;
                }
                if d >= start {
                    out.push(d);
                }
            }
        }
    }
    out
}

/// 给日期加上 n 个月，保持"日"不变；若目标月份没有这一天（如 1/31 -> 2 月），
/// 返回 None（当月不产生这次重复，符合大多数日历软件的习惯）。
fn add_months(base: NaiveDate, n: i32) -> Option<NaiveDate> {
    let total_month0 = (base.year() * 12 + base.month() as i32 - 1) + n;
    let year = total_month0.div_euclid(12);
    let month = total_month0.rem_euclid(12) + 1;
    NaiveDate::from_ymd_opt(year, month as u32, base.day())
}

/// 给日期加上 n 年；若目标年份没有这一天（如 2/29 非闰年），返回 None（当年不重复）。
fn add_years(base: NaiveDate, n: i32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(base.year() + n, base.month(), base.day())
}

/// 计算某年某月的"第 nth 个星期 weekday"（weekday: 0=周一..6=周日）；不存在则返回 None
/// （例如某月没有第 5 个周五）。
fn nth_weekday_of_month(year: i32, month: u32, nth: u8, weekday: u8) -> Option<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let first_weekday = first.weekday().num_days_from_monday() as i64;
    let target_weekday = weekday as i64;
    let offset = (target_weekday - first_weekday).rem_euclid(7);
    let day = 1 + offset + (nth as i64 - 1) * 7;
    let candidate = first + chrono::Duration::days(day - 1);
    if candidate.month() == month {
        Some(candidate)
    } else {
        None
    }
}
