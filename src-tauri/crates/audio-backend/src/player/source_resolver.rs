//! Just-in-time playback source resolution.
//!
//! This is a deliberate *port of the frontend policy*, not a reimplementation
//! of the Netease API. All crypto (weapi/eapi/xeapi, anonymous_token, cookie
//! signing) lives elsewhere: either in the deployed NeteaseCloudMusicApi
//! service, or — when the user selects the in-process transport — in the
//! embedded protocol layer that the frontend is already using, reached here
//! through a callback (see `NcmCallHook`). From this module both look like
//! "ask for `/song/url/v1`, get JSON back".
//!
//! The only thing duplicated is the five-rule fallback policy in
//! `src/utils/AudioContext/resolveSongUrl.ts`:
//!
//! 1. quality level selection
//! 2. VIP pre-check (`fee == 1 || fee == 4`, and not cloud-uploaded) → UNM first
//! 3. trial-clip detection (`jd-musicrep-ts` in the URL) → treat as no URL
//! 4. UNM fallback when NCM yields nothing
//! 5. kuwo.cn → prefer `proxyUrl`
//!
//! The policy half (`plan_resolution`, `classify_ncm_url`, `pick_unm_url`) is
//! pure and unit-tested against the same cases as the TS implementation. Only
//! `resolve_blocking` performs I/O, and it runs on a blocking worker — never on
//! the player loop and never anywhere near an audio callback.
//!
//! Secrets discipline: the cookie is sent as a header, is never logged, and
//! never enters a persisted snapshot or an emitted event.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crate::types::{NativeManifestEntry, NativeResolverConfig, TrackIdentity};

// ── In-process protocol layer hook ───────────────────────────────
//
// The embedded NCM protocol layer (`ncm-core`) lives in the host application,
// not here: this crate also compiles to `wasm32-unknown-unknown` for the web
// build, where a QuickJS isolate has no place. So the dependency is inverted —
// the app installs a callback and this module uses it when present.
//
// Signature mirrors `NcmCore::call`: `(endpoint, query_json) -> envelope_json`.

pub type NcmCallHook = Arc<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;

static NCM_HOOK: OnceLock<NcmCallHook> = OnceLock::new();

/// Install the in-process protocol layer. Idempotent by construction — the
/// first installation wins and later ones are ignored, which is what we want
/// for something wired once at startup.
///
/// Returns whether this call was the one that installed it.
pub fn install_ncm_call_hook(hook: NcmCallHook) -> bool {
    NCM_HOOK.set(hook).is_ok()
}

/// Whether an in-process protocol layer is available.
pub fn has_ncm_call_hook() -> bool {
    NCM_HOOK.get().is_some()
}

/// The installed protocol layer, for other backend modules that need to reach
/// Netease without a live WebView (see `metadata_fetch`).
pub(super) fn ncm_call_hook() -> Option<&'static NcmCallHook> {
    NCM_HOOK.get()
}

/// Assumed lifetime of a resolved Netease CDN URL. Their links are typically
/// valid for ~20 minutes; we treat them as good for 10 with a safety margin so
/// a one-ahead source prepared now is still playable when the current track
/// ends. Expiry is advisory — a 403 at play time re-resolves regardless.
const ASSUMED_URL_TTL: Duration = Duration::from_secs(600);

/// Network budget per attempt. Kept tight: a hung resolve must not stall the
/// hand-off to the next track past the point where the current one ends.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(15);

/// Where a resolved URL came from. Mirrors the TS `source` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceOrigin {
    Ncm,
    Unm,
    LocalFile,
}

#[derive(Debug, Clone)]
pub struct ResolvedSource {
    pub identity: TrackIdentity,
    pub uri: String,
    pub origin: SourceOrigin,
    resolved_at: Instant,
    ttl: Option<Duration>,
}

impl ResolvedSource {
    pub fn local(identity: TrackIdentity, path: String) -> Self {
        Self {
            identity,
            uri: path,
            origin: SourceOrigin::LocalFile,
            resolved_at: Instant::now(),
            // A path on disk does not expire.
            ttl: None,
        }
    }

    pub fn remote(identity: TrackIdentity, uri: String, origin: SourceOrigin) -> Self {
        Self {
            identity,
            uri,
            origin,
            resolved_at: Instant::now(),
            ttl: Some(ASSUMED_URL_TTL),
        }
    }

    /// Whether this source is past its assumed lifetime and should be
    /// re-resolved before use.
    pub fn is_stale(&self) -> bool {
        match self.ttl {
            Some(ttl) => self.resolved_at.elapsed() >= ttl,
            None => false,
        }
    }
}

/// Classification of a resolve failure, used by the planner's bounded
/// skip/retry policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveErrorKind {
    /// Network/5xx/timeout — worth one retry.
    Transient,
    /// 401/403 — credentials rejected.
    Auth,
    /// Resolved to nothing playable (region-locked, taken down, trial only).
    Unavailable,
    /// Local file is gone.
    LocalMissing,
    /// Resolver is not configured (no API base URL).
    NotConfigured,
}

