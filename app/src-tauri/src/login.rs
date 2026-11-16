//! Connexion France Student par e-mail + mot de passe (Auth.js + WHMCS OAuth).
//!
//! Le mot de passe n'est jamais enregistré : il sert à ouvrir une session web, dont seul le
//! cookie `next-auth.session-token` (longue durée) est conservé dans `session.json`.
//! Ce cookie permet de redemander automatiquement un jeton d'accès court (`/api/auth/api-token`)
//! chaque fois que le précédent expire.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use regex::Regex;
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, COOKIE, LOCATION, SET_COOKIE,
    USER_AGENT,
};
use reqwest::{redirect::Policy, Client, Method, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const ORIGIN_URL: &str = "https://ia.francestudent.org";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:157.0) Gecko/20100101 Firefox/157.0";
const PLATFORM: &str = "web_fs_ai";
const CLIENT_VERSION: &str = "4005601";
const SESSION_COOKIE: &str = "next-auth.session-token";
const DEVICE_COOKIE: &str = "francestudent-device-id";
const HTML_ACCEPT: &str = "text/html,application/xhtml+xml,application/json;q=0.9,*/*;q=0.8";
/// Marge avant expiration à partir de laquelle le jeton d'accès est renouvelé.
const REFRESH_MARGIN: u64 = 60;
/// Nombre maximal de pages HTML (hors redirections) parcourues pendant la connexion.
const MAX_PAGES: usize = 6;
/// Nombre maximal de requêtes (redirections comprises) pendant la connexion.
const MAX_REQUESTS: usize = 25;

const ERR_BLOCKED: &str = "France Student demande une vérification anti-robot (Cloudflare) : la connexion par mot de passe est bloquée pour le moment. Utilise un jeton de connexion (Réglages > Compte).";
const ERR_CAPTCHA: &str = "La page de connexion demande un captcha, impossible de le résoudre ici. Utilise un jeton de connexion (Réglages > Compte).";
const ERR_2FA: &str = "Ce compte demande une double authentification, non prise en charge ici. Utilise un jeton de connexion (Réglages > Compte).";

/// Corps d'une balise HTML : tout sauf `>` hors guillemets.
const TB: &str = r#"(?:"[^"]*"|'[^']*'|[^'">])*"#;

macro_rules! re {
    ($p:expr) => {{
        static R: OnceLock<Regex> = OnceLock::new();
        R.get_or_init(|| Regex::new($p).expect("regex valide"))
    }};
}

// ---------- Session persistée et cache du jeton d'accès ----------

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Session {
    session_token: String,
    device_id: String,
}

/// Jeton d'accès (JWT court) et son expiration, en secondes Unix.
static TOKEN: Mutex<Option<(String, u64)>> = Mutex::new(None);
/// Empêche plusieurs renouvellements simultanés (et les écritures concurrentes de la session).
static REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn session_path(dir: &Path) -> PathBuf {
    dir.join("session.json")
}

fn load_session(dir: &Path) -> Option<Session> {
    let raw = std::fs::read_to_string(session_path(dir)).ok()?;
    let mut s: Session = serde_json::from_str(&raw).ok()?;
    s.session_token = s.session_token.trim().to_string();
    s.device_id = s.device_id.trim().to_string();
    if s.session_token.is_empty() || s.device_id.is_empty() {
        return None;
    }
    Some(s)
}

fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(path)?;
    f.write_all(data)?;
    f.sync_all()
}

fn save_session(dir: &Path, s: &Session) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
    let tmp = dir.join("session.json.tmp");
    write_private(&tmp, &json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, session_path(dir)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

fn new_device_id() -> Result<String, String> {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).map_err(|e| e.to_string())?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    ))
}

fn cached_token() -> Option<String> {
    let g = TOKEN.lock().ok()?;
    match g.as_ref() {
        Some((t, exp)) if now_secs() + REFRESH_MARGIN < *exp => Some(t.clone()),
        _ => None,
    }
}

fn set_cached_token(value: Option<(String, u64)>) {
    if let Ok(mut g) = TOKEN.lock() {
        *g = value;
    }
}

/// Une session de connexion est-elle enregistrée sur ce poste ?
pub fn has_session(dir: &Path) -> bool {
    load_session(dir).is_some()
}

