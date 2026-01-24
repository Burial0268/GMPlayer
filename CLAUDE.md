# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

GMPlayer (SPlayer) is a Vue 3 web music player with Tauri desktop support. It integrates with Netease Cloud Music API and features advanced audio visualization, Apple Music-like lyrics, and real-time spectrum analysis.

**Live Demo:** https://music.gbclstudio.cn/
**License:** AGPL-3.0

## Development Commands

```bash
pnpm dev      # Start dev server (port 25536)
pnpm build    # Production build
pnpm preview  # Preview production build
```

**Requirements:**
- pnpm (v9.15.9+)
- Node.js 16+
- External API services configured in `.env`

## Architecture

### Tech Stack
- **Framework:** Vue 3 + Vite
- **State:** Pinia with persistence
- **UI:** Naive UI + custom components
- **Audio:** Web Audio API + PixiJS visualization
- **Desktop:** Tauri

### Key Directories

```
src/
├── api/          # Netease Music API integration
├── components/   # Reusable components
│   └── Player/   # Core player (BigPlayer.vue is 56KB)
├── views/        # Route page components
├── store/        # Pinia stores (musicData.js is main player state)
├── utils/        # Utilities
│   ├── Player.js             # Audio playback & spectrum analysis
│   ├── parseLyric.ts         # LRC/YRC/TTML lyric parsing
│   └── lowFreqVolumeAnalyzer.ts  # Bass detection for animations
├── services/     # Service layer
│   └── lyricsService.ts      # Lyric fetching & processing
├── libs/
│   ├── apple-music-like/     # Advanced lyric animation engine
│   └── fbm-renderer/         # WebGL fractal background effects
└── locale/       # i18n (zh-CN, en)
```

### API Proxies (vite.config.js)
- `/api/ncm` → Netease Cloud Music API
- `/api/unm` → UnblockNeteaseMusic (greyed songs replacement)
- `/api/la` → Lyric Atlas API

### State Management

**musicData.js** (primary player store):
- `playState` - playback status
- `songLyric` - complete lyric data (LRC, YRC, TTML formats)
- `spectrumsData` - real-time frequency data
- `lowFreqVolume` - bass level for background animations
- `playSongMode` - "normal", "random", "single"

### Audio Processing

The player uses Web Audio API with:
- FFT spectrum analysis in `Player.js`
- Low-frequency smoothing in `lowFreqVolumeAnalyzer.ts`
- PixiJS rendering for visualization in `Spectrum.vue`

### Lyric System

Supports multiple formats:
- **LRC** - standard timestamps
- **YRC** - Netease character-by-character timing
- **TTML** - XML-based timing format

`lyricsService.ts` handles fetching; `parseLyric.ts` handles parsing; `apple-music-like/` provides animated rendering.

## Environment Variables

```env
VITE_MUSIC_API         # Required: Netease Cloud Music API endpoint
VITE_UNM_API           # Optional: UnblockNeteaseMusic API
VITE_LYRIC_ATLAS_API_URL  # Optional: Lyric Atlas API
```

## Codebase Patterns

- Components use `<script setup>` with Composition API
- Auto-import enabled for Vue APIs and components
- Routes are lazy-loaded for code splitting
- Mixed JavaScript/TypeScript (gradual migration)
- SCSS for styling with CSS variables for theming
