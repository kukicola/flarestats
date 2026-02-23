use chrono::{NaiveDate, NaiveDateTime, Timelike, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tokio::task::JoinHandle;

pub struct RefreshTask(pub Mutex<Option<JoinHandle<()>>>);

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    pub token: String,
    pub account_id: String,
    pub period: String,
    #[serde(default = "default_true")]
    pub exclude_bots: bool,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval: String,
}

fn default_true() -> bool {
    true
}

fn default_theme() -> String {
    "auto".to_string()
}

fn default_refresh_interval() -> String {
    "15m".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            token: String::new(),
            account_id: String::new(),
            period: String::new(),
            exclude_bots: true,
            theme: "auto".to_string(),
            refresh_interval: "15m".to_string(),
        }
    }
}

#[derive(Serialize, Clone)]
pub struct SiteData {
    pub name: String,
    pub visits: u64,
    pub page_views: u64,
    pub series: Vec<SeriesPoint>,
}

#[derive(Serialize, Clone)]
pub struct SeriesPoint {
    pub timestamp: String,
    pub visits: u64,
    pub page_views: u64,
}

fn settings_path(app: &AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .expect("failed to get app data dir");
    fs::create_dir_all(&dir).ok();
    dir.join("settings.json")
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<Settings, String> {
    let path = settings_path(&app);
    if path.exists() {
        let data = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&data).map_err(|e| e.to_string())
    } else {
        Ok(Settings {
            period: "24h".to_string(),
            ..Default::default()
        })
    }
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let path = settings_path(&app);
    let data = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(&path, data).map_err(|e| e.to_string())
}

async fn fetch_analytics_inner(app: &AppHandle) -> Result<Vec<SiteData>, String> {
    let settings = get_settings(app.clone())?;
    if settings.token.is_empty() || settings.account_id.is_empty() {
        return Err("Please configure API token and Account ID in settings".to_string());
    }

    let client = Client::new();

    let sites = fetch_sites(&client, &settings.token, &settings.account_id).await?;

    let futures: Vec<_> = sites
        .into_iter()
        .map(|(name, site_tag)| {
            let client = client.clone();
            let token = settings.token.clone();
            let account_id = settings.account_id.clone();
            let period = settings.period.clone();
            let exclude_bots = settings.exclude_bots;
            async move {
                fetch_site_analytics(&client, &token, &account_id, &name, &site_tag, &period, exclude_bots).await
            }
        })
        .collect();

    let results = futures::future::join_all(futures).await;
    let mut sites_data = Vec::new();
    for result in results {
        match result {
            Ok(data) => sites_data.push(data),
            Err(e) => eprintln!("Error fetching site data: {}", e),
        }
    }

    sites_data.sort_by(|a, b| b.visits.cmp(&a.visits));

    Ok(sites_data)
}

#[tauri::command]
pub async fn fetch_analytics(app: AppHandle) -> Result<Vec<SiteData>, String> {
    fetch_analytics_inner(&app).await
}

fn parse_interval_ms(interval: &str) -> u64 {
    match interval {
        "5m" => 300_000,
        "15m" => 900_000,
        "60m" => 3_600_000,
        _ => 900_000,
    }
}

#[tauri::command]
pub async fn start_background_refresh(app: AppHandle) -> Result<(), String> {
    let settings = get_settings(app.clone())?;
    let interval_ms = parse_interval_ms(&settings.refresh_interval);

    let state = app.state::<RefreshTask>();
    let mut handle = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(h) = handle.take() {
        h.abort();
    }

    let app_clone = app.clone();
    let task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;
            match fetch_analytics_inner(&app_clone).await {
                Ok(data) => {
                    let _ = app_clone.emit("analytics-refreshed", data);
                }
                Err(e) => eprintln!("Background refresh error: {}", e),
            }
        }
    });

    *handle = Some(task);
    Ok(())
}

