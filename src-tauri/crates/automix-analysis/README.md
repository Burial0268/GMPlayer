# AutoMix PCM analysis

Shared PCM algorithms and result types for GMPlayer's native preparation tasks
and browser worker. The crate depends on Serde and RustFFT; it has no audio
output, file/network access, Tauri runtime or task scheduler.

`analyze_mono_samples` returns the existing `TrackAnalysis` shape: volume,
energy, BPM and local beat regions, spectral fingerprint, intro/outro, phrases,
sections, vocal activity and mix candidates. Algorithms and thresholds are moved
from `audio-backend::automix` without retuning. The backend re-exports these types
and this function so current Rust and IPC consumers keep their API.

`sections::StructureFeatureExtractor` accepts successive PCM chunks and produces
compact `StructureFeatures`. `sections::segment_song` consumes those features
and caller-supplied timing/tempo hints. The WASM `SongStructureAnalyzer` remains
a binding in `audio-backend`, using this same extractor.

| Responsibility | Owner |
| --- | --- |
| PCM features and serialized analysis results | This crate |
| Encoded bytes/file adapters and I/O errors | `audio-backend/src/automix/file.rs` |
| Symphonia/FFmpeg selection | `audio-backend/src/decoder/input.rs` |
| Sources, SAF descriptors and growing downloads | `audio-backend/src/source/` |
| Preparation, cache, cancellation and playback plans | `audio-backend/src/player/automix.rs` |
| Output callbacks and rendered transition timing | `audio-backend/src/output/` and `player/` |

`analyze_pcm` accepts a caller-owned `MonoPcm` reader. It accumulates volume,
energy and vocal filter state in blocks, extracts the same section features,
and reads only the required intro/outro, BPM and fingerprint windows. PCM work
buffers cover at most 60 seconds plus 200 warm-up samples; result arrays and
compact structure features still grow with duration. Reads remain cancellable
between blocks, including reads that fill a longer window.

Native file analysis uses `audio-backend/src/automix/spool.rs`: it decodes mono
once, keeps at most 4 MiB in a `tempfile::SpooledTempFile`, then spills to the app
cache. Scratch stores exact native f32 bytes through Bytemuck and disappears when
closed. Short inputs need no disk file; larger ones trade temporary storage
(`4 * decoded mono frames` bytes) for bounded PCM heap use. App hosts configure a
storage-backed cache through `set_stream_cache_dir`; a standalone host's default
system temp directory may be a memory filesystem. The array API and encoded-byte
adapter retain their existing contracts.

The worker design reviewed for this split is documented in
[`audio-backend/UPSTREAM.md`](../audio-backend/UPSTREAM.md#automix-worker-boundaries).

From `src-tauri`, run `cargo test --locked -p gmplayer-automix-analysis` without
platform audio development libraries. After changing the section implementation,
regenerate its browser consumer with `pnpm build:wasm:audio-backend` from the
repository root.
