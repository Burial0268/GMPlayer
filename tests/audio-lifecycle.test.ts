import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import { transformSync } from "esbuild";
import { effectScope, nextTick, reactive, ref, toRaw, watch } from "vue";
import { createPinia, defineStore } from "pinia";

const require = createRequire(resolve("package.json"));

// Exercise the production modules with controllable IPC and lifecycle boundaries.
function sourceModule(path: string, imports: Record<string, unknown>, globals = {}) {
  const module = { exports: {} as any };
  const code = transformSync(readFileSync(path, "utf8"), {
    loader: "ts",
    format: "cjs",
    define: { "import.meta.hot": "false", "import.meta.env.DEV": "false" },
  }).code;
  runInNewContext(code, {
    module,
    exports: module.exports,
    console,
    crypto: globalThis.crypto,
    queueMicrotask,
    setTimeout,
    clearTimeout,
    ...globals,
    require(id: string) {
      return imports[id] ?? {};
    },
  });
  return module.exports;
}

const musicTypes = sourceModule("src/store/musicTypes.ts", {});
const playlistOrder = sourceModule("src/utils/playlistOrder.ts", {});
const keyOf = (identity: any) =>
  identity ? `${identity.provider}:${identity.id ?? identity.path}` : null;
const identities = (ids: number[]) => ids.map((id) => ({ provider: "netease", id: String(id) }));

function queueEnvironment(native = true) {
  let music: any;
  let publisher: any;
  let shuffleDraws = 0;
  const math = Object.assign(Object.create(Math), {
    random: () => {
      shuffleDraws++;
      return 0.25;
    },
  });
  const window: any = {};
  const storage = new Map<string, string>();
  const localStorage = {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  };
  class NativeRustSound {
    manifests: any[] = [];
    isDestroyed() {
      return false;
    }
    setNativePlannerEnabled() {}
    setNativeManifest(manifest: any) {
      this.manifests.push(manifest);
    }
  }
  const storeOptions = sourceModule(
    "src/store/musicData.ts",
    {
      pinia: { defineStore: (_id: string, options: any) => options },
      vue: { nextTick },
      "./musicTypes": musicTypes,
      "@/utils/getLanguageData": { __esModule: true, default: (key: string) => key },
      "@/utils/tauri/core/runtime": { isTauri: () => native },
      "@/utils/tauri/audio/nativeRustSound": { NativeRustSound },
      "@/utils/tauri/audio/identity": { trackIdentityKey: keyOf },
      "@/utils/tauri/core/nativeLog": { logNative() {} },
      "@/utils/playlistOrder": playlistOrder,
      "@/utils/rawEntry": { asRawEntries: (songs: any[]) => songs.slice() },
      "@/utils/AudioContext": {
        requestNativeReshuffle: () => publisher.requestNativeReshuffle(),
        publishNativeManifest: (...args: any[]) => publisher.publishNativeManifest(...args),
        publishSessionControls() {},
        cancelNativeQueuePrefill() {},
        getAudioPreloader: () => ({ cleanup() {} }),
        getAutoMixEngine: () => ({ isHandoffActive: () => false }),
      },
    },
    { window, Math: math, $message: { info() {} } },
  ).default;
  function restore(saved?: any) {
    music?.$dispose();
    const useStore = defineStore("musicData-lifecycle", {
      ...storeOptions,
      state: () => ({
        persistData: reactive(saved ?? musicTypes.createDefaultPersistData()),
        playingSongId: saved?.playlists?.[saved.playSongIndex]?.id ?? 1,
        preloadedSongIds: new Set(),
      }),
    });
    music = useStore(createPinia());
    music.afterQueueReordered = () => {};
    music.resetPlaySongTime = () => {};
    music.resetSongLyricState = () => {};
    const revision = sourceModule(
      "src/utils/AudioContext/NativeManifestRevision.ts",
      {},
      { localStorage },
    );
    publisher = sourceModule(
      "src/utils/AudioContext/NativeManifestPublisher.ts",
      {
        vue: { toRaw },
        "@/utils/tauri/core/runtime": { isTauri: () => native },
        "@/store/musicTypes": musicTypes,
        "@/utils/tauri/audio/nativeRustSound": { NativeRustSound, setPlannerRevisionObserver() {} },
        "@/store/musicData": { __esModule: true, default: () => music },
        "@/store/listenTogether": { __esModule: true, default: () => ({ isInRoom: false }) },
        "@/utils/tauri/audio/identity": { trackIdentityKey: keyOf },
        "@/utils/tauri/core/nativeLog": { logNative() {} },
        "./NativeManifestRevision": revision,
      },
      { window },
    );
    return music;
  }
  restore();
  return {
    restore,
    get music() {
      return music;
    },
    get publisher() {
      return publisher;
    },
    get shuffleDraws() {
      return shuffleDraws;
    },
    attach() {
      window.$player = new NativeRustSound();
      return window.$player;
    },
  };
}

