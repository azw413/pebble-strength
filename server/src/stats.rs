//! Unlisted /stats dashboard: self-logged page views + referrers (Apache's
//! access logs live on the proxy machine, so the app records its own), plus
//! live counts from the DB. Gated to the admin email. Modelled on ~/game's
//! analytics: append page views to daily JSONL, aggregate on read.

use std::io::Write;
use std::path::Path;

use axum::extract::State;
use chrono::{Duration, Utc};
use diesel::prelude::*;

use crate::AppState;

fn today() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

// ---- write side: a middleware that logs HTML page GETs ----

/// Middleware: record a page view for real page navigations — not the API,
/// static assets, auth callbacks, or the /stats dashboard itself. No IP, no
/// cookie; just path, referrer host, and the day.
pub async fn log_pageview(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let path = req.uri().path().to_string();
    let is_get = req.method() == axum::http::Method::GET;
    let log_it = is_get
        && !path.starts_with("/api/")
        && !path.starts_with("/static/")
        && !path.starts_with("/auth/")
        && path != "/stats"
        && (path == "/" || path.ends_with(".html") || !path.contains('.'));
    if log_it {
        let referrer = req
            .headers()
            .get("referer")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        append_view(&state, &path, referrer.as_deref()).await;
    }
    next.run(req).await
}

async fn append_view(state: &AppState, path: &str, referrer: Option<&str>) {
    let line = format!(
        "{}\n",
        serde_json::json!({ "t": Utc::now().to_rfc3339(), "path": path, "ref": referrer })
    );
    let file = state.log_dir.join(format!("views-{}.jsonl", today()));
    let _guard = state.log_lock.lock().await;
    match std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        Ok(mut f) => {
            let _ = f.write_all(line.as_bytes());
        }
        Err(e) => eprintln!("stats: open {file:?}: {e}"),
    }
}

// ---- read side: aggregate the view logs + DB counts ----

pub struct KeyCount {
    pub key: String,
    pub n: u64,
}

pub struct DayCount {
    pub day: String,
    pub n: u64,
}

/// A DB entity count with a week-over-week trend.
pub struct Metric {
    pub label: String,
    pub total: i64,
    pub new_7d: i64,
    pub prev_7d: i64,
}
impl Metric {
    pub fn delta(&self) -> i64 {
        self.new_7d - self.prev_7d
    }
}

fn is_real_page(path: &str) -> bool {
    matches!(
        path,
        "/" | "/workouts"
            | "/sessions"
            | "/recordings"
            | "/devices"
            | "/privacy.html"
            | "/terms.html"
            | "/watch/config"
    ) || path.starts_with("/workouts/")
        || path.starts_with("/sessions/")
}

/// Display host of a referrer, dropping our own domain and empty referrers.
fn referrer_host(referrer: &str) -> Option<String> {
    let r = referrer.trim();
    if r.is_empty() {
        return None;
    }
    let after_scheme = r.split("://").nth(1).unwrap_or(r);
    let host = after_scheme.split('/').next().unwrap_or(after_scheme);
    let host = host.split(':').next().unwrap_or(host);
    let host = host.strip_prefix("www.").unwrap_or(host).to_lowercase();
    if host.is_empty() || host.contains("pebblestrength.app") || host == "localhost" {
        return None;
    }
    Some(host)
}

pub struct ViewStats {
    pub total: u64,
    pub bot_hits: u64,
    pub by_day: Vec<DayCount>,
    pub top_pages: Vec<KeyCount>,
    pub top_referrers: Vec<KeyCount>,
}

pub fn read_views(dir: &Path) -> ViewStats {
    use std::collections::BTreeMap;
    let mut by_day: BTreeMap<String, u64> = BTreeMap::new();
    let mut pages: BTreeMap<String, u64> = BTreeMap::new();
    let mut refs: BTreeMap<String, u64> = BTreeMap::new();
    let mut total = 0u64;
    let mut bot_hits = 0u64;

    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".jsonl") else { continue };
            let Some((kind, day)) = stem.split_once('-') else { continue };
            if kind != "views" {
                continue;
            }
            let day = day.to_string();
            let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
            for line in text.lines() {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
                let path = v.get("path").and_then(|p| p.as_str()).unwrap_or("");
                if !is_real_page(path) {
                    bot_hits += 1;
                    continue;
                }
                total += 1;
                *by_day.entry(day.clone()).or_default() += 1;
                *pages.entry(path.to_string()).or_default() += 1;
                if let Some(host) = v.get("ref").and_then(|r| r.as_str()).and_then(referrer_host) {
                    *refs.entry(host).or_default() += 1;
                }
            }
        }
    }

    let top = |m: BTreeMap<String, u64>, k: usize| -> Vec<KeyCount> {
        let mut v: Vec<KeyCount> = m.into_iter().map(|(key, n)| KeyCount { key, n }).collect();
        v.sort_by(|a, b| b.n.cmp(&a.n));
        v.truncate(k);
        v
    };
    ViewStats {
        total,
        bot_hits,
        by_day: by_day.into_iter().map(|(day, n)| DayCount { day, n }).collect(),
        top_pages: top(pages, 12),
        top_referrers: top(refs, 12),
    }
}

/// The four DB entity counts with week-over-week trends.
pub fn db_metrics(conn: &mut SqliteConnection) -> QueryResult<Vec<Metric>> {
    use crate::schema::{devices, sessions, users, workouts};
    let now = Utc::now().naive_utc();
    let w1 = now - Duration::days(7);
    let w2 = now - Duration::days(14);

    macro_rules! metric {
        ($label:expr, $tbl:ident, $created:expr) => {{
            let total: i64 = $tbl::table.count().get_result(conn)?;
            let new_7d: i64 = $tbl::table.filter($created.ge(w1)).count().get_result(conn)?;
            let prev_7d: i64 = $tbl::table
                .filter($created.ge(w2))
                .filter($created.lt(w1))
                .count()
                .get_result(conn)?;
            Metric { label: $label.into(), total, new_7d, prev_7d }
        }};
    }

    Ok(vec![
        metric!("Users", users, users::created_at),
        metric!("Devices", devices, devices::created_at),
        metric!("Workouts", workouts, workouts::created_at),
        metric!("Sessions", sessions, sessions::created_at),
    ])
}
