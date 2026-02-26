use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Global one-shot payload cache for inter-window data passing.
/// A creating window can store a payload before opening a new window,
/// and the new window takes (consumes) it on initialization.
static PAYLOAD_CACHE: LazyLock<Mutex<HashMap<String, Value>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct PayloadCache;

impl PayloadCache {
    /// Store a payload for a window label. Overwrites any existing payload.
    pub fn set(label: &str, value: Value) {
        PAYLOAD_CACHE.lock().insert(label.to_string(), value);
    }

    /// Take (consume) the payload for a window label. Returns None if no payload exists.
    pub fn take(label: &str) -> Option<Value> {
        PAYLOAD_CACHE.lock().remove(label)
    }

    /// Peek at the payload without consuming it.
    pub fn peek(label: &str) -> Option<Value> {
        PAYLOAD_CACHE.lock().get(label).cloned()
    }

    /// Clear all cached payloads.
    pub fn clear() {
        PAYLOAD_CACHE.lock().clear();
    }
}
