//! Browser login that captures the OAuth code without copy-pasting.
//!
//! Preferred: launch a Chromium-based browser with a throwaway profile and the
//! DevTools HTTP endpoint enabled, open the authorize URL in it, and poll the
//! open tabs until one lands on `REDIRECT_URI`. Fallback (Firefox etc.): open
//! the default browser and watch the clipboard for the redirect URL.
//!
//! `KRETA_BROWSER` overrides the browser binary, `KRETA_BROWSER_FLAGS` adds flags.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use tokio::process::{Child, Command};

use super::auth::{REDIRECT_URI, code_from_redirect};

const CHROMIUM_NAMES: [&str; 13] = [
    "brave-origin",
    "brave",
    "brave-browser",
    "chromium",
    "chromium-browser",
    "google-chrome-stable",
    "google-chrome",
    "vivaldi-stable",
    "vivaldi",
    "microsoft-edge-stable",
    "thorium-browser",
    "helium-browser",
    "opera",
];
const POLL: Duration = Duration::from_millis(400);
const TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// How the login window was opened; shown in the UI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Method {
    /// Dedicated Chromium window, code captured automatically.
    Chromium,
    /// Default browser + clipboard watching (+ manual paste).
    Clipboard,
}

fn on_path(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return p.is_file().then_some(p);
    }
    std::env::var_os("PATH")?.to_str()?.split(':').map(|d| Path::new(d).join(name)).find(|p| p.is_file())
}

fn looks_chromium(name: &str) -> bool {
    ["chrom", "brave", "vivaldi", "edge", "opera", "thorium", "helium"].iter().any(|k| name.contains(k))
}

/// `$KRETA_BROWSER`, else the default browser if it is Chromium-based, else any known one.
fn find_chromium() -> Option<PathBuf> {
    if let Ok(b) = std::env::var("KRETA_BROWSER") {
        return on_path(&b);
    }
    if let Some(exec) = default_browser_exec()
        && looks_chromium(&exec)
        && let Some(p) = on_path(&exec)
    {
        return Some(p);
    }
    CHROMIUM_NAMES.iter().find_map(|n| on_path(n))
}

fn default_browser_exec() -> Option<String> {
    let out = std::process::Command::new("xdg-settings").args(["get", "default-web-browser"]).output().ok()?;
    let desktop = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    let dirs = [dirs::data_dir(), Some(PathBuf::from("/usr/local/share")), Some(PathBuf::from("/usr/share"))];
    let content =
        dirs.into_iter().flatten().find_map(|d| std::fs::read_to_string(d.join("applications").join(&desktop)).ok())?;
    let exec = content.lines().find_map(|l| l.strip_prefix("Exec="))?;
    Some(exec.split_whitespace().next()?.to_owned())
}

#[derive(Deserialize)]
struct Target {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    url: String,
}

struct ChromiumSession {
    child: Child,
    profile: PathBuf,
}

impl Drop for ChromiumSession {
    /// Browser launchers are often wrapper scripts, so terminate the whole
    /// process group, then remove the throwaway profile once it has exited.
    fn drop(&mut self) {
        if let Some(pid) = self.child.id() {
            let _ = std::process::Command::new("kill").args(["-TERM", "--", &format!("-{pid}")]).status();
        }
        let _ = self.child.start_kill();
        let profile = self.profile.clone();
        std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(Duration::from_millis(250));
                if std::fs::remove_dir_all(&profile).is_ok() && !profile.exists() {
                    break;
                }
            }
        });
    }
}

