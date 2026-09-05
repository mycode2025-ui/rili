//! 日期计算器：两个日期之间的自然日间隔、工作日间隔，以及"从某天起过 N 天/N 个工作日"的推算。
//! 工作日判断复用 `holidays::work_state`（周末/法定节假日不算工作日，调休上班日算工作日）。

use crate::holidays::{self, WorkState};
use anyhow::{Context, Result};
use chrono::Datelike;
use chrono::NaiveDate;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DateDiff {
    pub start: String,
    pub end: String,
    /// 自然日间隔（end - start，可能为负）。
    pub calendar_days: i64,
    /// [start, end) 区间内的工作日天数（不含 end 当天；顺序反过来时为负）。
    pub workdays: i64,
}

/// Do not imply that an unsupported year's statutory holidays are known.
pub fn coverage_note(start: NaiveDate, end: NaiveDate) -> &'static str {
    if start.year() == 2026 && end.year() == 2026 {
        "工作日含 2026 年法定节假日及调休"
    } else {
        "注意：仅内置 2026 年节假日；其他年份按周一至周五估算，未计法定节假日及调休"
    }
}

/// 计算两个日期之间的自然日间隔与工作日间隔。
pub fn diff(start: NaiveDate, end: NaiveDate) -> DateDiff {
    let calendar_days = (end - start).num_days();
    let workdays = count_workdays_between(start, end);
    DateDiff {
        start: start.to_string(),
        end: end.to_string(),
        calendar_days,
        workdays,
    }
}

/// 统计 [start, end) 半开区间内的工作日天数；start > end 时返回负数（等价于 -diff(end, start).workdays）。
fn count_workdays_between(start: NaiveDate, end: NaiveDate) -> i64 {
    if start == end {
        return 0;
    }
    if start > end {
        return -count_workdays_between(end, start);
    }
    let mut count = 0i64;
    let mut d = start;
    while d < end {
        if is_workday(d) {
            count += 1;
        }
        let Some(next) = d.succ_opt() else {
            break;
        };
        d = next;
    }
    count
}

fn is_workday(date: NaiveDate) -> bool {
    matches!(
        holidays::work_state(date),
        WorkState::Workday | WorkState::MakeupWorkday
    )
}

/// 从 `start` 起，往后数 `n` 个自然日（n 可为负，表示往前）。
pub fn add_calendar_days(start: NaiveDate, n: i64) -> Result<NaiveDate> {
    start
        .checked_add_signed(chrono::Duration::try_days(n).context("推算天数过大")?)
        .context("日期推算超出支持范围")
}

/// 从 `start` 起（不含当天），往后数 `n` 个工作日，返回第 n 个工作日当天的日期（n 必须 >= 1）。
/// 若 n 为负，则往前数（返回第 |n| 个工作日之前的日期）。
pub fn add_workdays(start: NaiveDate, n: i64) -> Result<NaiveDate> {
    if n == 0 {
        return Ok(start);
    }
    let step = if n > 0 { 1 } else { -1 };
    anyhow::ensure!(
        n.unsigned_abs() <= 366_000,
        "推算天数过大，请输入 -366000 至 366000"
    );
    let mut remaining = n.abs();
    let mut d = start;
    while remaining > 0 {
        d = if step > 0 {
            d.succ_opt().context("日期推算超出支持范围")?
        } else {
            d.pred_opt().context("日期推算超出支持范围")?
        };
        if is_workday(d) {
            remaining -= 1;
        }
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_unsupported_holiday_years_and_extreme_offsets() {
        let known = NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
        let unknown = NaiveDate::from_ymd_opt(2027, 1, 1).unwrap();
        assert!(!coverage_note(known, known).contains("估算"));
        assert!(coverage_note(known, unknown).contains("估算"));
        assert!(add_workdays(known, i64::MIN).is_err());
        assert!(add_calendar_days(known, i64::MAX).is_err());
        assert!(add_calendar_days(known, i64::MIN).is_err());
        assert_eq!(diff(known, known).calendar_days, 0);
        assert_eq!(
            diff(known, unknown).workdays,
            -diff(unknown, known).workdays
        );
    }

    #[test]
    fn date_arithmetic_reports_supported_range_overflow() {
        assert!(add_calendar_days(NaiveDate::MAX, 1).is_err());
        assert!(add_workdays(NaiveDate::MAX, 1).is_err());
        assert!(add_workdays(NaiveDate::MIN, -1).is_err());
    }
}