function hydrateQueue(saved: any) {
  const scope = effectScope();
  const useStore = sourceModule(
    "src/store/musicPersistedData.ts",
    {
      pinia: { defineStore: (_id: string, setup: any) => setup },
      vue: { reactive, watch, toRaw },
      "throttle-debounce": require("throttle-debounce"),
      "./musicTypes": musicTypes,
      "@/utils/rawEntry": { asRawEntry: (song: any) => song },
    },
    {
      window: {
        localStorage: { getItem: () => JSON.stringify({ persistData: saved }) },
        addEventListener() {},
      },
      document: { addEventListener() {} },
    },
  ).default;
  const store = scope.run(() => useStore());
  scope.stop();
  return store.persistData;
}

function randomQueue() {
  return {
    ...musicTypes.createDefaultPersistData(),
    playSongMode: "random",
    playlists: [3, 1, 4, 2].map((id) => ({ id, name: String(id) })),
    preShuffleOrder: [1, 2, 3, 4],
    shuffleState: { id: "saved-pass", applied: true },
  };
}

test("natural order preserves stable ranks, duplicate occurrences and additions", () => {
  let random = 17;
  const next = () => (random = (Math.imul(random, 1664525) + 1013904223) >>> 0);
  for (let scenario = 0; scenario < 250; scenario++) {
    const snapshot = Array.from({ length: next() % 50 }, () => next() % 25);
    const list = Array.from({ length: next() % 80 }, (_, occurrence) => ({
      id: next() % 30,
      occurrence,
    }));
    const original = list.slice();
    const rank = new Map<number, number>();
    snapshot.forEach((id, index) => {
      if (!rank.has(id)) rank.set(id, index);
    });
    const expected = list
      .slice()
      .sort((a, b) => (rank.get(a.id) ?? snapshot.length) - (rank.get(b.id) ?? snapshot.length));
    const actual = playlistOrder.naturalQueueOrder(list, snapshot);
    assert.deepEqual(
      Array.from(actual, (index: number) => list[index]),
      expected,
    );
    assert.deepEqual(list, original, "restoring the order must not mutate the live queue");
  }
});

test("natural order is cached across cursor changes and invalidated by list or origin edits", () => {
  const env = queueEnvironment();
  env.restore(randomQueue());
  const first = env.music.getNaturalPlaylistOrder;
  const ids = (order: number[]) =>
    Array.from(order, (index) => env.music.persistData.playlists[index].id);
  assert.deepEqual(ids(first), [1, 2, 3, 4]);
  env.music.persistData.playSongIndex = 2;
  assert.equal(env.music.getNaturalPlaylistOrder, first);
  env.music.persistData.shuffleState = { id: "another-pass", applied: true };
  assert.equal(env.music.getNaturalPlaylistOrder, first);
  env.music.persistData.playlists.push({ id: 5, name: "5" });
  const added = env.music.getNaturalPlaylistOrder;
  assert.notEqual(added, first);
  assert.deepEqual(ids(added), [1, 2, 3, 4, 5]);
  env.music.persistData.preShuffleOrder = [4, 3, 2, 1];
  assert.notEqual(env.music.getNaturalPlaylistOrder, added);
  assert.deepEqual(ids(env.music.getNaturalPlaylistOrder), [4, 3, 2, 1, 5]);
  const beforeMirror = env.music.getNaturalPlaylistOrder;
  env.music.persistData.playlists.reverse();
  assert.notEqual(env.music.getNaturalPlaylistOrder, beforeMirror);
  assert.deepEqual(ids(env.music.getNaturalPlaylistOrder), [4, 3, 2, 1, 5]);
});

