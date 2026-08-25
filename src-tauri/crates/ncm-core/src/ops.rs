//! Host ops backing the JS shims in `js/shim/`.
//!
//! Everything the protocol layer would have asked Node for lands here: hashes,
//! AES, X25519, RSA, gzip, randomness, HTTP and the small amount of persisted
//! session state. The JS side owns *protocol* logic — which fields go in which
//! envelope — and nothing else.
//!
//! Two rules hold throughout:
//!
//! * **No panics across the FFI boundary.** Every failure becomes a JS
//!   exception with a message naming the op, because a panic unwinding through
//!   QuickJS is undefined behaviour.
//! * **No secrets in messages.** Op errors are surfaced to JS and can reach a
//!   log. They carry the operation and the shape of the failure, never key
//!   material, cookies or request bodies.
//!
//! Ops are free functions generic over `'js` rather than closures: a closure
//! written `|ctx: Ctx<'_>, v: Value<'_>|` gets two *independent* elided
//! lifetimes, which will not unify with the single `'js` the binding requires.
//! Per-instance state (the HTTP client, the state store) therefore lives in
//! context userdata instead of being captured.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyInit, KeyIvInit};
use aes_gcm::aead::Aead;
use aes_gcm::{Aes128Gcm, Aes256Gcm, Nonce};
use hmac::{Hmac, Mac};
use md5::Md5;
use rquickjs::function::Func;
use rquickjs::{Ctx, Exception, IntoJs, JsLifetime, Object, Promise, Result as JsResult, TypedArray, Value};
use sha2::{Digest, Sha256};

use crate::http::HttpClient;
use crate::state::StateStore;

type HmacSha256 = Hmac<Sha256>;

type Aes128EcbEnc = ecb::Encryptor<aes::Aes128>;
type Aes128EcbDec = ecb::Decryptor<aes::Aes128>;
type Aes256EcbEnc = ecb::Encryptor<aes::Aes256>;
type Aes256EcbDec = ecb::Decryptor<aes::Aes256>;
type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

/// Per-isolate host state, reachable from every op through `Ctx::userdata`.
pub(crate) struct HostState {
    pub http: Arc<HttpClient>,
    pub state: Arc<Mutex<StateStore>>,
}

// SAFETY: `HostState` holds no JS values, so it carries no `'js` lifetime and
// re-labelling it is a no-op.
unsafe impl<'js> JsLifetime<'js> for HostState {
    type Changed<'to> = HostState;
}

// ── helpers ──────────────────────────────────────────────────────

fn throw<T>(ctx: &Ctx<'_>, msg: impl AsRef<str>) -> JsResult<T> {
    Err(Exception::throw_message(ctx, msg.as_ref()))
}

fn bytes_out<'js>(ctx: &Ctx<'js>, data: &[u8]) -> JsResult<TypedArray<'js, u8>> {
    TypedArray::new_copy(ctx.clone(), data)
}

/// Read a `Uint8Array` argument. Accepts any typed-array view.
fn bytes_in<'js>(ctx: &Ctx<'js>, v: &Value<'js>, op: &str) -> JsResult<Vec<u8>> {
    match TypedArray::<u8>::from_value(v.clone()) {
        Ok(ta) => match ta.as_bytes() {
            Some(b) => Ok(b.to_vec()),
            // Detached happens when the backing ArrayBuffer was transferred.
            None => throw(ctx, format!("{op}: byte argument is detached")),
        },
        Err(_) => throw(ctx, format!("{op}: expected a Uint8Array argument")),
    }
}

// ── text ─────────────────────────────────────────────────────────

fn op_utf8_encode<'js>(ctx: Ctx<'js>, s: String) -> JsResult<TypedArray<'js, u8>> {
    bytes_out(&ctx, s.as_bytes())
}

fn op_utf8_decode<'js>(ctx: Ctx<'js>, v: Value<'js>) -> JsResult<String> {
    let b = bytes_in(&ctx, &v, "utf8Decode")?;
    // Lossy on purpose: Netease occasionally returns lone surrogates in
    // user-generated fields, and a hard failure there would take down an
    // otherwise good response.
    Ok(String::from_utf8_lossy(&b).into_owned())
}

// ── digests ──────────────────────────────────────────────────────

