import assert from "node:assert/strict";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import { buildSync } from "esbuild";

import type {
  AudioThreadEvent,
  AudioThreadMessage,
  EventEnvelope,
  NativeSessionSnapshot,
  TrackIdentity,
} from "../src/utils/tauri/audio/protocol";

type NativeRustSoundModule = typeof import("../src/utils/AudioContext/sound/NativeRustSound") &
  typeof import("../src/utils/AudioContext/sound/SoundManager");

const bundled = buildSync({
  // One bundle, so the sound and the manager that names it active share a module.
  stdin: {
    contents: `export * from "./src/utils/AudioContext/sound/NativeRustSound";
export { SoundManager } from "./src/utils/AudioContext/sound/SoundManager";`,
    resolveDir: process.cwd(),
    loader: "ts",
  },
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
 * A fake backend: every subscription opens with a `hello` holding `snapshot`
 * (it never enters the player loop), while the `syncStatus` reply is held for
 * `statusDelayMs` (Infinity = never) — the player loop being busy with a
 * download. A `setPlaylist` is acknowledged with the `loadAudio` the real
 * backend would emit after restarting the track. Envelopes are stamped the way
 * the bus stamps them: the epoch an event names, the last command received.
 */
function environment(snapshot: NativeSessionSnapshot | null, statusDelayMs: number) {
  const sent: AudioThreadMessage[] = [];
  let channel: { onmessage?: (envelope: EventEnvelope) => void } | null = null;
  let seq = 0;
  let epoch = 0;
  let ack = 0;
  const emit = (data: AudioThreadEvent) => {
    const named = (data as any).data?.timelineEpoch;
    if (typeof named === "number") epoch = Math.max(epoch, named);
    channel?.onmessage?.({ seq: ++seq, epoch, ack, data });
  };

  class Channel {
    onmessage?: (envelope: EventEnvelope) => void;
  }

  const invoke = async (cmd: string, args?: Record<string, any>) => {
    switch (cmd) {
      case "audio_subscribe_events":
        channel = args?.channel;
        emit({
          type: "hello",
          data: { bootId: 0, snapshot: snapshot ?? ({ hasTrack: false } as NativeSessionSnapshot) },
        });
        return undefined;
      case "audio_send_msg": {
        const msg = args?.msg?.data as AudioThreadMessage;
        ack = args?.msg?.cmd ?? ack;
        sent.push(msg);
        if (msg.type === "syncStatus" && snapshot?.hasTrack && Number.isFinite(statusDelayMs)) {
          setTimeout(
            () => emit(statusFor(snapshot, snapshot.position + statusDelayMs / 1000)),
            statusDelayMs,
          );
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
            // An autoplay load starts playback itself, right after LoadAudio.
            if (msg.autoplay) emit({ type: "playStatus", data: { isPlaying: true } });
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
  };

  const module = { exports: {} as NativeRustSoundModule };
  runInNewContext(bundled, {
    module,
    exports: module.exports,
    console,
    window,
    setTimeout,
    clearTimeout,
    queueMicrotask,
    Date,
    CustomEvent: class {
      constructor(
        public type: string,
        public init?: unknown,
      ) {}
    },
    require(id: string) {
      if (id === "vue") return require("vue");
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

  return { ...module.exports, window, sent, restarts, emit };
}

async function attach(env: ReturnType<typeof environment>, storedPosition: number) {
  const sound = new env.NativeRustSound(sourceUrl);
  env.SoundManager.setCurrentSound(sound);
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
  assert.ok(Math.abs(sound.position - 187.4) < 1, `position ${sound.position}`);

  // The reply lands afterwards: same track, so it only refines the clock.
  await wait(500);
  assert.deepEqual(env.restarts(), []);
  assert.equal(sound.playing(), true);
  assert.ok(sound.position >= 187.4);
});

test("a status reply that never arrives still attaches", async () => {
  const env = environment(session({ isPlaying: false, position: 42 }), Infinity);
  const { sound, loaded } = await attach(env, 0);

  assert.equal(loaded(), 1);
  assert.deepEqual(env.restarts(), []);
  assert.equal(sound.playing(), false);
  assert.ok(Math.abs(sound.position - 42) < 1e-6);
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
  const env = environment(
    session({ identity: otherIdentity, musicId: "local:https://cdn.example/other" }),
    Infinity,
  );
  const { loaded } = await attach(env, 30);

  assert.equal(loaded(), 1);
  const [restart, ...rest] = env.restarts();
  assert.deepEqual(rest, []);
  assert.ok(restart && restart.type === "setPlaylist");
  assert.equal(restart.playIndex, 0);
  assert.equal(restart.initialPosition, 30);
});

test("an autoplay load starts playing without a play event before load or a second resume", async () => {
  const env = environment(null, Infinity);
  const sound = new env.NativeRustSound(sourceUrl);
  env.SoundManager.setCurrentSound(sound);
  const events: string[] = [];
  for (const name of ["load", "play", "pause"] as const) sound.on(name, () => events.push(name));
  await sound.load(0, { autoplay: true });

  const [load] = env.restarts();
  assert.ok(load && load.type === "setPlaylist");
  assert.equal(load.autoplay, true);
  assert.deepEqual(events, ["load"], "the load handler starts the track, once");
  assert.equal(sound.playing(), true);
  sound.play();
  assert.deepEqual(
    env.sent.filter((msg) => msg.type === "resumeAudio"),
    [],
    "the backend is already playing",
  );
});

test("one backend track change is announced once, whole, however many events describe it", async () => {
  const env = environment(session(), 20);
  const { sound } = await attach(env, 0);
  const changes: any[] = [];
  sound.on("trackchange", (change) => changes.push(change));

  const next = session({
    musicId: "local:https://cdn.example/track-2002.mp3?token=rust",
    identity: otherIdentity,
    playlistIndex: 4,
    position: 0,
    duration: 200,
  });
  const info = (statusFor(next, 0) as any).data.musicInfo;
  const quality = { bitrate: 320, sampleRate: 44100, channels: 2 };
  const changed: AudioThreadEvent = {
    type: "trackChanged",
    data: {
      timelineEpoch: 13,
      musicId: next.musicId,
      identity: next.identity,
      currentPlayIndex: next.playlistIndex,
      musicInfo: info,
      quality,
      isPlaying: true,
    },
  };
  const status = statusFor(next, 0) as any;
  status.data.timelineEpoch = 13;

  // The order the backend emits a planner advance in, plus a duplicate edge.
  env.emit(changed);
  env.emit(changed);
  env.emit({
    type: "loadAudio",
    data: {
      musicId: next.musicId,
      musicInfo: info,
      quality,
      currentPlayIndex: next.playlistIndex,
      identity: next.identity,
      timelineEpoch: 13,
    },
  });
  env.emit(status);
  env.emit({
    type: "nativePlannerAdvanced",
    data: {
      manifestRevision: 7,
      identity: otherIdentity,
      playlistIndex: next.playlistIndex,
      musicId: next.musicId,
    },
  } as AudioThreadEvent);
  env.emit(status);

  assert.equal(changes.length, 1);
  assert.deepEqual(
    { ...changes[0], position: Math.round(changes[0].position) },
    {
      timelineEpoch: 13,
      identity: otherIdentity,
      musicId: next.musicId,
      playlistIndex: 4,
      position: 0,
      duration: 200,
    },
  );
  // The new track's clock, not the old track's elapsed time carried over.
  assert.ok(sound.position < 5, `position ${sound.position}`);
  assert.equal(sound.duration(), 200);

  // A load the frontend asked for is not a backend-initiated change.
  env.emit({ ...changed, data: { ...(changed as any).data, timelineEpoch: 14, loadRequestId: 9 } });
  assert.equal(changes.length, 1);
});

test("a play status emitted before a pause was processed does not undo the pause", async () => {
  const env = environment(session(), 20);
  const { sound } = await attach(env, 0);
  assert.equal(sound.playing(), true);
  const events: string[] = [];
  for (const name of ["play", "pause"] as const) sound.on(name, () => events.push(name));

  sound.pause();
  // Still in flight: the backend has not taken the pause up, so this describes
  // the state the user just left.
  env.emit({ type: "playStatus", data: { isPlaying: true } });
  await wait(0);
  assert.equal(sound.playing(), false);
  // The pause's own answer, which acknowledges it.
  env.emit({ type: "playStatus", data: { isPlaying: false } });
  await wait(0);
  assert.equal(sound.playing(), false);
  assert.deepEqual(events, ["pause"], "one pause, no play bounced back in between");
});
