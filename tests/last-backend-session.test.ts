import assert from "node:assert/strict";
import { test } from "node:test";

import {
  pickResume,
  planQueueAdoption,
  type LastBackendSession,
} from "@/utils/AudioContext/native/LastBackendSession";

const rows = [
  { id: 1 },
  { id: 2 },
  { id: 3 },
  { id: 2 },
  { id: -5, local: { uri: "content://a" } },
];
const last = (over: Partial<LastBackendSession> = {}): LastBackendSession => ({
  identity: { provider: "netease", id: "3" },
  position: 42,
  duration: 200,
  savedAt: 1,
  ...over,
});

test("the backend advanced past the page's cursor while it was frozen", () => {
  assert.deepEqual(pickResume(last(), rows, 0), { index: 2, position: 42, duration: 200 });
});

test("same track, later position: the backend's clock still wins", () => {
  assert.deepEqual(pickResume(last({ position: 180 }), rows, 2), {
    index: 2,
    position: 180,
    duration: 200,
  });
});

test("a track queued twice resumes on the row the page is on", () => {
  const record = last({ identity: { provider: "netease", id: "2" } });
  assert.equal(pickResume(record, rows, 3)?.index, 3);
  assert.equal(pickResume(record, rows, 0)?.index, 1);
});

test("local files match by locator", () => {
  const record = last({ identity: { provider: "local", path: "content://a" } });
  assert.equal(pickResume(record, rows, 0)?.index, 4);
});

test("nothing to resume: no record, or a track the queue no longer lists", () => {
  assert.equal(pickResume(null, rows, 0), null);
  assert.equal(pickResume(last({ identity: { provider: "netease", id: "99" } }), rows, 0), null);
  assert.equal(pickResume(last(), [], 0), null);
});

test("bad numbers clamp instead of seeking somewhere invalid", () => {
  assert.deepEqual(pickResume(last({ position: NaN, duration: -1 }), rows, 0), {
    index: 2,
    position: 0,
    duration: 0,
  });
  assert.equal(pickResume(last({ position: 500 }), rows, 0)?.position, 200);
});

const order = {
  manifestRevision: 7,
  order: [{ provider: "netease" as const, id: "3" }],
  shuffleId: "native:7:1",
};

test("a backend-drawn shuffle is mirrored without asking for another", () => {
  assert.deepEqual(planQueueAdoption({ playMode: "random", order }), {
    mode: "random",
    backendOrdered: true,
    order,
  });
});

test("random with no pending order adopts the mode and lets the page reshuffle", () => {
  assert.deepEqual(planQueueAdoption({ playMode: "random", order: null }), {
    mode: "random",
    backendOrdered: false,
    order: null,
  });
});

test("an order never applies outside random, and an empty record changes nothing", () => {
  assert.deepEqual(planQueueAdoption({ playMode: "normal", order }), {
    mode: "normal",
    backendOrdered: false,
    order: null,
  });
  assert.deepEqual(
    planQueueAdoption({ playMode: "random", order: { ...order, order: [] } }).order,
    null,
  );
  assert.deepEqual(planQueueAdoption(null), { mode: null, backendOrdered: false, order: null });
});