test("an applied shuffle survives repeated hydrate and startup without reshuffling", async () => {
  const env = queueEnvironment();
  let saved = randomQueue();
  for (let boot = 0; boot < 3; boot++) {
    env.restore(JSON.parse(JSON.stringify(saved)));
    const sound = env.attach();
    env.music.ensureShuffledInRandomMode();
    env.publisher.publishNativeManifest();
    await Promise.resolve();
    assert.equal(sound.manifests.length, 1);
    assert.equal(sound.manifests[0].reshuffle, false);
    assert.equal(sound.manifests[0].shuffle.id, "saved-pass");
    assert.deepEqual(
      Array.from(env.music.persistData.playlists, (s: any) => s.id),
      [3, 1, 4, 2],
    );
    saved = JSON.parse(JSON.stringify(env.music.persistData));
  }
});

test("a pending shuffle keeps its id across retries and reload; only its answer completes it", async () => {
  const env = queueEnvironment();
  env.restore(randomQueue());
  env.publisher.requestNativeReshuffle();
  const id = env.music.persistData.shuffleState.id;
  const sound = env.attach();
  await Promise.resolve();
  env.publisher.publishNativeManifest({ force: true });
  await Promise.resolve();
  assert.equal(sound.manifests.length, 2);
  assert.equal(sound.manifests[0].shuffle.id, id);
  assert.equal(sound.manifests[1].shuffle.id, id);
  env.restore(JSON.parse(JSON.stringify(env.music.persistData)));
  env.attach();
  env.music.ensureShuffledInRandomMode();
  assert.equal(env.music.persistData.shuffleState.id, id);
  env.music.adoptBackendQueueOrder(identities([1, 2, 3, 4]), 100, { shuffleId: "old-request" });
  assert.equal(env.music.persistData.shuffleState.applied, false);
  assert.equal(env.music.persistData.playlists[0].id, 3);
  const selected: number[] = [];
  const stop = watch(
    () => env.music.getPlaySongData.id,
    (id) => selected.push(id),
    { flush: "sync" },
  );
  env.music.adoptBackendQueueOrder(identities([1, 2, 3, 4]), 101, { shuffleId: id });
  assert.deepEqual(selected, [], "mirroring must not trigger a track load");
  assert.equal(env.music.getPlaySongData.id, 3);
  assert.equal(env.music.persistData.shuffleState.applied, true);
  env.music.ensureShuffledInRandomMode();
  assert.equal(env.music.persistData.shuffleState.id, id);
  stop();
  await Promise.resolve();
});

test("an unchanged permutation still completes its transaction", () => {
  const env = queueEnvironment();
  env.restore(randomQueue());
  env.music.persistData.shuffleState = { id: "pending", applied: false };
  assert.equal(
    env.music.adoptBackendQueueOrder(identities([3, 1, 4, 2]), 1, { shuffleId: "pending" }),
    false,
  );
  assert.equal(env.music.persistData.shuffleState.applied, true);
});

