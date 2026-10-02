//! e-KRÉTA IDP login: OAuth2 authorization code flow with PKCE.
//!
//! The official app opens the IDP login page in a web view and catches the
//! redirect to `REDIRECT_URI`. We do the same headlessly: fetch the login form,
//! post the credentials, and stop following redirects once the IDP sends us to
//! the redirect URI with the `code` in its query string.

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const IDP: &str = "https://idp.e-kreta.hu";
pub const CLIENT_ID: &str = "kreta-ellenorzo-student-mobile-ios";
pub const REDIRECT_URI: &str = "https://mobil.e-kreta.hu/ellenorzo-student/prod/oauthredirect";
const SCOPE: &str = "openid email offline_access kreta-ellenorzo-webapi.public \
    kreta-eugyintezes-webapi.public kreta-fileservice-webapi.public \
    kreta-mobile-global-webapi.public kreta-dkt-webapi.public kreta-ier-webapi.public";
pub const BROWSER_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";
const TOKEN_UA: &str = "eKretaStudent/264745 CFNetwork/1494.0.7 Darwin/23.4.0";

/// A school as returned by the IDP's institute search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct School {
    /// Institute code used for login and the API subdomain, e.g. `klik035046001`.
    pub code: String,
    pub name: String,
    /// OM identifier, sometimes with a site suffix (`203058/002`).
    pub om: String,
}

/// Search schools by OM identifier, name fragment or institute code (min. 3 chars),
/// using the same endpoint as the IDP login page's autocomplete.
pub async fn search_schools(query: &str) -> Result<Vec<School>> {
    let html = reqwest::Client::builder()
        .user_agent(BROWSER_UA)
        .build()?
        .get(format!("{IDP}/logininstituteselector?searchValue={}", urlencoding::encode(query)))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(parse_schools(&html))
}

fn parse_schools(html: &str) -> Vec<School> {
    let re = Regex::new(r#"data-val="([^"]*)"[^>]*>([^<]*)</a>"#).expect("valid regex");
    re.captures_iter(html)
        .filter_map(|c| {
            let code = html_unescape(&c[1]);
            if code.is_empty() {
                return None;
            }
            let text = html_unescape(c[2].trim());
            // "Name (code - om)"
            let (name, om) = match text.rfind(" (") {
                Some(i) if text.ends_with(')') => {
                    let inner = &text[i + 2..text.len() - 1];
                    (text[..i].to_owned(), inner.rsplit(" - ").next().unwrap_or_default().to_owned())
                }
                _ => (text.clone(), String::new()),
            };
            Some(School { code, name, om })
        })
        .collect()
}

/// Persisted login state. Only tokens are stored, never the password.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub institute_code: String,
    pub username: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
}