/// Oublie la session (déconnexion) : supprime le fichier et vide le cache.
pub fn clear_session(dir: &Path) -> Result<(), String> {
    set_cached_token(None);
    match std::fs::remove_file(session_path(dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Impossible d'effacer la session : {e}")),
    }
}

/// Jeton d'accès valide pour l'API, renouvelé automatiquement avec la session enregistrée.
/// `None` si aucune session n'existe ou si France Student la refuse.
pub async fn access_token(dir: &Path) -> Option<String> {
    if let Some(t) = cached_token() {
        return Some(t);
    }
    let _guard = REFRESH.lock().await;
    if let Some(t) = cached_token() {
        return Some(t);
    }
    let session = load_session(dir)?;
    let http = http_client().ok()?;
    let tok = fetch_api_token(&http, &session).await.ok()?;
    if let Some(cookie) = &tok.new_cookie {
        let renewed = Session {
            session_token: cookie.clone(),
            device_id: session.device_id.clone(),
        };
        // Ne pas masquer une erreur d'écriture : le cookie renouvelé serait perdu.
        save_session(dir, &renewed).ok()?;
    }
    set_cached_token(Some((tok.access.clone(), tok.exp)));
    Some(tok.access)
}

/// Ouvre une session avec l'e-mail et le mot de passe, vérifie qu'un jeton d'accès peut en être
/// obtenu, puis enregistre uniquement la session (jamais le mot de passe).
pub async fn login_with_password(dir: &Path, email: &str, password: &str) -> Result<(), String> {
    let email = email.trim();
    if email.is_empty() || password.is_empty() {
        return Err("L'e-mail et le mot de passe sont requis.".to_string());
    }
    if email.len() > 320 || password.len() > 1024 {
        return Err("L'e-mail ou le mot de passe est trop long.".to_string());
    }
    let device_id = match load_session(dir) {
        Some(s) => s.device_id,
        None => new_device_id()?,
    };
    let http = http_client()?;
    let cookie = web_login(&http, &device_id, email, password).await?;
    let mut session = Session {
        session_token: cookie,
        device_id,
    };
    let tok = fetch_api_token(&http, &session).await.map_err(|e| {
        format!("Connexion réussie, mais le jeton d'accès n'a pas pu être obtenu : {e}")
    })?;
    if let Some(c) = &tok.new_cookie {
        session.session_token = c.clone();
    }
    let _guard = REFRESH.lock().await;
    save_session(dir, &session)
        .map_err(|e| format!("Impossible d'enregistrer la session : {e}"))?;
    set_cached_token(Some((tok.access, tok.exp)));
    Ok(())
}

// ---------- HTTP ----------

fn http_client() -> Result<Client, String> {
    Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.without_url().to_string())
}

fn net_err(e: reqwest::Error) -> String {
    format!(
        "Connexion à France Student impossible : {}",
        e.without_url()
    )
}

fn origin_url() -> Url {
    Url::parse(ORIGIN_URL).expect("origine valide")
}

/// Cookies par hôte (le CLI de référence fait pareil : ni domaine ni chemin).
#[derive(Default)]
struct Jar(BTreeMap<String, BTreeMap<String, String>>);

impl Jar {
    fn set(&mut self, host: &str, name: &str, value: &str) {
        self.0
            .entry(host.to_string())
            .or_default()
            .insert(name.to_string(), value.to_string());
    }

    fn get(&self, host: &str, name: &str) -> Option<&String> {
        self.0.get(host)?.get(name)
    }