async fn fetch_sites(
    client: &Client,
    token: &str,
    account_id: &str,
) -> Result<Vec<(String, String)>, String> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/rum/site_info/list",
        account_id
    );

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("API error {}: {}", status, body));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    let sites = body["result"]
        .as_array()
        .ok_or("Invalid response: missing result array")?
        .iter()
        .filter_map(|site| {
            let name = site["ruleset"]["zone_name"].as_str()?.to_string();
            let tag = site["site_tag"].as_str()?.to_string();
            Some((name, tag))
        })
        .collect();

    Ok(sites)
}

async fn fetch_site_analytics(
    client: &Client,
    token: &str,
    account_id: &str,
    name: &str,
    site_tag: &str,
    period: &str,
    exclude_bots: bool,
) -> Result<SiteData, String> {
    let (start, end, ts_field) = get_time_range(period);

    let query = format!(
        r#"{{
  viewer {{
    accounts(filter: {{ accountTag: $accountTag }}) {{
      totals: rumPageloadEventsAdaptiveGroups(limit: 1, filter: $filter) {{
        count
        sum {{ visits }}
      }}
      series: rumPageloadEventsAdaptiveGroups(limit: 5000, filter: $filter) {{
        count
        sum {{ visits }}
        dimensions {{ ts: {ts_field} }}
      }}
    }}
  }}
}}"#
    );

    let mut filters = vec![
        serde_json::json!({ "datetime_geq": start, "datetime_leq": end }),
        serde_json::json!({ "siteTag": site_tag }),
    ];
    if exclude_bots {
        filters.push(serde_json::json!({ "bot": 0 }));
    }

    let variables = serde_json::json!({
        "accountTag": account_id,
        "filter": { "AND": filters }
    });

    let body = serde_json::json!({
        "query": query,
        "variables": variables,
    });

    let resp = client
        .post("https://api.cloudflare.com/client/v4/graphql")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("GraphQL error: {}", resp.status()));
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    if let Some(errors) = data["errors"].as_array() {
        if !errors.is_empty() {
            return Err(format!("GraphQL errors: {:?}", errors));
        }
    }

    let accounts = &data["data"]["viewer"]["accounts"][0];

    let totals = accounts["totals"]
        .as_array()
        .and_then(|arr| arr.first());
    let page_views = totals.map_or(0, |t| t["count"].as_u64().unwrap_or(0));
    let visits = totals.map_or(0, |t| t["sum"]["visits"].as_u64().unwrap_or(0));

    let empty = vec![];
    let raw_series: HashMap<String, (u64, u64)> = accounts["series"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|point| {
            let ts = point["dimensions"]["ts"].as_str()?.to_string();
            let v = point["sum"]["visits"].as_u64().unwrap_or(0);
            let pv = point["count"].as_u64().unwrap_or(0);
            Some((ts, (v, pv)))
        })
        .collect();

    let series_data = fill_series_gaps(&start, &end, ts_field, &raw_series);

    Ok(SiteData {
        name: name.to_string(),
        visits,
        page_views,
        series: series_data,
    })
}

fn fill_series_gaps(
    start: &str,
    end: &str,
    ts_field: &str,
    data: &HashMap<String, (u64, u64)>,
) -> Vec<SeriesPoint> {
    let mut series = Vec::new();

    if ts_field == "datetimeHour" {
        let start_dt = NaiveDateTime::parse_from_str(start, "%Y-%m-%dT%H:%M:%SZ")
            .unwrap_or_default();
        let end_dt = NaiveDateTime::parse_from_str(end, "%Y-%m-%dT%H:%M:%SZ")
            .unwrap_or_default();
        let mut current = start_dt;
        while current <= end_dt {
            let key = current.format("%Y-%m-%dT%H:%M:%SZ").to_string();
            let (v, pv) = data.get(&key).copied().unwrap_or((0, 0));
            series.push(SeriesPoint {
                timestamp: key,
                visits: v,
                page_views: pv,
            });
            current += chrono::Duration::hours(1);
        }
    } else {
        let start_d = NaiveDate::parse_from_str(&start[..10], "%Y-%m-%d")
            .unwrap_or_default();
        let end_d = NaiveDate::parse_from_str(&end[..10], "%Y-%m-%d")
            .unwrap_or_default();
        let mut current = start_d;
        while current <= end_d {
            let key = current.format("%Y-%m-%d").to_string();
            let (v, pv) = data.get(&key).copied().unwrap_or((0, 0));
            series.push(SeriesPoint {
                timestamp: key,
                visits: v,
                page_views: pv,
            });
            current += chrono::Duration::days(1);
        }
    }

    series
}

