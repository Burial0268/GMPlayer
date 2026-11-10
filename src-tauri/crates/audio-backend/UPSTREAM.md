# Audio engine references

The CoreAudio device watcher in `src/output/device_watcher/macos.rs` adapts the
property-listener approach and output-property coverage from
[SPlayer-Next](https://github.com/SPlayer-Dev/SPlayer-Next), by the SPlayer-Dev
contributors, under the GNU Affero General Public License version 3. The license
is available in this repository's root `LICENSE`.

Reference revision: `a8716df8f1a2cd08db3307ad592669df24bc05f4`.

Reference source:
[`native/audio-engine/src/output/device_watcher/macos.rs`](https://github.com/SPlayer-Dev/SPlayer-Next/blob/a8716df8f1a2cd08db3307ad592669df24bc05f4/native/audio-engine/src/output/device_watcher/macos.rs).

GMPlayer's adaptation uses its existing `coreaudio-rs` bindings. Callbacks look up
a context id, accumulate atomic change flags and wake the registration worker.
Device-list changes update registrations by device id; ordinary property changes
do not re-register every device. The player consumes hints through its existing
output refresh path, independently of JS. Retired context ids are removed from
the registry, so a late CoreAudio callback cannot dereference released data or
signal a newer watcher.

The platform split and distinction between default-output and device-list events
also follow SPlayer-Next's watcher design. GMPlayer accumulates both event kinds
rather than sending a boolean through a one-slot channel: a list notification
must not hide a subsequent default-output change.

Linux uses the `pulseaudio` crate's public protocol API and subscription example
(the crate is already a CPAL dependency), rather than SPlayer-Next's PipeWire
backend. It works with PulseAudio and pipewire-pulse without adding native
PipeWire development dependencies. Authentication is bounded, idle listening
blocks on the socket, teardown shuts that socket down, and a disconnected
listener retries after five seconds. Existing device polling remains the
fallback on all platforms, including ALSA-only hosts. Windows retains its
existing MMDevAPI registration, concrete endpoint resolution and COM policy.

## Validation

From `src-tauri`:

```sh
cargo check -p gmplayer-audio-backend
cargo test -p gmplayer-audio-backend --lib output::device_watcher
```

For a real Linux server and CPAL output test, install `pulseaudio` and
`pulseaudio-utils`, then run from the repository root:

```sh
bash scripts/test-audio-device-watch.sh
```

The script starts its own server with virtual sinks and removes it on exit. It
checks default-output notifications, resolved output identities, stream opening,
and a device-list change that leaves the default unchanged. It also breaks the
listener's connection and verifies reconnection and subsequent default changes.
Ordinary unit tests use paired Unix sockets to exercise the actual subscription
handshake, fragmented events, disconnects, cancellation and handshake timeout
without a running server.

macOS still requires validation on a Mac: switch the default output while playing
and paused, add/remove a USB output, change the selected output's sample rate,
and verify that a named selection does not follow an unrelated default change.

## Native codec fallback

Native playback keeps Symphonia for supported streams and selects FFmpeg when
that decoder cannot open the audio. Selection happens before creating a concrete
worker, so both decoders use the same block converter, mixer, DSP, output and
clock without per-sample dynamic dispatch. FFmpeg only converts its decoded
sample representation to interleaved f32; the existing converter owns output
sample-rate and channel conversion.

The Rust wrapper is adapted from
[SPlayer-Dev/ffmpeg-audio](https://github.com/SPlayer-Dev/ffmpeg-audio), revision
`fe78660bd7e3a231f70222f3da5f1eadc0db87ba`. Its GPL-3.0-only license and local
changes are documented in [`../ffmpeg-audio/VENDORING.md`](../ffmpeg-audio/VENDORING.md).
The sys crate compiles an audio-only FFmpeg source bundle statically; playback
does not spawn a process, download binaries at runtime or require system FFmpeg.
The wrapper's HTTP feature is removed: every reader still uses `source::open` or
`source::open_playback`, including growing downloads and Android SAF descriptors.
The browser's `wasm32-unknown-unknown` backend keeps Symphonia.

Native metadata and file AutoMix analysis also fall back to FFmpeg. File-picker
candidates live in `src/formats.rs` and are shared with the local scanner. Added
candidates include APE, WMA, WavPack, TTA, TAK, Musepack, DSF, AC-3/E-AC-3, DTS,
AMR, AU and Wave64; Opus is now decodable rather than merely listed. Extensions
are a prefilter: the actual codec must still probe successfully.

The build requires Rust 1.88+, a C compiler and libclang. Linux CI installs
`libclang-dev`; Windows installs LLVM for bindgen; macOS and Android reuse their
LLVM setup. The Android NDK/bindgen environment remains supplied by
`scripts/tauri.mjs`. The C bundle and its architecture configurations are pinned
together by the sys crate revision.

Codec tests use generated, two-second Opus/WMA/WavPack tone files with tags. They
verify unsupported Symphonia inputs, finite PCM, exact sample counts for Opus and
WavPack, seek alignment, metadata, block conversion and native file AutoMix.
The wrapper also retains its upstream timeline/cursor unit tests.

```sh
cd src-tauri
cargo test -p gmplayer-ffmpeg-audio --lib
cargo test -p gmplayer-audio-backend --lib
cargo check
cargo check -p gmplayer-audio-backend --target wasm32-unknown-unknown
```

### AV3A / Audio Vivid

AV3A is not included. Standard FFmpeg and the selected audio-only configuration
have no AV3A audio decoder. FFmpeg's `libuavs3d` decodes AVS3 **video**, which does
not add Audio Vivid support.

A concrete source candidate is
[MOLAN-AV3A](https://github.com/baiji6/MOLAN-AV3A). Its
[license](https://github.com/baiji6/MOLAN-AV3A/blob/main/LICENSE) explicitly says the
decoder and wrapper are governed by the UWA Code Sharing Policy and carry no
separate open-source license. That is a different integration decision from
using GPL FFmpeg; this change does not establish permission to redistribute it
with GMPlayer. Other inspected projects distribute platform binaries or use an
emulated Android decoder, which does not provide the portable source-backed
codec needed here.

If an appropriate AV3A implementation is obtained, it will additionally need MP4
`av3a` sample-entry/raw-stream demuxing, state reset and pre-roll for seek, and an
explicit channel layout/downmix policy. Decoding a 7.1.4 bed to PCM alone does not
implement Audio Vivid object rendering or binaural spatial audio.

## Native beat matching experiment

The first native implementation borrows the bounded-stretch and source-sample
accounting approach from [SPlayer-Next PR #429](https://github.com/SPlayer-Dev/SPlayer-Next/pull/429),
revision `ae4d3255de854ed36e8eeaa447d03f0aa0589cd2`. The reference is AGPL-3.0;
Signalsmith Stretch's Rust wrapper is an MIT dependency.

`automix/beats.rs` measures spectral-flux onset peaks in the first and last
40 seconds. It fits observed stable runs of at least eight pulses and three
seconds, requires 90% of the pulses to fit, and never extrapolates the grid beyond
its measured interval. The existing global BPM result remains available to the
legacy structure planner; the new pair planner uses the local periods and phases
instead. Four-pulse grouping is a conservative phrase-length heuristic, not a
claim that a downbeat model has identified the meter.

`player/automix/beat_plan.rs` tests half/double-time folds and permits at most
±4% stretch of the outgoing A track. B starts at its original beginning and
remains at its original speed. A cue is chosen so the observed pulse phases
meet; the complete overlap must fit both stable runs and the configured duration
budget. Missing evidence, an unsafe tempo ratio or insufficient preparation lead
retains ordinary crossfade. Pair fitting runs before the legacy end clamp so a
slightly earlier real pulse can recover an otherwise shortened overlap.

The Signalsmith processor is constructed on a prepare thread and applied in the
decoder only from the future source cue. Its look-ahead and output pre-roll are
accounted separately; draining synthetic zeros never adds source time. A unity
rate takes the ordinary decode path. `PcmPosition` follows source position and
rate through deck blocks. The mixer splits at the planned cue and handoff frame,
and submits sparse timeline markers with PCM. Only consumption by the device
callback publishes the markers. `PlayerClock` binds them by playback id and
retains the outgoing mapping until B is adopted. Native completion no longer
uses a wall-clock timer, so pausing cannot finish a fade that has not rendered.

Native frontend preparation survives pauses. The old frontend wall-time watchdog
is removed, retired async URL resolutions cannot publish a later prepare, and
track adoption retires old local readiness flags without cancelling the backend's
already playing sound. Reconfiguration clears a previous failed attempt instead
of replaying its failure into the next attempt. Adoption also updates the source
locator alongside the decoder/local path, so returning to A cannot reuse B's
cached file. Repeated preparation from a recreated WebView attaches to
the same active pair by stable identity instead of resetting its decks; a signed
URL is never used as a cross-resolver identity.

Validation:

```sh
pnpm test:automix-native
cd src-tauri
cargo test --locked -p gmplayer-audio-backend --lib
```

The deterministic callback harness checks exact cue/handoff frames for mono,
stereo and six channels, missing incoming audio, pause/resume, cancellation,
source-sample totals, pitch preservation and seek/epoch clock guards. The isolated
PulseAudio test exercises the real player, decoder, mixer and CPAL callback:

```sh
bash scripts/test-audio-device-watch.sh live_native_automix
```

It uses generated local pulses, pauses mid-overlap, repeats preparation with a
rotated URL to simulate page recreation, rejects a retired track's request, and
rebuilds output at the handoff while verifying B's actual source position, and
returns to A to verify the source-cache binding. A moved logical playlist index
is retained through the same transition.
PulseAudio's virtual-sink default can deliver large callback
batches, so this test allows a broad wall-time deadline; the deterministic harness
checks the frame boundaries independently.

A standalone release harness using this exact processor measured 30 seconds of
48 kHz stereo at 0.97x in 857 ms and at 1.03x in 775 ms (35x/39x realtime on the
measuring Linux machine). Processor setup took 0.7–1.6 ms and occurs off the audio
workers. Reproduce the benchmark with:

```sh
cd src-tauri
cargo test --release -p gmplayer-audio-backend benchmark_automix_tempo -- --ignored --nocapture
```

The Android C++ runtime is handled by Tauri's `cargo-mobile2` library staging:
`android::target::Target::symlink_libs` detects `libc++_shared.so` in the produced
library and stages the matching NDK runtime. No global C++ runtime override is
introduced. The backend was cross-built and linked for `aarch64-linux-android`
with NDK r27c and API 26; its ELF dependencies include `libc++_shared.so`.
Full APK installation/listening, other Android ABIs, macOS and Windows still need
their own validation. The measurements above do not establish mobile CPU or audio
quality.

Scope limits: this is native pulse matching, without Beat This!/Demucs or object
separation. The browser keeps its existing transition path. Frontend/system-media
effective-rate reporting is not expanded here; their existing position-anchor
updates remain in use. Starting each new preparation still originates in the
existing frontend lifecycle, so this does not promise repeated unattended AutoMix
when Android freezes or destroys the WebView. That ownership change is separate
from preserving an already prepared or active native transition.

## Other upstream candidates

These remain separate from device notifications:

- The remaining general playback-speed/pitch controls in `dsp/tempo.rs` are
  separate from the AutoMix-only processor now adopted. They would need a user
  workflow and effective-rate propagation across all display surfaces.
- `output/wasapi/`: a reference for Windows exclusive output, format negotiation
  and PCM conversion. Adopting it needs a defined user setting, shared-mode
  fallback behavior and validation on Windows hardware. The existing shared
  output and its COM policy stay the baseline.
- `dsp/equalizer.rs` and `dsp/limiter.rs`: no replacement is planned. GMPlayer
  already supports configurable peaking/shelf bands, sample-rate-based limiter
  release and a block-level bypass.

No performance improvement is claimed from the source comparison alone.
