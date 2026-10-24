import assert from "node:assert/strict";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import { buildSync } from "esbuild";

import type {
  AudioThreadEvent,
  AudioThreadEventMessage,
  AudioThreadMessage,
  NativeSessionSnapshot,
  TrackIdentity,
} from "../src/utils/tauri/audio/protocol";

type NativeRustSoundModule = typeof import("../src/utils/tauri/audio/nativeRustSound");

const bundled = buildSync({
  entryPoints: ["src/utils/tauri/audio/nativeRustSound.ts"],
  bundle: true,
  platform: "node",
  format: "cjs",
  packages: "external",
  define: { __GMPLAYER_TAURI_BUILD__: "true" },
  write: false,
}).outputFiles[0].text;

const wait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

const identity: TrackIdentity = { provider: "netease", id: "1001" };
const otherIdentity: TrackIdentity = { provider: "netease", id: "2002" };
const sourceUrl = "https://cdn.example/track-1001.mp3?token=session-a";

const session = (overrides: Partial<NativeSessionSnapshot> = {}): NativeSessionSnapshot => ({
  personalFm: null,
  hasTrack: true,
  musicId: `local:${sourceUrl}`,
  identity,
  playlistIndex: 3,
  position: 187.4,
  duration: 241.2,
  isPlaying: true,
  volume: 0.8,
  manifestRevision: 7,
  plannerActive: true,
  ...overrides,
});

const statusFor = (snapshot: NativeSessionSnapshot, position: number): AudioThreadEvent => ({
  type: "syncStatus",
  data: {
    musicId: snapshot.musicId,
    musicInfo: {
      name: "",
      artist: "",
      album: "",
      lyric: "",
      coverMediaType: "",
      cover: null,
      comment: "",
      duration: snapshot.duration,
      position,
    },
    isPlaying: snapshot.isPlaying,
    duration: snapshot.duration,
    position,
    volume: snapshot.volume,
    loadPosition: 0,
    playlistInited: true,
    playlist: [],
    currentPlayIndex: snapshot.playlistIndex,
    quality: { bitrate: 320, sampleRate: 44100, channels: 2 },
    identity: snapshot.identity,
    timelineEpoch: 12,
  },
});

/**
 * A fake backend: `audio_get_session` answers at once from `snapshot`, while
 * the `syncStatus` reply is held for `statusDelayMs` (Infinity = never) — the
 * player loop being busy with a download. A `setPlaylist` is acknowledged with
 * the `loadAudio` the real backend would emit after restarting the track.
 */
function environment(
  snapshot: NativeSessionSnapshot | null,
  statusDelayMs: number,
  clock?: { now: number },
) {
  const sent: AudioThreadMessage[] = [];
  let channel: { onmessage?: (envelope: AudioThreadEventMessage<AudioThreadEvent>) => void } | null =
    null;
  let seq = 0;
  const emit = (data: AudioThreadEvent, sentAt?: number) => {
    channel?.onmessage?.({ callbackId: "", data, seq: ++seq, sentAt });
  };

  class Channel {
    onmessage?: (envelope: AudioThreadEventMessage<AudioThreadEvent>) => void;
  }

  const invoke = async (cmd: string, args?: Record<string, any>) => {
    switch (cmd) {
      case "audio_subscribe_events":
        channel = args?.channel;
        return undefined;
      case "audio_get_session":
        return snapshot;
      case "audio_send_msg": {
        const msg = args?.msg?.data as AudioThreadMessage;
        sent.push(msg);
        if (msg.type === "syncStatus" && snapshot?.hasTrack && Number.isFinite(statusDelayMs)) {
          setTimeout(() => emit(statusFor(snapshot, snapshot.position + statusDelayMs / 1000)), statusDelayMs);
        }
        if (msg.type === "setPlaylist" && msg.playIndex !== undefined) {
          const song = msg.songs[msg.playIndex];
          setTimeout(() => {
            emit({
              type: "loadAudio",
              data: {
                musicId: `local:${song.filePath}`,
                musicInfo: {
                  name: "",
                  artist: "",
                  album: "",
                  lyric: "",
                  coverMediaType: "",
                  cover: null,
                  comment: "",
                  duration: 241.2,
                  position: msg.initialPosition ?? 0,
                },
                quality: { bitrate: 320, sampleRate: 44100, channels: 2 },
                currentPlayIndex: 0,
                loadRequestId: msg.loadRequestId,
                identity,
                timelineEpoch: 13,
              },
            });
          }, 0);
        }
        return undefined;
      }
      default:
        throw new Error(`unexpected invoke ${cmd}`);
    }
  };

  const window: Record<string, unknown> = {
    __TAURI__: { core: { invoke }, event: { listen: async () => () => {} } },
    navigator: { userAgent: "Android", platform: "Linux armv8l", maxTouchPoints: 5 },
    dispatchEvent: () => true,
    setTimeout,
    clearTimeout,
    $player: undefined,
  };

  const module = { exports: {} as NativeRustSoundModule };
  let dateGlobal: DateConstructor = Date;
  if (clock) {
    class ClockDate extends Date {
      static override now(): number {
        return clock.now;
      }
    }
    dateGlobal = ClockDate;
  }
  runInNewContext(bundled, {
    module,
    exports: module.exports,
    console,
    window,
    setTimeout,
    clearTimeout,
    queueMicrotask,
    Date: dateGlobal,
    CustomEvent: class {
      constructor(
        public type: string,
        public init?: unknown,
      ) {}
    },
    require(id: string) {
      assert.equal(id, "@tauri-apps/api/core");
      return { invoke, Channel };
    },
  });

  const restarts = () =>
    sent.filter(
      (msg) =>
        (msg.type === "setPlaylist" && msg.playIndex !== undefined) ||
        msg.type === "jumpToSong" ||
        msg.type === "jumpToSongAt",
    );

  return { NativeRustSound: module.exports.NativeRustSound, window, sent, restarts, emit };
}