impl Session {
    pub fn is_expired(&self) -> bool {
        Utc::now() + Duration::seconds(30) >= self.expires_at
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

/// PKCE verifier/challenge pair plus the authorize URL built from it.
pub struct Pkce {
    pub verifier: String,
    pub authorize_url: String,
}

impl Pkce {
    pub fn new() -> Self {
        let verifier = random_token(32);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let authorize_url = format!(
            "{IDP}/connect/authorize?prompt=login&nonce={}&response_type=code\
             &code_challenge_method=S256&scope={}&code_challenge={challenge}\
             &redirect_uri={}&client_id={CLIENT_ID}&state={}",
            random_token(32),
            urlencoding::encode(SCOPE),
            urlencoding::encode(REDIRECT_URI),
            random_token(16),
        );
        Self { verifier, authorize_url }
    }
}

fn random_token(len: usize) -> String {
    let mut buf = vec![0u8; len];
    rand::rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

/// Client that stops at the OAuth redirect instead of trying to load it.
fn login_client() -> Result<reqwest::Client> {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.url().as_str().starts_with(REDIRECT_URI) || attempt.previous().len() > 10 {
            attempt.stop()
        } else {
            attempt.follow()
        }
    });
    Ok(reqwest::Client::builder().cookie_store(true).redirect(policy).user_agent(BROWSER_UA).build()?)
}

fn html_attr_value(html: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(r#"name="{}"[^>]*?value="([^"]*)""#, regex::escape(name))).ok()?;
    re.captures(html).map(|c| html_unescape(&c[1]))
}

fn html_unescape(s: &str) -> String {
    let re = Regex::new(r"&#(x[0-9a-fA-F]+|[0-9]+);").expect("valid regex");
    let decoded = re.replace_all(s, |c: &regex::Captures| {
        let n = &c[1];
        let code = match n.strip_prefix('x') {
            Some(hex) => u32::from_str_radix(hex, 16).ok(),
            None => n.parse().ok(),
        };
        code.and_then(char::from_u32).map(String::from).unwrap_or_else(|| c[0].to_owned())
    });
    decoded.replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// Pull a human-readable error out of a failed login page.
fn login_error_text(html: &str) -> Option<String> {
    let re = Regex::new(r#"(?s)class="[^"]*validation-summary-errors[^"]*"[^>]*>(.*?)</div>"#).ok()?;
    let block = re.captures(html)?.get(1)?.as_str().to_owned();
    let tags = Regex::new(r"<[^>]+>").ok()?;
    let text = tags.replace_all(&block, " ");
    let text = html_unescape(text.split_whitespace().collect::<Vec<_>>().join(" ").trim());
    (!text.is_empty()).then_some(text)
}

/// Extract `code` from the redirect URL (also accepts a URL pasted by the user).
pub fn code_from_redirect(url: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == "code").then(|| urlencoding::decode(v).map(|c| c.into_owned()).ok())?
    })
}

/// Headless login with username/password.
pub async fn login(institute_code: &str, username: &str, password: &str) -> Result<Session> {
    let pkce = Pkce::new();
    let client = login_client()?;

    let page = client.get(&pkce.authorize_url).send().await.context("nem érhető el az idp.e-kreta.hu")?;
    let login_url = page.url().clone();
    let html = page.text().await?;

    let token = html_attr_value(&html, "__RequestVerificationToken")
        .ok_or_else(|| anyhow!("nem található a bejelentkezési űrlap (változott a KRÉTA oldala?)"))?;
    let return_url = html_attr_value(&html, "ReturnUrl").unwrap_or_default();

    let form = [
        ("ReturnUrl", return_url.as_str()),
        ("IsTemporaryLogin", "False"),
        ("UserName", username),
        ("Password", password),
        ("InstituteCode", institute_code),
        ("loginType", "InstituteLogin"),
        ("ClientId", ""),
        ("__RequestVerificationToken", token.as_str()),
    ];
    let res = client
        .post(format!("{IDP}/account/login"))
        .header("Referer", login_url.as_str())
        .header("Origin", IDP)
        .form(&form)
        .send()
        .await?;

    let location = res.headers().get("location").and_then(|l| l.to_str().ok()).map(str::to_owned);
    if let Some(code) = location.as_deref().and_then(code_from_redirect) {
        return exchange_code(&code, &pkce.verifier).await;
    }

    let body = res.text().await.unwrap_or_default();
    if let Some(msg) = login_error_text(&body) {
        bail!("{msg}");
    }
    if body.contains("g-recaptcha") || body.contains("data-sitekey") {
        bail!("a KRÉTA captchát kér – használd a böngészős belépést (Ctrl+B)");
    }
    bail!("sikertelen bejelentkezés – ellenőrizd az adatokat, vagy próbáld böngészővel (Ctrl+B)")
}

async fn token_request(form: &[(&str, &str)]) -> Result<TokenResponse> {
    let res = reqwest::Client::new()
        .post(format!("{IDP}/connect/token"))
        .header("User-Agent", TOKEN_UA)
        .header("Accept", "*/*")
        .form(form)
        .send()
        .await?;
    let status = res.status();
    let text = res.text().await?;
    serde_json::from_str(&text).with_context(|| {
        format!("váratlan token-válasz (HTTP {status}): {}", text.chars().take(200).collect::<String>())
    })
}

fn session_from_token(t: TokenResponse) -> Result<Session> {
    if let Some(err) = t.error {
        bail!("{err}: {}", t.error_description.unwrap_or_default());
    }
    let access_token = t.access_token.ok_or_else(|| anyhow!("nincs access_token a válaszban"))?;
    let claims = jwt_claims(&access_token)?;
    let claim = |k: &str| claims.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_owned();
    Ok(Session {
        institute_code: claim("kreta:institute_code"),
        username: claim("kreta:user_name"),
        refresh_token: t.refresh_token.unwrap_or_default(),
        expires_at: Utc::now() + Duration::seconds(t.expires_in.unwrap_or(1800)),
        access_token,
    })
}

/// Trade an authorization code for tokens.
pub async fn exchange_code(code: &str, verifier: &str) -> Result<Session> {
    let t = token_request(&[
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", REDIRECT_URI),
        ("client_id", CLIENT_ID),
        ("grant_type", "authorization_code"),
    ])
    .await?;
    session_from_token(t)
}

pub async fn refresh(session: &Session) -> Result<Session> {
    let t = token_request(&[
        ("refresh_token", session.refresh_token.as_str()),
        ("institute_code", session.institute_code.as_str()),
        ("client_id", CLIENT_ID),
        ("grant_type", "refresh_token"),
        ("refresh_user_data", "false"),
    ])
    .await?;
    let mut new = session_from_token(t)?;
    // The refresh response may omit claims we already know.
    if new.institute_code.is_empty() {
        new.institute_code = session.institute_code.clone();
    }
    if new.username.is_empty() {
        new.username = session.username.clone();
    }
    if new.refresh_token.is_empty() {
        new.refresh_token = session.refresh_token.clone();
    }
    Ok(new)
}

/// Best-effort token revocation on logout.
pub async fn revoke(session: &Session) {
    let _ = reqwest::Client::new()
        .post(format!("{IDP}/connect/revocation"))
        .header("User-Agent", TOKEN_UA)
        .form(&[
            ("token", session.refresh_token.as_str()),
            ("token_type_hint", "refresh_token"),
            ("client_id", CLIENT_ID),
        ])
        .send()
        .await;
}

fn jwt_claims(jwt: &str) -> Result<serde_json::Map<String, serde_json::Value>> {
    let payload = jwt.split('.').nth(1).ok_or_else(|| anyhow!("hibás JWT"))?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('='))?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_code_from_redirect() {
        let url = format!("{REDIRECT_URI}?code=AB%2BC&scope=openid&state=x");
        assert_eq!(code_from_redirect(&url).as_deref(), Some("AB+C"));
        assert_eq!(code_from_redirect(REDIRECT_URI), None);
    }