#[derive(Debug, Clone)]
pub struct ResolveError {
    pub kind: ResolveErrorKind,
    /// Already redacted — safe to log and to send to the frontend.
    pub message: String,
}

impl ResolveError {
    fn new(kind: ResolveErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn is_retryable(&self) -> bool {
        self.kind == ResolveErrorKind::Transient
    }
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

// ── Pure policy ──────────────────────────────────────────────────

/// What the resolver should try, in order. Mirrors the branch structure of
/// `resolveSongUrl` so the two stay comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionPlan {
    /// Local path: nothing to fetch.
    LocalPath,
    /// Normal path — NCM first, UNM as fallback when enabled.
    NcmThenUnm { unm_enabled: bool },
    /// VIP/paid pre-check hit: skip NCM entirely and go straight to UNM.
    UnmOnly,
}

/// Decide the resolution strategy for `entry`. Pure — no I/O, no allocation
/// beyond the returned enum.
pub fn plan_resolution(
    entry: &NativeManifestEntry,
    config: &NativeResolverConfig,
) -> ResolutionPlan {
    if matches!(entry.identity, TrackIdentity::Local { .. }) {
        return ResolutionPlan::LocalPath;
    }

    let unm_enabled = config.unm_enabled
        && config
            .unm_base_url
            .as_deref()
            .is_some_and(|base| !base.trim().is_empty());

    // VIP pre-check: fee=1 (VIP) or fee=4 (paid album), and not a
    // cloud-uploaded track (`pc` present bypasses the check).
    let vip_locked = matches!(entry.fee, Some(1) | Some(4)) && !entry.has_pc;
    if unm_enabled && vip_locked {
        return ResolutionPlan::UnmOnly;
    }

    ResolutionPlan::NcmThenUnm { unm_enabled }
}

/// Normalize and validate a URL returned by `/song/url/v1`. Returns `None`
/// when the response carries nothing playable — including the trial-clip case,
/// which reports a real URL that is only a preview.
pub fn classify_ncm_url(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let upgraded = upgrade_to_https(raw);
    // Trial/preview clip — the frontend nullifies these so UNM can take over.
    if upgraded.contains("jd-musicrep-ts") {
        return None;
    }
    Some(upgraded)
}

/// Apply the kuwo proxy rule to a UNM response.
pub fn pick_unm_url(url: Option<&str>, proxy_url: Option<&str>) -> Option<String> {
    let url = url?.trim();
    if url.is_empty() {
        return None;
    }
    let upgraded = upgrade_to_https(url);
    if upgraded.to_ascii_lowercase().contains("kuwo.cn") {
        if let Some(proxy) = proxy_url.map(str::trim).filter(|p| !p.is_empty()) {
            return Some(proxy.to_string());
        }
    }
    Some(upgraded)
}

fn upgrade_to_https(url: &str) -> String {
    match url.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_string(),
    }
}

/// Join an API base with a path, tolerating a trailing slash on the base
/// (`VITE_MUSIC_API` carries one).
pub fn join_url(base: &str, path: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{base}/{path}")
}

/// Strip anything credential-shaped out of a string before it is logged or
/// emitted. Defence in depth: callers should not be putting cookies in error
/// text in the first place.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for part in text.split_whitespace() {
        let lower = part.to_ascii_lowercase();
        if lower.contains("music_u")
            || lower.contains("cookie")
            || lower.contains("csrf")
            || lower.contains("token")
        {
            out.push_str("[redacted]");
        } else {
            out.push_str(part);
        }
        out.push(' ');
    }
    out.trim_end().to_string()
}

// ── I/O ──────────────────────────────────────────────────────────

/// Resolve `entry` to a playable source. Blocking; call from
/// `spawn_blocking`, never from the player loop.
pub fn resolve_blocking(
    entry: &NativeManifestEntry,
    config: &NativeResolverConfig,
) -> Result<ResolvedSource, ResolveError> {
    match plan_resolution(entry, config) {
        ResolutionPlan::LocalPath => {
            let TrackIdentity::Local { path } = &entry.identity else {
                return Err(ResolveError::new(
                    ResolveErrorKind::Unavailable,
                    "local plan for a non-local identity",
                ));
            };
            if !std::path::Path::new(path).exists() {
                return Err(ResolveError::new(
                    ResolveErrorKind::LocalMissing,
                    "local file no longer exists",
                ));
            }
            Ok(ResolvedSource::local(entry.identity.clone(), path.clone()))
        }
        ResolutionPlan::UnmOnly => resolve_via_unm(entry, config),
        ResolutionPlan::NcmThenUnm { unm_enabled } => {
            let ncm_result = resolve_via_ncm(entry, config);
            match ncm_result {
                Ok(source) => Ok(source),
                Err(err) => {
                    // Auth failures are not something UNM can fix, but an
                    // unavailable/trial track is exactly what it is for.
                    if unm_enabled && err.kind != ResolveErrorKind::NotConfigured {
                        resolve_via_unm(entry, config).map_err(|unm_err| {
                            // Report the more actionable of the two.
                            if err.kind == ResolveErrorKind::Auth {
                                err
                            } else {
                                unm_err
                            }
                        })
                    } else {
                        Err(err)
                    }
                }
            }
        }
    }
}