async fn chromium_login(browser: &Path, url: &str, cancel: &AtomicBool) -> Result<String> {
    // Sweep profiles left behind by earlier runs that exited mid-cleanup.
    if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
        for e in entries.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("kreta-login-")) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    let profile = std::env::temp_dir().join(format!("kreta-login-{}", std::process::id()));
    std::fs::create_dir_all(&profile)?;
    let child = Command::new(browser)
        .arg(format!("--user-data-dir={}", profile.display()))
        .args(["--remote-debugging-port=0", "--no-first-run", "--no-default-browser-check", "--new-window"])
        .arg("--window-size=520,760")
        .args(std::env::var("KRETA_BROWSER_FLAGS").unwrap_or_default().split_whitespace())
        .arg("about:blank")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let mut session = ChromiumSession { child, profile };

    // The browser writes its DevTools port into the profile once it is up.
    let port_file = session.profile.join("DevToolsActivePort");
    let mut port = None;
    for _ in 0..75 {
        if let Some(p) = std::fs::read_to_string(&port_file).ok().and_then(|s| s.lines().next()?.parse::<u16>().ok()) {
            port = Some(p);
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let port = port.ok_or_else(|| anyhow!("a böngésző nem indult el"))?;
    let base = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(5)).build()?;

    let opened: Target = http.put(format!("{base}/json/new?{}", urlencoding::encode(url))).send().await?.json().await?;

    let started = std::time::Instant::now();
    let mut closed = std::collections::HashSet::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            bail!("megszakítva");
        }
        if started.elapsed() > TIMEOUT {
            bail!("lejárt az idő");
        }
        if session.child.try_wait()?.is_some() {
            bail!("a böngészőablak bezárult");
        }
        if let Ok(res) = http.get(format!("{base}/json/list")).send().await
            && let Ok(targets) = res.json::<Vec<Target>>().await
        {
            if let Some(code) =
                targets.iter().filter(|t| t.url.starts_with(REDIRECT_URI)).find_map(|t| code_from_redirect(&t.url))
            {
                return Ok(code);
            }
            // Startup/welcome windows can appear well after launch (only in a
            // visible browser), so keep closing internal pages for the whole login.
            for t in targets.iter().filter(|t| t.kind == "page" && t.id != opened.id && is_internal(&t.url)) {
                if closed.insert(t.id.clone()) {
                    let _ = http.get(format!("{base}/json/close/{}", t.id)).send().await;
                }
            }
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Browser-internal pages (startup, new tab, welcome) as opposed to real sites.
fn is_internal(url: &str) -> bool {
    url.is_empty() || url.starts_with("about:") || !url.starts_with("http") && url.contains("://")
}

fn read_clipboard() -> Option<String> {
    let attempts: [&[&str]; 3] = [&["wl-paste", "-n"], &["xclip", "-o", "-selection", "clipboard"], &["xsel", "-ob"]];
    attempts.iter().find_map(|cmd| {
        let out = std::process::Command::new(cmd[0]).args(&cmd[1..]).stderr(Stdio::null()).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    })
}

async fn clipboard_login(url: &str, cancel: &AtomicBool) -> Result<String> {
    let _ = std::process::Command::new("xdg-open").arg(url).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    let initial = read_clipboard();
    let started = std::time::Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            bail!("megszakítva");
        }
        if started.elapsed() > TIMEOUT {
            bail!("lejárt az idő");
        }
        let clip = read_clipboard();
        if clip != initial
            && let Some(code) =
                clip.as_deref().map(str::trim).filter(|c| c.starts_with(REDIRECT_URI)).and_then(code_from_redirect)
        {
            return Ok(code);
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Which method `login` will use, decided up front so the UI can explain it.
pub fn method() -> Method {
    if find_chromium().is_some() { Method::Chromium } else { Method::Clipboard }
}

/// Run the browser login until a code arrives or `cancel` is set.
pub async fn login(url: String, cancel: Arc<AtomicBool>) -> Result<String> {
    match find_chromium() {
        Some(browser) => chromium_login(&browser, &url, &cancel).await,
        None => clipboard_login(&url, &cancel).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a Chromium-based browser: `cargo test browser -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn captures_code_from_redirect() {
        unsafe { std::env::set_var("KRETA_BROWSER_FLAGS", "--headless=new") };
        let browser = find_chromium().expect("no Chromium-based browser found");
        let url = format!("{REDIRECT_URI}?code=TEST%2BCODE&state=x");
        let code = chromium_login(&browser, &url, &AtomicBool::new(false)).await.unwrap();
        assert_eq!(code, "TEST+CODE");
    }
}