fn op_md5<'js>(ctx: Ctx<'js>, v: Value<'js>) -> JsResult<TypedArray<'js, u8>> {
    let b = bytes_in(&ctx, &v, "md5")?;
    bytes_out(&ctx, &Md5::digest(&b))
}

fn op_sha256<'js>(ctx: Ctx<'js>, v: Value<'js>) -> JsResult<TypedArray<'js, u8>> {
    let b = bytes_in(&ctx, &v, "sha256")?;
    bytes_out(&ctx, &Sha256::digest(&b))
}

fn op_hmac_sha256<'js>(
    ctx: Ctx<'js>,
    key: Value<'js>,
    data: Value<'js>,
) -> JsResult<TypedArray<'js, u8>> {
    let k = bytes_in(&ctx, &key, "hmacSha256")?;
    let d = bytes_in(&ctx, &data, "hmacSha256")?;
    let mut mac = match <HmacSha256 as Mac>::new_from_slice(&k) {
        Ok(m) => m,
        Err(_) => return throw(&ctx, "hmacSha256: invalid key length"),
    };
    mac.update(&d);
    bytes_out(&ctx, &mac.finalize().into_bytes())
}

// ── AES ──────────────────────────────────────────────────────────
//
// Node applies PKCS#7 padding by default and the protocol layer never turns it
// off, so these pad and unpad. Both `aes-128` and `aes-256` are reachable:
// xeapi runs a 16-byte dynamic key and a 32-byte static key through the same
// helper.

fn op_aes_ecb_encrypt<'js>(
    ctx: Ctx<'js>,
    key: Value<'js>,
    data: Value<'js>,
) -> JsResult<TypedArray<'js, u8>> {
    let k = bytes_in(&ctx, &key, "aesEcbEncrypt")?;
    let d = bytes_in(&ctx, &data, "aesEcbEncrypt")?;
    let out = match k.len() {
        16 => Aes128EcbEnc::new(k.as_slice().into()).encrypt_padded_vec_mut::<Pkcs7>(&d),
        32 => Aes256EcbEnc::new(k.as_slice().into()).encrypt_padded_vec_mut::<Pkcs7>(&d),
        n => return throw(&ctx, format!("aesEcbEncrypt: unsupported key length {n}")),
    };
    bytes_out(&ctx, &out)
}

fn op_aes_ecb_decrypt<'js>(
    ctx: Ctx<'js>,
    key: Value<'js>,
    data: Value<'js>,
) -> JsResult<TypedArray<'js, u8>> {
    let k = bytes_in(&ctx, &key, "aesEcbDecrypt")?;
    let d = bytes_in(&ctx, &data, "aesEcbDecrypt")?;
    let out = match k.len() {
        16 => Aes128EcbDec::new(k.as_slice().into()).decrypt_padded_vec_mut::<Pkcs7>(&d),
        32 => Aes256EcbDec::new(k.as_slice().into()).decrypt_padded_vec_mut::<Pkcs7>(&d),
        n => return throw(&ctx, format!("aesEcbDecrypt: unsupported key length {n}")),
    };
    match out {
        Ok(v) => bytes_out(&ctx, &v),
        // Bad padding means the wrong key or a truncated body — both are real
        // failures the caller must see, not something to paper over.
        Err(_) => throw(&ctx, "aesEcbDecrypt: invalid padding"),
    }
}

fn op_aes_cbc_encrypt<'js>(
    ctx: Ctx<'js>,
    key: Value<'js>,
    iv: Value<'js>,
    data: Value<'js>,
) -> JsResult<TypedArray<'js, u8>> {
    let k = bytes_in(&ctx, &key, "aesCbcEncrypt")?;
    let i = bytes_in(&ctx, &iv, "aesCbcEncrypt")?;
    let d = bytes_in(&ctx, &data, "aesCbcEncrypt")?;
    if k.len() != 16 || i.len() != 16 {
        return throw(&ctx, "aesCbcEncrypt: expected a 16-byte key and IV");
    }
    let out = Aes128CbcEnc::new(k.as_slice().into(), i.as_slice().into())
        .encrypt_padded_vec_mut::<Pkcs7>(&d);
    bytes_out(&ctx, &out)
}

