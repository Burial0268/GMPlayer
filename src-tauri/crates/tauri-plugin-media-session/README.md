# tauri-plugin-media-session

A Tauri 2 plugin for mobile media session integration. Displays lockscreen controls, album artwork, and transport buttons — with a dead-simple API.

## Compatibility

| Tauri build target | Status              | Notes                                    |
| ------------------ | ------------------- | ---------------------------------------- |
| Android            | ✅ Supported        | Full plugin support                      |
| iOS                | ⚠️ WIP              | MPNowPlayingInfoCenter + MPRemoteCommandCenter |
| macOS              | ❌ Not yet          | Not implemented                          |
| Windows            | ❌ Not yet          | Not implemented                          |
| Linux              | ❌ Not yet          | Not implemented                          |

## Features

- MediaStyle notification with play/pause/skip/seek controls (Android)
- Lockscreen and Control Center media controls (iOS)
- Hardware media button support (headphones, Bluetooth, etc.)
- Album artwork with smart caching and automatic downsampling
- Playback speed support
- Merge semantics — only send what changed, previous values are preserved
- Runtime notification permission handling (Android 13+)
- Full TypeScript bindings with JSDoc
- Typed Rust API for backend consumers
- Automatic lifecycle management

## Installation

### 1. Add the Rust crate

```toml
# src-tauri/Cargo.toml
[dependencies]
tauri-plugin-media-session = "0.2"
```

### 2. Register the plugin

```rust
// src-tauri/lib.rs
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_media_session::init())
        .run(tauri::generate_context!())
        .expect("error while running application");
}
```

### 3. Add the capability

```json
{
  "permissions": ["media-session:default"]
}
```

### 4. Install the TypeScript bindings

```bash
npm install tauri-plugin-media-session-api
# or
pnpm add tauri-plugin-media-session-api
```

### 5. iOS: enable background audio

Add `audio` to `UIBackgroundModes` in your `Info.plist`:

```xml
<key>UIBackgroundModes</key>
<array>
  <string>audio</string>
</array>
```

## Usage

### TypeScript (recommended)

```typescript
import { updateState, updateTimeline, onAction, clear } from 'tauri-plugin-media-session-api';

// Set the full state on track change
await updateState({
  title: 'Bohemian Rhapsody',
  artist: 'Queen',
  album: 'A Night at the Opera',
  artworkUrl: 'https://cdn.example.com/cover.jpg',
  duration: 354,
  position: 0,
  isPlaying: true,
  canPrev: true,
  canNext: true,
});

// Later — only update what changed (merge semantics)
await updateState({ isPlaying: false });

// Lightweight timeline sync (no notification rebuild)
await updateTimeline({ position: 120.5 });
await updateTimeline({ playbackSpeed: 1.5 });

// Listen for media actions
const listener = await onAction((event) => {
  switch (event.action) {
    case 'play':     player.play(); break;
    case 'pause':    player.pause(); break;
    case 'next':     player.next(); break;
    case 'previous': player.previous(); break;
    case 'seek':     player.seekTo(event.seekPosition!); break;
    case 'stop':     player.stop(); break;
  }
});

// Stop listening when done
await listener.unregister();

// Clear the notification and release resources
await clear();
```

### Rust (backend)

```rust
use tauri_plugin_media_session::{MediaSessionExt, MediaState};

// From a command or anywhere you have access to the app handle:
app.media_session().update_state(MediaState {
    title: Some("Bohemian Rhapsody".into()),
    artist: Some("Queen".into()),
    is_playing: Some(true),
    ..Default::default()
})?;

app.media_session().clear()?;
```

## API Reference

### `updateState(state: MediaState)`

Update the media session and notification. Auto-initializes on first call.

All fields are optional — omitted fields keep their previous values.

| Field           | Type      | Description                                              |
| --------------- | --------- | -------------------------------------------------------- |
| `title`         | `string`  | Track title                                              |
| `artist`        | `string`  | Artist name                                              |
| `album`         | `string`  | Album name                                               |
| `artworkUrl`    | `string`  | Image URL — downloaded natively (no CORS restrictions)   |
| `duration`      | `number`  | Track duration in seconds                                |
| `position`      | `number`  | Current playback position in seconds                     |
| `playbackSpeed` | `number`  | Playback speed multiplier (default: 1.0)                 |
| `isPlaying`     | `boolean` | Whether media is currently playing                       |
| `canPrev`       | `boolean` | Enable "previous track" button                           |
| `canNext`       | `boolean` | Enable "next track" button                               |
| `canSeek`       | `boolean` | Enable seeking (default: true)                           |

### `onAction(handler): Promise<PluginListener>`

Listen for media control actions. Returns a listener you can unregister.

| Action       | Description                                   |
| ------------ | --------------------------------------------- |
| `'play'`     | Play button pressed                           |
| `'pause'`    | Pause button pressed                          |
| `'stop'`     | Stop action triggered                         |
| `'next'`     | Next track button pressed                     |
| `'previous'` | Previous track button pressed                 |
| `'seek'`     | Seek action — `event.seekPosition` in seconds |

### `updateTimeline(timeline: TimelineUpdate)`

Lightweight position/speed sync — skips notification rebuild (Android) / updates `MPNowPlayingInfoCenter` only (iOS).

Use this for frequent updates during playback (seek, speed change). The session must already be initialized via `updateState()`.

| Field           | Type     | Description                  |
| --------------- | -------- | ---------------------------- |
| `position`      | `number` | Playback position in seconds |
| `duration`      | `number` | Track duration in seconds    |
| `playbackSpeed` | `number` | Speed multiplier             |

> **Note:** Both Android and iOS automatically extrapolate the playback position while `isPlaying` is `true`. You don't need to call `updateTimeline()` every second — only on seek, track change, or speed change.

### `clear()`

Dismiss the notification (Android) / clear Now Playing info (iOS), release the media session, and free all resources.
The session is automatically re-created on the next `updateState()` call.

### `initialize()`

Pre-initialize the session and request notification permissions (Android) / configure audio session (iOS).
Optional — `updateState()` handles this automatically.

## Platform notes

### Android
- Requires `POST_NOTIFICATIONS` permission (requested automatically on Android 13+)
- Notification uses `MediaStyle` with `MediaSessionCompat`
- Playback runs under a long-lived `mediaPlayback` foreground service. It is
  created once and *demoted* when playback stops — never stopped — so a clear
  can never race a restart. Session state is process-scoped and survives
  Activity recreation.

### iOS
- Requires `UIBackgroundModes: audio` in `Info.plist`
- Uses `MPNowPlayingInfoCenter` for metadata and `MPRemoteCommandCenter` for controls
- Configures `AVAudioSession` with `.playback` category

## License

MIT OR Apache-2.0