test("manifest carries original identity order including local tracks, removals and additions", async () => {
  const env = queueEnvironment();
  const saved = randomQueue();
  saved.playlists = [3, -1, 1, 5].map((id) => ({
    id,
    name: String(id),
    ...(id < 0 ? { local: { uri: "/music/a.flac" } } : {}),
  }));
  saved.preShuffleOrder = [1, 2, -1, 3];
  env.restore(saved);
  const sound = env.attach();
  env.publisher.publishNativeManifest();
  await Promise.resolve();
  assert.deepEqual(JSON.parse(JSON.stringify(sound.manifests[0].naturalOrder)), [
    { provider: "netease", id: "1" },
    { provider: "local", path: "/music/a.flac" },
    { provider: "netease", id: "3" },
    { provider: "netease", id: "5" },
  ]);
  env.music.restorePlaylistOrder();
  assert.equal(env.music.persistData.shuffleState, null);
  assert.deepEqual(
    Array.from(env.music.persistData.playlists, (s: any) => s.id),
    [1, -1, 3, 5],
  );
});

test("legacy persisted queues migrate to an explicit completion or pending token", () => {
  for (const pending of [null, 42]) {
    const saved: any = randomQueue();
    delete saved.shuffleState;
    saved.pendingReshuffleRevision = pending;
    const persistData = hydrateQueue(saved);
    assert.equal(persistData.shuffleState.applied, pending === null);
    assert.equal("pendingReshuffleRevision" in persistData, false);
  }
});

test("persistence writes the applied token and its permutation in the same snapshot", async () => {
  const saved = randomQueue();
  saved.shuffleState = { id: "landing", applied: false };
  let raw = JSON.stringify({ persistData: saved });
  const writes: string[] = [];
  const localStorage = {
    getItem: () => raw,
    setItem(_key: string, value: string) {
      raw = value;
      writes.push(value);
    },
  };
  const scope = effectScope();
  const useStore = sourceModule(
    "src/store/musicPersistedData.ts",
    {
      pinia: { defineStore: (_id: string, setup: any) => setup },
      vue: { reactive, watch, toRaw: require("vue").toRaw },
      "throttle-debounce": require("throttle-debounce"),
      "./musicTypes": musicTypes,
      "@/utils/rawEntry": { asRawEntry: (song: any) => song },
    },
    {
      window: { localStorage, addEventListener() {} },
      document: { addEventListener() {} },
    },
  ).default;
  const persisted = scope.run(() => useStore());
  const env = queueEnvironment();
  env.restore(persisted.persistData);
  env.music.adoptBackendQueueOrder(identities([1, 2, 3, 4]), 10, { shuffleId: "landing" });
  await nextTick();
  assert.equal(writes.length, 1);
  const snapshot = JSON.parse(writes[0]).persistData;
  assert.deepEqual(
    snapshot.playlists.map((song: any) => song.id),
    [1, 2, 3, 4],
  );
  assert.deepEqual(snapshot.shuffleState, { id: "landing", applied: true });
  const marker = env.music.persistData.shuffleState;
  let mutations = 0;
  const stop = watch(env.music.persistData, () => mutations++);
  for (let revision = 11; revision < 14; revision++) {
    env.music.adoptBackendQueueOrder(identities([1, 2, 3, 4]), revision, { shuffleId: "landing" });
    await nextTick();
    assert.equal(env.music.persistData.shuffleState, marker);
    assert.equal(mutations, 0, "a duplicate confirmation must not dirty persistence");
    assert.equal(writes.length, 1);
  }
  stop();
  scope.stop();
});

test("web shuffle also records completion and clears it when restoring order", () => {
  const env = queueEnvironment(false);
  env.music.persistData.playSongMode = "random";
  env.music.setPlaylists([1, 2, 3, 4].map((id) => ({ id, name: String(id) })));
  assert.equal(env.music.persistData.shuffleState.applied, true);
  const id = env.music.persistData.shuffleState.id;
  env.music.ensureShuffledInRandomMode();
  assert.equal(env.music.persistData.shuffleState.id, id);
  env.music.restorePlaylistOrder();
  assert.equal(env.music.persistData.shuffleState, null);
  assert.deepEqual(
    Array.from(env.music.persistData.playlists, (s: any) => s.id),
    [1, 2, 3, 4],
  );
});

