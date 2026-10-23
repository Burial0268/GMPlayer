import assert from "node:assert/strict";
import { test } from "node:test";
import { fmSongId, isNewNativeFmSnapshot } from "../src/utils/AudioContext/PersonalFmState";
import type { NativeFmSnapshot } from "../src/utils/tauri/audio/protocol";

const snapshot = (sessionId: number, revision: number): NativeFmSnapshot => ({
  sessionId,
  revision,
  tracks: [],
  currentIdentity: null,
  desiredPlaying: true,
  refilling: false,
  waiting: true,
  error: null,
});

test("native string identities match API numeric identities in the store", () => {
  assert.equal(fmSongId("123"), 123);
  assert.equal(fmSongId(123), 123);
});

test("invalid and unsafe numeric identities are never silently rounded", () => {
  for (const id of [undefined, null, "", 0, -1, "x", "9007199254740993"]) {
    assert.equal(fmSongId(id), null);
  }
});

test("duplicate and older snapshots cannot move a live session backwards", () => {
  const current = snapshot(10, 4);
  assert.equal(isNewNativeFmSnapshot(current, snapshot(10, 4)), false);
  assert.equal(isNewNativeFmSnapshot(current, snapshot(10, 3)), false);
  assert.equal(isNewNativeFmSnapshot(current, snapshot(9, 999)), false);
  assert.equal(isNewNativeFmSnapshot(current, snapshot(10, 5)), true);
  assert.equal(isNewNativeFmSnapshot(current, snapshot(11, 1)), true);
});

test("a waiting session with no loaded track is still adoptable", () => {
  const waiting = snapshot(10, 1);
  assert.equal(waiting.currentIdentity, null);
  assert.equal(waiting.tracks.length, 0);
  assert.equal(isNewNativeFmSnapshot(null, waiting), true);
});
