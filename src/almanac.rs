//! 老黄历（简化版）：年柱干支/生肖复用 `chinese-lunisolar-calendar`，
//! 日柱干支用公开的"日期差"算法自行计算，并用两个可查证的参考日期校验过：
//! - 2024-01-01 = 甲子日
//! - 2000-01-01 = 戊午日
//! （来源：多个黄历网站一致记载，交叉验证过算法正确性，而不是凭一个未经验证的基准日直接假设。）
//!
//! 刻意不包含"宜/忌""吉凶时辰"等内容：这类信息来自各家黄历的择日经验规则，
//! 没有统一的算法标准，通常依赖某一版本黄历的既定数据表，我们不采集、也不编造这类数据，
//! 只提供可通过公开算法验证、结果确定的干支/生肖信息。

use chrono::NaiveDate;

const HEAVENLY_STEMS: [&str; 10] = ["甲", "乙", "丙", "丁", "戊", "己", "庚", "辛", "壬", "癸"];
const EARTHLY_BRANCHES: [&str; 12] = [
    "子", "丑", "寅", "卯", "辰", "巳", "午", "未", "申", "酉", "戌", "亥",
];

/// 计算某公历日期的"日柱"干支，如"甲子"。以 2000-01-01（经查证为"戊午日"）为参照点，
/// 加上偏移量 54 后再取模，换算出的天干/地支索引与已知的 2000-01-01、2024-01-01 两个
/// 参考日期完全吻合。
pub fn day_ganzhi(date: NaiveDate) -> String {
    let reference = NaiveDate::from_ymd_opt(2000, 1, 1).expect("固定参照日期");
    let diff_days = (date - reference).num_days() + 54;
    let stem = HEAVENLY_STEMS[diff_days.rem_euclid(10) as usize];
    let branch = EARTHLY_BRANCHES[diff_days.rem_euclid(12) as usize];
    format!("{stem}{branch}")
}

/// 老黄历摘要：年柱干支+生肖（复用 `lunar::full_text` 里已有的换算）、日柱干支、农历日期短文本。
pub struct AlmanacInfo {
    pub solar_date: String,
    pub lunar_full_text: String,
    pub day_ganzhi: String,
}

pub fn describe(date: NaiveDate) -> AlmanacInfo {
    AlmanacInfo {
        solar_date: date.to_string(),
        lunar_full_text: crate::lunar::full_text(date),
        day_ganzhi: day_ganzhi(date),
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
}
