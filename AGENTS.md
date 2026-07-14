# Repository Guidelines

## Agent Notes

Address the project owner as the Operator. Treat this file as the shared guide for all coding agents; `CLAUDE.md` is only a compatibility link.

## Project Overview

GMPlayer is a Vue 3 + Vite music player with Tauri 2 desktop/mobile support. It integrates with Netease Cloud Music API, local playback, Apple Music-like lyrics, real-time spectrum analysis, AutoMix/crossfade, PWA features, and i18n. License: AGPL-3.0.

## Project Structure

- `src/`: frontend application code.
- `src/api/`: Netease/feature API modules such as `song`, `user`, `playlist`, `login`, `video`.
- `src/components/Player/`: main player UI, lyrics, covers, spectrum, and playback controls.
- `src/views/`: routed pages and slave-window views such as `MiniPlayer`, `DesktopLyrics`, `TaskbarLyrics`, `TrayPopup`.
- `src/store/`: Pinia stores for music, settings, user, site, and listen-together state.
- `src/utils/AudioContext/`: playback engine, sound abstractions, spectrum, AutoMix, native backend bridge.
- `src/utils/tauri/`: frontend Tauri window, tray, bridge, and native integration helpers.
- `src/locale/`: `zh-CN` and `en` translations.
- `public/`: static assets; `screenshots/` and `docs/`: supporting documentation.
- `src-tauri/`: Tauri app; `src-tauri/src/` contains app/window code, `src-tauri/crates/` contains Rust workspace crates.

## Commands

Use PNPM.

- `pnpm install`: install dependencies.
- `pnpm dev`: start Vite dev server.
- `pnpm build`: build frontend to `dist/`.
- `pnpm preview`: preview production build.
- `pnpm build:wasm`: build audio-analysis and audio-backend WASM packages.
- `pnpm lint` / `pnpm lint:fix`: run oxlint on `src/`.
- `pnpm fmt` / `pnpm fmt:check`: format or check `src/` with oxfmt.
- `cd src-tauri && cargo check`: validate Rust/Tauri code.
- `cd src-tauri && cargo test`: run Rust tests when present.

## Environment

Required: PNPM, Node.js, Rust toolchain for Tauri work, and `.env` with `VITE_MUSIC_API` pointing to a Netease Cloud Music API endpoint. Optional variables include `VITE_UNM_API`, `VITE_SITE_TITLE`, and `VITE_SITE_DES`. Never commit secrets or private endpoints.

## Coding Style

Frontend formatting is defined in `.oxfmtrc.jsonc`: 2-space indentation, semicolons, double quotes, trailing commas, LF endings, and 100-column print width. Use `<script setup>` and Composition API for Vue SFCs. Prefer PascalCase component names, `useXxx` composables, domain-named stores, and the `@` alias for `src/`. Do not manually edit generated `auto-imports.d.ts` or `components.d.ts`. Rust should follow `rustfmt`, snake_case for modules/functions, and PascalCase for types.

## Testing & Validation

There is no dedicated frontend test script. For frontend changes, run focused `oxfmt --check`, `oxlint`, and `pnpm build` when practical; manually exercise UI flows in `pnpm dev`. For Rust/Tauri changes, run `cargo check` from `src-tauri/` and add focused `#[test]` coverage for nontrivial logic.

## Architecture Notes

The audio system centers on `src/utils/AudioContext/PlayerFunctions.ts`, `SoundManager`, `BufferedSound`, `NativeRustSound`, spectrum helpers, and the `AutoMix/` state machine. `window.$player` holds the active `ISound`. Playback state is coordinated through `src/store/musicData.ts`, then broadcast to slave windows through `src/utils/tauri/playerBridge.ts`.

The native playback queue is a bounded prefill window, not a mirror of the frontend playlist. `src/utils/AudioContext/NativeQueuePrefill.ts` (Tauri-only) sends the current track plus pre-resolved next tracks, carrying real playlist indices as `origOrder` and `windowed: true`; the Rust queue stops at the window edge instead of wrapping, and wrap-around is reserved for repeat semantics (`windowed: false`). `SetPlaylist` re-anchors `current_index` by song id, never by clamped position. Track-end transitions are backend-initiated: while a window is applied, `NativeRustSound` suppresses the legacy `end` event and adopts the backend advance (sync event → `applyNativeAutoMixCompletion` → native-advance hold, so the debounced Player watcher reuses the already-playing sound instead of recreating it). Fallback timers restore the JS-driven `end` path when no advance lands — keep that fallback intact, and keep the prefill gated off for WASM/web, personal FM, and listen-together.