fn op_aes_cbc_decrypt<'js>(
    ctx: Ctx<'js>,
    key: Value<'js>,
    iv: Value<'js>,
    data: Value<'js>,
) -> JsResult<TypedArray<'js, u8>> {
    let k = bytes_in(&ctx, &key, "aesCbcDecrypt")?;
    let i = bytes_in(&ctx, &iv, "aesCbcDecrypt")?;
    let d = bytes_in(&ctx, &data, "aesCbcDecrypt")?;
    if k.len() != 16 || i.len() != 16 {
        return throw(&ctx, "aesCbcDecrypt: expected a 16-byte key and IV");
    }
    match Aes128CbcDec::new(k.as_slice().into(), i.as_slice().into())
        .decrypt_padded_vec_mut::<Pkcs7>(&d)
    {
        Ok(v) => bytes_out(&ctx, &v),
        Err(_) => throw(&ctx, "aesCbcDecrypt: invalid padding"),
    }
}

fn op_aes_gcm_encrypt<'js>(
    ctx: Ctx<'js>,
    key: Value<'js>,
    iv: Value<'js>,
    data: Value<'js>,
) -> JsResult<Object<'js>> {
    let k = bytes_in(&ctx, &key, "aesGcmEncrypt")?;
    let i = bytes_in(&ctx, &iv, "aesGcmEncrypt")?;
    let d = bytes_in(&ctx, &data, "aesGcmEncrypt")?;
    if i.len() != 12 {
        return throw(&ctx, "aesGcmEncrypt: expected a 12-byte IV");
    }
    let sealed = match k.len() {
        16 => Aes128Gcm::new(k.as_slice().into()).encrypt(Nonce::from_slice(&i), d.as_ref()),
        32 => Aes256Gcm::new(k.as_slice().into()).encrypt(Nonce::from_slice(&i), d.as_ref()),
        n => return throw(&ctx, format!("aesGcmEncrypt: unsupported key length {n}")),
    };
    let sealed = match sealed {
        Ok(v) => v,
        Err(_) => return throw(&ctx, "aesGcmEncrypt: encryption failed"),
    };
    // The AEAD crate appends the 16-byte tag; Node exposes it separately via
    // getAuthTag(), and util/crypto.js concatenates it in its own order. Split
    // so the JS side stays in control of the layout.
    let split = sealed.len() - 16;
    let out = Object::new(ctx.clone())?;
    out.set("ciphertext", bytes_out(&ctx, &sealed[..split])?)?;
    out.set("tag", bytes_out(&ctx, &sealed[split..])?)?;
    Ok(out)
}

fn op_aes_gcm_decrypt<'js>(
    ctx: Ctx<'js>,
    key: Value<'js>,
    iv: Value<'js>,
    data: Value<'js>,
    tag: Value<'js>,
) -> JsResult<TypedArray<'js, u8>> {
    let k = bytes_in(&ctx, &key, "aesGcmDecrypt")?;
    let i = bytes_in(&ctx, &iv, "aesGcmDecrypt")?;
    let mut d = bytes_in(&ctx, &data, "aesGcmDecrypt")?;
    let t = bytes_in(&ctx, &tag, "aesGcmDecrypt")?;
    if i.len() != 12 {
        return throw(&ctx, "aesGcmDecrypt: expected a 12-byte IV");
    }
    d.extend_from_slice(&t);
    let opened = match k.len() {
        16 => Aes128Gcm::new(k.as_slice().into()).decrypt(Nonce::from_slice(&i), d.as_ref()),
        32 => Aes256Gcm::new(k.as_slice().into()).decrypt(Nonce::from_slice(&i), d.as_ref()),
        n => return throw(&ctx, format!("aesGcmDecrypt: unsupported key length {n}")),
    };
    match opened {
        Ok(v) => bytes_out(&ctx, &v),
        Err(_) => throw(&ctx, "aesGcmDecrypt: authentication failed"),
    }
}

// ── X25519 ───────────────────────────────────────────────────────

fn op_x25519_generate<'js>(ctx: Ctx<'js>) -> JsResult<Object<'js>> {
    let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
    let public = x25519_dalek::PublicKey::from(&secret);
    let out = Object::new(ctx.clone())?;
    out.set("publicKey", bytes_out(&ctx, public.as_bytes())?)?;
    out.set("privateKey", bytes_out(&ctx, &secret.to_bytes())?)?;
    Ok(out)
}