    fn header_for(&self, host: &str) -> Option<String> {
        let m = self.0.get(host)?;
        if m.is_empty() {
            return None;
        }
        Some(
            m.iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }

    fn absorb(&mut self, host: &str, headers: &HeaderMap) {
        for v in headers.get_all(SET_COOKIE) {
            let Ok(raw) = v.to_str() else { continue };
            match parse_set_cookie(raw) {
                Some((name, Some(value))) => self.set(host, &name, &value),
                Some((name, None)) => {
                    if let Some(m) = self.0.get_mut(host) {
                        m.remove(&name);
                    }
                }
                None => {}
            }
        }
    }
}

/// `(nom, Some(valeur))` pour un cookie posé, `(nom, None)` pour un cookie supprimé.
fn parse_set_cookie(raw: &str) -> Option<(String, Option<String>)> {
    let mut parts = raw.split(';');
    let (name, value) = parts.next()?.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let value = value.trim();
    let mut expired = value.is_empty();
    for attr in parts {
        let a = attr.trim();
        let (k, v) = a.split_once('=').unwrap_or((a, ""));
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        if k == "max-age" && v.parse::<i64>().is_ok_and(|n| n <= 0) {
            expired = true;
        }
        if k == "expires" && v.contains("1970") {
            expired = true;
        }
    }
    let value = if expired {
        None
    } else {
        Some(value.to_string())
    };
    Some((name.to_string(), value))
}

struct Resp {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
    url: Url,
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

/// Page de défi Cloudflare (ou blocage équivalent) au lieu de la vraie réponse.
fn is_blocked(r: &Resp) -> bool {
    let challenge = r
        .headers
        .get("cf-mitigated")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("challenge"));
    let suspicious = matches!(r.status.as_u16(), 403 | 429 | 503);
    challenge
        || (suspicious
            && re!(r"(?i)just a moment|cf-chl|attention required|cloudflare").is_match(&r.body))
}

/// Envoie une requête avec le jar de cookies, sans suivre les redirections.
async fn send(
    c: &Client,
    jar: &mut Jar,
    device_id: &str,
    method: Method,
    url: &Url,
    extra: &[(&str, &str)],
    body: Option<String>,
) -> Result<Resp, String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Adresse de redirection non prise en charge.".to_string());
    }
    let host = url.host_str().unwrap_or_default().to_string();

    let mut h = HeaderMap::new();
    h.insert(USER_AGENT, HeaderValue::from_static(UA));
    h.insert(ACCEPT, HeaderValue::from_static("*/*"));
    h.insert(
        ACCEPT_LANGUAGE,
        HeaderValue::from_static("fr-FR,fr;q=0.9,en;q=0.8"),
    );
    if Some(host.as_str()) == origin_url().host_str() {
        h.insert(
            HeaderName::from_static("francestudent-client-platform"),
            HeaderValue::from_static(PLATFORM),
        );
        h.insert(
            HeaderName::from_static("francestudent-client-version"),
            HeaderValue::from_static(CLIENT_VERSION),
        );
        if let Ok(v) = HeaderValue::from_str(device_id) {
            h.insert(HeaderName::from_static("francestudent-device-id"), v);
        }
    }
    for (k, v) in extra {
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            h.insert(n, v);
        }
    }
    if let Some(v) = jar
        .header_for(&host)
        .and_then(|c| HeaderValue::from_str(&c).ok())
    {
        h.insert(COOKIE, v);
    }

    let mut req = c.request(method, url.clone()).headers(h);
    if let Some(b) = body {
        req = req.body(b);
    }
    let resp = req.send().await.map_err(net_err)?;
    let status = resp.status();
    let headers = resp.headers().clone();
    jar.absorb(&host, &headers);
    let body = resp.text().await.map_err(net_err)?;
    let r = Resp {
        status,
        headers,
        body,
        url: url.clone(),
    };
    if is_blocked(&r) {
        return Err(ERR_BLOCKED.to_string());
    }
    Ok(r)
}

fn form_encode(pairs: &[(String, String)]) -> String {
    let mut u = Url::parse("http://form.invalid/").expect("url valide");
    u.query_pairs_mut()
        .extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    u.query().unwrap_or("").to_string()
}

fn header_str<'a>(h: &'a HeaderMap, name: HeaderName) -> Option<&'a str> {
    h.get(name)?.to_str().ok()
}

// ---------- Connexion web ----------

/// Choisit le fournisseur OAuth de même origine, sinon l'URL WHMCS par défaut.
fn pick_signin(origin: &Url, body: &str) -> Url {
    let default = origin.join("/api/auth/signin/whmcs").expect("url valide");
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return default;
    };
    let Some(obj) = v.as_object() else {
        return default;
    };
    for p in obj.values() {
        if p.get("type").and_then(Value::as_str) != Some("oauth") {
            continue;
        }
        let Some(u) = p
            .get("signinUrl")
            .and_then(Value::as_str)
            .and_then(|s| origin.join(s).ok())
        else {
            continue;
        };
        if u.origin() == origin.origin() {
            return u;
        }
    }
    default
}