fn get_time_range(period: &str) -> (String, String, &'static str) {
    let now = Utc::now();

    match period {
        "24h" => {
            let start = (now - chrono::Duration::hours(24))
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap();
            (
                start.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                now.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                "datetimeHour",
            )
        }
        "7d" => {
            let start = (now - chrono::Duration::days(6))
                .format("%Y-%m-%dT00:00:00Z")
                .to_string();
            let end = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
            (start, end, "date")
        }
        _ => {
            // 30d default
            let start = (now - chrono::Duration::days(29))
                .format("%Y-%m-%dT00:00:00Z")
                .to_string();
            let end = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
            (start, end, "date")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    // --- get_time_range tests ---

    #[test]
    fn test_get_time_range_24h_returns_datetime_hour() {
        let (start, end, field) = get_time_range("24h");
        assert_eq!(field, "datetimeHour");
        assert!(start.ends_with("Z"));
        assert!(end.ends_with("Z"));
        // start should be parseable and roughly 24h before end
        let start_dt = NaiveDateTime::parse_from_str(&start, "%Y-%m-%dT%H:%M:%SZ").unwrap();
        let end_dt = NaiveDateTime::parse_from_str(&end, "%Y-%m-%dT%H:%M:%SZ").unwrap();
        let diff = end_dt - start_dt;
        assert!(diff >= Duration::hours(23) && diff <= Duration::hours(25));
        // start should have minutes/seconds zeroed
        assert_eq!(start_dt.minute(), 0);
        assert_eq!(start_dt.second(), 0);
    }

    #[test]
    fn test_get_time_range_7d_returns_date() {
        let (start, end, field) = get_time_range("7d");
        assert_eq!(field, "date");
        assert!(start.contains("T00:00:00Z"));
        let start_dt = NaiveDateTime::parse_from_str(&start, "%Y-%m-%dT%H:%M:%SZ").unwrap();
        let end_dt = NaiveDateTime::parse_from_str(&end, "%Y-%m-%dT%H:%M:%SZ").unwrap();
        let diff = end_dt - start_dt;
        assert!(diff >= Duration::days(5) && diff <= Duration::days(7));
    }

    #[test]
    fn test_get_time_range_30d_returns_date() {
        let (start, end, field) = get_time_range("30d");
        assert_eq!(field, "date");
        assert!(start.contains("T00:00:00Z"));
        let start_dt = NaiveDateTime::parse_from_str(&start, "%Y-%m-%dT%H:%M:%SZ").unwrap();
        let end_dt = NaiveDateTime::parse_from_str(&end, "%Y-%m-%dT%H:%M:%SZ").unwrap();
        let diff = end_dt - start_dt;
        assert!(diff >= Duration::days(28) && diff <= Duration::days(30));
    }

    #[test]
    fn test_get_time_range_unknown_period_defaults_to_30d() {
        let (start, _, field) = get_time_range("unknown");
        assert_eq!(field, "date");
        assert!(start.contains("T00:00:00Z"));
    }

    // --- fill_series_gaps tests ---

    #[test]
    fn test_fill_series_gaps_hourly_fills_missing() {
        let data: HashMap<String, (u64, u64)> = HashMap::from([
            ("2024-01-15T00:00:00Z".to_string(), (10, 20)),
            ("2024-01-15T02:00:00Z".to_string(), (30, 40)),
        ]);
        let series = fill_series_gaps(
            "2024-01-15T00:00:00Z",
            "2024-01-15T03:00:00Z",
            "datetimeHour",
            &data,
        );
        assert_eq!(series.len(), 4); // 00, 01, 02, 03
        assert_eq!(series[0].visits, 10);
        assert_eq!(series[0].page_views, 20);
        assert_eq!(series[1].visits, 0); // gap filled
        assert_eq!(series[1].page_views, 0);
        assert_eq!(series[2].visits, 30);
        assert_eq!(series[2].page_views, 40);
        assert_eq!(series[3].visits, 0); // gap filled
    }

    #[test]
    fn test_fill_series_gaps_daily_fills_missing() {
        let data: HashMap<String, (u64, u64)> = HashMap::from([
            ("2024-01-15".to_string(), (100, 200)),
            ("2024-01-17".to_string(), (300, 400)),
        ]);
        let series = fill_series_gaps(
            "2024-01-15T00:00:00Z",
            "2024-01-18T00:00:00Z",
            "date",
            &data,
        );
        assert_eq!(series.len(), 4); // 15, 16, 17, 18
        assert_eq!(series[0].visits, 100);
        assert_eq!(series[1].visits, 0); // gap filled
        assert_eq!(series[2].visits, 300);
        assert_eq!(series[3].visits, 0);
    }

    #[test]
    fn test_fill_series_gaps_empty_data() {
        let data: HashMap<String, (u64, u64)> = HashMap::new();
        let series = fill_series_gaps(
            "2024-01-15T00:00:00Z",
            "2024-01-15T02:00:00Z",
            "datetimeHour",
            &data,
        );
        assert_eq!(series.len(), 3);
        assert!(series.iter().all(|p| p.visits == 0 && p.page_views == 0));
    }

    #[test]
    fn test_fill_series_gaps_full_data_no_gaps() {
        let data: HashMap<String, (u64, u64)> = HashMap::from([
            ("2024-01-15".to_string(), (1, 2)),
            ("2024-01-16".to_string(), (3, 4)),
        ]);
        let series = fill_series_gaps(
            "2024-01-15T00:00:00Z",
            "2024-01-16T23:59:59Z",
            "date",
            &data,
        );
        assert_eq!(series.len(), 2);
        assert_eq!(series[0].visits, 1);
        assert_eq!(series[1].visits, 3);
    }

    // --- Settings defaults tests ---

    #[test]
    fn test_settings_default_exclude_bots_true() {
        let settings = Settings::default();
        assert!(settings.exclude_bots);
    }

    #[test]
    fn test_settings_default_fields() {
        let settings = Settings::default();
        assert_eq!(settings.token, "");
        assert_eq!(settings.account_id, "");
        assert_eq!(settings.period, "");
        assert!(settings.exclude_bots);
    }

    #[test]
    fn test_settings_deserialize_missing_exclude_bots_defaults_true() {
        let json = r#"{"token":"t","account_id":"a","period":"24h"}"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        assert!(settings.exclude_bots);
    }

    #[test]
    fn test_settings_deserialize_explicit_exclude_bots_false() {
        let json = r#"{"token":"t","account_id":"a","period":"24h","exclude_bots":false}"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        assert!(!settings.exclude_bots);
    }

    // --- parse_interval_ms tests ---

    #[test]
    fn test_parse_interval_ms_known_values() {
        assert_eq!(parse_interval_ms("5m"), 300_000);
        assert_eq!(parse_interval_ms("15m"), 900_000);
        assert_eq!(parse_interval_ms("60m"), 3_600_000);
    }

    #[test]
    fn test_parse_interval_ms_unknown_defaults_to_15m() {
        assert_eq!(parse_interval_ms("unknown"), 900_000);
        assert_eq!(parse_interval_ms(""), 900_000);
    }

    #[test]
    fn test_settings_deserialize_missing_refresh_interval_defaults() {
        let json = r#"{"token":"t","account_id":"a","period":"24h"}"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.refresh_interval, "15m");
    }
}