/// HTTP agent for resolver traffic, built per call.
///
/// A shared `ureq::Agent` — i.e. a live connection pool — was tried here and on
/// the download path in `decoder`, to save a DNS+TCP+TLS handshake per resolve.
/// It came back as tracks decoding to the *previous* track's length plus their
/// own: the signature of a pooled connection handing over a body that was not
/// fully drained, and for an MP3 with no Xing header symphonia estimates
/// duration from file size, so a concatenated body reads as a longer song
/// instead of failing. Both were reverted together; do not reintroduce one
/// without the other, and not without first proving the drain behaviour.
///
/// The pooling that *did* stay is `ncm-core`'s reqwest client, which is a
/// different stack, was already pooling before, and only had its idle window
/// widened.
pub(super) fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .build()
}

fn resolve_via_ncm(
    entry: &NativeManifestEntry,
    config: &NativeResolverConfig,
) -> Result<ResolvedSource, ResolveError> {
    let Some(song_id) = entry.identity.netease_id() else {
        return Err(ResolveError::new(
            ResolveErrorKind::Unavailable,
            "identity is not a netease track",
        ));
    };
    let level = config.level.as_deref().unwrap_or("exhigh");

    // Honour the same transport the frontend picked. Playback resolving through
    // the deployed API while the UI talks in-process (or the reverse) would
    // mean two different sessions, two different source IPs, and one of them
    // silently not benefiting from the change.
    let raw_body = match (config.use_local_ncm, NCM_HOOK.get()) {
        (true, Some(hook)) => resolve_ncm_body_local(hook, song_id, level, config)?,
        _ => resolve_ncm_body_remote(song_id, level, config)?,
    };

    let parsed: serde_json::Value = serde_json::from_str(&raw_body)
        .map_err(|_| ResolveError::new(ResolveErrorKind::Transient, "malformed NCM JSON"))?;

    let raw_url = parsed
        .get("data")
        .and_then(|data| data.get(0))
        .and_then(|first| first.get("url"))
        .and_then(|url| url.as_str());

    match classify_ncm_url(raw_url) {
        Some(url) => Ok(ResolvedSource::remote(
            entry.identity.clone(),
            url,
            SourceOrigin::Ncm,
        )),
        None => Err(ResolveError::new(
            ResolveErrorKind::Unavailable,
            "NCM returned no playable URL",
        )),
    }
}

/// `/song/url/v1` through the in-process protocol layer.
///
/// Returns the same JSON body shape the deployed API would have produced, so
/// the caller's parsing and the `classify_ncm_url` policy are untouched.
fn resolve_ncm_body_local(
    hook: &NcmCallHook,
    song_id: &str,
    level: &str,
    config: &NativeResolverConfig,
) -> Result<String, ResolveError> {
    let mut query = serde_json::json!({ "id": song_id, "level": level });
    if let Some(cookie) = config
        .cookie
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        query["cookie"] = serde_json::Value::String(cookie.to_string());
    }

    let envelope = hook("song_url_v1", &query.to_string()).map_err(|e| {
        // The hook redacts before returning; this is safe to surface.
        ResolveError::new(
            ResolveErrorKind::Transient,
            format!("in-process NCM call failed: {}", redact(&e)),
        )
    })?;

    let parsed: serde_json::Value = serde_json::from_str(&envelope).map_err(|_| {
        ResolveError::new(ResolveErrorKind::Transient, "malformed NCM envelope")
    })?;

    // The envelope wraps the upstream body. A non-200 upstream code is a real
    // answer, not a transport failure — 401/403 must stay distinguishable so
    // the planner's retry policy does not burn attempts on a credential
    // problem.
    let status = parsed.get("status").and_then(|s| s.as_u64()).unwrap_or(0);
    if parsed.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let kind = match status {
            401 | 403 => ResolveErrorKind::Auth,
            _ => ResolveErrorKind::Transient,
        };
        return Err(ResolveError::new(
            kind,
            format!("in-process NCM call returned status {status}"),
        ));
    }

    let body = parsed
        .get("body")
        .ok_or_else(|| ResolveError::new(ResolveErrorKind::Transient, "NCM envelope has no body"))?;
    Ok(body.to_string())
}

