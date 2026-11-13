//! The weather folded into the Today widget, from Open-Meteo (no key, no account): the current
//! temperature and condition, and today's high and low, for a place the user typed in Settings
//! (looked up once with Open-Meteo's geocoding and saved as name and coordinates). Requests carry
//! the coordinates and nothing else about the user. Parsing is pure and tested; the requests are
//! blocking, for a worker thread. The last reading is cached to disk.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const FORECAST: &str = "https://api.open-meteo.com/v1/forecast";
pub const GEOCODING: &str = "https://geocoding-api.open-meteo.com/v1/search";

/// What the icon shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sky {
    Clear,
    ClearNight,
    PartlyCloudy,
    PartlyCloudyNight,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Sleet,
    Snow,
    Thunder,
}

/// The icon and the words for a WMO weather code (Open-Meteo's `weather_code`).
pub fn condition(code: u32, day: bool) -> (Sky, &'static str) {
    match code {
        0 => (if day { Sky::Clear } else { Sky::ClearNight }, if day { "sunny" } else { "clear" }),
        1 => (if day { Sky::Clear } else { Sky::ClearNight }, "mainly clear"),
        2 => (if day { Sky::PartlyCloudy } else { Sky::PartlyCloudyNight }, "partly cloudy"),
        3 => (Sky::Cloudy, "cloudy"),
        45 | 48 => (Sky::Fog, "fog"),
        51 | 53 | 55 => (Sky::Drizzle, "drizzle"),
        56 | 57 => (Sky::Sleet, "freezing drizzle"),
        61 => (Sky::Rain, "light rain"),
        63 => (Sky::Rain, "rain"),
        65 => (Sky::Rain, "heavy rain"),
        66 | 67 => (Sky::Sleet, "freezing rain"),
        71 => (Sky::Snow, "light snow"),
        73 => (Sky::Snow, "snow"),
        75 => (Sky::Snow, "heavy snow"),
        77 => (Sky::Snow, "snow grains"),
        80 => (Sky::Rain, "light showers"),
        81 => (Sky::Rain, "showers"),
        82 => (Sky::Rain, "heavy showers"),
        85 | 86 => (Sky::Snow, "snow showers"),
        95 => (Sky::Thunder, "thunderstorm"),
        96 | 99 => (Sky::Thunder, "thunderstorm, hail"),
        _ => (Sky::Cloudy, "—"),
    }
}

/// The units the user wants, as saved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Units {
    /// As the Windows region says.
    #[default]
    Auto,
    Metric,
    Imperial,
}

impl Units {
    pub const ALL: [Units; 3] = [Units::Auto, Units::Metric, Units::Imperial];

    pub fn label(self) -> &'static str {
        match self {
            Units::Auto => "As region",
            Units::Metric => "°C",
            Units::Imperial => "°F",
        }
    }

    /// Whether Fahrenheit applies, with `region_imperial` for Auto.
    pub fn fahrenheit(self, region_imperial: bool) -> bool {
        match self {
            Units::Auto => region_imperial,
            Units::Metric => false,
            Units::Imperial => true,
        }
    }
}

/// A reading for one place.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Weather {
    /// When it was read, UTC seconds.
    pub fetched: i64,
    /// The place's name, as saved.
    pub place: String,
    pub lat: f64,
    pub lon: f64,
    pub fahrenheit: bool,
    pub temp: f64,
    pub code: u32,
    pub day: bool,
    pub high: f64,
    pub low: f64,
}

impl Weather {
    pub fn condition(&self) -> (Sky, &'static str) {
        condition(self.code, self.day)
    }

    /// Whether it was read for this place and these units.
    pub fn is_for(&self, lat: f64, lon: f64, fahrenheit: bool) -> bool {
        (self.lat - lat).abs() < 1e-4 && (self.lon - lon).abs() < 1e-4 && self.fahrenheit == fahrenheit
    }
}

/// `6°`, rounded.
pub fn degrees(t: f64) -> String {
    let r = t.round();
    // No "-0°".
    format!("{}°", if r == 0.0 { 0.0 } else { r })
}

pub fn forecast_url(lat: f64, lon: f64, fahrenheit: bool) -> String {
    let mut u = format!(
        "{FORECAST}?latitude={lat:.4}&longitude={lon:.4}&current=temperature_2m,weather_code,is_day&daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1"
    );
    if fahrenheit {
        u.push_str("&temperature_unit=fahrenheit");
    }
    u
}

/// The parts of a forecast answer the widget uses.
pub fn parse_forecast(json: &str) -> Option<(f64, u32, bool, f64, f64)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let cur = v.get("current")?;
    let temp = cur.get("temperature_2m")?.as_f64()?;
    let code = cur.get("weather_code")?.as_u64()? as u32;
    let day = cur.get("is_day").and_then(|d| d.as_u64()).is_none_or(|d| d != 0);
    let daily = v.get("daily")?;
    let first = |k: &str| daily.get(k)?.as_array()?.first()?.as_f64();
    Some((temp, code, day, first("temperature_2m_max")?, first("temperature_2m_min")?))
}

