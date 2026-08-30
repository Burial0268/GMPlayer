# ncm-core

In-process NeteaseCloudMusic protocol layer: a QuickJS isolate hosting the
upstream endpoint scripts over Rust crypto, compression and HTTP primitives.

Replaces the network hop to a deployed NeteaseCloudMusicApi instance. Measured
saving is ~38–48 ms per call; see `docs/native-ncm-api-embedding-plan.md` for
the evaluation, the measurement method, and why this is QuickJS rather than
`deno_core`.

## Layout

```
js/ncm-protocol.js   generated bundle, committed — `cargo build` never needs Node
js/shim/             the Node surface the bundle expects, routed to host ops
src/ops.rs           every host op (crypto, gzip, HTTP, state, logging)
src/http.rs          reqwest client, retries, per-call budget, timing log
src/throttle.rs      per-host admission control (AIMD concurrency + spacing)
src/cache.rs         two-tier response cache, stale-while-revalidate
src/disk.rs          the persistent tier — survives a restart
src/inflight.rs      share one request between identical concurrent callers
src/batch.rs         merge calls that differ only by id into one request
src/state.rs         anonymous_token / xeapi_public_key / bootstrap marker
tests/               offline (4) + live (11, `--ignored`)
```

The split is deliberate: **JS owns protocol logic** — which fields go in which
weapi/eapi/xeapi envelope — because that is what changes upstream. **Rust owns
the primitives**, which have not changed in a decade. Upgrading to a new
upstream release should mean regenerating the bundle and nothing else.

## Regenerating the bundle

```bash
pnpm build:ncm-protocol      # rebuild from the pinned tarball
pnpm check:ncm-protocol      # CI: fail if the committed output is stale
```

The upstream version and its npm integrity hash are pinned at the top of
`scripts/build-ncm-protocol.mjs`. Bump both together.

## Using it

```rust
// One dedicated worker thread owns this. NcmCore is not Send or Sync — a
// QuickJS runtime is bound to its creating thread — and `call` blocks on
// network I/O, so it must never run on the player loop or near an audio
// callback.
let core = NcmCore::new(Some(state_dir))?;
let json = core.call("song_url_v1", r#"{"id":"347230","level":"standard"}"#)?;
```

`call` returns `{ ok, status, body, cookie }` as JSON. A non-200 upstream
response is **not** an `Err`: callers act on in-band codes like 301 (not logged
in), so the body is preserved and `ok` is false. `Err` means the isolate itself
failed.

`NcmCore::new` performs a one-time bootstrap handshake when its persisted state
is cold. `NcmCore::offline()` skips it and touches no network — for tests and
diagnostics only, since xeapi endpoints cannot work without the server's public
key.

## Latency

Measured, because it decides where optimizing is worth anything:

| | |
|---|---|
| one warm request to Netease | ~60–80 ms |
| opening a connection (DNS+TCP+TLS) | ~160–190 ms |
| isolate overhead per call | ~12 µs |
| 1.5 MB JSON round trip through QuickJS | ~7 ms |
| cold isolate + bootstrap handshake | ~350–650 ms |

A call *is* its round trip. Netease negotiates HTTP/2 on both hosts, so requests
to one host share a connection rather than needing one each — but the ceiling of
six in flight per host stays, because it is a *politeness* limit rather than a
connection limit: bursting past it is what gets a client shed (see `throttle`),
and `throttle::MAX_IN_FLIGHT` and `http::POOL_MAX_IDLE_PER_HOST` both encode it.
Multiplexing makes a burst cheaper to hold open; it does not make one round trip
shorter. **The only way to be faster is to make fewer requests**, which is what
the layers in front of `call` do:

- **`cache` / `disk`** — an answer already held, in memory or from a previous
  launch. Past its TTL but inside its stale window it is still served, at once,
  and refreshed behind the caller.
- **`batch`** — calls that differ only by id become one request. Twenty song
  cards are one `song_detail`; a prefill window is one `song_url_v1`.
- **`inflight`** — one request shared between callers asking for the same thing
  before the answer arrives.
- **`prefetch`** — ask before the answer is needed, so the call that needs it
  finds it already there.

Steady state, one endpoint call is exactly one HTTP request; the live tests
assert that rather than assuming it.