/// `/song/url/v1` through the deployed NeteaseCloudMusicApi.
fn resolve_ncm_body_remote(
    song_id: &str,
    level: &str,
    config: &NativeResolverConfig,
) -> Result<String, ResolveError> {
    let Some(base) = config
        .ncm_base_url
        .as_deref()
        .map(str::trim)
        .filter(|base| !base.is_empty())
    else {
        return Err(ResolveError::new(
            ResolveErrorKind::NotConfigured,
            "no NCM API base URL configured",
        ));
    };

    let url = join_url(base, "song/url/v1");
    let mut request = agent()
        .get(&url)
        .query("id", song_id)
        .query("level", level)
        // The deployed API accepts `realIP`-style params and cookies; we only
        // need the cookie for VIP-quality entitlement.
        .set("X-Requested-With", "XMLHttpRequest");
    if let Some(cookie) = config
        .cookie
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        request = request.set("Cookie", cookie);
    }

    send_and_read(request)
}

fn resolve_via_unm(
    entry: &NativeManifestEntry,
    config: &NativeResolverConfig,
) -> Result<ResolvedSource, ResolveError> {
    if !config.unm_enabled {
        return Err(ResolveError::new(
            ResolveErrorKind::NotConfigured,
            "UNM fallback is disabled",
        ));
    }
    let Some(base) = config
        .unm_base_url
        .as_deref()
        .map(str::trim)
        .filter(|base| !base.is_empty())
    else {
        return Err(ResolveError::new(
            ResolveErrorKind::NotConfigured,
            "no UNM base URL configured",
        ));
    };
    let Some(song_id) = entry.identity.netease_id() else {
        return Err(ResolveError::new(
            ResolveErrorKind::Unavailable,
            "identity is not a netease track",
        ));
    };

    let url = join_url(base, "match");
    let request = agent()
        .get(&url)
        .query("id", song_id)
        .query("server", "qq,pyncmd");

    let body = send_and_read(request)?;
    let parsed: serde_json::Value = serde_json::from_str(&body)
        .map_err(|_| ResolveError::new(ResolveErrorKind::Transient, "malformed UNM JSON"))?;

    let code_ok = parsed
        .get("code")
        .and_then(|code| code.as_i64())
        .is_some_and(|code| code == 200);
    if !code_ok {
        return Err(ResolveError::new(
            ResolveErrorKind::Unavailable,
            "UNM reported no match",
        ));
    }

    let data = parsed.get("data");
    let candidate = data.and_then(|d| d.get("url")).and_then(|u| u.as_str());
    let proxy = data
        .and_then(|d| d.get("proxyUrl"))
        .and_then(|u| u.as_str());

    match pick_unm_url(candidate, proxy) {
        Some(url) => Ok(ResolvedSource::remote(
            entry.identity.clone(),
            url,
            SourceOrigin::Unm,
        )),
        None => Err(ResolveError::new(
            ResolveErrorKind::Unavailable,
            "UNM returned no playable URL",
        )),
    }
}

fn send_and_read(request: ureq::Request) -> Result<String, ResolveError> {
    match request.call() {
        Ok(response) => response.into_string().map_err(|_| {
            ResolveError::new(ResolveErrorKind::Transient, "failed to read response body")
        }),
        Err(ureq::Error::Status(status, _)) => {
            let kind = match status {
                401 | 403 => ResolveErrorKind::Auth,
                404 | 410 => ResolveErrorKind::Unavailable,
                _ => ResolveErrorKind::Transient,
            };
            Err(ResolveError::new(kind, format!("HTTP {status}")))
        }
        // `ureq::Error::Transport` carries the URL, which for a signed CDN link
        // can embed a token — never surface it verbatim.
        Err(ureq::Error::Transport(_)) => Err(ResolveError::new(
            ResolveErrorKind::Transient,
            "transport error",
        )),
    }
}

// ── Account writes ───────────────────────────────────────────────

/// Add or remove the track from 我喜欢的音乐 (`/like`).
///
/// Lives here rather than in the frontend because the notification's heart has
/// to work with no WebView alive — the same reason the transport buttons are
/// wired into Rust. It reuses the resolver's transport choice for the reason
/// spelled out in `resolve_via_ncm`: playing through one session and writing
/// through another means two source IPs for one user action, and Netease's risk
/// control is entitled to notice.
///
/// Blocking: call it from `spawn_blocking`.
pub(super) fn set_favourite(
    song_id: &str,
    like: bool,
    config: &NativeResolverConfig,
) -> Result<(), String> {
    let body = match (config.use_local_ncm, NCM_HOOK.get()) {
        (true, Some(hook)) => favourite_body_local(hook, song_id, like, config),
        _ => favourite_body_remote(song_id, like, config),
    }
    .map_err(|err| redact(&err.message))?;

    // Netease answers HTTP 200 with the real verdict in the body, so the status
    // line says nothing. A missing `code` is treated as success: the deployed
    // API has more than one envelope shape and a like that worked must not be
    // reverted in the UI because we could not find a field.
    let parsed: serde_json::Value = match serde_json::from_str(&body) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(()),
    };
    match parsed.get("code").and_then(|code| code.as_i64()) {
        Some(200) | None => Ok(()),
        // 301 is "not logged in", which is the one worth naming: it is what the
        // user sees when their cookie expired while the app sat in the tray.
        Some(301) => Err("not logged in".into()),
        Some(code) => Err(format!("netease returned code {code}")),
    }
}