fn op_x25519_diffie<'js>(
    ctx: Ctx<'js>,
    priv_key: Value<'js>,
    pub_key: Value<'js>,
) -> JsResult<TypedArray<'js, u8>> {
    let sk = bytes_in(&ctx, &priv_key, "x25519Diffie")?;
    let pk = bytes_in(&ctx, &pub_key, "x25519Diffie")?;
    if sk.len() != 32 || pk.len() != 32 {
        return throw(&ctx, "x25519Diffie: expected 32-byte keys");
    }
    let mut sk_arr = [0u8; 32];
    let mut pk_arr = [0u8; 32];
    sk_arr.copy_from_slice(&sk);
    pk_arr.copy_from_slice(&pk);
    let shared = x25519_dalek::StaticSecret::from(sk_arr)
        .diffie_hellman(&x25519_dalek::PublicKey::from(pk_arr));
    bytes_out(&ctx, shared.as_bytes())
}

// ── RSA ──────────────────────────────────────────────────────────
//
// Textbook RSA: m^e mod n, zero-padded to the modulus width. This is what
// forge's 'NONE' scheme does and what weapi expects. It is deliberately *not* a
// general-purpose RSA op — no padding scheme is applied — so it must never be
// repurposed beyond this fixed protocol handshake.

fn op_rsa_raw_pem<'js>(ctx: Ctx<'js>, pem: String, data: Value<'js>) -> JsResult<TypedArray<'js, u8>> {
    use rsa::traits::PublicKeyParts;

    let msg = bytes_in(&ctx, &data, "rsaRawPem")?;
    let key = match parse_public_key(&pem) {
        Some(k) => k,
        None => return throw(&ctx, "rsaRawPem: could not parse the public key"),
    };
    let k = key.size();
    if msg.len() > k {
        return throw(&ctx, "rsaRawPem: message longer than the modulus");
    }
    let c = rsa::BigUint::from_bytes_be(&msg).modpow(key.e(), key.n());
    // Left-pad to the modulus width: the protocol hexes a fixed-width value and
    // a short encoding is rejected.
    let raw = c.to_bytes_be();
    let mut out = vec![0u8; k];
    out[k - raw.len()..].copy_from_slice(&raw);
    bytes_out(&ctx, &out)
}

/// Parse an RSA public key, tolerating non-RFC-7468 armor.
///
/// The weapi key is embedded upstream as a single 216-character base64 line.
/// `pem-rfc7468` — what `from_public_key_pem` uses — enforces the 64-character
/// line limit and rejects it outright, so strict parsing is only the first
/// attempt; the fallback strips the armor and decodes the DER directly.
fn parse_public_key(pem: &str) -> Option<rsa::RsaPublicKey> {
    use base64::Engine as _;
    use rsa::pkcs8::DecodePublicKey;

    if let Ok(k) = rsa::RsaPublicKey::from_public_key_pem(pem.trim()) {
        return Some(k);
    }
    let body: String = pem
        .lines()
        .filter(|l| !l.trim_start().starts_with("-----"))
        .flat_map(|l| l.chars().filter(|c| !c.is_whitespace()))
        .collect();
    let der = base64::engine::general_purpose::STANDARD.decode(body).ok()?;
    rsa::RsaPublicKey::from_public_key_der(&der).ok()
}

// ── compression ──────────────────────────────────────────────────

fn op_gunzip<'js>(ctx: Ctx<'js>, v: Value<'js>) -> JsResult<TypedArray<'js, u8>> {
    use std::io::Read;
    let b = bytes_in(&ctx, &v, "gunzip")?;
    let mut out = Vec::new();
    match flate2::read::GzDecoder::new(&b[..]).read_to_end(&mut out) {
        Ok(_) => bytes_out(&ctx, &out),
        Err(_) => throw(&ctx, "gunzip: malformed gzip stream"),
    }
}

fn op_gzip<'js>(ctx: Ctx<'js>, v: Value<'js>) -> JsResult<TypedArray<'js, u8>> {
    use std::io::Write;
    let b = bytes_in(&ctx, &v, "gzip")?;
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    if e.write_all(&b).is_err() {
        return throw(&ctx, "gzip: compression failed");
    }
    match e.finish() {
        Ok(out) => bytes_out(&ctx, &out),
        Err(_) => throw(&ctx, "gzip: compression failed"),
    }
}

