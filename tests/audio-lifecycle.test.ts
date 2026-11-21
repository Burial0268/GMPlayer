import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import { transformSync } from "esbuild";
import { effectScope, nextTick, reactive, ref, toRaw, watch } from "vue";
import { createPinia, defineStore } from "pinia";
import { identityForSong } from "../src/utils/tauri/audio/identity";
import { IDBFactory, IDBObjectStore } from "fake-indexeddb";

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
const playlistOrder = sourceModule("src/utils/library/playlistOrder.ts", {});
const soundTypes = sourceModule("src/utils/AudioContext/sound/types.ts", {});
const keyOf = (identity: any) =>
  identity ? `${identity.provider}:${identity.id ?? identity.path}` : null;
const identities = (ids: number[]) => ids.map((id) => ({ provider: "netease", id: String(id) }));

function queueEnvironment(native = true) {
  let music: any;
  const autoMixCancels: unknown[] = [];
  let publisher: any;
  let shuffleDraws = 0;
  const math = Object.assign(Object.create(Math), {
    random: () => {
      shuffleDraws++;
      return 0.25;
    },
  });
  const window: any = {};
  const activeSound: { value: any } = { value: undefined };
  const storage = new Map<string, string>();
  const localStorage = {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  };
  class NativeRustSound {
    kind = "backend";
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
      "@/utils/app/languageData": { __esModule: true, default: (key: string) => key },
      "@/utils/tauri/core/runtime": { isTauri: () => native },
      "@/utils/AudioContext/sound/types": soundTypes,
      "@/utils/tauri/audio/identity": { trackIdentityKey: keyOf },
      "@/utils/tauri/core/nativeLog": { logNative() {} },
      "@/utils/library/playlistOrder": playlistOrder,
      "@/utils/rawEntry": { asRawEntries: (songs: any[]) => songs.slice() },
      "@/utils/AudioContext": {
        requestNativeReshuffle: () => publisher.requestNativeReshuffle(),
        publishNativeManifest: (...args: any[]) => publisher.publishNativeManifest(...args),
        publishSessionControls() {},
        cancelNativeQueuePrefill() {},
        getAudioPreloader: () => ({ cleanup() {} }),
        getAutoMixEngine: () => ({
          isHandoffActive: () => false,
          cancelIfHeadingFor: (songId: unknown) => {
            autoMixCancels.push(songId);
            return false;
          },
        }),
        stopActiveSound() {},
      },
    },
    { window, Math: math, $message: { info() {}, success() {} } },
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
      "src/utils/AudioContext/native/NativeManifestRevision.ts",
      {},
      { localStorage },
    );
    publisher = sourceModule(
      "src/utils/AudioContext/native/NativeManifestPublisher.ts",
      {
        vue: { toRaw },
        "@/utils/tauri/core/runtime": { isTauri: () => native },
        "@/store/musicTypes": musicTypes,
        "@/utils/AudioContext/sound/NativeRustSound": { setPlannerRevisionObserver() {} },
        "@/utils/AudioContext/sound/types": soundTypes,
        "@/store/musicData": { __esModule: true, default: () => music },
        "@/store/listenTogether": { __esModule: true, default: () => ({ isInRoom: false }) },
        "@/utils/tauri/audio/identity": { trackIdentityKey: keyOf, identityForSong },
        "@/utils/tauri/core/nativeLog": { logNative() {} },
        "./NativeManifestRevision": revision,
        "../sound/SoundManager": { activeSound },
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
    autoMixCancels,
    attach() {
      activeSound.value = new NativeRustSound();
      return activeSound.value;
    },
  };
}

function persistModule(window: any) {
  return sourceModule(
    "src/store/musicPersistedData.ts",
    {
      pinia: { defineStore: (_id: string, setup: any) => setup },
      vue: { reactive, watch, toRaw },
      "throttle-debounce": require("throttle-debounce"),
      "./musicTypes": musicTypes,
      "@/utils/rawEntry": { asRawEntry: (song: any) => song },
    },
    { window: { addEventListener() {}, ...window }, document: { addEventListener() {} } },
  );
}