/// Query for the `like` endpoint.
///
/// `like` is spelled as a **string**, never as a JSON boolean, and that is
/// load-bearing. The endpoint module normalises it with `query.like != "false"`,
/// and JavaScript's `!=` coerces both sides to numbers — `Number(false)` is `0`,
/// `Number("false")` is `NaN`, so `false != "false"` is *true*. A boolean unlike
/// therefore reaches Netease as a **like**: the account keeps the track, the
/// response is a perfectly ordinary `code: 200`, and nothing on this side can
/// tell that the write did the opposite of what was asked. The symptom is a
/// heart that empties optimistically and is filled again by the next likelist
/// fetch — i.e. a button that appears to do nothing.
///
/// The deployed API is reached over a query string, where `"true"`/`"false"` is
/// the only representation there is, which is why this only ever bit the
/// in-process transport — the default one.
fn favourite_query(song_id: &str, like: bool, cookie: Option<&str>) -> String {
    let mut query = serde_json::json!({
        "id": song_id,
        "like": if like { "true" } else { "false" },
    });
    if let Some(cookie) = trimmed(cookie) {
        query["cookie"] = serde_json::Value::String(cookie.to_string());
    }
    query.to_string()
}

fn favourite_body_local(
    hook: &NcmCallHook,
    song_id: &str,
    like: bool,
    config: &NativeResolverConfig,
) -> Result<String, ResolveError> {
    let query = favourite_query(song_id, like, config.cookie.as_deref());

    let envelope = hook("like", &query).map_err(|e| {
        ResolveError::new(
            ResolveErrorKind::Transient,
            format!("in-process NCM call failed: {}", redact(&e)),
        )
    })?;
    let parsed: serde_json::Value = serde_json::from_str(&envelope)
        .map_err(|_| ResolveError::new(ResolveErrorKind::Transient, "malformed NCM envelope"))?;

    if parsed.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let status = parsed.get("status").and_then(|s| s.as_u64()).unwrap_or(0);
        let kind = match status {
            401 | 403 => ResolveErrorKind::Auth,
            _ => ResolveErrorKind::Transient,
        };
        return Err(ResolveError::new(
            kind,
            format!("in-process NCM call returned status {status}"),
        ));
    }
    let body = parsed
        .get("body")
        .ok_or_else(|| ResolveError::new(ResolveErrorKind::Transient, "NCM envelope has no body"))?;
    Ok(body.to_string())
}

fn favourite_body_remote(
    song_id: &str,
    like: bool,
    config: &NativeResolverConfig,
) -> Result<String, ResolveError> {
    let Some(base) = trimmed(config.ncm_base_url.as_deref()) else {
        return Err(ResolveError::new(
            ResolveErrorKind::NotConfigured,
            "no NCM API base URL configured",
        ));
    };

    let url = join_url(base, "like");
    let mut request = agent()
        .get(&url)
        .query("id", song_id)
        .query("like", if like { "true" } else { "false" })
        .set("X-Requested-With", "XMLHttpRequest");
    if let Some(cookie) = trimmed(config.cookie.as_deref()) {
        request = request.set("Cookie", cookie);
    }
    send_and_read(request)
}

/// Non-empty, whitespace-trimmed view of an optional config string.
fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

/// Fetch the signed-in account's liked track ids (`/likelist`).
///
/// The backend needs this to answer "is the playing track liked" on its own. The
/// notification's heart is drawn while the WebView is dead — the only time the
/// notification is the user's only UI — so a state that only the frontend can
/// supply is a state that is missing exactly when it matters.
///
/// Blocking: call it from `spawn_blocking`.
pub(super) fn fetch_likelist(config: &NativeResolverConfig) -> Result<Vec<String>, String> {
    let Some(uid) = trimmed(config.user_id.as_deref()) else {
        // Signed out. Not an error — there is simply no list.
        return Ok(Vec::new());
    };

    let body = match (config.use_local_ncm, NCM_HOOK.get()) {
        (true, Some(hook)) => likelist_body_local(hook, uid, config),
        _ => likelist_body_remote(uid, config),
    }
    .map_err(|err| redact(&err.message))?;

    let parsed: serde_json::Value = serde_json::from_str(&body)
        .map_err(|_| "malformed likelist JSON".to_string())?;
    let ids = parsed
        .get("ids")
        .and_then(|ids| ids.as_array())
        .ok_or_else(|| "likelist response has no ids".to_string())?;

    // Ids arrive as JSON numbers; keep them as strings so they compare directly
    // against `TrackIdentity::netease_id` without a lossy round trip through f64.
    Ok(ids
        .iter()
        .filter_map(|id| {
            id.as_i64()
                .map(|n| n.to_string())
                .or_else(|| id.as_str().map(str::to_string))
        })
        .collect())
}