/// Déroule le flux de connexion et renvoie le cookie `next-auth.session-token`.
async fn web_login(
    c: &Client,
    device_id: &str,
    email: &str,
    password: &str,
) -> Result<String, String> {
    let origin = origin_url();
    let host = origin.host_str().unwrap_or_default().to_string();
    let mut jar = Jar::default();
    jar.set(&host, DEVICE_COOKIE, device_id);
    let join = |p: &str| origin.join(p).map_err(|e| e.to_string());

    // 1. Fournisseur OAuth.
    let r = send(
        c,
        &mut jar,
        device_id,
        Method::GET,
        &join("/api/auth/providers")?,
        &[("Accept", "application/json")],
        None,
    )
    .await?;
    let signin = if r.status.is_success() {
        pick_signin(&origin, &r.body)
    } else {
        pick_signin(&origin, "")
    };

    // 2. Jeton CSRF.
    let r = send(
        c,
        &mut jar,
        device_id,
        Method::GET,
        &join("/api/auth/csrf")?,
        &[("Accept", "application/json")],
        None,
    )
    .await?;
    if !r.status.is_success() {
        return Err(format!(
            "France Student a répondu HTTP {} (jeton CSRF).",
            r.status.as_u16()
        ));
    }
    let csrf = serde_json::from_str::<Value>(&r.body)
        .ok()
        .and_then(|v| v.get("csrfToken")?.as_str().map(str::to_string))
        .filter(|t| !t.is_empty())
        .ok_or("Réponse inattendue de France Student (jeton CSRF).")?;

    // 3. URL d'autorisation.
    let body = form_encode(&[
        ("csrfToken".to_string(), csrf),
        ("callbackUrl".to_string(), "/".to_string()),
    ]);
    let r = send(
        c,
        &mut jar,
        device_id,
        Method::POST,
        &signin,
        &[
            ("Accept", "application/json"),
            ("Content-Type", "application/x-www-form-urlencoded"),
            ("Origin", ORIGIN_URL),
            ("Referer", ORIGIN_URL),
            ("X-Auth-Return-Redirect", "1"),
        ],
        Some(body),
    )
    .await?;
    let authorize = header_str(&r.headers, LOCATION)
        .and_then(|l| signin.join(l).ok())
        .or_else(|| {
            serde_json::from_str::<Value>(&r.body)
                .ok()
                .and_then(|v| signin.join(v.get("url")?.as_str()?).ok())
        })
        .ok_or("Réponse inattendue de France Student (étape d'autorisation).")?;

    // 4. Pages d'autorisation / de connexion, redirections suivies à la main.
    let mut next = Next {
        post: false,
        url: authorize,
        body: String::new(),
    };
    let mut referer = origin.clone();
    let mut submitted = false;
    let mut pages = 0usize;
    for _ in 0..MAX_REQUESTS {
        let referer_s = referer.as_str().to_string();
        let origin_s = referer.origin().ascii_serialization();
        let mut extra: Vec<(&str, &str)> = vec![("Accept", HTML_ACCEPT), ("Referer", &referer_s)];
        let (method, body) = if next.post {
            extra.push(("Content-Type", "application/x-www-form-urlencoded"));
            extra.push(("Origin", &origin_s));
            (Method::POST, Some(next.body.clone()))
        } else {
            (Method::GET, None)
        };
        let r = send(c, &mut jar, device_id, method, &next.url, &extra, body).await?;

        if let Some(t) = jar.get(&host, SESSION_COOKIE) {
            return Ok(t.clone());
        }
        if is_redirect(r.status) {
            if let Some(loc) = header_str(&r.headers, LOCATION).and_then(|l| r.url.join(l).ok()) {
                next = Next {
                    post: false,
                    url: loc,
                    body: String::new(),
                };
                referer = r.url;
                continue;
            }
        }
        if r.status.as_u16() >= 400 {
            return Err(format!(
                "France Student a répondu HTTP {} pendant la connexion.",
                r.status.as_u16()
            ));
        }
        pages += 1;
        if pages > MAX_PAGES {
            return Err("Connexion interrompue : trop d'étapes inattendues.".to_string());
        }
        let (n, is_login) = analyze(&r.url, &r.body, email, password, submitted)?;
        submitted |= is_login;
        next = n;
        referer = r.url;
    }
    Err("Connexion interrompue : trop de redirections.".to_string())
}

// ---------- Jeton d'accès ----------

struct Token {
    access: String,
    exp: u64,
    /// Nouveau cookie de session si le serveur l'a renouvelé.
    new_cookie: Option<String>,
}