async function hydrateQueue(saved: any) {
  const scope = effectScope();
  const mod = persistModule({
    localStorage: { getItem: () => JSON.stringify({ persistData: saved }) },
  });
  await mod.loadMusicPersist();
  const store = scope.run(() => mod.default());
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

test("removing an upcoming song asks AutoMix to drop a transition to it; the playing one does not", () => {
  const env = queueEnvironment();
  env.restore({
    ...musicTypes.createDefaultPersistData(),
    playlists: [1, 2, 3].map((id) => ({ id, name: String(id) })),
    playSongIndex: 0,
  });
  env.music.checkpointPlaySongTime = () => {};
  env.music.removeSong(1);
  assert.deepEqual(env.autoMixCancels, [2]);
  assert.deepEqual(
    Array.from(env.music.persistData.playlists, (s: any) => s.id),
    [1, 3],
  );
  env.music.removeSong(0);
  assert.deepEqual(env.autoMixCancels, [2], "deleting the playing song has its own path");
});

test("legacy persisted queues migrate to an explicit completion or pending token", async () => {
  for (const pending of [null, 42]) {
    const saved: any = randomQueue();
    delete saved.shuffleState;
    saved.pendingReshuffleRevision = pending;
    const persistData = await hydrateQueue(saved);
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
  const mod = persistModule({ localStorage });
  await mod.loadMusicPersist();
  const persisted = scope.run(() => mod.default());
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

function memoryStorage(initial: Record<string, string> = {}) {
  const map = new Map(Object.entries(initial));
  return {
    map,
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => void map.set(key, value),
    removeItem: (key: string) => void map.delete(key),
  };
}

async function bootPersist(indexedDB: any, localStorage: any) {
  const scope = effectScope();
  const mod = persistModule({ localStorage, indexedDB });
  await mod.loadMusicPersist();
  const store = scope.run(() => mod.default());
  return { mod, persistData: store.persistData, stop: () => scope.stop() };
}

async function idbRecords(indexedDB: any) {
  const db: any = await new Promise((resolve, reject) => {
    const req = indexedDB.open("gmplayer-music", 1);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
  const out: Record<string, any> = {};
  await new Promise<void>((resolve) => {
    const tx = db.transaction("persist");
    for (const key of ["state", "queue", "history"]) {
      const req = tx.objectStore("persist").get(key);
      req.onsuccess = () =>
        (out[key] = req.result === undefined ? undefined : JSON.parse(req.result));
    }
    tx.oncomplete = () => resolve();
  });
  db.close();
  return out;
}

// IndexedDB commits after the microtask that issues the write; give it a macrotask.
// `ms` past the 400 ms persist throttle lets a second write leave its trailing edge.
const settle = (ms = 20) => new Promise((resolve) => setTimeout(resolve, ms));

test("the localStorage queue migrates into IndexedDB once and the legacy copy is removed", async () => {
  const idb = new IDBFactory();
  const saved: any = randomQueue();
  saved.playSongIndex = 2;
  saved.playVolume = 0.3;
  saved.playHistory = [{ id: 9, name: "9" }];
  const localStorage = memoryStorage({ musicData: JSON.stringify({ persistData: saved }) });

  const first = await bootPersist(idb, localStorage);
  first.stop();
  assert.equal(localStorage.map.has("musicData"), false);
  const records = await idbRecords(idb);
  assert.deepEqual(records.queue, saved.playlists);
  assert.deepEqual(records.history, saved.playHistory);
  assert.equal(records.state.playSongIndex, 2);
  assert.equal("playlists" in records.state, false);

  // Next launch: nothing left in localStorage, everything comes back from IndexedDB.
  const second = await bootPersist(idb, memoryStorage());
  assert.deepEqual(
    Array.from(second.persistData.playlists, (song: any) => song.id),
    [3, 1, 4, 2],
  );
  assert.equal(second.persistData.playSongIndex, 2);
  assert.equal(second.persistData.playVolume, 0.3);
  assert.deepEqual({ ...second.persistData.shuffleState }, { id: "saved-pass", applied: true });
  assert.equal(second.persistData.playHistory[0].id, 9);
  second.stop();
});

test("a localStorage queue left by a fallback session wins over an older IndexedDB copy", async () => {
  const idb = new IDBFactory();
  const older = await bootPersist(
    idb,
    memoryStorage({
      musicData: JSON.stringify({ persistData: { ...randomQueue(), playSongIndex: 0 } }),
    }),
  );
  older.stop();
  const newer = { ...randomQueue(), playlists: [{ id: 7, name: "7" }], playSongIndex: 0 };
  const localStorage = memoryStorage({ musicData: JSON.stringify({ persistData: newer }) });
  const boot = await bootPersist(idb, localStorage);
  assert.deepEqual(
    Array.from(boot.persistData.playlists, (song: any) => song.id),
    [7],
  );
  assert.deepEqual((await idbRecords(idb)).queue, newer.playlists);
  assert.equal(localStorage.map.has("musicData"), false);
  boot.stop();
});

test("without IndexedDB the queue stays in localStorage and keeps being written there", async () => {
  const saved = randomQueue();
  const localStorage = memoryStorage({ musicData: JSON.stringify({ persistData: saved }) });
  const boot = await bootPersist(undefined, localStorage);
  assert.equal(boot.persistData.playlists.length, 4);
  boot.persistData.playVolume = 0.1;
  await nextTick();
  await settle();
  assert.equal(JSON.parse(localStorage.map.get("musicData")!).persistData.playVolume, 0.1);
  boot.stop();
});

test("a volume change rewrites only the small record; a reorder lands with its token", async () => {
  const idb = new IDBFactory();
  const seeded = await bootPersist(
    idb,
    memoryStorage({
      musicData: JSON.stringify({
        persistData: { ...randomQueue(), shuffleState: { id: "landing", applied: false } },
      }),
    }),
  );
  seeded.stop();
  const boot = await bootPersist(idb, memoryStorage());
  const realPut = IDBObjectStore.prototype.put;
  const puts: string[] = [];
  IDBObjectStore.prototype.put = function (value: any, key: any) {
    puts.push(key);
    return realPut.call(this, value, key);
  };
  try {
    boot.persistData.playVolume = 0.2;
    await nextTick();
    await settle();
    assert.deepEqual(puts, ["state"]);

    puts.length = 0;
    const env = queueEnvironment();
    env.restore(boot.persistData);
    env.music.adoptBackendQueueOrder(identities([1, 2, 3, 4]), 10, { shuffleId: "landing" });
    await nextTick();
    await settle(450);
    assert.deepEqual(puts.sort(), ["queue", "state"], "queue and its token share one transaction");
    const records = await idbRecords(idb);
    assert.deepEqual(
      records.queue.map((song: any) => song.id),
      [1, 2, 3, 4],
    );
    assert.deepEqual(records.state.shuffleState, { id: "landing", applied: true });
    assert.equal(records.state.playVolume, 0.2);

    // A scalar touched before the list in the same tick must not leave on its own.
    await settle(450);
    puts.length = 0;
    boot.persistData.playSongIndex = 1;
    boot.persistData.playlists = [
      { id: 5, name: "5" },
      { id: 6, name: "6" },
    ];
    await nextTick();
    await settle();
    assert.deepEqual(puts.sort(), ["queue", "state"]);
    const after = await idbRecords(idb);
    assert.equal(after.state.playSongIndex, 1);
    assert.deepEqual(
      after.queue.map((song: any) => song.id),
      [5, 6],
    );
  } finally {
    IDBObjectStore.prototype.put = realPut;
    boot.stop();
  }
});

test("a flush commits a same-tick change inside the throttle window before it resolves", async () => {
  const idb = new IDBFactory();
  const seeded = await bootPersist(
    idb,
    memoryStorage({
      musicData: JSON.stringify({ persistData: randomQueue() }),
    }),
  );
  seeded.stop();
  const boot = await bootPersist(idb, memoryStorage());
  // Take the throttle's leading edge so the next ordinary write would wait 400 ms.
  boot.persistData.playVolume = 0.4;
  await nextTick();
  await settle();
  // As at exit: a checkpoint and a queue edit, then the flush in the same tick.
  boot.persistData.playSongIndex = 3;
  boot.persistData.playlists = [{ id: 8, name: "8" }];
  await boot.mod.flushMusicPersist();
  const records = await idbRecords(idb);
  assert.equal(records.state.playSongIndex, 3);
  assert.deepEqual(
    records.queue.map((song: any) => song.id),
    [8],
  );
  boot.stop();
});

test("a store instance without a preloaded snapshot never overwrites the saved queue", async () => {
  const idb = new IDBFactory();
  const seeded = await bootPersist(
    idb,
    memoryStorage({
      musicData: JSON.stringify({ persistData: randomQueue() }),
    }),
  );
  seeded.stop();
  const scope = effectScope();
  const mod = persistModule({ localStorage: memoryStorage(), indexedDB: idb });
  const store = scope.run(() => mod.default());
  store.persistData.playlists = [];
  await nextTick();
  await settle();
  assert.equal((await idbRecords(idb)).queue.length, 4);
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

test("web refresh keeps a completed permutation, cursor and token across repeated hydrates", async () => {
  const env = queueEnvironment(false);
  const saved = randomQueue();
  saved.playSongIndex = 2;
  for (let boot = 0; boot < 3; boot++) {
    env.restore(await hydrateQueue(JSON.parse(JSON.stringify(saved))));
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

test("web migration preserves an already shuffled legacy queue without drawing again", async () => {
  const env = queueEnvironment(false);
  const saved: any = randomQueue();
  delete saved.shuffleState;
  env.restore(await hydrateQueue(saved));
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

test("web startup upgrades an unshuffled legacy queue once and pins the playing track", async () => {
  const env = queueEnvironment(false);
  const saved: any = randomQueue();
  delete saved.shuffleState;
  saved.playlists = [1, 2, 3, 4].map((id) => ({ id, name: String(id) }));
  saved.playSongIndex = 2;
  saved.preShuffleOrder = [];
  env.restore(await hydrateQueue(saved));
  env.music.ensureShuffledInRandomMode();
  assert.equal(env.shuffleDraws, 3);
  assert.equal(env.music.getPlaySongData.id, 3);
  assert.equal(env.music.persistData.shuffleState.applied, true);
  const completed = JSON.parse(JSON.stringify(env.music.persistData));
  env.restore(await hydrateQueue(completed));
  env.music.ensureShuffledInRandomMode();
  assert.equal(env.shuffleDraws, 3);
  assert.equal(env.music.persistData.shuffleState.id, completed.shuffleState.id);
  assert.deepEqual(
    Array.from(env.music.persistData.playlists, (song: any) => song.id),
    completed.playlists.map((song: any) => song.id),
  );
});

test("web completes a persisted pending shuffle locally and retains its original order", async () => {
  for (const legacy of [false, true]) {
    const env = queueEnvironment(false);
    const saved: any = randomQueue();
    if (legacy) {
      delete saved.shuffleState;
      saved.pendingReshuffleRevision = 42;
    } else saved.shuffleState = { id: "native-pending", applied: false };
    env.restore(await hydrateQueue(saved));
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
  const channels: any[] = [];
  const sent: any[] = [];
  const failures = new Map<number, Error>();
  const transportModule = sourceModule(
    "src/utils/tauri/audio/transport/tauri.ts",
    {
      "@tauri-apps/api/core": {
        Channel: class {
          constructor() {
            channels.push(this);
          }
        },
        invoke: () => handshake.promise,
      },
      "../bridge": {
        sendAudioCommand: (envelope: any) => {
          sent.push(envelope);
          const failure = failures.get(envelope.cmd);
          return failure ? Promise.reject(failure) : Promise.resolve();
        },
      },
      "../../core/nativeLog": { logNative() {} },
      "./ticket": sourceModule("src/utils/tauri/audio/transport/ticket.ts", {}),
    },
    {
      requestAnimationFrame: (fn: () => void) => setTimeout(fn, 0),
      cancelAnimationFrame: clearTimeout,
    },
  );
  /** What the backend's bus would put on the channel. */
  const envelope = (seq: number, data: any, ack = 0, epoch = 0) => ({ seq, epoch, ack, data });
  const hello = (seq: number, ack = 0, bootId = 0) =>
    envelope(seq, { type: "hello", data: { bootId, snapshot: { hasTrack: false } } }, ack);
  return {
    transport: transportModule.createTauriAudioBackendTransport(),
    handshake,
    channels,
    sent,
    failures,
    envelope,
    hello,
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
  const controls = sourceModule("src/utils/AudioContext/native/NativeSessionControlsSync.ts", {
    "@/utils/tauri/core/runtime": { isTauri: () => true },
    "@/utils/tauri/audio/backendEvents": backendEvents,
  });
  const drop = controls.installSessionControlsSubscriber();
  assert.equal(typeof drop, "function");
  const connected = env.transport.connect();
  env.handshake.resolve();
  env.channels[0].onmessage(env.hello(1));
  await connected;
  drop();
});

test("a backend mode nobody has stated yet is not adopted", () => {
  const handlers = new Map<string, Function>();
  const adopted: string[] = [];
  const music = {
    playingSongId: null,
    getPlaySongData: null,
    adoptPlaySongMode: (mode: string) => adopted.push(mode),
  };
  const controls = sourceModule("src/utils/AudioContext/native/NativeSessionControlsSync.ts", {
    "@/utils/tauri/core/runtime": { isTauri: () => true },
    "@/utils/tauri/audio/backendEvents": {
      onBackendEvent: (type: string, handler: Function) => {
        handlers.set(type, handler);
        return () => {};
      },
    },
    "@/store": { musicStore: () => music, userStore: () => ({}), useLocalLibraryStore: () => ({}) },
    "./NativeManifestPublisher": { publishNativeManifest() {} },
  });
  controls.installSessionControlsSubscriber();
  const emit = (playModeKnown: boolean) =>
    handlers.get("sessionControlsChanged")!({
      data: {
        controls: { playMode: "normal", favourite: false, canFavourite: false, playModeKnown },
      },
    });

  // A fresh backend's default leaking out with a like-state change.
  emit(false);
  assert.deepEqual(adopted, []);
  emit(true);
  assert.deepEqual(adopted, ["normal"]);
});

test("subscribers bound before connecting hear the hello and everything after it, once", async () => {
  const env = transportEnvironment();
  const seen: any[] = [];
  const drop = env.transport.subscribe((event: any) => seen.push(event.type));
  const first = env.transport.connect();
  const second = env.transport.connect();
  assert.equal(first, second);
  assert.equal(env.channels.length, 1);
  env.handshake.resolve();
  env.channels[0].onmessage(env.hello(5));
  env.channels[0].onmessage(env.envelope(6, pause));
  await first;
  assert.deepEqual(seen, ["hello", "playStatus"]);
  drop();
  env.channels[0].onmessage(env.envelope(7, pause));
  assert.equal(seen.length, 2);
});

test("commands wait for the hello and are numbered above what the backend has taken up", async () => {
  const env = transportEnvironment();
  env.transport.send({ type: "pauseAudio" });
  env.transport.send({ type: "resumeAudio" });
  assert.equal(env.sent.length, 0);
  env.handshake.resolve();
  // A previous page left the backend at command 41.
  env.channels[0].onmessage(env.hello(1, 41));
  assert.deepEqual(
    env.sent.map((e: any) => [e.cmd, e.data.type]),
    [
      [42, "pauseAudio"],
      [43, "resumeAudio"],
    ],
  );
  env.transport.send({ type: "pauseAudio" });
  assert.equal(env.sent[2].cmd, 44);
});

test("a ticket is acknowledged only by events emitted after its command was taken up", async () => {
  const env = transportEnvironment();
  env.handshake.resolve();
  const connected = env.transport.connect();
  env.channels[0].onmessage(env.hello(1, 0));
  await connected;
  const ticket = env.transport.send({ type: "resumeAudio" });
  assert.equal(ticket.acknowledgedBy({ seq: 2, epoch: 0, ack: 0 }), false);
  assert.equal(ticket.acknowledgedBy({ seq: 3, epoch: 0, ack: 1 }), true);

  // A command the backend never received must not be waited on forever.
  env.failures.set(2, new Error("player channel closed"));
  const lost = env.transport.send({ type: "pauseAudio" });
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(lost.acknowledgedBy({ seq: 4, epoch: 0, ack: 1 }), true);
});

test("a resync drops whatever its hello already covers, from either channel", async () => {
  const env = transportEnvironment();
  const seen: any[] = [];
  env.transport.subscribe((event: any, meta: any) => seen.push([event.type, meta.seq]));
  env.handshake.resolve();
  const connected = env.transport.connect();
  env.channels[0].onmessage(env.hello(1));
  await connected;
  env.channels[0].onmessage(env.envelope(2, pause));

  const resynced = env.transport.resync();
  // The fresh hello is numbered after event 3, which the old channel then delivers late.
  env.channels[1].onmessage(env.hello(4));
  env.channels[0].onmessage(env.envelope(3, pause));
  env.channels[1].onmessage(env.envelope(5, pause));
  await resynced;
  assert.deepEqual(seen, [
    ["hello", 1],
    ["playStatus", 2],
    ["hello", 4],
    ["playStatus", 5],
  ]);
});

test("shutdown drops queued commands instead of sending them later", async () => {
  const env = transportEnvironment();
  const ticket = env.transport.send({ type: "pauseAudio" });
  const connecting = env.transport.connect();
  env.transport.shutdown();
  env.handshake.resolve();
  env.channels[0].onmessage(env.hello(1));
  assert.equal(env.sent.length, 0);
  assert.equal(ticket.acknowledgedBy({ seq: 0, epoch: 0, ack: 0 }), true);
  void connecting;
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

test("the page cannot end loading while the backend still reports it", () => {
  const env = queueEnvironment();
  const music = env.music;

  music.adoptBackendPlaybackPhase("loading");
  music.settleLoading();
  assert.equal(music.isLoadingSong, true, "the decoder being up is not audio");
  assert.equal(music.loadingStage, "buffering");

  music.adoptBackendPlaybackPhase("buffering");
  music.settleLoading();
  assert.equal(music.loadingStage, "rebuffering");

  music.adoptBackendPlaybackPhase("playing");
  assert.equal(music.isLoadingSong, false);
  assert.equal(music.loadingStage, "idle");

  // A page-owned stage survives a phase that says nothing about it, and the
  // page itself may end it.
  music.loadingStage = "resolving";
  music.isLoadingSong = true;
  music.adoptBackendPlaybackPhase("paused");
  assert.equal(music.loadingStage, "resolving");
  music.settleLoading();
  assert.equal(music.isLoadingSong, false);
  assert.equal(music.loadingStage, "idle");
  env.restore();
});

test("a switch keeps the spinner through the old track's paused phase, until the load reaches the backend", () => {
  const env = queueEnvironment();
  const music = env.music;
  music.persistData.playlists = [1, 2, 1].map((id) => ({ id, name: String(id) }));
  music.persistData.playSongIndex = 0;
  music.commitPlaySongIndex = (index: number) => {
    music.persistData.playSongIndex = index;
    return true;
  };
  music.adoptBackendPlaybackPhase("playing");

  music.selectPlaySongByIndex(1);
  assert.equal(music.loadingStage, "resolving");
  // The backend answering the switch's own stop of the old track.
  music.adoptBackendPlaybackPhase("paused");
  assert.equal(music.isLoadingSong, true);
  // The new song's load reaches the backend and owns the spinner from here.
  music.adoptBackendPlaybackPhase("loading");
  music.adoptBackendPlaybackPhase("playing");
  assert.equal(music.isLoadingSong, false);
  assert.equal(music.loadingStage, "idle");

  // A duplicate row of the same song starts no load, so nothing would end `resolving`.
  music.selectPlaySongByIndex(2);
  music.adoptBackendPlaybackPhase("loading");
  music.adoptBackendPlaybackPhase("playing");
  music.selectPlaySongByIndex(0);
  assert.equal(music.loadingStage, "idle");
  env.restore();
});

test("sound listeners fire persistent-first, once-listeners once, and survive re-registration", () => {
  const { SoundEvents } = sourceModule("src/utils/AudioContext/sound/SoundEvents.ts", {});
  class Probe extends SoundEvents {
    emit(event: string, ...args: unknown[]) {
      this._emit(event, ...args);
    }
  }
  const sound = new Probe();
  const calls: string[] = [];
  const again = () => {
    calls.push("once");
    sound.once("play", again);
  };
  const removed = () => calls.push("removed");
  sound.once("play", again);
  sound.on("play", () => calls.push("on"));
  sound.on("play", removed).off("play", removed);
  sound.on("loaderror", () => {
    throw new Error("listener failure must not stop the rest");
  });
  sound.on("loaderror", (error: unknown) => calls.push(String(error)));

  sound.emit("play");
  sound.emit("play");
  assert.deepEqual(calls, ["on", "once", "on", "once"]);

  const error = console.error;
  console.error = () => {};
  try {
    sound.emit("loaderror", "boom");
  } finally {
    console.error = error;
  }
  assert.equal(calls.at(-1), "boom");

  sound.off("play");
  sound.emit("play");
  assert.equal(calls.length, 5);
});

test("the lyric segmenter worker is held while lyrics are visible and released when idle", async () => {
  let now = 0;
  const timers = new Map<number, { at: number; run: () => void }>();
  let timerId = 0;
  const advance = (ms: number) => {
    now += ms;
    for (const [id, timer] of [...timers]) {
      if (timer.at > now || !timers.delete(id)) continue;
      timer.run();
    }
  };
  const workers: any[] = [];
  class Worker {
    terminated = false;
    onmessage?: (event: { data: any }) => void;
    posted: any[] = [];
    constructor() {
      workers.push(this);
    }
    postMessage(message: any) {
      this.posted.push(message);
    }
    terminate() {
      this.terminated = true;
    }
  }
  const segmenter = sourceModule(
    "src/utils/lyrics/segmenter.ts",
    {},
    {
      Worker,
      URL: class {},
      setTimeout: (run: () => void, ms: number) => {
        timers.set(++timerId, { at: now + ms, run });
        return timerId;
      },
      clearTimeout: (id: number) => timers.delete(id),
    },
  );
  const live = () => workers.filter((worker) => !worker.terminated).length;
  const idle = 10_000;

  // Started once the app is up, then let go: nothing asked it for anything.
  advance(1000);
  assert.equal(live(), 1);
  advance(idle);
  assert.equal(live(), 0);

  // Off screen a song's lines start it, and an answer still owed keeps it past the countdown.
  const { late } = segmenter.prepareLyricSegments(["辞职信里"], 300);
  assert.equal(live(), 1);
  advance(idle);
  assert.equal(live(), 1, "a pending request must not be terminated under");
  const worker = workers.at(-1);
  worker.onmessage({ data: { id: worker.posted[0].id, results: [new Uint32Array([5, 9])] } });
  advance(300);
  await late;

  // The answer outlives the worker: the cache is on this thread.
  advance(idle);
  assert.equal(live(), 0);
  segmenter.prepareLyricSegments(["辞职信里"], 300);
  assert.equal(live(), 0, "a cached line needs no worker");

  // On screen it is held for as long as the view is, then released again.
  const first = segmenter.holdLyricSegmenter();
  const second = segmenter.holdLyricSegmenter();
  first();
  first();
  assert.equal(live(), 1);
  advance(idle * 20);
  assert.equal(live(), 1, "visible lyrics keep the worker however long nothing is asked");
  second();
  advance(idle * 2);
  assert.equal(live(), 0);
  assert.equal(workers.length, 3);
});