async function attach(env: ReturnType<typeof environment>, storedPosition: number) {
  const sound = new env.NativeRustSound(sourceUrl);
  env.window.$player = sound;
  let loaded = 0;
  sound.on("load", () => loaded++);
  await sound.load(storedPosition, { attachIdentity: identity });
  return { sound, loaded: () => loaded };
}

test("a late status reply attaches from the session snapshot instead of restarting", async () => {
  // The store still holds the track's load anchor from before the background
  // run, and the player loop is busy for longer than the status wait.
  const env = environment(session(), 700);
  const { sound, loaded } = await attach(env, 0);

  assert.equal(loaded(), 1);
  assert.deepEqual(env.restarts(), [], "a track the backend is playing must not be reloaded");
  assert.equal(sound.playing(), true);
  assert.ok(Math.abs((sound.seek() as number) - 187.4) < 1, `seek ${sound.seek()}`);

  // The reply lands afterwards: same track, so it only refines the clock.
  await wait(500);
  assert.deepEqual(env.restarts(), []);
  assert.equal(sound.playing(), true);
  assert.ok((sound.seek() as number) >= 187.4);
});

test("a status reply that never arrives still attaches", async () => {
  const env = environment(session({ isPlaying: false, position: 42 }), Infinity);
  const { sound, loaded } = await attach(env, 0);

  assert.equal(loaded(), 1);
  assert.deepEqual(env.restarts(), []);
  assert.equal(sound.playing(), false);
  assert.ok(Math.abs((sound.seek() as number) - 42) < 1e-6);
});

test("a prompt status reply attaches as before", async () => {
  const env = environment(session(), 20);
  const { sound, loaded } = await attach(env, 0);

  assert.equal(loaded(), 1);
  assert.deepEqual(env.restarts(), []);
  assert.equal(sound.playing(), true);
  assert.equal(env.sent.filter((msg) => msg.type === "syncStatus").length, 1);
});

const trackStatus = (
  position: number,
  isPlaying: boolean,
  epoch = 13,
): AudioThreadEvent => ({
  type: "syncStatus",
  data: {
    musicId: `local:${sourceUrl}`,
    musicInfo: {
      name: "",
      artist: "",
      album: "",
      lyric: "",
      coverMediaType: "",
      cover: null,
      comment: "",
      duration: 241.2,
      position,
    },
    isPlaying,
    duration: 241.2,
    position,
    volume: 1,
    loadPosition: 0,
    playlistInited: true,
    playlist: [],
    currentPlayIndex: 0,
    quality: { bitrate: 320, sampleRate: 44100, channels: 2 },
    identity,
    timelineEpoch: epoch,
  },
});

