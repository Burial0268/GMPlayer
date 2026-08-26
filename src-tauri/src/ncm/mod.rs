//! Tauri bridge for the in-process NCM protocol layer.
//!
//! [`ncm_core::NcmCore`] is `Send + Sync` and internally async, so it lives
//! directly in Tauri state — no worker thread, no channel. JS execution inside
//! it is still serialized behind the QuickJS runtime lock, but the HTTP op
//! returns a real promise, so concurrent `ncm_request` calls overlap their
//! network waits. That matters: a page load fans out a dozen endpoint calls,
//! and serializing them would give back far more than the hop this removes.
//!
//! The isolate is created lazily on first use. Construction performs a one-time
//! handshake with Netease when its persisted state is cold, and there is no
//! reason to pay that at startup for a session that never leaves the remote
//! transport.
//!
//! See `docs/native-ncm-api-embedding-plan.md`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ncm_core::NcmCore;
use serde::Serialize;
use tauri::Manager;
use tokio::sync::OnceCell;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolInfo {
    /// Upstream `@neteasecloudmusicapienhanced/api` version the bundle was
    /// built from.
    pub version: String,
    pub endpoint_count: usize,
}

pub struct NcmState {
    /// `OnceCell` rather than `OnceLock`: building the isolate is async, and
    /// it also guarantees the cold-start handshake happens exactly once even
    /// if several requests race on startup.
    core: OnceCell<Arc<NcmCore>>,
    state_dir: PathBuf,
}

impl NcmState {
    pub fn new(app: &tauri::AppHandle) -> Self {
        // `anonymous_token` and the cached xeapi public key live here. Both are
        // session-identifying, so they belong in app data next to the rest of
        // the app's private state — not in a temp dir, which is where the
        // upstream package puts them.
        let state_dir = app
            .path()
            .app_data_dir()
            .map(|d| d.join("ncm"))
            .unwrap_or_else(|_| std::env::temp_dir().join("gmplayer-ncm"));
        Self {
            core: OnceCell::new(),
            state_dir,
        }
    }

    pub(crate) async fn core(&self) -> Result<&Arc<NcmCore>, String> {        self.core
            .get_or_try_init(|| async {
                let core = tokio::time::timeout(
                    INIT_TIMEOUT,
                    NcmCore::new(Some(self.state_dir.clone())),
                )
                .await
                .map_err(|_| {
                    let msg = format!("the embedded protocol layer did not start within {INIT_TIMEOUT:?}");
                    log::error!(target: "ncm", "{msg}");
                    msg
                })?
                .map_err(|e| {
                    let msg = e.to_string();
                    log::error!(target: "ncm", "{msg}");
                    msg
                })?;
                match (core.protocol_version().await, core.endpoints().await) {
                    (Ok(v), Ok(e)) => {
                        log::info!(target: "ncm", "protocol layer ready: upstream {v}, {} endpoints", e.len())
                    }
                    _ => log::warn!(target: "ncm", "protocol layer ready but not introspectable"),
                }
                Ok(Arc::new(core))
            })
            .await
    }

    /// The isolate, only if it is already built.
    ///
    /// For hints, which must never be the reason the cold-start handshake runs:
    /// paying ~500 ms and two network requests to act on a guess is the opposite
    /// of what a hint is for. `warm` builds it at startup, so in practice this is
    /// `Some` by the time any hint arrives.
    pub(crate) fn built(&self) -> Option<&Arc<NcmCore>> {
        self.core.get()
    }
}

/// Give the audio backend's playback resolver access to the in-process
/// protocol layer.
///
/// The backend cannot depend on `ncm-core` directly — it also builds for
/// `wasm32-unknown-unknown`, where a QuickJS isolate has no place — so the
/// dependency is inverted into a callback installed once at startup.
///
/// Blocking is safe here despite the async core: `resolve_blocking` is invoked
/// from `tokio::task::spawn_blocking`, which is not an async context, so
/// `Handle::block_on` will not panic and no async worker is parked. It must
/// never be called from the player loop or anywhere near an audio callback —
/// the same rule that already governs `resolve_blocking`.
pub fn install_resolver_hook(app: &tauri::AppHandle) {
    let app = app.clone();
    let installed = gmplayer_audio_backend::install_ncm_call_hook(Arc::new(
        move |endpoint: &str, query: &str| -> Result<String, String> {
            let rt = tokio::runtime::Handle::try_current()
                .map_err(|_| "no tokio runtime on the resolver thread".to_string())?;
            let app = app.clone();
            let endpoint = endpoint.to_owned();
            let query = query.to_owned();
            rt.block_on(async move {
                let state = app.state::<NcmState>();
                let core = state.core().await?;
                // Bounded for the same reason as `ncm_request`: the planner
                // resolves one track ahead, and a hang here stalls the hand-off
                // to the next track with no error anyone can see.
                tokio::time::timeout(CALL_TIMEOUT, core.call(&endpoint, &query))
                    .await
                    .map_err(|_| {
                        log::error!(
                            target: "ncm",
                            "resolver call to {endpoint} did not settle within {CALL_TIMEOUT:?}"
                        );
                        format!("the embedded protocol layer did not answer within {CALL_TIMEOUT:?}")
                    })?
                    .map_err(|e| e.to_string())
            })
        },
    ));
    if !installed {
        log::warn!(target: "ncm", "resolver hook was already installed");
    }
}

