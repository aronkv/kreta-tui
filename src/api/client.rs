use std::sync::Arc;

use anyhow::{Result, bail};
use chrono::{DateTime, Local, NaiveDate, Utc};
use serde_json::Value;
use tokio::sync::Mutex;

use super::auth::{self, Session};
use crate::store;

const API_UA: &str = "hu.ekreta.student/1.0.5/Android/0/0";

/// Authenticated client for the student ("ellenőrző") API.
/// Refreshes the access token transparently and persists the new session.
#[derive(Clone)]
pub struct KretaClient {
    http: reqwest::Client,
    session: Arc<Mutex<Session>>,
}

impl KretaClient {
    pub fn new(session: Session) -> Self {
        Self {
            http: reqwest::Client::builder().user_agent(API_UA).build().expect("http client"),
            session: Arc::new(Mutex::new(session)),
        }
    }

    pub async fn session(&self) -> Session {
        self.session.lock().await.clone()
    }

    async fn token(&self, force_refresh: bool) -> Result<(String, String)> {
        let mut s = self.session.lock().await;
        if force_refresh || s.is_expired() {
            *s = auth::refresh(&s).await?;
            store::save_session(&s)?;
        }
        Ok((s.access_token.clone(), s.institute_code.clone()))
    }

    /// GET `https://{institute}.e-kreta.hu/ellenorzo/V3/Sajat/{path}`.
    pub async fn get(&self, path: &str) -> Result<Value> {
        for attempt in 0..2 {
            let (token, inst) = self.token(attempt > 0).await?;
            let url = format!("https://{inst}.e-kreta.hu/ellenorzo/V3/Sajat/{path}");
            let res = self.http.get(&url).bearer_auth(token).send().await?;
            match res.status() {
                s if s.is_success() => return Ok(res.json().await?),
                reqwest::StatusCode::UNAUTHORIZED if attempt == 0 => continue,
                s => bail!("{path}: HTTP {s}"),
            }
        }
        unreachable!()
    }

    pub async fn student(&self) -> Result<Value> {
        self.get("TanuloAdatlap").await
    }
    pub async fn grades(&self) -> Result<Value> {
        self.get("Ertekelesek").await
    }
    pub async fn absences(&self) -> Result<Value> {
        self.get("Mulasztasok").await
    }
    pub async fn exams(&self) -> Result<Value> {
        self.get("BejelentettSzamonkeresek").await
    }
    pub async fn notes(&self) -> Result<Value> {
        self.get("Feljegyzesek").await
    }
    pub async fn homework(&self, from: NaiveDate) -> Result<Value> {
        self.get(&format!("HaziFeladatok?datumTol={}", from.format("%Y-%m-%d"))).await
    }
    /// Lessons between two local dates (inclusive start, exclusive end).
    pub async fn timetable(&self, from: NaiveDate, to: NaiveDate) -> Result<Value> {
        let iso = |d: NaiveDate| {
            let local = d.and_hms_opt(0, 0, 0).unwrap().and_local_timezone(Local).unwrap();
            local.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
        };
        self.get(&format!("OrarendElemek?datumTol={}&datumIg={}", iso(from), iso(to))).await
    }
}

/// Lenient accessors for the KRÉTA JSON (fields are frequently null or missing).
pub trait JsonExt {
    fn str_at(&self, key: &str) -> String;
    fn path_str(&self, path: &[&str]) -> String;
    fn f64_at(&self, key: &str) -> Option<f64>;
    fn date_at(&self, key: &str) -> Option<DateTime<Local>>;
}

impl JsonExt for Value {
    fn str_at(&self, key: &str) -> String {
        match self.get(key) {
            Some(Value::String(s)) => s.trim().to_owned(),
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::Bool(b)) => b.to_string(),
            _ => String::new(),
        }
    }
    fn path_str(&self, path: &[&str]) -> String {
        let (last, init) = path.split_last().expect("non-empty path");
        init.iter().try_fold(self, |v, k| v.get(k)).map(|v| v.str_at(last)).unwrap_or_default()
    }
    fn f64_at(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Value::as_f64)
    }
    fn date_at(&self, key: &str) -> Option<DateTime<Local>> {
        parse_date(self.get(key)?.as_str()?)
    }
}

pub fn parse_date(s: &str) -> Option<DateTime<Local>> {
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Some(d.with_timezone(&Local));
    }
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok().map(|n| n.and_utc().with_timezone(&Local))
}
