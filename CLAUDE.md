# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

You MUST REMEMBER that the user should be called the **Operator**.

## Project Overview

GMPlayer (SPlayer) is a Vue 3 web music player with Tauri desktop support. It integrates with Netease Cloud Music API and features advanced audio visualization, Apple Music-like lyrics, and real-time spectrum analysis.

**Live Demo:** https://music.gbclstudio.cn/
**License:** AGPL-3.0

## Development Commands

```bash
pnpm dev      # Start dev server (port 25536, opens browser)
pnpm build    # Production build (output to dist/)
pnpm preview  # Preview production build
```

**Requirements:**
- pnpm (v9.15.9+)
- Node.js 16+
- `.env` file with at minimum `VITE_MUSIC_API` set to a Netease Cloud Music API endpoint

## Architecture

### Tech Stack
- **Framework:** Vue 3 + Vite
- **State:** Pinia with persistence (localStorage)
- **UI:** Naive UI (auto-resolved) + custom components
- **Audio:** Web Audio API + PixiJS visualization
- **Desktop:** Tauri (in development)
- **i18n:** vue-i18n (zh-CN, en)

### Key Directories

```
src/
├── api/             # Netease Music API modules (album, artist, comment, home, login, playlist, search, song, user, video)
├── components/
│   └── Player/      # Core player UI
│       ├── BigPlayer.vue         # Full-screen player (~58KB, largest component)
│       ├── index.vue             # Bottom bar player controls
│       ├── PlayerCover.vue       # Album cover with animations
│       ├── Spectrum.vue          # PixiJS audio spectrum visualization
│       └── BlurBackgroundRender.vue  # WebGL blur background
├── views/           # Route page components (Home, Search, Discover, User, etc.)
├── pages/           # Additional page components (artist, discover, search, setting, user)
├── store/           # Pinia stores
│   ├── musicData.js    # Primary player state (playback, lyrics, spectrum, playlist)
│   ├── settingData.js  # User preferences (theme, lyrics config, player style, background)
│   ├── userData.js     # Login/user state
│   └── siteData.js     # Site-level state
├── utils/
│   ├── AudioContext/    # Modular audio engine (refactored from Player.js)
│   │   ├── NativeSound.ts          # HTMLAudioElement wrapper implementing ISound interface
│   │   ├── PlayerFunctions.ts      # Public API: createSound, setVolume, fadePlayOrPause, etc.
│   │   ├── SoundManager.ts         # Singleton managing active sound instance
│   │   ├── AudioEffectManager.ts   # Web Audio API nodes (analyser, gain, effects)
│   │   └── LowFreqVolumeAnalyzer.ts # Bass detection for background animations
│   ├── parseLyric.ts      # LRC/YRC/TTML lyric format parsing
│   └── getCoverColor.ts   # Album art color extraction (Material color utilities)
├── services/
│   └── lyricsService.ts   # Lyric fetching & processing (~40KB)
├── libs/
│   ├── apple-music-like/  # Advanced lyric animation engine (AMLL)
│   └── fbm-renderer/      # WebGL fractal Brownian motion background
├── router/            # Vue Router with lazy-loaded routes
└── locale/            # i18n translation files
```

### API Proxies (vite.config.js)
- `/api/ncm` → `VITE_MUSIC_API` (Netease Cloud Music API, required)
- `/api/unm` → `VITE_UNM_API` (UnblockNeteaseMusic, optional)
- `/api/la` → `VITE_LYRIC_ATLAS_API_URL` (Lyric Atlas API, optional)

### Audio Pipeline

The audio system uses a modular architecture in `src/utils/AudioContext/`:
1. `PlayerFunctions.ts` — public API consumed by the store (`createSound`, `fadePlayOrPause`, `processSpectrum`)
2. `SoundManager.ts` — singleton tracking the active `NativeSound` instance (`window.$player`)
3. `NativeSound.ts` — wraps `HTMLAudioElement` + Web Audio API nodes, implements `ISound` interface with event system
4. `AudioEffectManager.ts` — manages `AnalyserNode`, `GainNode`, and the audio graph
5. `LowFreqVolumeAnalyzer.ts` — EMA-smoothed bass detection driving background animations

`musicData.js` store orchestrates playback, calling AudioContext functions and updating reactive state (`spectrumsData`, `lowFreqVolume`, `playSongTime`).

