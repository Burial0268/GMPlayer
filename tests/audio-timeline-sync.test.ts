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

test("a playing backstep inside the tolerance is held, so a cold-start lead would stick", () => {
  let now = 1_700_000_000_000;
  const timeline = new AudioTimelineSync({ nowMs: () => now });
  timeline.setPlaybackState(true);
  now += 80;
  // A lyric frame writes the extrapolated lead back into the anchor.
  close(timeline.readPosition(), 0.08);

  const held = timeline.acceptIncomingPosition(0.02, now - 10);
  close(held, 0.08);

  now += 1_000;
  close(timeline.readPosition(), 1.08);
  // The backend caught up by the same second and is still 60ms behind.
  // The smoother holds again, so the lead never converges on its own.
  close(timeline.acceptIncomingPosition(1.02, now), 1.08);
});

test("adopting the start anchor drops a cold-start lead the smoother would freeze", () => {
  let now = 1_700_000_000_000;
  const timeline = new AudioTimelineSync({ nowMs: () => now });
  timeline.setPlaybackState(true);
  now += 500;
  close(timeline.readPosition(), 0.5);

  // Direct play: the first backend sample wins, at arrival, with no sentAt.
  const anchored = timeline.adoptPlaybackAnchor(0.02, 200);
  close(anchored ?? -1, 0.02);
  close(timeline.duration, 200);

  now += 1_000;
  close(timeline.readPosition(), 1.02);
  // Heartbeats after the start anchor still compensate delivery latency.
  timeline.acceptIncomingPosition(1.02, now - 40);
  close(timeline.readPosition(), 1.06);
});

test("a resume seek-guard keeps a saved position ahead of a pre-seek zero", () => {
  let now = 1_700_000_000_000;
  const timeline = new AudioTimelineSync({ nowMs: () => now });
  timeline.setLocalPosition(30, { guardSeek: true });
  timeline.setPlaybackState(true);
  now += 400;
  close(timeline.readPosition(), 30.4);

  assert.equal(timeline.adoptPlaybackAnchor(0, 200), null);
  close(timeline.readPosition(), 30.4);

  const anchored = timeline.adoptPlaybackAnchor(30.05, 200);
  close(anchored ?? -1, 30.05);
  close(timeline.readPosition(), 30.05);
});

test("pause then resume stays on the adopted anchor, and a later seek still rejects stale packets", () => {
  let now = 1_700_000_000_000;
  const timeline = new AudioTimelineSync({ nowMs: () => now });
  timeline.setPlaybackState(true);
  now += 500;
  timeline.readPosition();
  timeline.adoptPlaybackAnchor(0.02, 200);

  now += 1_000;
  close(timeline.readPosition(), 1.02);
  timeline.setPlaybackState(false);
  // Pause adopts the backend sample even when it is nowhere near the local clock.
  close(timeline.acceptIncomingPosition(1.2, now - 300), 1.2);
  close(timeline.readPosition(), 1.2);

  timeline.setPlaybackState(true);
  now += 500;
  close(timeline.readPosition(), 1.7);

  timeline.setLocalPosition(40, { guardSeek: true });
  const held = timeline.acceptIncomingPosition(1.9, now - 50);
  close(held, 40);
  now += 200;
  close(timeline.readPosition(), 40.2);
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