test("web mode changes pin the playing track and restore natural order without native IPC", async () => {
  const env = queueEnvironment(false);
  const saved = randomQueue();
  saved.playSongMode = "normal";
  saved.playlists = [1, 2, 3, 4].map((id) => ({ id, name: String(id) }));
  saved.playSongIndex = 2;
  saved.preShuffleOrder = [];
  saved.shuffleState = null;
  env.restore(saved);
  const sound = env.attach();
  const changedTracks: number[] = [];
  const stop = watch(
    () => env.music.getPlaySongData.id,
    (id) => changedTracks.push(id),
    { flush: "sync" },
  );
  env.music.setPlaySongMode("random");
  assert.equal(env.music.getPlaySongData.id, 3);
  assert.equal(env.music.persistData.playSongIndex, 0);
  assert.equal(env.music.persistData.shuffleState.applied, true);
  assert.equal(env.shuffleDraws, 3);
  const marker = env.music.persistData.shuffleState;
  env.music.setPlaySongMode("random");
  assert.equal(env.shuffleDraws, 3);
  assert.equal(env.music.persistData.shuffleState, marker);
  env.music.setPlaySongMode("single");
  assert.equal(env.music.persistData.shuffleState, null);
  assert.deepEqual(
    Array.from(env.music.persistData.playlists, (song: any) => song.id),
    [1, 2, 3, 4],
  );
  assert.equal(env.music.getPlaySongData.id, 3);
  env.music.setPlaySongMode("normal");
  assert.equal(env.shuffleDraws, 3);
  assert.deepEqual(changedTracks, []);
  await Promise.resolve();
  assert.equal(sound.manifests.length, 0);
  stop();
});

test("web refresh keeps a completed permutation, cursor and token across repeated hydrates", () => {
  const env = queueEnvironment(false);
  const saved = randomQueue();
  saved.playSongIndex = 2;
  for (let boot = 0; boot < 3; boot++) {
    env.restore(hydrateQueue(JSON.parse(JSON.stringify(saved))));
    env.music.ensureShuffledInRandomMode();
    assert.equal(env.shuffleDraws, 0);
    assert.equal(env.music.getPlaySongData.id, 4);
    assert.equal(env.music.persistData.playSongIndex, 2);
    assert.equal(env.music.persistData.shuffleState.id, "saved-pass");
    assert.deepEqual(
      Array.from(env.music.persistData.playlists, (song: any) => song.id),
      [3, 1, 4, 2],
    );
  }
});

test("web migration preserves an already shuffled legacy queue without drawing again", () => {
  const env = queueEnvironment(false);
  const saved: any = randomQueue();
  delete saved.shuffleState;
  env.restore(hydrateQueue(saved));
  env.music.ensureShuffledInRandomMode();
  assert.equal(env.shuffleDraws, 0);
  assert.equal(env.music.persistData.shuffleState.applied, true);
  assert.deepEqual(
    Array.from(env.music.persistData.playlists, (song: any) => song.id),
    [3, 1, 4, 2],
  );
  env.music.setPlaySongMode("normal");
  assert.deepEqual(
    Array.from(env.music.persistData.playlists, (song: any) => song.id),
    [1, 2, 3, 4],
  );
});

test("web startup upgrades an unshuffled legacy queue once and pins the playing track", () => {
  const env = queueEnvironment(false);
  const saved: any = randomQueue();
  delete saved.shuffleState;
  saved.playlists = [1, 2, 3, 4].map((id) => ({ id, name: String(id) }));
  saved.playSongIndex = 2;
  saved.preShuffleOrder = [];
  env.restore(hydrateQueue(saved));
  env.music.ensureShuffledInRandomMode();
  assert.equal(env.shuffleDraws, 3);
  assert.equal(env.music.getPlaySongData.id, 3);
  assert.equal(env.music.persistData.shuffleState.applied, true);
  const completed = JSON.parse(JSON.stringify(env.music.persistData));
  env.restore(hydrateQueue(completed));
  env.music.ensureShuffledInRandomMode();
  assert.equal(env.shuffleDraws, 3);
  assert.equal(env.music.persistData.shuffleState.id, completed.shuffleState.id);
  assert.deepEqual(
    Array.from(env.music.persistData.playlists, (song: any) => song.id),
    completed.playlists.map((song: any) => song.id),
  );
});

