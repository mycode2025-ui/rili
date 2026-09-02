//! 法定节假日 / 调休工作日数据表。
//!
//! 国务院办公厅每年会单独发文公布下一年的节假日安排（一般在上一年 11 月左右），
//! 这个表**不是**算法能推导出来的，必须每年从官方通知手工更新一次。
//! 当前内置了 2026 年的公开安排，供演示与本地使用；升级到新的一年时，
//! 只需要在下方补充一个新的 `HOLIDAYS_*` / `MAKEUP_WORKDAYS_*` 常量并在
//! `holiday_name` / `is_makeup_workday` 里加一段匹配即可。

use chrono::{Datelike, NaiveDate, Weekday};

/// 2026 年法定节假日（含调休后连续放假的周末），(月, 日, 假期名称)。
const HOLIDAYS_2026: &[(u32, u32, &str)] = &[
    (1, 1, "元旦"),
    (1, 2, "元旦"),
    (1, 3, "元旦"),
    (2, 15, "春节"),
    (2, 16, "春节"),
    (2, 17, "春节"),
    (2, 18, "春节"),
    (2, 19, "春节"),
    (2, 20, "春节"),
    (2, 21, "春节"),
    (2, 22, "春节"),
    (2, 23, "春节"),
    (4, 4, "清明节"),
    (4, 5, "清明节"),
    (4, 6, "清明节"),
    (5, 1, "劳动节"),
    (5, 2, "劳动节"),
    (5, 3, "劳动节"),
    (5, 4, "劳动节"),
    (5, 5, "劳动节"),
    (6, 19, "端午节"),
    (6, 20, "端午节"),
    (6, 21, "端午节"),
    (9, 25, "中秋节"),
    (9, 26, "中秋节"),
    (9, 27, "中秋节"),
    (10, 1, "国庆节"),
    (10, 2, "国庆节"),
    (10, 3, "国庆节"),
    (10, 4, "国庆节"),
    (10, 5, "国庆节"),
    (10, 6, "国庆节"),
    (10, 7, "国庆节"),
];

/// 2026 年调休上班日（周末但需要正常上班），(月, 日)。
const MAKEUP_WORKDAYS_2026: &[(u32, u32)] = &[(1, 4), (2, 14), (2, 28), (5, 9), (9, 20), (10, 10)];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkState {
    Workday,
    Weekend,
    Holiday,
    MakeupWorkday,
}

/// 若某天是法定节假日，返回节日名称；否则返回 None。
pub fn holiday_name(date: NaiveDate) -> Option<&'static str> {
    match date.year() {
        2026 => HOLIDAYS_2026
            .iter()
            .find(|(m, d, _)| *m == date.month() && *d == date.day())
            .map(|(_, _, name)| *name),
        _ => None,
    }
}

/// 若某天是调休后需要上班的周末，返回 true。
pub fn is_makeup_workday(date: NaiveDate) -> bool {
    match date.year() {
        2026 => MAKEUP_WORKDAYS_2026
            .iter()
            .any(|(m, d)| *m == date.month() && *d == date.day()),
        _ => false,
    }
}

pub fn is_weekend(date: NaiveDate) -> bool {
    matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
}

pub fn work_state(date: NaiveDate) -> WorkState {
    if is_makeup_workday(date) {
        WorkState::MakeupWorkday
    } else if holiday_name(date).is_some() {
        WorkState::Holiday
    } else if is_weekend(date) {
        WorkState::Weekend
    } else {
        WorkState::Workday
    }
}

/// 年历小格中的节日/节气标签。法定假期只在连续假期的第一天显示名称，
/// 避免一个月卡片里重复出现同一段文字。
pub fn notable_day_label(date: NaiveDate) -> Option<String> {
    if let Some(name) = crate::lunar::festival_label(date) {
        return Some(name.to_string());
    }
    if let Some(name) = solar_term_name(date) {
        return Some(name.to_string());
    }
    if let Some(name) = holiday_name(date) {
        // 传统节日与清明由其真实农历/节气日期负责标记，不能把调休区间
        // 的第一天误当作节日本身。
        if matches!(name, "春节" | "清明节" | "端午节" | "中秋节") {
            return None;
        }
        let previous_same = date
            .pred_opt()
            .and_then(holiday_name)
            .is_some_and(|previous| previous == name);
        if !previous_same {
            let short = match name {
                "清明节" => "清明",
                "劳动节" => "劳动",
                "端午节" => "端午",
                "中秋节" => "中秋",
                "国庆节" => "国庆",
                other => other,
            };
            return Some(short.to_string());
        }
    }
    None
}

/// 2001–2099 年二十四节气的常用日序算法，精度足够用于桌面年历标记。
/// 节气跨日发生在午夜附近时，民用日历可能与天文时刻相差一天。
fn solar_term_name(date: NaiveDate) -> Option<&'static str> {
    if !(2001..=2099).contains(&date.year()) {
        return None;
    }
    const TERMS: &[(u32, f64, &str)] = &[
        (1, 5.4055, "小寒"),
        (1, 20.12, "大寒"),
        (2, 3.87, "立春"),
        (2, 18.73, "雨水"),
        (3, 5.63, "惊蛰"),
        (3, 20.646, "春分"),
        (4, 4.81, "清明"),
        (4, 20.1, "谷雨"),
        (5, 5.52, "立夏"),
        (5, 21.04, "小满"),
        (6, 5.678, "芒种"),
        (6, 21.37, "夏至"),
        (7, 7.108, "小暑"),
        (7, 22.83, "大暑"),
        (8, 7.5, "立秋"),
        (8, 23.13, "处暑"),
        (9, 7.646, "白露"),
        (9, 23.042, "秋分"),
        (10, 8.318, "寒露"),
        (10, 23.438, "霜降"),
        (11, 7.438, "立冬"),
        (11, 22.36, "小雪"),
        (12, 7.18, "大雪"),
        (12, 21.94, "冬至"),
    ];
    let short_year = (date.year() % 100) as f64;
    TERMS.iter().find_map(|(month, constant, name)| {
        if *month != date.month() {
            return None;
        }
        let day =
            (short_year * 0.2422 + constant).floor() as u32 - ((date.year() % 100 - 1) / 4) as u32;
        (day == date.day()).then_some(*name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn year_view_labels_real_festival_days_without_holiday_duplicates() {
        assert_eq!(
            notable_day_label(NaiveDate::from_ymd_opt(2026, 4, 4).unwrap()),
            None
        );
        assert_eq!(
            notable_day_label(NaiveDate::from_ymd_opt(2026, 4, 5).unwrap()).as_deref(),
            Some("清明")
        );
        assert_eq!(
            notable_day_label(NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()).as_deref(),
            Some("国庆")
        );
        assert_eq!(
            notable_day_label(NaiveDate::from_ymd_opt(2026, 9, 23).unwrap()).as_deref(),
            Some("秋分")
        );
    }
}