fn op_inflate<'js>(ctx: Ctx<'js>, v: Value<'js>) -> JsResult<TypedArray<'js, u8>> {
    use std::io::Read;
    let b = bytes_in(&ctx, &v, "inflate")?;
    let mut out = Vec::new();
    match flate2::read::ZlibDecoder::new(&b[..]).read_to_end(&mut out) {
        Ok(_) => bytes_out(&ctx, &out),
        Err(_) => throw(&ctx, "inflate: malformed zlib stream"),
    }
}

fn op_deflate<'js>(ctx: Ctx<'js>, v: Value<'js>) -> JsResult<TypedArray<'js, u8>> {
    use std::io::Write;
    let b = bytes_in(&ctx, &v, "deflate")?;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    if e.write_all(&b).is_err() {
        return throw(&ctx, "deflate: compression failed");
    }
    match e.finish() {
        Ok(out) => bytes_out(&ctx, &out),
        Err(_) => throw(&ctx, "deflate: compression failed"),
    }
}

// ── randomness ───────────────────────────────────────────────────

fn op_random_bytes<'js>(ctx: Ctx<'js>, n: usize) -> JsResult<TypedArray<'js, u8>> {
    // Bounded: a bad length from JS should not become a huge allocation.
    if n > 1 << 20 {
        return throw(&ctx, "randomBytes: refusing an absurd length");
    }
    let mut buf = vec![0u8; n];
    if getrandom::getrandom(&mut buf).is_err() {
        return throw(&ctx, "randomBytes: the system RNG failed");
    }
    bytes_out(&ctx, &buf)
}

fn op_random_uuid(ctx: Ctx<'_>) -> JsResult<String> {
    let mut b = [0u8; 16];
    if getrandom::getrandom(&mut b).is_err() {
        return throw(&ctx, "randomUuid: the system RNG failed");
    }
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // variant 1
    let h = |s: &[u8]| s.iter().map(|x| format!("{x:02x}")).collect::<String>();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        h(&b[0..4]),
        h(&b[4..6]),
        h(&b[6..8]),
        h(&b[8..10]),
        h(&b[10..16])
    ))
}

// ── persisted session state ──────────────────────────────────────

fn op_state_read(ctx: Ctx<'_>, key: String) -> JsResult<Option<String>> {
    let Some(host) = ctx.userdata::<HostState>() else {
        return throw(&ctx, "stateRead: host state is unavailable");
    };
    Ok(host.state.lock().ok().and_then(|s| s.get(&key)))
}

fn op_state_write(ctx: Ctx<'_>, key: String, value: String) -> JsResult<()> {
    let Some(host) = ctx.userdata::<HostState>() else {
        return throw(&ctx, "stateWrite: host state is unavailable");
    };
    if let Ok(mut s) = host.state.lock() {
        s.set(&key, value);
    }
    Ok(())
}

// ── HTTP ─────────────────────────────────────────────────────────
//
// Returns a JS promise rather than blocking. The endpoint modules already
// `await` their request, so nothing above changes — but the isolate is now free
// to run other jobs while a response is outstanding, which is what lets a page
// load fan out a dozen calls instead of serializing them.

/// What the HTTP future resolves to.
///
/// Modelled as a value with an `IntoJs` impl rather than a `Result` so the
/// failure arm can throw a message naming the op: `Promise::wrap_future`
/// rejects with whatever `into_js` throws.
enum HttpOutcome {
    Response(crate::http::HttpResponse),
    Failed(String),
}

