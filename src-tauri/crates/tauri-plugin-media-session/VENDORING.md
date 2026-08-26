# tauri-plugin-media-session (vendored)

Android/iOS lockscreen media controls. **Vendored — we maintain this now.**

## Where it came from

Upstream is `solrmax/tauri-plugin-media-session`, which went closed-source; the
crates.io release `0.2.4` was the last thing published. This copy came from
[`sak96/tauri-plugin-media-session`](https://github.com/sak96/tauri-plugin-media-session)
@ `78f2539` (2026-07-21), which is that published tarball recovered into a git
repo — its `Cargo.toml` was still the cargo-normalized publish artifact, and
`Cargo.toml.orig` (restored here as `Cargo.toml`) the real one.

Vendored rather than tracked as a git dependency for two reasons: the upstream
author is gone, so there is nobody to send fixes to; and a git dependency would
put GitHub availability on the critical path of every CI build.

## Migration was a no-op at the API level

Verified symbol by symbol against what `src/media/mod.rs` uses before switching
— same version, so this is the same code we were already compiling:

| Used by us | Status |
| --- | --- |
| `init()` | unchanged |
| `MediaSessionExt` | unchanged |
| `MediaState` (`title`/`artist`/`album`/`artwork_url`/`duration`/`position`/`is_playing`/`can_prev`/`can_next`/`can_seek`) | unchanged |
| `TimelineUpdate` (`position`/`duration`) | unchanged |
| `update_state` / `update_timeline` / `clear` → `Result<(), String>` | unchanged |
| ACL `media-session:default` | unchanged (`allow-initialize`, `allow-update-state`, `allow-update-timeline`, `allow-clear`) |

## Things to know before editing

- **Only compiles into Android/iOS builds.** A desktop `cargo check` proves
  nothing about this crate; verify with
  `cargo check -p gmplayer --target aarch64-linux-android`.
- The Kotlin half can be compiled on its own, without a full
  `tauri android build`:
  `../../gen/android/gradlew.bat -p . compileDebugKotlin` from `android/`
  (that directory is a standalone Gradle project; its `gradle.properties`
  exists only for that path and is ignored when the app includes this as a
  module).
- **The Kotlin package is `com.gbclstudio.gmplayer.media`, not upstream's
  `app.tauri.mediasession`.** Tauri resolves the plugin class as
  `<identifier>.<class>`, so `PLUGIN_IDENTIFIER` in `src/lib.rs`, `namespace` in
  `android/build.gradle.kts`, the class names in `android/src/main/AndroidManifest.xml`
  and the `package` lines in the Kotlin sources all have to move together. A
  mismatch fails at runtime, not at compile time.
- **`links = "tauri-plugin-media-session"`** in `Cargo.toml` is load-bearing:
  Cargo permits exactly one crate with a given `links` value in the graph, so
  the crates.io version and this one can never coexist. That is the desired
  behaviour — it makes a stale dependency a build error rather than a silent
  duplicate.
- The Kotlin/Swift halves live in `android/` and `ios/`; changing a `@Command`
  name there means changing `permissions/` to match, and the ACL is checked
  before dispatch, so a mismatch fails at runtime rather than at compile time.
- `guest-js/` is the JS binding. We do not use it — the app drives the session
  from Rust (`src/media/mod.rs`) precisely so it keeps working while the
  Android WebView is destroyed.

## Where it now diverges from upstream

Kept deliberately, so a future re-sync does not undo them:

- **`is_loading` / `STATE_BUFFERING`.** New field on `MediaState` and
  `UpdateStateArgs`, plus a loading glyph on the transport action. Upstream
  only had playing/paused, so a slow resolve looked like a play button that
  does nothing.
- **`res/drawable/ic_media_session_notification.xml`.** Upstream fell back to
  the host app's `ic_launcher_foreground`, which on API 24+ is the Android
  Studio template robot. The small icon is drawn as an alpha mask, so it has
  to be a monochrome vector; a host app can still override with its own
  `drawable/ic_notification`.
- **Audio-focus policy corrected and moved into `MediaSessionController`:**
  upstream did not pause on `AUDIOFOCUS_LOSS_TRANSIENT` and *did* pause on
  `AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK`, which is backwards. Harmless while the
  events went nowhere; audible once they reach the backend.
- **`clear` no longer emits a transport action,** and no longer tears anything
  down. Clearing means "nothing to show", and with actions wired to the backend
  a pause there would stop a track that is starting.
- **`update_state` / `update_timeline` / `clear` / `initialize` are async,**
  built on `run_mobile_plugin_async`. The blocking variant parks the caller on
  a `recv()` with no timeout until the Android main thread services the call;
  from an async drain task that both parks a runtime worker and wedges every
  later push behind a stuck one, which froze the session while the screen was
  locked. Callers must bound them (`media::APPLY_TIMEOUT`) and must not *drop*
  a pending call — tauri's response handler unwraps its `send`.
- **The `on_action` callback runs before the `media_action` emit,** so a button
  press is not queued behind a webview round-trip.
- **The Kotlin half was rewritten** (see below). Upstream's single
  `MediaSessionPlugin` + `MediaSessionCleanupService` pair is gone.

## Kotlin architecture (a rewrite, not a patch)

Three rules, all of them load-bearing. The layout follows
[Moriafly/media-kit](https://github.com/Moriafly/media-kit), whose
`MediaNotificationPost` documents most of these traps.

**1. State is process-scoped, not per-Activity.** `MediaSessionController` is an
`object` keyed off `applicationContext` and owns the session, the merge state,
the artwork and the action `Channel`. `MediaSessionPlugin` is a shell that parses
arguments and forwards. Tauri's `PluginManager` builds a plugin exactly once per
process and never replaces it, yet still forwards `onDestroy` when the Activity
goes away — so the old code's teardown there ran with no matching setup
afterwards, permanently killing the notification and its buttons while Rust kept
playing. `MainActivity.installRenderProcessGuard` calls `recreate()` on purpose
when the WebView render process dies, so this is a routine path, not a corner.
The action `Channel` in particular must survive: Rust hands it over once at
plugin setup and never again.

**2. `PlaybackService` is created once and never stops itself.** `clear`
*demotes* it and removes the notification. Upstream called `stopSelf()`, and
because that only schedules `onDestroy` on the main looper, the next track's
`updateState` would build a fresh session and notification that the late
`onDestroy` then tore down again — which is how the notification vanished
mid-playlist. For the same reason nothing calls a global "force cleanup", and
nothing deletes the notification channel: posting to a deleted channel is
silently dropped.

**3. Foreground promotion is done from inside the running service** —
`startForegroundService` immediately followed by `ServiceCompat.startForeground`
— so the start contract cannot time out, and every one of those calls is
wrapped: `ForegroundServiceStartNotAllowedException` (API 31+) and the API 34+
service-type exceptions are *expected* outcomes that fall back to a plain
`notify()`, not crashes. An unfulfilled contract is unrecoverable (Google closed
that as working-as-intended), which is why `onStartCommand` settles one even when
it is about to withdraw again.

Two smaller invariants that came out of the same bug hunt:

- **Never recycle a bitmap handed to the framework.** The previous artwork is
  still referenced by the posted notification and by the metadata the session
  published; SystemUI draws and parcels both long after we move on, and
  recycling throws `Canvas: trying to use a recycled bitmap` *inside SystemUI*,
  which drops the notification. Dropping the reference is enough.
- **Wake lock and audio focus follow playback, not the service.** Focus is
  re-requested after a loss — upstream never did, so a manual resume after a
  permanent loss played on with no focus at all — and the wake lock is released
  while paused.