In `src-tauri/crates/audio-backend/src/player/`, `mod.rs` owns only the `AudioPlayer` struct, construction, and the `run()` event loop; message dispatch, track loading, seek, output-device runtime, native AutoMix runtime, and status emission live in sibling modules (`messages`, `playback`, `seek`, `output_runtime`, `automix_runtime`, `status`). Keep new `AudioPlayer` logic in the module that owns that concern instead of growing `mod.rs`.

Steady-state resource rules for the backend: the producer park cap (duplicated in `output/mod.rs` and `player/mixer.rs`) IS the wakeup cadence, because rings run full during playback — do not shrink it casually, and change both copies together. Default-device polling uses an adaptive stride (`output_runtime`) because the probe enumerates devices on hosts without a platform default-device id (per-device server roundtrips on PulseAudio). Linux shares Android's latency profile (deep 48-block software queues plus start/seek prebuffer watermarks, cfg `any(android, linux)`); the `stable_buffer_size` device-buffer policy must not be changed without a concrete platform reason.

Cross-window time sync is anchor-based: the master (`playerCommunication.ts`) broadcasts `PlayerTimePayload` only on timeline discontinuities (play/pause/seek/track/lyric-line change — including seeks while paused) plus a low-rate heartbeat, and slave windows extrapolate locally in `playerBridge.ts` with `sentAt` latency compensation. Keep the payload wire-compatible and do not reintroduce per-frame broadcasts. Android media controls (`src/composables/useNativeMediaControls.ts`) read position from the live `window.$player` timeline rather than the store snapshot; the audio-focus handler must preserve transient-loss auto-resume and duck-volume restore semantics.

For `src-tauri/crates/audio-backend`, zero abstraction overhead, low latency, and minimal resource use are hard requirements. Keep blocking work, allocation, device enumeration, and heavyweight setup out of audio callbacks and hot playback loops; prefer concrete types, preallocation, block-level processing, and explicit bypass paths over dynamic dispatch or per-sample control checks. Any DSP/EQ work must preserve this constraint.

Native seek must not invalidate the active decoder generation. A seek should flush queued PCM from the active deck/output, update the decoder source position, reset analysis, and republish the position anchor; it must not call `DeckMixer::clear_all`, `DeckMixer::clear_deck`, or any path that bumps the active `DeckWriter`/`OutputWriter` generation. Generation bumps are reserved for replacing, cancelling, or retiring playback chains.

Frontend seek/autoresume must preserve the optimistic position anchor. After a seek, delayed native `syncStatus`/`playPosition` events from before the seek should not overwrite the new local position. Startup autoresume should snapshot the persisted position before loading and apply it before autoplay starts, especially for web `BufferedSound` where pending seek is consumed before pending play.

Lyrics support LRC, YRC, and TTML. Fetching and normalization live under `src/utils/LyricsProcessor/`, while AMLL rendering powers rich lyric views.

Tauri windows are created through Rust-side presets in `src-tauri/src/desktop/window/config.rs` and `manager.rs`. On Windows, WebView2 windows that share a profile must use consistent additional browser args.

## AutoMix & Crossfade Rules

AutoMix uses a state machine: `idle -> analyzing -> waiting -> crossfading -> finishing -> idle`. `monitorPlayback()` runs per frame and must stay synchronous. Crossfade gain targets must preserve LUFS gain adjustment; after crossfade, persistent gain adjustment must continue applying through normal volume changes.

Keep analysis-based adjustments conservative. Shape, duration, spectral alignment, energy gate, and intro/outro heuristics compound quickly; prefer stable equal-power behavior over aggressive shaping. The finishing hold protects against the player watcher creating a duplicate sound while store updates settle.

## Commit & PR Guidelines

History uses Conventional Commit-style prefixes such as `fix(Android): ...`, `feat(Tauri): ...`, `chore: ...`, and `docs(README): ...`. Keep commits scoped and imperative. PRs should include summary, affected platforms, validation commands, linked issues, and screenshots or recordings for visible UI changes.