test("direct play anchors lyrics on the backend sample instead of a cold-start lead", async () => {
  const clock = { now: 1_700_000_000_000 };
  const env = environment(null, Infinity, clock);
  const sound = new env.NativeRustSound(sourceUrl);
  env.window.$player = sound;
  const loading = sound.load(0);

  // LoadAudio, then the same order the backend emits: PlayStatus, then the
  // position snapshot. The lyric clock reads `seek()` in between.
  await wait(30);
  env.emit({ type: "playStatus", data: { isPlaying: true } });
  clock.now += 500;
  const lead = sound.seek() as number;
  assert.ok(lead > 0.4, `wall clock should have run ahead, got ${lead}`);

  env.emit(trackStatus(0.02, true), clock.now - 20);
  const anchored = sound.seek() as number;
  assert.ok(Math.abs(anchored - 0.02) < 1e-6, `start anchor ${anchored}`);

  // Later heartbeats still follow the backend, including delivery compensation.
  clock.now += 1_000;
  env.emit(
    { type: "playPosition", data: { position: 1.02, timelineEpoch: 13 } },
    clock.now - 40,
  );
  const tracked = sound.seek() as number;
  assert.ok(Math.abs(tracked - 1.06) < 1e-6, `tracked ${tracked}`);

  // Pause adopts the backend position even when it disagrees, then resume
  // extrapolates from that anchor rather than from the old lead.
  sound.pause();
  env.emit({ type: "playStatus", data: { isPlaying: false } });
  env.emit({ type: "playPosition", data: { position: 1.2, timelineEpoch: 13 } });
  assert.ok(Math.abs((sound.seek() as number) - 1.2) < 1e-6);

  sound.play();
  env.emit({ type: "playStatus", data: { isPlaying: true } });
  clock.now += 500;
  const resumed = sound.seek() as number;
  assert.ok(Math.abs(resumed - 1.7) < 1e-6, `resumed ${resumed}`);

  // Seek still ignores a packet from before the seek.
  sound.seek(40);
  clock.now += 200;
  env.emit(
    { type: "playPosition", data: { position: 1.9, timelineEpoch: 13 } },
    clock.now - 50,
  );
  const afterSeek = sound.seek() as number;
  assert.ok(Math.abs(afterSeek - 40.2) < 1e-6, `after seek ${afterSeek}`);

  await loading;
});

test("a pre-play position zero does not spend the start anchor", async () => {
  const clock = { now: 1_700_000_000_000 };
  const env = environment(null, Infinity, clock);
  const sound = new env.NativeRustSound(sourceUrl);
  env.window.$player = sound;
  const loading = sound.load(0);
  await wait(30);

  // Published while the output is still paused for prebuffer. Playback has
  // not started on this side, so this must not be the anchor that sticks.
  env.emit({ type: "playPosition", data: { position: 0, timelineEpoch: 13 } });
  env.emit({ type: "playStatus", data: { isPlaying: true } });
  clock.now += 500;
  assert.ok((sound.seek() as number) > 0.4);

  env.emit({ type: "playPosition", data: { position: 0.02, timelineEpoch: 13 } });
  assert.ok(Math.abs((sound.seek() as number) - 0.02) < 1e-6, `late anchor ${sound.seek()}`);

  await loading;
});

test("a small cold-start lead is snapped instead of held for the rest of the track", async () => {
  const clock = { now: 1_700_000_000_000 };
  const env = environment(null, Infinity, clock);
  const sound = new env.NativeRustSound(sourceUrl);
  env.window.$player = sound;
  const loading = sound.load(0);
  await wait(30);

  env.emit({ type: "playStatus", data: { isPlaying: true } });
  clock.now += 80;
  assert.ok(Math.abs((sound.seek() as number) - 0.08) < 1e-6);

  // 60ms behind the lyric clock: inside the smoother's hold, which used to
  // freeze the lead until pause. The start anchor takes the backend sample.
  env.emit(
    { type: "playPosition", data: { position: 0.02, timelineEpoch: 13 } },
    clock.now,
  );
  assert.ok(Math.abs((sound.seek() as number) - 0.02) < 1e-6, `held lead ${sound.seek()}`);

  await loading;
});

test("startup resume keeps a saved position when the first sample is still zero", async () => {
  const clock = { now: 1_700_000_000_000 };
  const env = environment(null, Infinity, clock);
  const sound = new env.NativeRustSound(sourceUrl);
  env.window.$player = sound;
  const loading = sound.load(30);
  await wait(30);

  env.emit({ type: "playStatus", data: { isPlaying: true } });
  clock.now += 500;
  const lead = sound.seek() as number;
  assert.ok(Math.abs(lead - 30.5) < 1e-6, `resume lead ${lead}`);

  env.emit({ type: "playPosition", data: { position: 0, timelineEpoch: 13 } });
  assert.ok(
    Math.abs((sound.seek() as number) - 30.5) < 1e-6,
    `pre-seek zero moved the resume clock to ${sound.seek()}`,
  );

  env.emit(trackStatus(30.02, true));
  assert.ok(Math.abs((sound.seek() as number) - 30.02) < 1e-6, `resume anchor ${sound.seek()}`);

  await loading;
});

test("a backend on another track is not adopted and the track loads normally", async () => {
  const env = environment(session({ identity: otherIdentity, musicId: "local:https://cdn.example/other" }), Infinity);
  const { loaded } = await attach(env, 30);

  assert.equal(loaded(), 1);
  const [restart, ...rest] = env.restarts();
  assert.deepEqual(rest, []);
  assert.ok(restart && restart.type === "setPlaylist");
  assert.equal(restart.playIndex, 0);
  assert.equal(restart.initialPosition, 30);
});
