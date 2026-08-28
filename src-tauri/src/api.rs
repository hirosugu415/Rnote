//! Local HTTP API — this app's slice of the shared "R経済圏" API.
//!
//! Loopback only, bearer-token auth, everything under `/v1/`. Other apps in the
//! suite (currently Rhub) use this instead of scanning the notes folder or the
//! IPC surface directly, so the wire format here is the contract and `/v1/` is
//! meant to stay frozen — a breaking change adds `/v2/` beside it.
//!
//! `GET /v1/notes` reads; `POST /v1/notes` creates one (the recipe engine's
//! `rnote.create` step). Otherwise near-identical to Rfinder's and Rclip's
//! `api.rs`: only [`SERVICE_NAME`] and [`route`] differ.

use crate::config::Status;
use crate::notes::search::{apply, Filter};
use crate::state::AppState;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use tiny_http::{Header, Request, Response, Server, StatusCode};

/// Bumped only on a breaking change to the wire format.
pub const API_VERSION: u32 = 1;

/// Identifies this app in `%APPDATA%\R\services\<name>.json` and `/v1/health`.
pub const SERVICE_NAME: &str = "rnote";

/// `%APPDATA%\R` — the one directory every app in the suite agrees on.
fn r_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("R")
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 32 hex chars from the OS-seeded hasher state. Not a crate-grade CSPRNG, but
/// the token only has to be unguessable by code that cannot read the file it
/// lives in — a stray `fetch()` from a web page, say — and this is that.
fn random_hex_32() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::with_capacity(32);
    while out.len() < 32 {
        let n = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        out.push_str(&format!("{n:016x}"));
    }
    out.truncate(32);
    out
}

/// The shared bearer token. The first app to start writes it; everyone else
/// reads it. `create_new` makes the race harmless — a loser just re-reads.
fn load_or_create_token() -> std::io::Result<String> {
    let path = r_dir().join("token");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        if existing.trim().len() >= 16 {
            return Ok(existing.trim().to_string());
        }
    }
    std::fs::create_dir_all(r_dir())?;
    let token = random_hex_32();
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(token.as_bytes())?;
            Ok(token)
        }
        Err(_) => Ok(std::fs::read_to_string(&path)?.trim().to_string()),
    }
}

fn service_file() -> PathBuf {
    r_dir().join("services").join(format!("{SERVICE_NAME}.json"))
}

/// Announce this app on the shared registry. Best-effort — Rhub health-checks
/// every entry before trusting it, so a stale file is tolerated.
fn write_service_file(port: u16) {
    let dir = r_dir().join("services");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("[api] could not create {}: {e}", dir.display());
        return;
    }
    let body = serde_json::json!({
        "name": SERVICE_NAME,
        "appVersion": env!("CARGO_PKG_VERSION"),
        "apiVersion": API_VERSION,
        "port": port,
        "pid": std::process::id(),
        "startedAt": now_unix(),
    });
    if let Err(e) = std::fs::write(service_file(), body.to_string()) {
        eprintln!("[api] could not write service file: {e}");
    }
}

/// Remove this app's registry entry. Called from the `RunEvent::Exit` handler.
pub fn remove_service_file() {
    let _ = std::fs::remove_file(service_file());
}

/// Start the API server on a background thread. A no-op (with a log line) if it
/// is disabled in config or the port will not bind.
pub fn spawn(handle: AppHandle) {
    let (enabled, port) = {
        let state = handle.state::<AppState>();
        let cfg = state.config.read();
        (cfg.api_enabled, cfg.api_port)
    };
    if !enabled || port == 0 {
        eprintln!("[api] disabled");
        return;
    }

    let token = match load_or_create_token() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[api] no shared token, API disabled: {e}");
            return;
        }
    };

    let server = match Server::http(("127.0.0.1", port)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[api] could not bind 127.0.0.1:{port}, API disabled: {e}");
            return;
        }
    };

    write_service_file(port);
    eprintln!("[api] {SERVICE_NAME} listening on http://127.0.0.1:{port}/v1/");

    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let handle = handle.clone();
            let token = token.clone();
            std::thread::spawn(move || dispatch(&handle, &token, request));
        }
    });
}