fn json_num(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_f64().filter(|f| *f > 0.0).map(|f| f as u64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn parse_token_json(v: &Value, now: u64) -> Option<(String, u64)> {
    let access = v
        .get("accessToken")?
        .as_str()
        .map(str::trim)
        .filter(|t| !t.is_empty())?
        .to_string();
    let exp = v
        .get("expiresAt")
        .and_then(json_num)
        .map(|t| if t > 1_000_000_000_000 { t / 1000 } else { t })
        .filter(|t| *t > now)
        .or_else(|| {
            v.get("expiresIn")
                .and_then(json_num)
                .filter(|n| *n > 0)
                .map(|n| now + n)
        })
        .unwrap_or(now + 300);
    Some((access, exp))
}

async fn fetch_api_token(c: &Client, s: &Session) -> Result<Token, String> {
    let origin = origin_url();
    let host = origin.host_str().unwrap_or_default().to_string();
    let mut jar = Jar::default();
    jar.set(&host, SESSION_COOKIE, &s.session_token);
    jar.set(&host, DEVICE_COOKIE, &s.device_id);
    let url = origin
        .join("/api/auth/api-token")
        .map_err(|e| e.to_string())?;
    let referer = format!("{ORIGIN_URL}/");
    let r = send(
        c,
        &mut jar,
        &s.device_id,
        Method::GET,
        &url,
        &[("Origin", ORIGIN_URL), ("Referer", &referer)],
        None,
    )
    .await?;
    if matches!(r.status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) || is_redirect(r.status)
    {
        return Err("session expirée ou refusée par France Student.".to_string());
    }
    if !r.status.is_success() {
        return Err(format!(
            "France Student a répondu HTTP {} (jeton d'accès).",
            r.status.as_u16()
        ));
    }
    let (access, exp) = serde_json::from_str::<Value>(&r.body)
        .ok()
        .and_then(|v| parse_token_json(&v, now_secs()))
        .ok_or("réponse inattendue de France Student (jeton d'accès).")?;
    let new_cookie = jar
        .get(&host, SESSION_COOKIE)
        .filter(|c| **c != s.session_token)
        .cloned();
    Ok(Token {
        access,
        exp,
        new_cookie,
    })
}

// ---------- Analyse des pages HTML ----------

#[derive(Debug, Clone)]
struct Field {
    name: String,
    kind: String,
    value: String,
}

#[derive(Debug, Clone)]
struct Button {
    name: String,
    value: String,
    label: String,
}

#[derive(Debug, Clone)]
struct Form {
    action: String,
    method: String,
    fields: Vec<Field>,
    buttons: Vec<Button>,
}

/// Prochaine requête à envoyer.
struct Next {
    post: bool,
    url: Url,
    body: String,
}

fn decode_entities(s: &str) -> String {
    re!(r"&(#[xX][0-9a-fA-F]+|#[0-9]+|[A-Za-z]+);")
        .replace_all(s, |c: &regex::Captures| {
            let e = &c[1];
            let decoded: Option<String> =
                if let Some(h) = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")) {
                    u32::from_str_radix(h, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .map(String::from)
                } else if let Some(d) = e.strip_prefix('#') {
                    d.parse::<u32>()
                        .ok()
                        .and_then(char::from_u32)
                        .map(String::from)
                } else {
                    match e {
                        "amp" => Some("&".to_string()),
                        "lt" => Some("<".to_string()),
                        "gt" => Some(">".to_string()),
                        "quot" => Some("\"".to_string()),
                        "apos" => Some("'".to_string()),
                        "nbsp" => Some(" ".to_string()),
                        _ => None,
                    }
                };
            decoded.unwrap_or_else(|| c[0].to_string())
        })
        .into_owned()
}

fn strip_tags(s: &str) -> String {
    let text = re!(r"(?s)<[^>]*>").replace_all(s, " ");
    decode_entities(&text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Retire commentaires, scripts, styles, templates et noscript.
fn clean_html(html: &str) -> String {
    re!(r"(?is)<!--.*?-->|<script\b.*?</script\s*>|<style\b.*?</style\s*>|<template\b.*?</template\s*>|<noscript\b.*?</noscript\s*>")
 .replace_all(html, " ")
 .into_owned()
}

fn parse_attrs(body: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for c in re!(r#"(?s)([^\s"'<>/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+)))?"#)
        .captures_iter(body)
    {
        let v = c
            .get(2)
            .or_else(|| c.get(3))
            .or_else(|| c.get(4))
            .map(|x| x.as_str())
            .unwrap_or("");
        m.entry(c[1].to_ascii_lowercase())
            .or_insert_with(|| decode_entities(v));
    }
    m
}

fn parse_forms(html: &str) -> Vec<Form> {
    let clean = clean_html(html);
    let form_re = re!(&format!(r"(?is)<form\b({TB})>(.*?)</form\s*>"));
    let input_re = re!(&format!(r"(?is)<input\b({TB})>"));
    let button_re = re!(&format!(r"(?is)<button\b({TB})>(.*?)</button\s*>"));

    let mut forms = Vec::new();
    for fc in form_re.captures_iter(&clean) {
        let attrs = parse_attrs(&fc[1]);
        let inner = &fc[2];
        let mut fields = Vec::new();
        let mut buttons = Vec::new();

        for ic in input_re.captures_iter(inner) {
            let a = parse_attrs(&ic[1]);
            if a.contains_key("disabled") {
                continue;
            }
            let kind = a
                .get("type")
                .map(|t| t.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "text".to_string());
            let name = a.get("name").cloned().unwrap_or_default();
            let value = a.get("value").cloned().unwrap_or_default();
            match kind.as_str() {
                "submit" | "image" => buttons.push(Button {
                    name,
                    label: value.clone(),
                    value,
                }),
                "button" | "reset" | "file" => {}
                "checkbox" | "radio" => {
                    if a.contains_key("checked") && !name.is_empty() {
                        let value = if value.is_empty() {
                            "on".to_string()
                        } else {
                            value
                        };
                        fields.push(Field { name, kind, value });
                    }
                }
                _ => {
                    if !name.is_empty() {
                        fields.push(Field { name, kind, value });
                    }
                }
            }
        }

        for bc in button_re.captures_iter(inner) {
            let a = parse_attrs(&bc[1]);
            if a.contains_key("disabled") {
                continue;
            }
            let kind = a
                .get("type")
                .map(|t| t.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "submit".to_string());
            if kind == "button" || kind == "reset" {
                continue;
            }
            buttons.push(Button {
                name: a.get("name").cloned().unwrap_or_default(),
                value: a.get("value").cloned().unwrap_or_default(),
                label: strip_tags(&bc[2]),
            });
        }

        forms.push(Form {
            action: attrs.get("action").cloned().unwrap_or_default(),
            method: attrs
                .get("method")
                .map(|m| m.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "get".to_string()),
            fields,
            buttons,
        });
    }
    forms
}

fn is_text_kind(kind: &str) -> bool {
    matches!(kind, "text" | "email" | "tel")
}

/// Formulaire de connexion : un champ mot de passe nommé et un champ texte visible.
/// Renvoie (indice du formulaire, nom du champ identifiant, nom du champ mot de passe).
fn find_login_form(forms: &[Form]) -> Option<(usize, String, String)> {
    for (i, f) in forms.iter().enumerate() {
        let Some(pass) = f.fields.iter().find(|x| x.kind == "password") else {
            continue;
        };
        let texts: Vec<&Field> = f.fields.iter().filter(|x| is_text_kind(&x.kind)).collect();
        let Some(first) = texts.first() else { continue };
        let user = texts
            .iter()
            .find(|x| re!(r"(?i)user|mail|login|ident").is_match(&x.name))
            .unwrap_or(first);
        return Some((i, user.name.clone(), pass.name.clone()));
    }
    None
}

/// Formulaire de consentement : aucun champ de saisie, un bouton « autoriser ».
/// Renvoie (indice du formulaire, indice du bouton).
fn find_consent(forms: &[Form]) -> Option<(usize, usize)> {
    for (i, f) in forms.iter().enumerate() {
        if f.fields
            .iter()
            .any(|x| x.kind == "password" || is_text_kind(&x.kind))
        {
            continue;
        }
        for (j, b) in f.buttons.iter().enumerate() {
            let text = format!("{} {} {}", b.name, b.value, b.label);
            if re!(r"(?i)autoris|authori[sz]e|approve|allow|accepter|accept").is_match(&text) {
                return Some((i, j));
            }
        }
    }
    None
}

fn resolve_action(page: &Url, action: &str) -> Result<Url, String> {
    let a = action.trim();
    if a.is_empty() {
        return Ok(page.clone());
    }
    page.join(a)
        .map_err(|_| "Formulaire de connexion invalide (adresse d'envoi).".to_string())
}

fn finish(f: &Form, action: Url, pairs: Vec<(String, String)>) -> Next {
    if f.method == "post" {
        Next {
            post: true,
            url: action,
            body: form_encode(&pairs),
        }
    } else {
        let mut url = action;
        let q = form_encode(&pairs);
        url.set_query(if q.is_empty() { None } else { Some(&q) });
        Next {
            post: false,
            url,
            body: String::new(),
        }
    }
}

fn refused_message(html: &str) -> String {
    let alert = re!(r#"(?is)<(?:div|p|span|ul|li)\b[^>]*class\s*=\s*["'][^"']*(?:alert-danger|alert-error|login-error|errorbox)[^"']*["'][^>]*>(.*?)</(?:div|p|span|ul|li)\s*>"#)
 .captures(html)
        .map(|c| strip_tags(&c[1]))
 .filter(|t| !t.is_empty());
    match alert {
        Some(t) => format!(
            "Identifiants refusés par France Student : {}",
            t.chars().take(200).collect::<String>()
        ),
        None => "Identifiants refusés par France Student : vérifie ton e-mail et ton mot de passe."
            .to_string(),
    }
}

/// Décide quoi faire d'une page : soumettre le formulaire de connexion (`true`), le formulaire de
/// consentement (`false`), ou échouer avec un message clair.
fn analyze(
    page: &Url,
    html: &str,
    email: &str,
    password: &str,
    submitted: bool,
) -> Result<(Next, bool), String> {
    let forms = parse_forms(html);

    if let Some((i, user_name, pass_name)) = find_login_form(&forms) {
        if submitted {
            return Err(refused_message(html));
        }
        if re!(r"(?i)g-recaptcha|h-captcha|cf-turnstile").is_match(html) {
            return Err(ERR_CAPTCHA.to_string());
        }
        let f = &forms[i];
        if f.method != "post" {
            return Err(
                "Formulaire de connexion non sécurisé (envoi hors POST) : abandon.".to_string(),
            );
        }
        let action = resolve_action(page, &f.action)?;
        if action.scheme() != "https" {
            return Err("Formulaire de connexion non sécurisé (hors HTTPS) : abandon.".to_string());
        }
        let mut pairs: Vec<(String, String)> = Vec::new();
        let (mut user_done, mut pass_done) = (false, false);
        for fld in &f.fields {
            if fld.name == user_name {
                if !user_done {
                    pairs.push((user_name.clone(), email.to_string()));
                    user_done = true;
                }
            } else if fld.name == pass_name {
                if !pass_done {
                    pairs.push((pass_name.clone(), password.to_string()));
                    pass_done = true;
                }
            } else {
                pairs.push((fld.name.clone(), fld.value.clone()));
            }
        }
        if let Some(b) = f.buttons.iter().find(|b| !b.name.is_empty()) {
            pairs.push((b.name.clone(), b.value.clone()));
        }
        return Ok((finish(f, action, pairs), true));
    }

    if submitted
 && re!(r"(?i)double authentification|authentification (à|a) deux|two-?factor|\b2fa\b|code de v[ée]rification|authenticator|totp")
     .is_match(&clean_html(html))
    {
        return Err(ERR_2FA.to_string());
    }

    if let Some((i, j)) = find_consent(&forms) {
        let f = &forms[i];
        let action = resolve_action(page, &f.action)?;
        let mut pairs: Vec<(String, String)> = f
            .fields
            .iter()
            .map(|x| (x.name.clone(), x.value.clone()))
            .collect();
        let b = &f.buttons[j];
        if !b.name.is_empty() {
            pairs.push((b.name.clone(), b.value.clone()));
        }
        return Ok((finish(f, action, pairs), false));
    }

    Err("Page inattendue pendant la connexion à France Student. Utilise un jeton de connexion (Réglages > Compte).".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGIN_PAGE: &str = r#"<html><head><script>var x = "<form>";</script></head><body>
        <!-- <form action="/ignore"><input type="password" name="zz"></form> -->
 <form action="/login?x=1&amp;y=2" method="POST">
   <input type="hidden" name="token" value="a&amp;b">
   <input type="email" name="username" id="inputEmail">
   <input type="password" name="password">
          <input type="checkbox" name="remember" value="1">
   <input type="text" name="disabled_field" disabled>
          <button type="submit" name="go" value="1"><span>Connexion</span></button>
        </form></body></html>"#;

    fn page() -> Url {
        Url::parse("https://x.test/auth").unwrap()
    }

    #[test]
    fn login_form_is_detected_and_filled() {
        let (next, is_login) = analyze(&page(), LOGIN_PAGE, "a@b.fr", "p w", false).unwrap();
        assert!(is_login);
        assert!(next.post);
        assert_eq!(next.url.as_str(), "https://x.test/login?x=1&y=2");
        assert_eq!(next.body, "token=a%26b&username=a%40b.fr&password=p+w&go=1");
    }

    #[test]
    fn login_form_second_time_is_refused() {
        let html = format!(
            r#"<div class="alert alert-danger">Mot de passe &amp; e-mail invalides</div>{LOGIN_PAGE}"#
        );
        let err = analyze(&page(), &html, "a@b.fr", "pw", true).err().unwrap();
        assert!(err.contains("Identifiants refusés"));
        assert!(err.contains("Mot de passe & e-mail invalides"));
        assert!(!err.contains("pw\""));
    }

    #[test]
    fn captcha_and_insecure_forms_are_rejected() {
        let captcha = format!(r#"<div class="g-recaptcha"></div>{LOGIN_PAGE}"#);
        assert_eq!(
            analyze(&page(), &captcha, "a@b.fr", "pw", false)
                .err()
                .unwrap(),
            ERR_CAPTCHA
        );
        let http = LOGIN_PAGE.replace("/login?x=1&amp;y=2", "http://x.test/login");
        assert!(analyze(&page(), &http, "a@b.fr", "pw", false).is_err());
        let get = LOGIN_PAGE.replace("POST", "GET");
        assert!(analyze(&page(), &get, "a@b.fr", "pw", false).is_err());
    }

    #[test]
    fn consent_form_is_submitted() {
        let html = r#"<form action="https://x.test/consent" method="post">
     <input type="hidden" name="state" value="abc">
     <button name="decision" value="deny">Refuser</button>
     <button name="decision" value="allow">Autoriser</button></form>"#;
        let (next, is_login) = analyze(&page(), html, "a@b.fr", "pw", true).unwrap();
        assert!(!is_login);
        assert_eq!(next.url.as_str(), "https://x.test/consent");
        assert_eq!(next.body, "state=abc&decision=allow");
    }

    #[test]
    fn two_factor_page_is_detected() {
        let html = r#"<p>Code de vérification</p><form method="post"><input type="text" name="code"><button name="ok" value="1">Valider</button></form>"#;
        assert_eq!(
            analyze(&page(), html, "a@b.fr", "pw", true).err().unwrap(),
            ERR_2FA
        );
        assert!(analyze(&page(), html, "a@b.fr", "pw", false).is_err());
    }

    #[test]
    fn set_cookie_parsing() {
        assert_eq!(
            parse_set_cookie("next-auth.session-token=abc; Path=/; HttpOnly"),
            Some((
                "next-auth.session-token".to_string(),
                Some("abc".to_string())
            ))
        );
        assert_eq!(
            parse_set_cookie("a=b; Max-Age=0"),
            Some(("a".to_string(), None))
        );
        assert_eq!(
            parse_set_cookie("a=b; Expires=Thu, 01 Jan 1970 00:00:00 GMT"),
            Some(("a".to_string(), None))
        );
        assert_eq!(
            parse_set_cookie("a=; Path=/"),
            Some(("a".to_string(), None))
        );
        assert_eq!(parse_set_cookie("garbage"), None);
    }

    #[test]
    fn jar_absorbs_and_removes_cookies() {
        let mut jar = Jar::default();
        let mut h = HeaderMap::new();
        h.append(SET_COOKIE, HeaderValue::from_static("a=1; Path=/"));
        h.append(SET_COOKIE, HeaderValue::from_static("b=2"));
        jar.absorb("h", &h);
        assert_eq!(jar.header_for("h").unwrap(), "a=1; b=2");
        let mut h = HeaderMap::new();
        h.append(SET_COOKIE, HeaderValue::from_static("a=; Max-Age=0"));
        jar.absorb("h", &h);
        assert_eq!(jar.header_for("h").unwrap(), "b=2");
        assert!(jar.header_for("other").is_none());
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(
            decode_entities("a&amp;b &#233; &#x41; &unknown; &nbsp;"),
            "a&b é A &unknown;  "
        );
    }

    #[test]
    fn signin_provider_selection() {
        let o = origin_url();
        let body = r#"{"google":{"type":"oidc","signinUrl":"/x"},"whmcs":{"type":"oauth","signinUrl":"https://ia.francestudent.org/api/auth/signin/custom"}}"#;
        assert_eq!(
            pick_signin(&o, body).as_str(),
            "https://ia.francestudent.org/api/auth/signin/custom"
        );
        let foreign = r#"{"w":{"type":"oauth","signinUrl":"https://evil.test/signin"}}"#;
        assert_eq!(
            pick_signin(&o, foreign).as_str(),
            "https://ia.francestudent.org/api/auth/signin/whmcs"
        );
        assert_eq!(
            pick_signin(&o, "not json").as_str(),
            "https://ia.francestudent.org/api/auth/signin/whmcs"
        );
    }

    #[test]
    fn token_json_expiry() {
        let v: Value = serde_json::json!({"accessToken": " tok ", "expiresAt": 2000});
        assert_eq!(parse_token_json(&v, 1000), Some(("tok".to_string(), 2000)));
        let v: Value = serde_json::json!({"accessToken": "tok", "expiresAt": 2_000_000_000_000u64});
        assert_eq!(
            parse_token_json(&v, 1000),
            Some(("tok".to_string(), 2_000_000_000))
        );
        let v: Value = serde_json::json!({"accessToken": "tok", "expiresIn": "120"});
        assert_eq!(parse_token_json(&v, 1000), Some(("tok".to_string(), 1120)));
        let v: Value = serde_json::json!({"accessToken": "tok"});
        assert_eq!(parse_token_json(&v, 1000), Some(("tok".to_string(), 1300)));
        let v: Value = serde_json::json!({"accessToken": ""});
        assert_eq!(parse_token_json(&v, 1000), None);
    }

    #[test]
    fn device_id_is_uuid_v4() {
        let id = new_device_id().unwrap();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
    }

    #[test]
    fn session_roundtrip_and_clear() {
        let dir = std::env::temp_dir().join(format!("fs-login-test-{}", now_secs()));
        assert!(!has_session(&dir));
        save_session(
            &dir,
            &Session {
                session_token: "tok".into(),
                device_id: "dev".into(),
            },
        )
        .unwrap();
        assert!(has_session(&dir));
        assert_eq!(load_session(&dir).unwrap().session_token, "tok");
        clear_session(&dir).unwrap();
        assert!(!has_session(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
