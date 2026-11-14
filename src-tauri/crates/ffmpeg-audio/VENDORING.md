# Vendored FFmpeg audio wrapper

Source: [SPlayer-Dev/ffmpeg-audio](https://github.com/SPlayer-Dev/ffmpeg-audio)

Revision: `fe78660bd7e3a231f70222f3da5f1eadc0db87ba` (`v0.4.0-spn.1`).

The Rust sources under `src/` are copied from `crates/ffmpeg_audio/src/` at that
revision, except the HTTP transport. Upstream's GPL-3.0-only license is preserved
in `LICENSE`. The bundled C implementation is supplied by the upstream
`ffmpeg_audio_sys` dependency at the same fixed revision; no FFmpeg executable or
system FFmpeg installation is used for playback.

Local changes:

- Rename the package to `gmplayer-ffmpeg-audio` and depend on the pinned upstream
  sys crate. Keep only the native decoder/resampler dependencies and logging.
- Remove the optional HTTP module and exports. GMPlayer owns downloads, growing
  files and Android content descriptors through its existing source abstraction.
- Pass the audio stream time base to `Decoder::new` and assign
  `AVCodecContext.pkt_timebase` before `avcodec_open2`. FFmpeg needs it to adjust
  decoded frame timestamps for codec pre-skip. Without it, decoding our two-second
  Opus fixture produced 95,688 rather than 96,000 stereo frames and seeking
  returned different audio. The timeline then trimmed samples FFmpeg had already
  removed. Backend tests assert the full sample count and compare seek output
  with the corresponding decoded PCM.
- Apply the repository's Rust formatting.

When updating, compare the upstream decoder's timestamp handling before removing
this patch. Run both this crate's tests and `audio-backend`'s codec fixture tests.
The lockfile keeps bindgen 0.73's `syn` and `prettyplease` on compatible AST
versions (`syn` 2 with `prettyplease` 0.2); selecting `syn` 3 for that dependency
while retaining `prettyplease` 0.2 fails to compile. Other workspace macros can
still use `syn` 3 independently.