/// Build the isolate now, in the background, so the first API call does not pay
/// for it.
///
/// Measured cold start is ~440 ms: ~45 ms to evaluate the 228 KB protocol bundle
/// (twice — bootstrap runs in a throwaway context, see `NcmCore::build`) plus
/// two network round trips for the `generateConfig` handshake. Built lazily that
/// is charged to whichever request happens to be first, and the first thing the
/// app does is fan a dozen calls out for the home page — so all of them queue
/// behind it and the whole first screen arrives late.
///
/// Deliberately fire-and-forget. A failure here is not fatal and is already
/// logged by `core()`; the next real request retries, and `ncm_request` falls
/// back to the remote transport if the isolate genuinely cannot start.
///
/// Runs even when the user has selected the `remote` transport. That costs one
/// background handshake at launch and is worth it: `local` is the default, the
/// frontend cannot tell us which it picked until Pinia has hydrated, and
/// `ncm_protocol_info` — which the frontend uses to decide whether `local` is
/// available at all — builds the isolate anyway.
pub fn warm(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
        if app.state::<NcmState>().core().await.is_ok() {
            log::info!(
                target: "ncm",
                "protocol layer warmed in {:?}, ahead of the first request",
                started.elapsed()
            );
        }
    });
}

/// Hint that an endpoint's answer will probably be wanted shortly.
///
/// Fire-and-forget: returns as soon as the hint is recorded, never reports a
/// failure, and is safe to call speculatively and often. A hint that turns out to
/// be wrong costs one request during a moment when nobody was waiting; a hint
/// that cannot be served right now is dropped rather than queued.
///
/// The frontend sends these from `src/utils/ncmPrefetch.ts`. Kept deliberately
/// dumb on this side — *what* is worth guessing is a question about how the UI
/// behaves, and belongs where that is known.
///
/// Errors are not returned: `Ok(())` always. A hint is not a request, and a
/// caller that had to handle its failure would be doing more work than the hint
/// saves. (The `Result` is Tauri's requirement for an async command borrowing
/// state, not a channel for anything to be reported through.)
#[tauri::command]
pub async fn ncm_prefetch(
    state: tauri::State<'_, NcmState>,
    endpoint: String,
    query: String,
) -> Result<(), String> {
    // Only warms an isolate that is already built. Building one on a hint would
    // pay the whole cold-start handshake for a guess, which is the opposite of
    // the point.
    if let Some(core) = state.built() {
        core.prefetch(&endpoint, &query);
    }
    Ok(())
}

/// Ceiling on a single endpoint call, measured from the IPC boundary.
///
/// Sits above the HTTP client's own 20s budget so a slow network still reports
/// its real error rather than this. The point is the isolate itself: if it ever
/// wedges — a promise that never settles, a lock held by a call that cannot
/// finish — the frontend must get an error it can act on. Without this the UI
/// simply stays in a loading state forever, which is both the worst failure
/// mode to debug and the one the transport fallback cannot see.
const CALL_TIMEOUT: Duration = Duration::from_secs(25);

/// Ceiling on building the isolate, which includes the cold-start handshake.
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Invoke an NCM endpoint in-process.
///
/// `endpoint` is an upstream module basename (`song_url_v1`); `query` is the
/// request parameters as a JSON object string, including `cookie` when the call
/// is authenticated.
///
/// Returns the raw JSON bytes of `{ ok, status, body, cookie }`. Raw rather
/// than a `String` so the response crosses IPC once instead of being
/// re-serialized into the invoke envelope — `playlist/track/all` reaches
/// megabytes.
///
/// A non-200 upstream response is a *successful* call with `ok: false`; `Err`
/// is reserved for the isolate itself failing.
#[tauri::command]
pub async fn ncm_request(
    state: tauri::State<'_, NcmState>,
    endpoint: String,
    query: String,
) -> Result<tauri::ipc::Response, String> {
    let core = state.core().await?;
    let json = tokio::time::timeout(CALL_TIMEOUT, core.call(&endpoint, &query))
        .await
        .map_err(|_| {
            // Deliberately loud: this means the isolate did not answer, which
            // is a bug here rather than a network condition.
            log::error!(target: "ncm", "endpoint {endpoint} did not settle within {CALL_TIMEOUT:?}");
            format!("the embedded protocol layer did not answer within {CALL_TIMEOUT:?}")
        })?
        .map_err(|e| e.to_string())?;
    Ok(tauri::ipc::Response::new(json.into_bytes()))
}

/// Whether the in-process transport is usable, and what it is running.
///
/// Building the isolate is what actually answers this, so the first call pays
/// the cold-start handshake. The frontend uses it to decide whether `local` is
/// an option at all: a build where the isolate cannot start must fall back to
/// the remote API rather than failing every request.
#[tauri::command]
pub async fn ncm_protocol_info(
    state: tauri::State<'_, NcmState>,
) -> Result<ProtocolInfo, String> {
    let core = state.core().await?;
    let version = core.protocol_version().await.map_err(|e| e.to_string())?;
    let endpoints = core.endpoints().await.map_err(|e| e.to_string())?;
    Ok(ProtocolInfo {
        version,
        endpoint_count: endpoints.len(),
    })
}