fn dispatch(handle: &AppHandle, token: &str, mut request: Request) {
    let method = request.method().to_string();
    let url = request.url().to_string();
    let (path, raw_query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let params = parse_query(raw_query);

    if path == "/v1/health" {
        let body = serde_json::json!({
            "name": SERVICE_NAME,
            "appVersion": env!("CARGO_PKG_VERSION"),
            "apiVersion": API_VERSION,
        });
        return respond(request, 200, &body.to_string());
    }

    if !authorized(&request, &params, token) {
        return respond(request, 401, r#"{"error":"unauthorized"}"#);
    }

    let body = read_json_body(&mut request);

    match route(handle, &method, path, &params, &body) {
        Ok(body) => respond(request, 200, &body),
        Err(RouteError::NotFound) => respond(request, 404, r#"{"error":"not found"}"#),
        Err(RouteError::Bad(msg)) => respond(
            request,
            400,
            &serde_json::json!({ "error": msg }).to_string(),
        ),
    }
}

enum RouteError {
    NotFound,
    Bad(String),
}

/// The only app-specific part of this module.
fn route(
    handle: &AppHandle,
    method: &str,
    path: &str,
    params: &HashMap<String, String>,
    body: &Value,
) -> Result<String, RouteError> {
    match (method, path) {
        ("GET", "/v1/notes") => Ok(notes_endpoint(handle, params)),
        ("POST", "/v1/notes") => create_endpoint(handle, body),
        _ => Err(RouteError::NotFound),
    }
}

fn read_json_body(request: &mut Request) -> Value {
    let mut s = String::new();
    let _ = request.as_reader().read_to_string(&mut s);
    if s.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&s).unwrap_or(Value::Null)
    }
}

/// `POST /v1/notes { content, tag?, status? }` → the created note (a `Note`
/// plus `apiVersion`). `tag`, if given and not already present in `content`, is
/// appended as an inline `#tag` line — Rnote's tags live in the body, not in
/// metadata. `status` is `scratch` (default) or `keep`.
fn create_endpoint(handle: &AppHandle, body: &Value) -> Result<String, RouteError> {
    let content = body
        .get("content")
        .or_else(|| body.get("body"))
        .and_then(Value::as_str)
        .ok_or_else(|| RouteError::Bad("missing \"content\"".into()))?;
    if content.trim().is_empty() {
        return Err(RouteError::Bad("\"content\" is empty".into()));
    }
    let tag = body.get("tag").and_then(Value::as_str).unwrap_or("").trim();
    let status = match body.get("status").and_then(Value::as_str) {
        Some("keep") => Status::Keep,
        _ => Status::Scratch,
    };

    let mut note_body = content.to_string();
    if !tag.is_empty()
        && !crate::notes::tags::extract(&note_body)
            .iter()
            .any(|t| t.eq_ignore_ascii_case(tag))
    {
        if !note_body.ends_with('\n') {
            note_body.push('\n');
        }
        note_body.push_str(&format!("\n#{tag}\n"));
    }

    let state = handle.state::<AppState>();
    let settings = state.config.read().clone();
    let note = crate::notes::store::create(&settings, &note_body, status)
        .map_err(|e| RouteError::Bad(e.to_string()))?;
    {
        let mut notes = state.notes.write();
        notes.push(note.clone());
        notes.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    }

    let mut value = serde_json::to_value(&note).unwrap_or(Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.insert("apiVersion".to_string(), Value::from(API_VERSION));
    }
    Ok(value.to_string())
}

/// `GET /v1/notes?q=&status=keep|scratch&limit=&offset=` →
/// `{ apiVersion, total, items }` where each item is a `NoteSummary`
/// (`filename,status,title,snippet,tags,createdAt,modifiedAt,isExpiring,
/// charCount`) plus a resolved absolute `path` — an additive field so a caller
/// can open or reveal the note without knowing where `notesDir` points.
/// `apply` has no paging of its own, so the page is sliced here.
fn notes_endpoint(handle: &AppHandle, params: &HashMap<String, String>) -> String {
    let q = params.get("q").cloned().unwrap_or_default();
    let status = match params.get("status").map(String::as_str) {
        Some("keep") => Some(Status::Keep),
        Some("scratch") => Some(Status::Scratch),
        _ => None,
    };
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50)
        .clamp(1, 500);
    let offset = params
        .get("offset")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);

    let filter = Filter {
        status,
        query: if q.is_empty() { None } else { Some(q) },
    };

    let state = handle.state::<AppState>();
    let notes_dir = state.config.read().notes_dir.clone();
    let all = apply(&state.notes.read(), &filter);
    let total = all.len();
    let items: Vec<serde_json::Value> = all
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|summary| {
            let path = notes_dir
                .join(summary.status.dirname())
                .join(&summary.filename);
            let mut value = serde_json::to_value(&summary).unwrap_or_default();
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "path".to_string(),
                    serde_json::Value::String(path.to_string_lossy().into_owned()),
                );
            }
            value
        })
        .collect();

    serde_json::json!({
        "apiVersion": API_VERSION,
        "total": total,
        "items": items,
    })
    .to_string()
}

fn respond(request: Request, code: u16, body: &str) {
    let header = Header::from_bytes(
        &b"Content-Type"[..],
        &b"application/json; charset=utf-8"[..],
    )
    .expect("static header");
    let response = Response::from_string(body)
        .with_status_code(StatusCode(code))
        .with_header(header);
    let _ = request.respond(response);
}

fn authorized(request: &Request, params: &HashMap<String, String>, token: &str) -> bool {
    for header in request.headers() {
        if header.field.equiv("Authorization") {
            if let Some(rest) = header.value.as_str().strip_prefix("Bearer ") {
                if constant_time_eq(rest.trim(), token) {
                    return true;
                }
            }
        }
    }
    params
        .get("token")
        .map(|t| constant_time_eq(t, token))
        .unwrap_or(false)
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `a=1&b=hello%20world` → map. `+` and `%XX` are decoded in values.
fn parse_query(raw: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for pair in raw.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        out.insert(percent_decode(k), percent_decode(v));
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
