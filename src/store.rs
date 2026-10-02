//! On-disk state: the login session (tokens only) and a cache of the last
//! downloaded data, so the UI can render instantly on startup.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::auth::{School, Session};

/// Raw API responses, kept as JSON so the cache survives model changes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RawData {
    pub student: Value,
    pub grades: Value,
    pub absences: Value,
    pub exams: Value,
    pub homework: Value,
    pub notes: Value,
    /// Keyed by the Monday of the week, `YYYY-MM-DD`.
    pub timetable: BTreeMap<String, Value>,
    pub fetched_at: Option<DateTime<Local>>,
}

fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("kreta-tui")
}

fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".")).join("kreta-tui")
}

/// Write with 0600 permissions: these files hold tokens and personal data.
fn write_private(path: &PathBuf, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
    f.write_all(bytes)?;
    fs::rename(&tmp, path).with_context(|| format!("nem sikerült menteni: {}", path.display()))
}

pub fn load_session() -> Option<Session> {
    serde_json::from_slice(&fs::read(data_dir().join("session.json")).ok()?).ok()
}

pub fn save_session(s: &Session) -> Result<()> {
    write_private(&data_dir().join("session.json"), &serde_json::to_vec_pretty(s)?)
}

/// Last school used for a password login, to prefill the form. Kept on logout.
pub fn load_school() -> Option<School> {
    serde_json::from_slice(&fs::read(data_dir().join("school.json")).ok()?).ok()
}

pub fn save_school(s: &School) -> Result<()> {
    write_private(&data_dir().join("school.json"), &serde_json::to_vec_pretty(s)?)
}

pub fn load_cache() -> RawData {
    fs::read(cache_dir().join("data.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save_cache(data: &RawData) -> Result<()> {
    write_private(&cache_dir().join("data.json"), &serde_json::to_vec(data)?)
}

/// Keep the page a failed headless login ended on, for troubleshooting.
pub fn save_login_debug(html: &str) {
    let _ = write_private(&cache_dir().join("login-debug.html"), html.as_bytes());
}

pub fn clear() {
    let _ = fs::remove_file(data_dir().join("session.json"));
    let _ = fs::remove_file(cache_dir().join("data.json"));
}