### Lyric System

Three lyric formats with increasing precision:
- **LRC** — standard line-level timestamps
- **YRC** — Netease character-by-character timing (逐字歌词)
- **TTML** — XML-based timing format

`lyricsService.ts` fetches lyrics (with Lyric Atlas API fallback). `parseLyric.ts` normalizes all formats. The `apple-music-like/` lib renders animated lyrics with spring physics and blur effects.

### Global Variables

Several globals are used (declared in `AudioContext/types.ts`):
- `window.$player` — current `ISound` instance
- `window.$message` — Naive UI message API
- `window.$setSiteTitle` — updates document title
- `window.$getPlaySongData` — fetches song data

## Codebase Patterns

- Components use `<script setup>` with Composition API
- Auto-import enabled for Vue APIs (`ref`, `computed`, etc.) and Naive UI composables (`useMessage`, etc.)
- Naive UI components are auto-resolved (no manual imports needed)
- Path alias: `@` maps to `src/`
- Routes are lazy-loaded via dynamic `import()`
- Mixed JavaScript/TypeScript (gradual migration — newer modules like AudioContext are TypeScript)
- SCSS styling with CSS variables for theming
- All Pinia stores use `persist` plugin with localStorage
- WASM support enabled via `vite-plugin-wasm` + `vite-plugin-top-level-await`

## Environment Variables

```env
VITE_MUSIC_API            # Required: Netease Cloud Music API endpoint
VITE_UNM_API              # Optional: UnblockNeteaseMusic API
VITE_LYRIC_ATLAS_API_URL  # Optional: Lyric Atlas API
VITE_SITE_TITLE           # Site title (used in PWA manifest)
VITE_SITE_DES             # Site description (used in PWA manifest)
```

## AutoMix / Crossfade Architecture

### CrossfadeManager (`CrossfadeManager.ts`)
- `getCrossfadeValues(progress, curve, inShape, outShape)` — core curve function
  - Three curves: `linear`, `equalPower` (cos/sin), `sCurve` (smootherstep → cos/sin)
  - `inShape`/`outShape` exponents control ramp speed (<1 = faster, >1 = slower)
  - **Power normalization**: after applying shape exponents to equal-power/S-curve, `cos²+sin²=1` is broken. We re-normalize by `1/sqrt(outVol²+inVol²)` to restore constant perceived loudness. Linear curves are not normalized (3dB midpoint dip is by design).
- `scheduleFullCrossfade()` — linear uses Web Audio `linearRampToValueAtTime` (sample-accurate), equalPower/sCurve use RAF loop
- `_incomingTargetGain = incomingGain * incomingGainAdjustment` — includes LUFS normalization. All ramp targets (linear, resume, finish) must use this value, not raw `params.incomingGain`.

### AutoMixEngine (`AutoMixEngine.ts`)
- State machine: `IDLE → ANALYZING → WAITING → CROSSFADING → FINISHING → IDLE`
- `monitorPlayback()` is called per RAF frame (synchronous, never blocks)
- Pre-buffering: during WAITING, fetches/downloads/analyzes next track so crossfade starts instantly
- `_finalizeCrossfadeParams()` — single consolidated pass combining: smart curve per outro type, spectral alignment, intro character adjustment, spectral similarity duration scaling, energy contrast handling, LUFS normalization
- Energy gate (`_shouldDeferCrossfade`): delays crossfade if outgoing energy is still high and not declining

### Common Crossfade Pitfalls
- **Shape exponents break constant-power**: raw `pow(cos(x), shape)` + `pow(sin(x), shape)` ≠ 1. Must power-normalize after. Without this, asymmetric shapes (e.g., 'hard' outro: inShape=0.7, outShape=1.5) cause up to 24% volume dip at midpoint.
- **Linear ramp target must include gainAdjustment**: `linearRampToValueAtTime` must target `_incomingTargetGain` (not `params.incomingGain`), otherwise LUFS normalization is lost during linear crossfades.
- **`fadeInOnly` mode**: outgoing gain held constant (song's own fade handles it), only incoming ramps up. Power normalization still applies — slightly boosts incoming to compensate for the phantom outgoing curve, which is beneficial since the actual outgoing audio is declining.
- **Gain adjustment persistence**: after crossfade completes, `_activeGainAdjustment` is persisted so that `setVolume()` can continue applying LUFS normalization during regular playback.
