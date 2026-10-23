//! Session state the protocol layer keeps between calls.
//!
//! Three values. The first two are written to `os.tmpdir()` by the upstream
//! package; the third is ours.
//!
//! * `anonymous_token` — the anonymous device token used before login.
//! * `xeapi_public_key` — the server's current xeapi public key, fetched at
//!   runtime. Re-fetching it on every launch is a wasted round trip, which is
//!   the whole thing this project is trying to avoid.
//! * `bootstrap_version` — which bundle last completed the cold-start handshake.
//!   See [`BOOTSTRAP_VERSION`].
//!
//! Persistence is optional and injected by the caller. When no path is set the
//! store is in-memory, which is the right default for tests and for any host
//! that would rather not put protocol state on disk.
//!
//! None of these is a credential in the `MUSIC_U` sense, but they identify a
//! session, so they are written with the same discipline as the cookie: never
//! logged, never included in an error message.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Records that the cold-start handshake has already been done, and by which
/// bundle.
///
/// The `cold` test used to be "is either value missing", and in practice
/// `anonymous_token` is routinely one of them: `register_anonimous` often answers
/// without a `MUSIC_A` at all, so the token never lands and every launch re-ran
/// the whole two-request handshake to fail at the same step again. Recording the
/// *attempt* separates "we have never done this" from "we did it and Netease
/// declined half of it".
///
/// Keyed by bundle version so regenerating the protocol layer re-runs it, which
/// is when the handshake's shape could actually have changed.
const BOOTSTRAP_VERSION: &str = "bootstrap_version";

#[derive(Default)]
pub struct StateStore {
    values: HashMap<String, String>,
    dir: Option<PathBuf>,
}

/// Keys the protocol layer is allowed to persist. An allowlist rather than a
/// sanitizer: `node-fs.js` derives the key from a path basename, and this is
/// the boundary that keeps a surprising path from becoming a file write.
const PERSISTED: [&str; 3] = ["anonymous_token", "xeapi_public_key", BOOTSTRAP_VERSION];

impl StateStore {
    /// See the module-level constant of the same name.
    pub const BOOTSTRAP_VERSION: &'static str = BOOTSTRAP_VERSION;

    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Back the store with files under `dir`, loading whatever is already there.
    pub fn persisted(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref().to_path_buf();
        let mut values = HashMap::new();
        for key in PERSISTED {
            if let Ok(text) = std::fs::read_to_string(dir.join(key)) {
                values.insert(key.to_string(), text);
            }
        }
        Self {
            values,
            dir: Some(dir),
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.values.get(key).cloned()
    }

    pub fn set(&mut self, key: &str, value: String) {
        if !PERSISTED.contains(&key) {
            log::warn!(target: "ncm-core", "refusing to store unknown state key {key:?}");
            return;
        }
        if let Some(dir) = &self.dir {
            if std::fs::create_dir_all(dir).is_ok() {
                // A failed write is not fatal: the value stays in memory and
                // the next launch just re-fetches it.
                if let Err(e) = std::fs::write(dir.join(key), &value) {
                    log::warn!(target: "ncm-core", "could not persist {key}: {}", e.kind());
                }
            }
        }
        self.values.insert(key.to_string(), value);
    }
}
