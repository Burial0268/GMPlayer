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

Lyrics support LRC, YRC, and TTML. Fetching and normalization live under `src/utils/LyricsProcessor/`, while AMLL rendering powers rich lyric views.

Tauri windows are created through Rust-side presets in `src-tauri/src/desktop/window/config.rs` and `manager.rs`. On Windows, WebView2 windows that share a profile must use consistent additional browser args.

## AutoMix & Crossfade Rules

AutoMix uses a state machine: `idle -> analyzing -> waiting -> crossfading -> finishing -> idle`. `monitorPlayback()` runs per frame and must stay synchronous. Crossfade gain targets must preserve LUFS gain adjustment; after crossfade, persistent gain adjustment must continue applying through normal volume changes.

Keep analysis-based adjustments conservative. Shape, duration, spectral alignment, energy gate, and intro/outro heuristics compound quickly; prefer stable equal-power behavior over aggressive shaping. The finishing hold protects against the player watcher creating a duplicate sound while store updates settle.

## Commit & PR Guidelines

History uses Conventional Commit-style prefixes such as `fix(Android): ...`, `feat(Tauri): ...`, `chore: ...`, and `docs(README): ...`. Keep commits scoped and imperative. PRs should include summary, affected platforms, validation commands, linked issues, and screenshots or recordings for visible UI changes.