impl<'js> IntoJs<'js> for HttpOutcome {
    fn into_js(self, ctx: &Ctx<'js>) -> JsResult<Value<'js>> {
        let resp = match self {
            HttpOutcome::Response(r) => r,
            // Already redacted by the client; safe to surface and to log.
            HttpOutcome::Failed(msg) => {
                return Err(Exception::throw_message(ctx, &format!("httpRequest: {msg}")))
            }
        };

        let out = Object::new(ctx.clone())?;
        out.set("status", resp.status)?;
        out.set("statusText", resp.status_text)?;

        let hdrs = Object::new(ctx.clone())?;
        for (k, v) in &resp.headers {
            // set-cookie is the one header the protocol layer expects as an
            // array; everything else is joined into a single string, as axios
            // does.
            if k == "set-cookie" {
                hdrs.set(k.as_str(), v.clone())?;
            } else {
                hdrs.set(k.as_str(), v.join(", "))?;
            }
        }
        out.set("headers", hdrs)?;
        out.set("body", bytes_out(ctx, &resp.body)?)?;
        out.into_js(ctx)
    }
}

fn op_http_request<'js>(ctx: Ctx<'js>, cfg: Object<'js>) -> JsResult<Promise<'js>> {
    // Everything is read out of JS *before* the await: JS values are tied to
    // the context lock and must not be held across one.
    let client = {
        let Some(host) = ctx.userdata::<HostState>() else {
            return throw(&ctx, "httpRequest: host state is unavailable");
        };
        host.http.clone()
    };

    let method: String = cfg.get("method").unwrap_or_else(|_| "GET".into());
    let url: String = match cfg.get("url") {
        Ok(u) => u,
        Err(_) => return throw(&ctx, "httpRequest: missing url"),
    };
    let timeout_ms: u64 = cfg.get("timeoutMs").unwrap_or(0);

    let mut headers: Vec<(String, String)> = Vec::new();
    if let Ok(h) = cfg.get::<_, Object>("headers") {
        for entry in h.props::<String, String>().flatten() {
            headers.push(entry);
        }
    }

    let body = match cfg.get::<_, Value>("body") {
        Ok(v) if !v.is_null() && !v.is_undefined() => Some(bytes_in(&ctx, &v, "httpRequest")?),
        _ => None,
    };

    let timeout = (timeout_ms > 0).then(|| Duration::from_millis(timeout_ms));

    Promise::wrap_future(&ctx, async move {
        match client
            .request(&method, &url, &headers, body, timeout)
            .await
        {
            Ok(r) => HttpOutcome::Response(r),
            Err(e) => HttpOutcome::Failed(e.to_string()),
        }
    })
}

// ── logging ──────────────────────────────────────────────────────

fn op_log(level: String, msg: String) {
    let msg = crate::redact(&msg);
    match level.as_str() {
        "error" => log::error!(target: "ncm-core", "{msg}"),
        "warn" => log::warn!(target: "ncm-core", "{msg}"),
        "debug" => log::debug!(target: "ncm-core", "{msg}"),
        _ => log::info!(target: "ncm-core", "{msg}"),
    }
}

// ── Registration ─────────────────────────────────────────────────

/// Install `globalThis.__ncm_host` into `ctx`.
pub(crate) fn install(
    ctx: &Ctx<'_>,
    http: Arc<HttpClient>,
    state: Arc<Mutex<StateStore>>,
) -> JsResult<()> {
    if ctx.store_userdata(HostState { http, state }).is_err() {
        return throw(ctx, "ncm-core: host state is already borrowed");
    }

    let host = Object::new(ctx.clone())?;

    host.set("utf8Encode", Func::from(op_utf8_encode))?;
    host.set("utf8Decode", Func::from(op_utf8_decode))?;

    host.set("md5", Func::from(op_md5))?;
    host.set("sha256", Func::from(op_sha256))?;
    host.set("hmacSha256", Func::from(op_hmac_sha256))?;

    host.set("aesEcbEncrypt", Func::from(op_aes_ecb_encrypt))?;
    host.set("aesEcbDecrypt", Func::from(op_aes_ecb_decrypt))?;
    host.set("aesCbcEncrypt", Func::from(op_aes_cbc_encrypt))?;
    host.set("aesCbcDecrypt", Func::from(op_aes_cbc_decrypt))?;
    host.set("aesGcmEncrypt", Func::from(op_aes_gcm_encrypt))?;
    host.set("aesGcmDecrypt", Func::from(op_aes_gcm_decrypt))?;

    host.set("x25519Generate", Func::from(op_x25519_generate))?;
    host.set("x25519Diffie", Func::from(op_x25519_diffie))?;
    host.set("rsaRawPem", Func::from(op_rsa_raw_pem))?;

    host.set("gunzip", Func::from(op_gunzip))?;
    host.set("gzip", Func::from(op_gzip))?;
    host.set("inflate", Func::from(op_inflate))?;
    host.set("deflate", Func::from(op_deflate))?;

    host.set("randomBytes", Func::from(op_random_bytes))?;
    host.set("randomUuid", Func::from(op_random_uuid))?;

    host.set("stateRead", Func::from(op_state_read))?;
    host.set("stateWrite", Func::from(op_state_write))?;

    host.set("httpRequest", Func::from(op_http_request))?;
    host.set("log", Func::from(op_log))?;

    ctx.globals().set("__ncm_host", host)?;
    Ok(())
}