test("web completes a persisted pending shuffle locally and retains its original order", () => {
  for (const legacy of [false, true]) {
    const env = queueEnvironment(false);
    const saved: any = randomQueue();
    if (legacy) {
      delete saved.shuffleState;
      saved.pendingReshuffleRevision = 42;
    } else saved.shuffleState = { id: "native-pending", applied: false };
    env.restore(hydrateQueue(saved));
    const playing = env.music.getPlaySongData.id;
    env.music.ensureShuffledInRandomMode();
    assert.equal(env.shuffleDraws, 3);
    assert.equal(env.music.getPlaySongData.id, playing);
    assert.equal(env.music.persistData.shuffleState.applied, true);
    env.music.ensureShuffledInRandomMode();
    assert.equal(env.shuffleDraws, 3);
    env.music.setPlaySongMode("normal");
    assert.deepEqual(
      Array.from(env.music.persistData.playlists, (song: any) => song.id),
      [1, 2, 3, 4],
    );
  }
});

test("web replacing a random playlist starts a new completed shuffle", () => {
  const env = queueEnvironment(false);
  env.restore(randomQueue());
  env.music.setPlaylists([5, 6, 7, 8].map((id) => ({ id, name: String(id) })));
  assert.equal(env.shuffleDraws, 3);
  assert.equal(env.music.persistData.shuffleState.applied, true);
  assert.notEqual(env.music.persistData.shuffleState.id, "saved-pass");
  assert.deepEqual(Array.from(env.music.persistData.preShuffleOrder), [5, 6, 7, 8]);
  env.music.setPlaySongMode("normal");
  assert.deepEqual(
    Array.from(env.music.persistData.playlists, (song: any) => song.id),
    [5, 6, 7, 8],
  );
});

test("web empty and single-track queues do not start unnecessary shuffles", () => {
  for (const ids of [[], [1]]) {
    const env = queueEnvironment(false);
    env.music.persistData.playlists = ids.map((id) => ({ id, name: String(id) }));
    env.music.setPlaySongMode("random");
    env.music.ensureShuffledInRandomMode();
    assert.equal(env.shuffleDraws, 0);
    assert.equal(env.music.persistData.shuffleState, null);
    env.music.setPlaySongMode("normal");
    assert.deepEqual(
      Array.from(env.music.persistData.playlists, (song: any) => song.id),
      ids,
    );
  }
});

