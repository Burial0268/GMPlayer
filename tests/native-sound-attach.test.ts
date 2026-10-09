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
function environment(snapshot: NativeSessionSnapshot | null, statusDelayMs: number) {
  const sent: AudioThreadMessage[] = [];
  let channel: { onmessage?: (envelope: AudioThreadEventMessage<AudioThreadEvent>) => void } | null =
    null;
  let seq = 0;
  const emit = (data: AudioThreadEvent) => {
    channel?.onmessage?.({ callbackId: "", data, seq: ++seq });
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
  runInNewContext(bundled, {
    module,
    exports: module.exports,
    console,
    window,
    setTimeout,
    clearTimeout,
    Date,
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