/// Whether a `/likelist` body claims `id` is liked.
///
/// Split out so the shape handling is testable without a network. Ids are kept
/// as strings deliberately: `TrackIdentity::netease_id` is a string, and routing
/// a Netease id through `f64` would silently corrupt anything past 2^53.
#[cfg(test)]
fn likelist_contains(body: &str, id: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|parsed| parsed.get("ids").and_then(|ids| ids.as_array()).cloned())
        .is_some_and(|ids| {
            ids.iter().any(|entry| {
                entry
                    .as_i64()
                    .map(|n| n.to_string())
                    .or_else(|| entry.as_str().map(str::to_string))
                    .is_some_and(|entry| entry == id)
            })
        })
}

fn likelist_body_local(
    hook: &NcmCallHook,
    uid: &str,
    config: &NativeResolverConfig,
) -> Result<String, ResolveError> {
    let mut query = serde_json::json!({ "uid": uid });
    if let Some(cookie) = trimmed(config.cookie.as_deref()) {
        query["cookie"] = serde_json::Value::String(cookie.to_string());
    }
    let envelope = hook("likelist", &query.to_string()).map_err(|e| {
        ResolveError::new(
            ResolveErrorKind::Transient,
            format!("in-process NCM call failed: {}", redact(&e)),
        )
    })?;
    let parsed: serde_json::Value = serde_json::from_str(&envelope)
        .map_err(|_| ResolveError::new(ResolveErrorKind::Transient, "malformed NCM envelope"))?;
    if parsed.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let status = parsed.get("status").and_then(|s| s.as_u64()).unwrap_or(0);
        return Err(ResolveError::new(
            if matches!(status, 401 | 403) {
                ResolveErrorKind::Auth
            } else {
                ResolveErrorKind::Transient
            },
            format!("in-process NCM call returned status {status}"),
        ));
    }
    let body = parsed
        .get("body")
        .ok_or_else(|| ResolveError::new(ResolveErrorKind::Transient, "NCM envelope has no body"))?;
    Ok(body.to_string())
}