    #[test]
    fn extracts_form_fields() {
        let html = r#"<input type="hidden" id="ReturnUrl" name="ReturnUrl" value="/connect?a=1&amp;b=2" />
            <input name="__RequestVerificationToken" type="hidden" value="tok123" />"#;
        assert_eq!(html_attr_value(html, "ReturnUrl").as_deref(), Some("/connect?a=1&b=2"));
        assert_eq!(html_attr_value(html, "__RequestVerificationToken").as_deref(), Some("tok123"));
    }

    #[test]
    fn parses_school_search() {
        let html = r##"<li><a href="#" class="dropdown-item" data-val="klik035046001">Zugl&#xF3;i Iskola (klik035046001 - 035046)</a></li>
            <li><a href="#" class="dropdown-item" data-val="bmszc-blathy">Bl&#xE1;thy (bmszc-blathy - 203058/002)</a></li>
            <li><a href="#" class="dropdown-item disabled" data-val="">Nincs tal&#xE1;lat</a></li>"##;
        let s = parse_schools(html);
        assert_eq!(s.len(), 2);
        assert_eq!(
            (s[0].code.as_str(), s[0].name.as_str(), s[0].om.as_str()),
            ("klik035046001", "Zuglói Iskola", "035046")
        );
        assert_eq!(s[1].om, "203058/002");
    }

    #[test]
    fn extracts_login_error() {
        let html =
            r#"<div class="validation-summary-errors text-danger"><ul><li>Hib&#xE1;s jelsz&#xF3;!</li></ul></div>"#;
        assert_eq!(login_error_text(html).as_deref(), Some("Hibás jelszó!"));
    }
}

#[cfg(test)]
mod live {
    /// Hits the real IDP with made-up credentials: `cargo test live -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn rejects_bad_credentials() {
        let err = super::login("klik000000001", "nemletezo-felhasznalo", "rossz-jelszo").await.unwrap_err();
        println!("login error: {err:#}");
    }
}