### Two findings worth not re-learning

**The two hosts disagree about connection pooling.** `pool_idle_timeout` was 300 s,
then cut to 5 s after measuring `interface3.music.163.com` and finding a pooled
connection slower than a fresh one across an idle gap. Both readings were real;
generalizing from one host was the mistake:

| idle | `music.163.com` pooled / fresh | `interface3` pooled / fresh |
|------|-------------------------------|-----------------------------|
| 0 ms | **21 / 46 ms** | 100 / 217 ms |
| 3 s | **30 / 266 ms** | 132 / 195 ms |
| 8 s | **19 / 56 ms** | **523** / 203 ms |
| 12 s | **20 / 410 ms** | 296 / 202 ms |

`music.163.com` (weapi — `song_detail`, search, login) rewards reuse at every gap;
`interface3` (xeapi — `song_url_v1`) closes early. reqwest's pool is per-client, so
one number serves both and it favours the host asked far more often. What bounds
being wrong is `tcp_keepalive` plus a **per-attempt** timeout: a socket the peer
dropped silently swallows the request and hangs, and without an attempt ceiling it
hangs for the whole call budget and surfaces as a 502 we invented ourselves.

**The cache was mostly switched off.** It refused any response carrying a
`Set-Cookie`, on the belief that read-only endpoints set none. Every **eapi**
response carries a ten-year tracking cookie, so `lyric_new`, `playlist_detail`,
`toplist` and most of the allowlist were never cached — only `song_detail`,
which happens to be weapi, ever hit. Cookies are now stripped rather than
refused; see `cache::storable_form`.

## Testing

```bash
cargo test -p ncm-core --release                            # offline
cargo test -p ncm-core --release -- --ignored --nocapture   # live, hits 163
```

The live tests are the real coverage: `xeapi_song_url_v1` exercises X25519 ECDH
→ HMAC key derivation → AES-128-GCM against the live service, which is the one
path where a wrong shim produces a plausible-looking request that the server
rejects with a generic error.

## Building for Android / iOS

`rquickjs-sys` ships pregenerated bindings for desktop targets only — there are
none for any `*-linux-android` triple. Those builds enable the `bindgen`
feature (already wired in `Cargo.toml`), which needs libclang plus explicit
paths to the NDK sysroot and clang resource dir. Without the latter, bindgen
fails with `'stdbool.h' file not found`.

```bash
NDK=$ANDROID_HOME/ndk/<version>/toolchains/llvm/prebuilt/<host>

export LIBCLANG_PATH="$NDK/bin"
export CC_aarch64_linux_android="$NDK/bin/aarch64-linux-android24-clang"
export AR_aarch64_linux_android="$NDK/bin/llvm-ar"
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android="\
  --target=aarch64-linux-android24 \
  --sysroot=$NDK/sysroot \
  -I$NDK/lib/clang/<clang-major>/include"

cargo check -p ncm-core --release --target aarch64-linux-android
```

On Windows hosts the compiler entries need the `.cmd` wrappers
(`aarch64-linux-android24-clang.cmd`, `llvm-ar.exe`).

Desktop targets need none of this.

## Constraints

- **Single-threaded JS.** QuickJS holds one runtime lock, so JS execution is
  serialized — but the HTTP op returns a real promise, so requests overlap. At
  12 µs of isolate time per call the lock is not a bottleneck.
- **Isolates are long-lived.** xeapi session keys, `WNMCID` and the deviceId are
  module-level state in the bundle; recreating the isolate per call would
  re-handshake every time.
- **Envelope field order is load-bearing.** `cache::is_storable` reads the
  envelope's *shape* to decide whether it may be remembered, so anything that
  builds an envelope must emit `ok`, `status`, `body`, `cookie` in that order.
  `serde_json::Map` is sorted, so a serialized struct would silently turn
  caching off.
- **Secrets.** Cookies are passed per call, never persisted, never logged, never
  included in an error. `redact()` is the last line of defence, not the first.
  Both the cache key and the batch group key contain the cookie, so two accounts
  never share an entry or a request.
- **Bounded.** 64 MB isolate heap, 32 MB response cap, 16 MB cache. All three
  exist so one bad response cannot take the process with it.
