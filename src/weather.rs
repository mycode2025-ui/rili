//! 天气预报：基于 Open-Meteo（`https://open-meteo.com`）的免费开放接口，**不需要注册、不需要 API Key**。
//! 网络请求只在后台线程里定时进行，绝不在 UI 线程/`refresh_all` 里同步发起，避免卡界面。
//! 结果缓存进 `settings` 表（键 `weather_cache`），GUI 侧只读缓存，读不到时安静地不显示天气，
//! 不会因为网络问题影响日历本身的可用性。

use anyhow::{Context, Result};
use chrono::Timelike;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::thread;
use std::time::Duration as StdDuration;

const GEOCODE_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
const MET_NO_FORECAST_URL: &str = "https://api.met.no/weatherapi/locationforecast/2.0/compact";
/// 后台刷新间隔：天气不需要分钟级实时性，30 分钟刷新一次足够，也减少对免费接口的请求压力。
const REFRESH_INTERVAL_SECS: u64 = 30 * 60;
/// 单次网络请求的超时时间：网络不通/被防火墙拦截时，最多等这么久就放弃，
/// 不会让后台线程或 CLI 卡住很久（ureq 默认没有显式超时，容易在受限网络环境下挂起）。
const HTTP_TIMEOUT_SECS: u64 = 8;

fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(StdDuration::from_secs(HTTP_TIMEOUT_SECS))
        .timeout_read(StdDuration::from_secs(HTTP_TIMEOUT_SECS))
        .timeout_write(StdDuration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyWeather {
    pub date: String,
    pub temp_max: f64,
    pub temp_min: f64,
    pub code: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherNow {
    pub city: String,
    pub temp_c: f64,
    pub code: i64,
    pub is_day: bool,
    pub updated_at: String,
    #[serde(default)]
    pub provider: String,
    pub daily: Vec<DailyWeather>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocationCache {
    city: String,
    latitude: f64,
    longitude: f64,
    resolved_name: String,
}

/// WMO 天气代码 -> (中文描述, 图标标识)。界面图标由 Slint SVG 资源负责，
/// 这里不返回 Emoji，避免 Windows 字体回退后出现彩色字形或方框。
pub fn describe_code(code: i64) -> (&'static str, &'static str) {
    match code {
        0 => ("晴", "sun"),
        1 => ("大致晴朗", "sun-cloud"),
        2 => ("多云", "cloud"),
        3 => ("阴", "cloud"),
        45 | 48 => ("雾", "fog"),
        51 | 53 | 55 => ("毛毛雨", "rain"),
        56 | 57 => ("冻雨", "rain"),
        61 | 63 | 65 => ("雨", "rain"),
        66 | 67 => ("冻雨", "rain"),
        71 | 73 | 75 => ("雪", "snow"),
        77 => ("阵雪粒", "snow"),
        80..=82 => ("阵雨", "rain"),
        85 | 86 => ("阵雪", "snow"),
        95 => ("雷阵雨", "storm"),
        96 | 99 => ("雷阵雨伴冰雹", "storm"),
        _ => ("未知", "unknown"),
    }
}

/// 返回桌面天气卡片使用的本地 SVG 类型。晴朗和少云会根据昼夜切换太阳/月亮，
/// 其余类型直接与 WMO 天气代码对应，确保图标和中文描述来自同一份数据。
pub fn icon_key(code: i64, is_day: bool) -> &'static str {
    let (_, kind) = describe_code(code);
    match (kind, is_day) {
        ("sun", false) => "moon",
        ("sun-cloud", false) => "moon-cloud",
        _ => kind,
    }
}

#[derive(Deserialize)]
struct GeocodeResponse {
    results: Option<Vec<GeocodeResult>>,
}

#[derive(Deserialize)]
struct GeocodeResult {
    name: String,
    latitude: f64,
    longitude: f64,
    country: Option<String>,
    admin1: Option<String>,
    admin2: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LocationCandidate {
    pub name: String,
    pub label: String,
    pub latitude: f64,
    pub longitude: f64,
}

fn geocode_search_terms(city: &str) -> Vec<String> {
    let city = city.trim();
    let mut terms = vec![city.to_string()];
    for separator in ["自治区", "省", "市"] {
        if let Some((_, tail)) = city.rsplit_once(separator) {
            if !tail.trim().is_empty() {
                terms.push(tail.trim().to_string());
            }
        }
    }
    let seeds = terms.clone();
    for seed in seeds {
        for suffix in ["自治县", "自治旗", "县", "区", "旗", "市"] {
            if let Some(value) = seed.strip_suffix(suffix) {
                if !value.trim().is_empty() {
                    terms.push(value.trim().to_string());
                }
                break;
            }
        }
    }
    terms.retain(|value| !value.is_empty());
    terms.dedup();
    terms
}

pub fn search_locations(city: &str) -> Result<Vec<LocationCandidate>> {
    let city = city.trim();
    anyhow::ensure!(!city.is_empty(), "请输入城市或区县名称");
    for term in geocode_search_terms(city) {
        let url = format!(
            "{GEOCODE_URL}?name={}&count=8&language=zh",
            urlencoding_lite(&term)
        );
        let resp: GeocodeResponse = http_agent()
            .get(&url)
            .call()
            .context("请求地理编码接口失败")?
            .into_json()
            .context("解析地理编码响应失败")?;
        let mut candidates = Vec::new();
        for result in resp.results.unwrap_or_default() {
            let mut regions = Vec::new();
            for region in [result.admin2, result.admin1, result.country]
                .into_iter()
                .flatten()
            {
                if region != result.name && !regions.contains(&region) {
                    regions.push(region);
                }
            }
            let label = if regions.is_empty() {
                result.name.clone()
            } else {
                format!("{} · {}", result.name, regions.join(" · "))
            };
            if !candidates.iter().any(|candidate: &LocationCandidate| {
                (candidate.latitude - result.latitude).abs() < 0.0001
                    && (candidate.longitude - result.longitude).abs() < 0.0001
            }) {
                candidates.push(LocationCandidate {
                    name: result.name,
                    label,
                    latitude: result.latitude,
                    longitude: result.longitude,
                });
            }
        }
        if !candidates.is_empty() {
            return Ok(candidates);
        }
    }
    anyhow::bail!("未找到城市或区县: {city}")
}

/// 把城市或区县名换算成经纬度。后台自动刷新沿用第一个候选；交互式修改城市时
/// 会把全部候选交给界面，让用户确认省市区县后再保存。
fn geocode(city: &str) -> Result<(f64, f64, String)> {
    let candidate = search_locations(city)?
        .into_iter()
        .next()
        .with_context(|| format!("未找到城市或区县: {city}"))?;
    Ok((candidate.latitude, candidate.longitude, candidate.name))
}

#[derive(Deserialize)]
struct ForecastResponse {
    current_weather: Option<CurrentWeather>,
    daily: Option<DailySection>,
}

#[derive(Deserialize)]
struct CurrentWeather {
    temperature: f64,
    weathercode: i64,
    is_day: i64,
}

#[derive(Deserialize)]
struct DailySection {
    time: Vec<String>,
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
    weathercode: Vec<i64>,
}

fn fetch_open_meteo_forecast(lat: f64, lon: f64) -> Result<(CurrentWeather, Vec<DailyWeather>)> {
    let url = format!(
        "{FORECAST_URL}?latitude={lat}&longitude={lon}&current_weather=true&daily=temperature_2m_max,temperature_2m_min,weathercode&timezone=auto&forecast_days=5"
    );
    let resp: ForecastResponse = http_agent()
        .get(&url)
        .call()
        .context("请求天气预报接口失败")?
        .into_json()
        .context("解析天气预报响应失败")?;
    let current = resp
        .current_weather
        .context("天气响应缺少 current_weather 字段")?;
    let daily = resp.daily.context("天气响应缺少 daily 字段")?;
    let days = daily
        .time
        .iter()
        .zip(daily.temperature_2m_max.iter())
        .zip(daily.temperature_2m_min.iter())
        .zip(daily.weathercode.iter())
        .map(|(((date, max), min), code)| DailyWeather {
            date: date.clone(),
            temp_max: *max,
            temp_min: *min,
            code: *code,
        })
        .collect();
    Ok((current, days))
}

#[derive(Deserialize)]
struct MetResponse {
    properties: MetProperties,
}

#[derive(Deserialize)]
struct MetProperties {
    timeseries: Vec<MetTimeSeries>,
}

#[derive(Deserialize)]
struct MetTimeSeries {
    time: String,
    data: MetData,
}

#[derive(Deserialize)]
struct MetData {
    instant: MetInstant,
    next_1_hours: Option<MetPeriod>,
    next_6_hours: Option<MetPeriod>,
    next_12_hours: Option<MetPeriod>,
}

#[derive(Deserialize)]
struct MetInstant {
    details: MetInstantDetails,
}

#[derive(Deserialize)]
struct MetInstantDetails {
    air_temperature: f64,
}

#[derive(Deserialize)]
struct MetPeriod {
    summary: MetSummary,
}

#[derive(Deserialize)]
struct MetSummary {
    symbol_code: String,
}

fn met_symbol(data: &MetData) -> &str {
    data.next_1_hours
        .as_ref()
        .or(data.next_6_hours.as_ref())
        .or(data.next_12_hours.as_ref())
        .map(|period| period.summary.symbol_code.as_str())
        .unwrap_or("cloudy")
}

fn met_symbol_to_wmo(symbol: &str) -> i64 {
    if symbol.contains("thunder") {
        95
    } else if symbol.contains("snow") {
        71
    } else if symbol.contains("sleet") {
        66
    } else if symbol.contains("rainshowers") {
        80
    } else if symbol.contains("rain") {
        61
    } else if symbol.contains("fog") {
        45
    } else if symbol.contains("partlycloudy") {
        2
    } else if symbol.contains("cloudy") {
        3
    } else if symbol.contains("fair") {
        1
    } else if symbol.contains("clearsky") {
        0
    } else {
        3
    }
}

fn weather_severity(code: i64) -> i32 {
    match code {
        95..=99 => 7,
        71..=86 => 6,
        61..=67 => 5,
        51..=57 => 4,
        45 | 48 => 3,
        3 => 2,
        1 | 2 => 1,
        _ => 0,
    }
}

/// Open-Meteo 的预报域名在部分网络中会被单独阻断。MET Norway 提供同样按经纬度
/// 查询的公开全球预报，因此只在主源失败时使用，避免一次网络故障让天气永久空白。
fn fetch_met_no_forecast(lat: f64, lon: f64) -> Result<(CurrentWeather, Vec<DailyWeather>)> {
    let url = format!("{MET_NO_FORECAST_URL}?lat={lat:.4}&lon={lon:.4}");
    let response: MetResponse = http_agent()
        .get(&url)
        .set(
            "User-Agent",
            "TimeHub/0.1 (local desktop calendar; no account)",
        )
        .call()
        .context("请求备用天气预报接口失败")?
        .into_json()
        .context("解析备用天气预报响应失败")?;
    let first = response
        .properties
        .timeseries
        .first()
        .context("备用天气响应没有时间序列")?;
    let first_symbol = met_symbol(&first.data);
    let first_time = chrono::DateTime::parse_from_rfc3339(&first.time)
        .context("备用天气当前时间格式无效")?
        .with_timezone(&chrono::Local);
    let current = CurrentWeather {
        temperature: first.data.instant.details.air_temperature,
        weathercode: met_symbol_to_wmo(first_symbol),
        is_day: if first_symbol.ends_with("_day") {
            1
        } else if first_symbol.ends_with("_night") {
            0
        } else if (6..18).contains(&first_time.hour()) {
            1
        } else {
            0
        },
    };

    let mut grouped: BTreeMap<String, DailyWeather> = BTreeMap::new();
    for point in &response.properties.timeseries {
        let timestamp = chrono::DateTime::parse_from_rfc3339(&point.time)
            .with_context(|| format!("备用天气时间格式无效: {}", point.time))?
            .with_timezone(&chrono::Local);
        let date = timestamp.date_naive().to_string();
        let temp = point.data.instant.details.air_temperature;
        let code = met_symbol_to_wmo(met_symbol(&point.data));
        grouped
            .entry(date.clone())
            .and_modify(|day| {
                day.temp_max = day.temp_max.max(temp);
                day.temp_min = day.temp_min.min(temp);
                if weather_severity(code) > weather_severity(day.code) {
                    day.code = code;
                }
            })
            .or_insert(DailyWeather {
                date,
                temp_max: temp,
                temp_min: temp,
                code,
            });
    }
    let daily = grouped.into_values().take(5).collect();
    Ok((current, daily))
}

fn fetch_forecast(lat: f64, lon: f64) -> Result<(CurrentWeather, Vec<DailyWeather>, &'static str)> {
    match fetch_open_meteo_forecast(lat, lon) {
        Ok((current, daily)) => Ok((current, daily, "Open-Meteo")),
        Err(primary_error) => fetch_met_no_forecast(lat, lon)
            .map(|(current, daily)| (current, daily, "MET Norway"))
            .with_context(|| format!("主天气源失败: {primary_error:#}")),
    }
}

/// 极简的 URL query 编码：只处理天气模块里会出现的中文城市名场景，不追求通用性。
fn urlencoding_lite(s: &str) -> String {
    let mut out = String::new();
    for byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// 读取缓存的天气数据（不发网络请求），供 GUI 在 `refresh_all` 里同步调用。
pub fn cached(conn: &Connection) -> Option<WeatherNow> {
    let raw = crate::db::get_setting(conn, "weather_cache", "").ok()?;
    if raw.is_empty() {
        return None;
    }
    serde_json::from_str(&raw).ok()
}

/// 实际发起网络请求刷新天气、写入缓存，并返回刷新后的结果；只应在后台线程或用户主动触发时调用
/// （CLI 的 `rili weather refresh` 也走这个函数）。
pub fn refresh_once(conn: &Connection) -> Result<WeatherNow> {
    let city =
        crate::db::get_setting(conn, "weather_city", "北京").unwrap_or_else(|_| "北京".to_string());
    anyhow::ensure!(
        !city.trim().is_empty(),
        "未设置城市（用 `rili weather set-city` 设置，或该功能已被关闭）"
    );
    let cached_location = crate::db::get_setting(conn, "weather_location_cache", "")
        .ok()
        .and_then(|raw| serde_json::from_str::<LocationCache>(&raw).ok())
        .filter(|location| location.city.trim().eq_ignore_ascii_case(city.trim()));
    let location = match cached_location {
        Some(location) => location,
        None => {
            let (latitude, longitude, resolved_name) = geocode(&city)?;
            let location = LocationCache {
                city: city.trim().to_string(),
                latitude,
                longitude,
                resolved_name,
            };
            let json = serde_json::to_string(&location).context("序列化天气城市坐标失败")?;
            crate::db::set_setting(conn, "weather_location_cache", &json)?;
            location
        }
    };
    let (current, daily, provider) = fetch_forecast(location.latitude, location.longitude)?;
    let now = WeatherNow {
        city: location.resolved_name,
        temp_c: current.temperature,
        code: current.weathercode,
        is_day: current.is_day != 0,
        updated_at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
        provider: provider.to_string(),
        daily,
    };
    let json = serde_json::to_string(&now).context("序列化天气缓存失败")?;
    crate::db::set_setting(conn, "weather_cache", &json)?;
    Ok(now)
}

pub fn refresh_for_location(
    conn: &Connection,
    query: &str,
    candidate: &LocationCandidate,
) -> Result<WeatherNow> {
    let query = query.trim();
    anyhow::ensure!(!query.is_empty(), "请输入城市或区县名称");
    crate::db::set_setting(conn, "weather_city", query)?;
    let location = LocationCache {
        city: query.to_string(),
        latitude: candidate.latitude,
        longitude: candidate.longitude,
        resolved_name: candidate.name.clone(),
    };
    crate::db::set_setting(
        conn,
        "weather_location_cache",
        &serde_json::to_string(&location).context("序列化天气城市坐标失败")?,
    )?;
    let (current, daily, provider) = fetch_forecast(candidate.latitude, candidate.longitude)?;
    let now = WeatherNow {
        city: candidate.name.clone(),
        temp_c: current.temperature,
        code: current.weathercode,
        is_day: current.is_day != 0,
        updated_at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
        provider: provider.to_string(),
        daily,
    };
    crate::db::set_setting(
        conn,
        "weather_cache",
        &serde_json::to_string(&now).context("序列化天气缓存失败")?,
    )?;
    Ok(now)
}

/// 启动后台天气刷新线程：独立打开自己的数据库连接，随进程退出而结束。
/// 启动后先立刻刷新一次，之后每 30 分钟刷新一次；任何网络错误只记录日志，不影响主程序运行。
pub fn spawn() {
    thread::spawn(|| loop {
        match crate::db::open() {
            Ok(conn) => {
                if let Err(e) = refresh_once(&conn) {
                    crate::error_reporter::report("后台天气刷新失败，继续使用缓存", &e);
                }
            }
            Err(e) => crate::error_reporter::report("天气刷新无法打开数据库", &e),
        }
        thread::sleep(StdDuration::from_secs(REFRESH_INTERVAL_SECS));
    });
}

#[cfg(test)]
mod tests {
    use super::{geocode_search_terms, icon_key};

    #[test]
    fn county_names_have_compatible_fallbacks() {
        assert_eq!(geocode_search_terms("固安县"), ["固安县", "固安"]);
        assert_eq!(
            geocode_search_terms("河北省固安县"),
            ["河北省固安县", "固安县", "河北省固安", "固安"]
        );
        assert_eq!(
            geocode_search_terms("北京市延庆区"),
            ["北京市延庆区", "延庆区", "北京市延庆", "延庆"]
        );
    }

    #[test]
    fn weather_icons_follow_code_and_daylight() {
        assert_eq!(icon_key(0, true), "sun");
        assert_eq!(icon_key(0, false), "moon");
        assert_eq!(icon_key(1, true), "sun-cloud");
        assert_eq!(icon_key(1, false), "moon-cloud");
        assert_eq!(icon_key(63, true), "rain");
        assert_eq!(icon_key(75, true), "snow");
        assert_eq!(icon_key(95, true), "storm");
    }
}
