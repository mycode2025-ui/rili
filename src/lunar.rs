//! 农历换算：基于 `chinese-lunisolar-calendar` crate，把公历日期转换成
//! 日历格子里展示用的短文本（初一/十五/腊月 等），以及供 CLI 使用的完整农历字符串。

use chinese_lunisolar_calendar::{LunisolarDate, SolarDate};
use chrono::NaiveDate;

/// 日历格子里展示的短文本：
/// - 农历初一当天展示月份名（如"正月"），更贴近纸质日历的习惯；
/// - 其余日期展示"初一/初二/.../三十"这样的日序。
/// - 换算失败（超出支持范围）时返回空字符串，调用方需要能容忍空文本。
pub fn short_label(date: NaiveDate) -> String {
    match to_lunisolar(date) {
        Some(lunisolar) => {
            let day_text = lunisolar.to_lunar_day().to_string();
            if day_text == "初一" {
                lunisolar.to_lunar_month().to_string()
            } else {
                day_text
            }
        }
        None => String::new(),
    }
}

/// 供 CLI / 详情面板使用的完整农历描述，例如 "二〇二六　丙午年　正月　初一"。
pub fn full_text(date: NaiveDate) -> String {
    match to_lunisolar(date) {
        Some(lunisolar) => lunisolar.to_string(),
        None => String::new(),
    }
}

/// 传统节日短标签。这里只返回适合年历小格展示的关键节日，
/// 普通农历日期仍由 `short_label` 提供。
pub fn festival_label(date: NaiveDate) -> Option<&'static str> {
    let lunisolar = to_lunisolar(date)?;
    let month = lunisolar.to_lunar_month().to_string();
    let day = lunisolar.to_lunar_day().to_string();
    match (month.as_str(), day.as_str()) {
        ("正月", "初一") => Some("春节"),
        ("正月", "十五") => Some("元宵"),
        ("五月", "初五") => Some("端午"),
        ("七月", "初七") => Some("七夕"),
        ("八月", "十五") => Some("中秋"),
        ("九月", "初九") => Some("重阳"),
        ("腊月", "初八") => Some("腊八"),
        _ => None,
    }
}

fn to_lunisolar(date: NaiveDate) -> Option<LunisolarDate> {
    use chrono::Datelike;
    let solar = SolarDate::from_ymd(
        date.year().try_into().ok()?,
        date.month() as u8,
        date.day() as u8,
    )
    .ok()?;
    LunisolarDate::from_solar_date(solar).ok()
}
