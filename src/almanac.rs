//! 老黄历：年柱干支/生肖复用 `chinese-lunisolar-calendar`，传统宜忌、值星、
//! 天神与冲煞来自开源 `lunar_rust` 数据表，日柱干支则用公开的日期差算法计算。
//! 日柱算法用两个可查证的参考日期校验过：
//! - 2024-01-01 = 甲子日
//! - 2000-01-01 = 戊午日
//!
//! （来源：多个黄历网站一致记载，交叉验证过算法正确性，而不是凭一个未经验证的基准日直接假设。）
//!
//! 宜忌等内容属于传统民俗信息，不作为现实决策建议；界面会明确标注这一点。

use chrono::Datelike;
use chrono::NaiveDate;
use lunar_rust::{lunar::LunarRefHelper, solar, solar::SolarRefHelper};

const HEAVENLY_STEMS: [&str; 10] = ["甲", "乙", "丙", "丁", "戊", "己", "庚", "辛", "壬", "癸"];
const EARTHLY_BRANCHES: [&str; 12] = [
    "子", "丑", "寅", "卯", "辰", "巳", "午", "未", "申", "酉", "戌", "亥",
];

/// 计算某公历日期的"日柱"干支，如"甲子"。以 2000-01-01（经查证为"戊午日"）为参照点，
/// 加上偏移量 54 后再取模，换算出的天干/地支索引与已知的 2000-01-01、2024-01-01 两个
/// 参考日期完全吻合。
pub fn day_ganzhi(date: NaiveDate) -> String {
    let reference = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap_or(NaiveDate::MIN);
    let diff_days = (date - reference).num_days() + 54;
    let stem = HEAVENLY_STEMS[diff_days.rem_euclid(10) as usize];
    let branch = EARTHLY_BRANCHES[diff_days.rem_euclid(12) as usize];
    format!("{stem}{branch}")
}

/// 黄历摘要：农历日期、日柱、值星、天神、传统宜忌与冲煞。
pub struct AlmanacInfo {
    pub solar_date: String,
    pub lunar_full_text: String,
    pub day_ganzhi: String,
    pub day_meta: String,
    pub suitable: String,
    pub avoid: String,
    pub clash: String,
}

pub fn describe(date: NaiveDate) -> AlmanacInfo {
    let solar = solar::from_ymd(date.year() as i64, date.month() as i64, date.day() as i64);
    let lunar = solar.get_lunar();
    let suitable = lunar
        .get_day_yi(None)
        .into_iter()
        .take(8)
        .collect::<Vec<_>>()
        .join(" · ");
    let avoid = lunar
        .get_day_ji(None)
        .into_iter()
        .take(8)
        .collect::<Vec<_>>()
        .join(" · ");
    AlmanacInfo {
        solar_date: date.to_string(),
        lunar_full_text: crate::lunar::full_text(date),
        day_ganzhi: day_ganzhi(date),
        day_meta: format!(
            "{}日 · {} · {}{}",
            lunar.get_day_in_gan_zhi(),
            lunar.get_zhi_xing(),
            lunar.get_day_tian_shen(),
            lunar.get_day_tian_shen_luck()
        ),
        suitable,
        avoid,
        clash: format!("冲{} · 煞{}", lunar.get_chong_sheng_xiao(), lunar.get_sha()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_known_reference_dates() {
        assert_eq!(
            day_ganzhi(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
            "甲子"
        );
        assert_eq!(
            day_ganzhi(NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()),
            "戊午"
        );
    }

    #[test]
    fn full_almanac_contains_daily_guidance() {
        let info = describe(NaiveDate::from_ymd_opt(2026, 9, 4).unwrap());
        assert!(!info.lunar_full_text.is_empty());
        assert!(!info.day_meta.is_empty());
        assert!(!info.suitable.is_empty());
        assert!(!info.avoid.is_empty());
        assert!(info.clash.starts_with('冲'));
    }
}