fn likelist_body_remote(
    uid: &str,
    config: &NativeResolverConfig,
) -> Result<String, ResolveError> {
    let Some(base) = trimmed(config.ncm_base_url.as_deref()) else {
        return Err(ResolveError::new(
            ResolveErrorKind::NotConfigured,
            "no NCM API base URL configured",
        ));
    };
    let url = join_url(base, "likelist");
    let mut request = agent()
        .get(&url)
        .query("uid", uid)
        .set("X-Requested-With", "XMLHttpRequest");
    if let Some(cookie) = trimmed(config.cookie.as_deref()) {
        request = request.set("Cookie", cookie);
    }
    send_and_read(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn netease_entry(fee: Option<i64>, has_pc: bool) -> NativeManifestEntry {
        NativeManifestEntry {
            identity: TrackIdentity::Netease { id: "123".into() },
            playlist_index: 0,
            title: None,
            artist: None,
            album: None,
            artwork_url: None,
            duration_ms: None,
            fee,
            has_pc,
        }
    }

    fn config(unm_enabled: bool) -> NativeResolverConfig {
        NativeResolverConfig {
            ncm_base_url: Some("https://ncm.example.com/".into()),
            unm_base_url: Some("https://unm.example.com/".into()),
            unm_enabled,
            cookie: Some("MUSIC_U=secret".into()),
            user_id: Some("42".into()),
            level: Some("exhigh".into()),
            // The policy tests cover transport-independent behaviour; the
            // remote path is the one with a base URL to point at.
            use_local_ncm: false,
        }
    }

    // ── Like list ────────────────────────────────────────────────

    /// Netease ids exceed 2^53, so the parse must never route them through a
    /// float. `serde_json` would happily hand back `1.9e18` for an `as_f64`, and
    /// the id would come back off by a few — matching nothing, so the heart would
    /// read empty for exactly the tracks with the largest ids.
    #[test]
    fn likelist_ids_survive_being_larger_than_a_float_can_hold() {
        let big = "1234567890123456789";
        let body = format!(r#"{{"ids":[{big},2001]}}"#);
        assert!(likelist_contains(&body, big));
        assert!(likelist_contains(&body, "2001"));
        assert!(!likelist_contains(&body, "1234567890123456788"));
    }

    /// The deployed API and the in-process layer have both been seen to answer
    /// with string ids; a heart must not depend on which.
    #[test]
    fn likelist_accepts_string_ids() {
        assert!(likelist_contains(r#"{"ids":["17","18"]}"#, "17"));
        assert!(!likelist_contains(r#"{"ids":["17"]}"#, "19"));
    }

    /// A malformed or empty answer means "not liked", never a crash and never a
    /// filled heart — one tap on a wrongly-filled heart *unlikes* a real track.
    #[test]
    fn a_broken_likelist_is_not_a_like() {
        for body in ["", "{", "null", "[]", r#"{"code":301}"#, r#"{"ids":null}"#] {
            assert!(!likelist_contains(body, "1"), "body {body:?} must not claim a like");
        }
    }

    // ── Like / unlike ────────────────────────────────────────────

    /// `like` must reach the endpoint module as a string.
    ///
    /// The module normalises it with `query.like != "false"`, which coerces
    /// through numbers: `Number(false)` is `0` and `Number("false")` is `NaN`, so
    /// the boolean `false` compares *unequal* to `"false"` and an unlike is
    /// rewritten into a like. Netease then answers `200`, so the only visible
    /// symptom is a heart that comes back filled — the press looked dead.
    #[test]
    fn an_unlike_is_spelled_the_way_the_endpoint_reads_it() {
        let parsed: serde_json::Value =
            serde_json::from_str(&favourite_query("1234", false, Some("MUSIC_U=secret"))).unwrap();
        assert_eq!(parsed["like"], serde_json::json!("false"));
        assert_eq!(parsed["id"], serde_json::json!("1234"));
        assert_eq!(parsed["cookie"], serde_json::json!("MUSIC_U=secret"));

        let parsed: serde_json::Value =
            serde_json::from_str(&favourite_query("1234", true, None)).unwrap();
        assert_eq!(parsed["like"], serde_json::json!("true"));
        // A blank cookie is omitted rather than sent empty: the endpoint reads it
        // as an object, and an empty one is not the same as none.
        assert!(parsed.get("cookie").is_none());
    }

    /// Whitespace-only credentials are not credentials.
    #[test]
    fn a_blank_cookie_is_not_attached() {
        let parsed: serde_json::Value =
            serde_json::from_str(&favourite_query("1", true, Some("   "))).unwrap();
        assert!(parsed.get("cookie").is_none());
    }

    // ── Rule 2: VIP pre-check ────────────────────────────────────

    #[test]
    fn vip_and_paid_album_go_straight_to_unm() {
        for fee in [1, 4] {
            assert_eq!(
                plan_resolution(&netease_entry(Some(fee), false), &config(true)),
                ResolutionPlan::UnmOnly,
                "fee={fee} must skip NCM"
            );
        }
    }

    #[test]
    fn cloud_uploaded_tracks_bypass_the_vip_precheck() {
        assert_eq!(
            plan_resolution(&netease_entry(Some(1), true), &config(true)),
            ResolutionPlan::NcmThenUnm { unm_enabled: true },
            "a `pc` track is playable through NCM even when fee=1"
        );
    }

    #[test]
    fn free_tracks_use_the_normal_path() {
        assert_eq!(
            plan_resolution(&netease_entry(Some(0), false), &config(true)),
            ResolutionPlan::NcmThenUnm { unm_enabled: true }
        );
        assert_eq!(
            plan_resolution(&netease_entry(None, false), &config(true)),
            ResolutionPlan::NcmThenUnm { unm_enabled: true }
        );
    }

    #[test]
    fn vip_precheck_is_skipped_when_unm_is_unavailable() {
        // Disabled by setting...
        assert_eq!(
            plan_resolution(&netease_entry(Some(1), false), &config(false)),
            ResolutionPlan::NcmThenUnm { unm_enabled: false },
            "without UNM there is nothing to pre-empt NCM with"
        );

        // ...and by having no endpoint at all.
        let mut cfg = config(true);
        cfg.unm_base_url = Some("   ".into());
        assert_eq!(
            plan_resolution(&netease_entry(Some(1), false), &cfg),
            ResolutionPlan::NcmThenUnm { unm_enabled: false }
        );
    }

    #[test]
    fn local_identities_never_hit_the_network() {
        let entry = NativeManifestEntry {
            identity: TrackIdentity::Local {
                path: "/music/a.flac".into(),
            },
            playlist_index: 3,
            title: None,
            artist: None,
            album: None,
            artwork_url: None,
            duration_ms: None,
            fee: Some(1),
            has_pc: false,
        };
        assert_eq!(
            plan_resolution(&entry, &config(true)),
            ResolutionPlan::LocalPath
        );
    }

    // ── Rule 3: trial-clip detection ─────────────────────────────

    #[test]
    fn trial_clips_are_treated_as_no_url() {
        assert_eq!(
            classify_ncm_url(Some("https://m8.music.126.net/jd-musicrep-ts/abc.mp3")),
            None,
            "a preview clip must fall through to UNM"
        );
    }

    #[test]
    fn ncm_urls_are_upgraded_to_https() {
        assert_eq!(
            classify_ncm_url(Some("http://m8.music.126.net/x/abc.mp3")).as_deref(),
            Some("https://m8.music.126.net/x/abc.mp3")
        );
    }

    #[test]
    fn empty_and_missing_ncm_urls_are_rejected() {
        assert_eq!(classify_ncm_url(None), None);
        assert_eq!(classify_ncm_url(Some("")), None);
        assert_eq!(classify_ncm_url(Some("   ")), None);
    }

    #[test]
    fn https_urls_are_left_alone() {
        assert_eq!(
            classify_ncm_url(Some("https://cdn.example.com/a.flac")).as_deref(),
            Some("https://cdn.example.com/a.flac")
        );
    }

    // ── Rule 5: kuwo proxy ───────────────────────────────────────

    #[test]
    fn kuwo_urls_prefer_the_proxy() {
        assert_eq!(
            pick_unm_url(
                Some("http://sycdn.kuwo.cn/song.mp3"),
                Some("https://proxy.example.com/song.mp3")
            )
            .as_deref(),
            Some("https://proxy.example.com/song.mp3")
        );
    }

    #[test]
    fn kuwo_without_a_proxy_still_returns_the_direct_url() {
        assert_eq!(
            pick_unm_url(Some("http://sycdn.kuwo.cn/song.mp3"), None).as_deref(),
            Some("https://sycdn.kuwo.cn/song.mp3")
        );
        assert_eq!(
            pick_unm_url(Some("http://sycdn.kuwo.cn/song.mp3"), Some("  ")).as_deref(),
            Some("https://sycdn.kuwo.cn/song.mp3"),
            "a blank proxy must not win over the real URL"
        );
    }

    #[test]
    fn non_kuwo_urls_ignore_the_proxy() {
        assert_eq!(
            pick_unm_url(
                Some("https://cdn.qq.com/song.mp3"),
                Some("https://proxy.example.com/song.mp3")
            )
            .as_deref(),
            Some("https://cdn.qq.com/song.mp3")
        );
    }

    #[test]
    fn unm_url_matching_is_case_insensitive() {
        assert_eq!(
            pick_unm_url(
                Some("https://SYCDN.KUWO.CN/song.mp3"),
                Some("https://proxy.example.com/song.mp3")
            )
            .as_deref(),
            Some("https://proxy.example.com/song.mp3")
        );
    }

    // ── URL joining ──────────────────────────────────────────────

    #[test]
    fn join_url_tolerates_trailing_and_leading_slashes() {
        for base in [
            "https://api.example.com",
            "https://api.example.com/",
            "  https://api.example.com/  ",
        ] {
            assert_eq!(
                join_url(base, "song/url/v1"),
                "https://api.example.com/song/url/v1",
                "base={base:?}"
            );
        }
        assert_eq!(
            join_url("https://api.example.com/", "/match"),
            "https://api.example.com/match"
        );
    }

    // ── Expiry ───────────────────────────────────────────────────

    #[test]
    fn local_sources_never_go_stale() {
        let source = ResolvedSource::local(
            TrackIdentity::Local {
                path: "/a.flac".into(),
            },
            "/a.flac".into(),
        );
        assert!(!source.is_stale());
    }

    #[test]
    fn fresh_remote_sources_are_not_stale() {
        let source = ResolvedSource::remote(
            TrackIdentity::Netease { id: "1".into() },
            "https://cdn/x".into(),
            SourceOrigin::Ncm,
        );
        assert!(!source.is_stale());
    }

    // ── Secrets ──────────────────────────────────────────────────

    #[test]
    fn redact_removes_credential_shaped_tokens() {
        let redacted = redact("failed for MUSIC_U=abc123 with csrf=zzz");
        assert!(!redacted.contains("abc123"), "cookie value leaked");
        assert!(!redacted.contains("zzz"), "csrf value leaked");
        assert!(redacted.contains("[redacted]"));
    }

    #[test]
    fn resolver_config_usability_requires_a_base_url() {
        assert!(config(true).is_usable());

        let mut cfg = config(true);
        cfg.ncm_base_url = None;
        assert!(!cfg.is_usable());

        cfg.ncm_base_url = Some("  ".into());
        assert!(!cfg.is_usable());
    }

    #[test]
    fn error_retryability_matches_classification() {
        assert!(ResolveError::new(ResolveErrorKind::Transient, "x").is_retryable());
        for kind in [
            ResolveErrorKind::Auth,
            ResolveErrorKind::Unavailable,
            ResolveErrorKind::LocalMissing,
            ResolveErrorKind::NotConfigured,
        ] {
            assert!(
                !ResolveError::new(kind, "x").is_retryable(),
                "{kind:?} must not be retried"
            );
        }
    }
}