function deferred() {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function transportEnvironment() {
  const handshake = deferred();
  const fallbackHandshake = deferred();
  const fallbackStarted = deferred();
  const channels: any[] = [];
  const sent: any[] = [];
  let globalEvent: any;
  let fallbackCount = 0;
  const transportModule = sourceModule("src/utils/tauri/audio/transport/tauri.ts", {
    "@tauri-apps/api/core": {
      Channel: class {
        constructor() {
          channels.push(this);
        }
      },
      invoke: () => handshake.promise,
    },
    "../bridge": {
      audioSendMsg: (msg: any) => sent.push(msg),
      listenPlayerEvents: (handler: any) => {
        fallbackCount++;
        globalEvent = handler;
        fallbackStarted.resolve();
        return fallbackHandshake.promise.then(() => () => {});
      },
    },
  });
  return {
    transport: transportModule.createTauriAudioBackendTransport(),
    handshake,
    fallbackHandshake,
    fallbackStarted,
    channels,
    sent,
    emitFallback: (event: any) => globalEvent(event),
    get fallbackCount() {
      return fallbackCount;
    },
  };
}

const pause = { type: "playStatus", data: { isPlaying: false } };

test("session-control subscribers install before the transport is connected", async () => {
  const env = transportEnvironment();
  const backendEvents = sourceModule("src/utils/tauri/audio/backendEvents.ts", {
    "../core/runtime": { isTauri: () => true },
    "../core/nativeLog": { logNative() {} },
    "./transport": { getAudioBackendTransport: () => env.transport },
  });
  const controls = sourceModule("src/utils/AudioContext/NativeSessionControlsSync.ts", {
    "@/utils/tauri/core/runtime": { isTauri: () => true },
    "@/utils/tauri/audio/backendEvents": backendEvents,
  });
  const drop = controls.installSessionControlsSubscriber();
  assert.equal(typeof drop, "function");
  const connected = env.transport.connect();
  env.handshake.resolve();
  await connected;
  drop();
});

test("cold subscribers hear events during a single shared connection handshake", async () => {
  const env = transportEnvironment();
  const seen: any[] = [];
  const drop = env.transport.subscribe((event: any) => seen.push(event));
  const first = env.transport.connect();
  const second = env.transport.connect();
  assert.equal(first, second);
  assert.equal(env.channels.length, 1);
  env.channels[0].onmessage({ data: pause, seq: 1 });
  assert.deepEqual(seen, [pause]);
  drop();
  env.channels[0].onmessage({ data: pause, seq: 2 });
  assert.equal(seen.length, 1);
  env.handshake.resolve();
  await first;
  env.transport.sendOrQueue({ type: "pauseAudio" });
  assert.equal(env.sent.length, 1);
});

test("cold subscribers and queued commands survive channel fallback", async () => {
  const env = transportEnvironment();
  const seen: any[] = [];
  env.transport.subscribe((event: any) => seen.push(event));
  env.transport.sendOrQueue({ type: "pauseAudio" });
  const connected = env.transport.connect();
  env.handshake.reject(new Error("channel unavailable"));
  await env.fallbackStarted.promise;
  assert.equal(env.fallbackCount, 1);
  env.emitFallback(pause);
  assert.deepEqual(seen, [pause]);
  env.fallbackHandshake.resolve();
  await connected;
  await Promise.resolve();
  assert.equal(env.sent.length, 1);
});

test("shutdown during connect cannot resurrect a transport or replay an old command", async () => {
  const env = transportEnvironment();
  env.transport.sendOrQueue({ type: "pauseAudio" });
  const first = env.transport.connect();
  env.transport.shutdown();
  const next = env.transport.connect();
  env.handshake.resolve();
  await Promise.all([first, next]);
  await Promise.resolve();
  assert.equal(env.channels.length, 2);
  assert.equal(env.sent.length, 0);
});

for (const permissionDenied of [false, true])
  test(`media initialization waits for session adoption (permission denied: ${permissionDenied})`, async () => {
    const mounted: Function[] = [];
    const unmounted: Function[] = [];
    const published: string[] = [];
    const ready = ref(false);
    const initialization = deferred();
    const env = transportEnvironment();
    const music = { persistData: reactive({ playSongMode: "normal" }) };
    const module = sourceModule("src/composables/useNativeMediaControls.ts", {
      vue: {
        ref,
        watch,
        onMounted: (fn: Function) => mounted.push(fn),
        onUnmounted: (fn: Function) => unmounted.push(fn),
      },
      pinia: { storeToRefs: () => ({ persistData: ref(music.persistData) }) },
      "@/store": { musicStore: () => music },
      "@/utils/tauri": { isTauri: () => true, isMobile: async () => true },
      "@/utils/AudioContext": {
        installSessionControlsSubscriber: () => env.transport.subscribe(() => {}),
        publishSessionControls: () => published.push(music.persistData.playSongMode),
      },
      "@/utils/tauri/media/notification": {
        initializeMediaNotification: () => initialization.promise,
      },
      "@/utils/tauri/media/nowPlaying": {},
    });
    const scope = effectScope();
    scope.run(() => module.useNativeMediaControls(ready));
    const start = mounted[0]();
    await Promise.resolve();
    if (permissionDenied) initialization.reject(new Error("notification permission unavailable"));
    else initialization.resolve();
    await start;
    await nextTick();
    assert.deepEqual(published, []);
    music.persistData.playSongMode = "random";
    ready.value = true;
    await nextTick();
    assert.deepEqual(published, ["random"]);
    unmounted[0]();
    scope.stop();
  });