/// Read the weather for a place now. Blocking.
pub fn fetch(place: &str, lat: f64, lon: f64, fahrenheit: bool, now: i64) -> Result<Weather, String> {
    let text = crate::net::get_text(&forecast_url(lat, lon, fahrenheit)).map_err(|e| e.to_string())?;
    let (temp, code, day, high, low) = parse_forecast(&text).ok_or("the weather service answered oddly")?;
    Ok(Weather { fetched: now, place: place.to_owned(), lat, lon, fahrenheit, temp, code, day, high, low })
}

/// A place found by name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Place {
    pub name: String,
    /// Region and country, for telling places of one name apart: "England, United Kingdom".
    pub area: String,
    pub lat: f64,
    pub lon: f64,
}

pub fn geocode_url(name: &str) -> String {
    format!("{GEOCODING}?name={}&count=6&language=en&format=json", crate::net::encode(name.trim()))
}

pub fn parse_geocode(json: &str) -> Vec<Place> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    let Some(list) = v.get("results").and_then(|r| r.as_array()) else { return Vec::new() };
    list.iter()
        .filter_map(|r| {
            let s = |k: &str| r.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();
            let area: Vec<String> = [s("admin1"), s("country")].into_iter().filter(|x| !x.is_empty()).collect();
            Some(Place { name: s("name"), area: area.join(", "), lat: r.get("latitude")?.as_f64()?, lon: r.get("longitude")?.as_f64()? })
        })
        .filter(|p| !p.name.is_empty())
        .collect()
}

/// Look places up by name. Blocking.
pub fn geocode(name: &str) -> Result<Vec<Place>, String> {
    if name.trim().is_empty() {
        return Ok(Vec::new());
    }
    let text = crate::net::get_text(&geocode_url(name)).map_err(|e| e.to_string())?;
    Ok(parse_geocode(&text))
}

fn cache_file() -> Option<PathBuf> {
    crate::config::cache_dir().map(|d| d.join("weather.json"))
}

pub fn load_cache() -> Option<Weather> {
    let w: Weather = crate::config::load_from(&cache_file()?);
    (w.fetched > 0).then_some(w)
}

pub fn save_cache(w: &Weather) {
    if let Some(f) = cache_file() {
        crate::config::save_to(&f, w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORECAST_JSON: &str = r#"{"latitude":51.5,"longitude":-0.12,"timezone":"Europe/London","current_units":{"temperature_2m":"°C"},
"current":{"time":"2026-11-12T12:30","interval":900,"temperature_2m":6.4,"weather_code":61,"is_day":1},
"daily_units":{},"daily":{"time":["2026-11-12"],"temperature_2m_max":[8.2],"temperature_2m_min":[2.6]}}"#;

    const GEO_JSON: &str = r#"{"results":[{"id":2643743,"name":"London","latitude":51.50853,"longitude":-0.12574,"country":"United Kingdom","admin1":"England"},
{"id":6058560,"name":"London","latitude":42.98339,"longitude":-81.23304,"country":"Canada","admin1":"Ontario"}],"generationtime_ms":0.5}"#;

    #[test]
    fn forecast_answer() {
        let (t, code, day, hi, lo) = parse_forecast(FORECAST_JSON).unwrap();
        assert_eq!((t, code, day, hi, lo), (6.4, 61, true, 8.2, 2.6));
        assert_eq!(condition(code, day), (Sky::Rain, "light rain"));
        assert_eq!(degrees(t), "6°");
        assert_eq!(degrees(-0.4), "0°");
        assert_eq!(degrees(-3.6), "-4°");
        assert!(parse_forecast("{}").is_none());
        assert!(parse_forecast("nonsense").is_none());
    }

    #[test]
    fn codes_have_icons_and_words() {
        assert_eq!(condition(0, true).0, Sky::Clear);
        assert_eq!(condition(0, false), (Sky::ClearNight, "clear"));
        assert_eq!(condition(2, false).0, Sky::PartlyCloudyNight);
        assert_eq!(condition(45, true).0, Sky::Fog);
        assert_eq!(condition(73, true), (Sky::Snow, "snow"));
        assert_eq!(condition(95, true).0, Sky::Thunder);
        assert_eq!(condition(66, true).0, Sky::Sleet);
        assert_eq!(condition(1234, true).0, Sky::Cloudy);
    }

    #[test]
    fn places_and_urls() {
        let p = parse_geocode(GEO_JSON);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].area, "England, United Kingdom");
        assert!((p[1].lon + 81.233).abs() < 0.01);
        assert!(parse_geocode(r#"{"generationtime_ms":1}"#).is_empty());
        let u = forecast_url(51.50853, -0.12574, true);
        assert!(u.starts_with("https://api.open-meteo.com/v1/forecast?latitude=51.5085&longitude=-0.1257&"));
        assert!(u.ends_with("&temperature_unit=fahrenheit"));
        assert!(geocode_url("São Paulo").contains("name=S%C3%A3o%20Paulo"));
        assert!(Units::Auto.fahrenheit(true) && !Units::Metric.fahrenheit(true) && Units::Imperial.fahrenheit(false));
    }
}
