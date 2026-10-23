import assert from "node:assert/strict";
import { test } from "node:test";

import { AudioTimelineSync } from "../src/utils/tauri/audio/timeline.ts";

function close(actual: number, expected: number) {
  assert.ok(Math.abs(actual - expected) < 1e-9, `${actual} != ${expected}`);
}

test("a stamped position backdates the extrapolation anchor by the delivery latency", () => {
  let now = 1_700_000_000_000;
  const timeline = new AudioTimelineSync({ nowMs: () => now });
  timeline.setPlaybackState(true);
  timeline.acceptIncomingPosition(10, now - 400);

  now += 1_000;
  // 10s at stamp time, plus 400ms already spent in transit, plus 1s since arrival.
  close(timeline.readPosition(), 11.4);
});

test("an unstamped position keeps anchoring at arrival time", () => {
  let now = 1_700_000_000_000;
  const timeline = new AudioTimelineSync({ nowMs: () => now });
  timeline.setPlaybackState(true);
  timeline.acceptIncomingPosition(10, 0);

  now += 1_000;
  close(timeline.readPosition(), 11);
});

test("paused playback ignores the stamp because the clock does not extrapolate", () => {
  let now = 1_700_000_000_000;
  const timeline = new AudioTimelineSync({ nowMs: () => now });
  timeline.acceptIncomingPosition(10, now - 400);

  now += 1_000;
  close(timeline.readPosition(), 10);
});

test("implausible stamps fall back to arrival time", () => {
  let now = 1_700_000_000_000;
  const timeline = new AudioTimelineSync({ nowMs: () => now });
  timeline.setPlaybackState(true);

  timeline.acceptIncomingPosition(10, now + 50);
  now += 1_000;
  close(timeline.readPosition(), 11);

  timeline.acceptIncomingPosition(20, now - 2_001);
  now += 1_000;
  close(timeline.readPosition(), 21);
});

test("seek rejection stays on arrival time and does not apply a rejected stamp", () => {
  let now = 1_700_000_000_000;
  const timeline = new AudioTimelineSync({ nowMs: () => now });
  timeline.setPlaybackState(true);
  timeline.setLocalPosition(30, { guardSeek: true });

  // A pre-seek packet, even one that left the backend a moment ago, is stale.
  const held = timeline.acceptIncomingPosition(5, now - 200);
  close(held, 30);

  now += 500;
  // The stale packet was dropped, so the clock keeps the seek anchor rather
  // than the packet's stamp (which would have read 30.7 here).
  close(timeline.readPosition(), 30.5);
});
