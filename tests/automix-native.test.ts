import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import { transformSync } from "esbuild";
import { identityForSong } from "../src/utils/tauri/audio/identity";

function environment(deferResolution = false) {
  let now = 0;
  let timerId = 0;
  let playing = true;
  const timers = new Map<number, { at: number; callback: () => void }>();
  const sent: unknown[] = [];
  const commits: number[] = [];
  const resolutions: Array<(value: unknown) => void> = [];
  const sound = {
    getSourceUrl: () => "https://example.test/current.mp3",
    playing: () => playing,
    duration: () => 100,
    pause() {
      playing = false;
    },
    play() {
      playing = true;
    },
  };
  const music = {
    persistData: {
      playlists: [
        { id: 1, name: "A" },
        { id: 2, name: "B" },
      ],
      playSongIndex: 0,
      playSongMode: "normal",
      personalFmMode: false,
    },
    commitPlaySongIndex(index: number) {
      commits.push(index);
      this.persistData.playSongIndex = index;
    },
    resetSongLyricState() {},
  };
  const imports: Record<string, unknown> = {
    "../SoundManager": { SoundManager: { getCurrentSound: () => sound } },
    "../PlayerFunctions": { syncNativeAutoMixCurrentSound: async () => {} },
    "../../tauri/audio/backendRuntime": { isNativeAudioBackendAvailable: () => true },
    "../../tauri/audio/identity": { identityForSong },
    "../../tauri/audio/transport": {
      getAudioBackendTransport: () => ({
        sendOrQueue: (msg: unknown) => sent.push(msg),
        connect: async () => {},
        subscribe: () => () => {},
      }),
    },
    "../../tauri/audio/bridge": {
      audioSendMsg: async (msg: unknown) => {
        sent.push(msg);
      },
    },
    "../resolveSongUrl": {
      resolveSongUrl: () =>
        deferResolution
          ? new Promise((resolve) => resolutions.push(resolve))
          : Promise.resolve({ url: "https://example.test/next.mp3", source: "test" }),
    },
  };
  const module = { exports: {} as any };
  const code = transformSync(
    readFileSync("src/utils/AudioContext/AutoMix/TransitionStateMachine.ts", "utf8"),
    {
      loader: "ts",
      format: "cjs",
      define: { "import.meta.env.DEV": "false" },
    },
  ).code;
  runInNewContext(code, {
    module,
    exports: module.exports,
    console,
    Date: { now: () => now },
    setTimeout(callback: () => void, delay: number) {
      const id = ++timerId;
      timers.set(id, { at: now + delay, callback });
      return id;
    },
    clearTimeout(id: number) {
      timers.delete(id);
    },
    require(id: string) {
      return imports[id] ?? {};
    },
  });
  const state = new module.exports.TransitionStateMachine(
    { getProgress: () => 0, cancel() {} },
    { cleanup() {} },
    {},
    {},
    { cleanup() {} },
  );
  state._storesReady = true;
  state._musicStoreRef = music;
  state._settingStoreRef = { autoMixEnabled: true };
  state._state = "waiting";
  state._nativeAutoMixReady = true;
  state._nativeNextIndex = 1;
  const transitionId = state._beginTransition(
    "native",
    0,
    1,
    "waiting",
    music.persistData.playlists[0],
    music.persistData.playlists[1],
  );
  return {
    state,
    sound,
    music,
    sent,
    commits,
    transitionId,
    resolveNext() {
      resolutions.shift()?.({ url: "https://example.test/next.mp3", source: "test" });
    },
    event(type: string, data: unknown) {
      state._handleNativeAutoMixEvent({ type, data });
    },
    advance(milliseconds: number) {
      now += milliseconds;
      for (const [id, timer] of timers) {
        if (timer.at <= now) {
          timers.delete(id);
          timer.callback();
        }
      }
    },
  };
}

test("a native crossfade survives a long pause and accepts the backend's eventual handoff", () => {
  const env = environment();
  env.event("automixCrossfadeStarted", { duration: 8, transitionId: env.transitionId });
  assert.equal(env.state.pauseCrossfade(), true);
  env.advance(60_000);
  assert.equal(
    env.state.getState(),
    "crossfading",
    "wall time must not fail a paused native transition",
  );
  assert.deepEqual(env.commits, []);
  env.state.resumeCrossfade();
  env.event("automixCrossfadeComplete", { currentIndex: 1, transitionId: env.transitionId });
  assert.deepEqual(env.commits, [1]);
  assert.equal(env.state.getState(), "finishing");
});

test("pausing while native analysis or preparation is pending preserves the backend plan", () => {
  for (const phase of ["analyzing", "waiting"]) {
    const env = environment();
    env.state._state = phase;
    env.sound.pause();
    env.state.monitorPlayback(env.sound);
    assert.equal(env.state.getState(), phase);
    assert.deepEqual(env.sent, [], "pause must not send automixCancel");
    assert.equal(env.state._activeTransition.id, env.transitionId);
  }
});

test("a backend track advance re-arms native preparation instead of retaining the previous ready flag", () => {
  const env = environment(true);
  env.music.persistData.playSongIndex = 1;
  env.state.onTrackStarted(env.sound, 2);
  assert.equal(env.state.getState(), "analyzing");
  assert.equal(env.state._nativeAutoMixPreparing, true);
  assert.equal(env.state._nativeAutoMixReady, false);
  assert.equal(env.state._nativeNextIndex, 0);
  assert.equal(env.state._activeTransition.fromSongId, 2);
  assert.notEqual(env.state._activeTransition.id, env.transitionId);
  assert.deepEqual(env.sent, [], "re-arming must not cancel the backend's playing sound");
});

test("a late URL resolution cannot prepare the retired track or clear a newer prepare", async () => {
  const env = environment(true);
  env.state._nativeAutoMixReady = false;
  env.state._startNativeAutoMixPrepare(100);
  const retired = env.state._activeTransition.id;
  env.state._clearTransition(retired);
  env.state._nativeAutoMixPreparing = false;
  env.state._nativeAutoMixReady = false;
  env.state._startNativeAutoMixPrepare(100);
  const current = env.state._activeTransition.id;
  assert.notEqual(current, retired);
  env.resolveNext();
  await new Promise<void>((resolve) => setImmediate(resolve));
  assert.deepEqual(env.sent, [], "retired resolution must not publish configure/prepare");
  assert.equal(
    env.state._nativeAutoMixPreparing,
    true,
    "old finally must not clear the new in-flight state",
  );
  env.resolveNext();
  await new Promise<void>((resolve) => setImmediate(resolve));
  const prepares = env.sent.filter((msg: any) => msg.type === "automixPrepareNext") as any[];
  assert.equal(prepares.length, 1);
  assert.equal(prepares[0].transitionId, current);
  assert.deepEqual(prepares[0].currentIdentity, { provider: "netease", id: "1" });
  assert.deepEqual(prepares[0].nextIdentity, { provider: "netease", id: "2" });
});
