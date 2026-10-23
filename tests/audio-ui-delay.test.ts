import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import { buildSync } from "esbuild";

const bundled = buildSync({
  entryPoints: ["src/utils/tauri/audio/uiDelay.ts"],
  bundle: true,
  platform: "node",
  format: "cjs",
  packages: "external",
  write: false,
}).outputFiles[0].text;

type UiDelay = typeof import("../src/utils/tauri/audio/uiDelay");

// Each platform gets a fresh module cache, just as separate app instances do.
function environment(native: boolean, mobile: boolean): UiDelay {
  const module = { exports: {} };
  runInNewContext(bundled, {
    module,
    exports: module.exports,
    console,
    window: {
      ...(native ? { __TAURI__: {} } : {}),
      navigator: {
        userAgent: mobile ? "Android" : "Windows",
        platform: mobile ? "Linux armv8l" : "Win32",
        maxTouchPoints: mobile ? 5 : 0,
      },
    },
    require(id: string) {
      assert.equal(id, "@tauri-apps/api/core");
      return { invoke: async () => !mobile };
    },
  });
  return module.exports as UiDelay;
}

function close(actual: number, expected: number) {
  assert.ok(Math.abs(actual - expected) < 1e-9, `${actual} != ${expected}`);
}

test("native Android lyrics share the progress display delay", () => {
  const time = environment(true, true);
  close(time.getLyricPresentationTimeSeconds(12), 11.55);
  assert.equal(time.getLyricPresentationTimeSeconds(12), time.applyMobileTauriAudioUiDelay(12));
});

test("browser and desktop playback do not gain a mobile output delay", async () => {
  for (const [native, mobile] of [
    [false, false],
    [false, true],
    [true, false],
  ]) {
    const time = environment(native, mobile);
    close(time.getLyricPresentationTimeSeconds(12), 12);
    // Also cover the cached result after native platform detection resolves.
    await new Promise<void>((resolve) => setImmediate(resolve));
    close(time.getLyricPresentationTimeSeconds(12), 12);
  }
});

test("user lyric offsets apply after output compensation in either direction", () => {
  const time = environment(true, true);
  close(time.getLyricPresentationTimeSeconds(12, 500), 12.05);
  close(time.getLyricPresentationTimeSeconds(12, -500), 11.05);
  close(environment(false, false).getLyricPresentationTimeSeconds(12, -500), 11.5);
});

test("the output delay clamps at track start without swallowing the user offset", () => {
  const time = environment(true, true);
  for (const raw of [0, 0.1, 0.45, -1, NaN]) {
    assert.equal(time.getLyricPresentationTimeSeconds(raw), 0);
    close(time.getLyricPresentationTimeSeconds(raw, 500), 0.5);
    close(time.getLyricPresentationTimeSeconds(raw, -500), -0.5);
  }
});

test("repeated presentation reads cannot accumulate delay or move the seek target", () => {
  const time = environment(true, true);
  const playbackPosition = 60;
  for (let frame = 0; frame < 120; frame++) {
    close(time.getLyricPresentationTimeSeconds(playbackPosition), 59.55);
  }
  assert.equal(playbackPosition, 60);
  close(time.getLyricPresentationTimeSeconds(0), 0);
  close(time.getLyricPresentationTimeSeconds(120), 119.55);
});

test("a lyric line changes when its compensated presentation time reaches it", () => {
  const time = environment(true, true);
  const lineStarts = [0, 10, 20];
  const indexAt = (raw: number, offsetMs = 0) => {
    const lyricTime = time.getLyricPresentationTimeSeconds(raw, offsetMs);
    return lineStarts.findLastIndex((start) => start <= lyricTime);
  };
  assert.equal(indexAt(10.2), 0);
  assert.equal(indexAt(10.45), 1);
  assert.equal(indexAt(10.2, 500), 1);
  assert.equal(indexAt(10.45, -500), 0);
});

test("AMLL initialization, resync and animation use the same presentation conversion", () => {
  const source = readFileSync("src/libs/apple-music-like/LyricPlayer.vue", "utf8");
  assert.match(source, /lyricPresentationTimeMs\(music\.getPlaySongPlaybackCurrentTime\(\)\)/);
  assert.match(source, /const lyricTime = lyricPresentationTimeMs\(currentTime\)/);
  assert.match(source, /const lyricTime = lyricPresentationTimeMs\(readPlaybackPosition\(\)\)/);
  // Resync must write the raw clock back to the store, not the delayed lyric time.
  assert.match(source, /music\.setPlaySongTime\(\{ currentTime, duration \}\)/);
  assert.doesNotMatch(source, /setPlaySongTime\([^;]*lyricTime/);
});

test("the lyric index consumes the same raw-to-presentation conversion", () => {
  const source = readFileSync("src/store/musicLyric.ts", "utf8");
  assert.match(source, /getLyricPresentationTimeSeconds\(\s*playbackCurrentTime,/);
});

test("a seek pins the lyric clock at its target until the delayed clock arrives", () => {
  const time = environment(true, true);
  // The reported flash: the view jumps to the tapped line, then the next frame
  // reads the clock 450 ms behind it and springs back to the previous line.
  time.armLyricSeekPresentationHold(60);
  close(time.getLyricPresentationTimeSeconds(60), 60);
  close(time.getLyricPresentationTimeSeconds(60.2), 60);
  close(time.getLyricPresentationTimeSeconds(60.449), 60);
  // Continuous hand-over: the delayed clock reaches the target and takes over.
  close(time.getLyricPresentationTimeSeconds(60.45), 60);
  close(time.getLyricPresentationTimeSeconds(60.5), 60.05);
  // The hold is spent — a later pass over the same position is not pinned.
  close(time.getLyricPresentationTimeSeconds(60.1), 59.65);
});

test("a hold ends when playback is not where the seek put it", () => {
  const time = environment(true, true);
  time.armLyricSeekPresentationHold(30);
  close(time.getLyricPresentationTimeSeconds(30.016), 30);
  // A track change restarts the clock at 0; the stale target must not pin the
  // next track when it later plays through 30 s.
  close(time.getLyricPresentationTimeSeconds(0), 0);
  close(time.getLyricPresentationTimeSeconds(30.1), 29.65);
  // Likewise a seek that landed somewhere else.
  time.armLyricSeekPresentationHold(60);
  close(time.getLyricPresentationTimeSeconds(10), 9.55);
  close(time.getLyricPresentationTimeSeconds(60.1), 59.65);
});

test("a clock paused at the seek target keeps showing the tapped line", () => {
  const time = environment(true, true);
  time.armLyricSeekPresentationHold(60);
  for (let frame = 0; frame < 60; frame++) {
    close(time.getLyricPresentationTimeSeconds(60), 60);
  }
  // Resuming hands over as usual.
  close(time.getLyricPresentationTimeSeconds(60.45), 60);
  close(time.getLyricPresentationTimeSeconds(61), 60.55);
});

test("the user lyric offset still applies around a held seek", () => {
  const time = environment(true, true);
  // Lyrics 500 ms early: the offset clock is already past the target.
  time.armLyricSeekPresentationHold(60);
  close(time.getLyricPresentationTimeSeconds(60, 500), 60.05);
  // Lyrics 300 ms late: pinned until the offset clock reaches the target.
  time.armLyricSeekPresentationHold(60);
  close(time.getLyricPresentationTimeSeconds(60, -300), 60);
  close(time.getLyricPresentationTimeSeconds(60.7, -300), 60);
  close(time.getLyricPresentationTimeSeconds(60.76, -300), 60.01);
});

test("desktop and web playback only hold a seek when a late offset needs it", () => {
  const time = environment(false, false);
  time.armLyricSeekPresentationHold(60);
  close(time.getLyricPresentationTimeSeconds(60.1), 60.1);
  time.armLyricSeekPresentationHold(60);
  close(time.getLyricPresentationTimeSeconds(60, -200), 60);
  close(time.getLyricPresentationTimeSeconds(60.1, -200), 60);
  close(time.getLyricPresentationTimeSeconds(60.25, -200), 60.05);
});

test("a non-finite read neither pins nor ends a hold, and an invalid target is ignored", () => {
  const time = environment(true, true);
  time.armLyricSeekPresentationHold(60);
  assert.equal(time.getLyricPresentationTimeSeconds(NaN), 0);
  close(time.getLyricPresentationTimeSeconds(60.1), 60);

  const fresh = environment(true, true);
  fresh.armLyricSeekPresentationHold(NaN);
  fresh.armLyricSeekPresentationHold(-1);
  close(fresh.getLyricPresentationTimeSeconds(60), 59.55);
});

test("the hold is armed by the seek funnel with the position actually seeked to", () => {
  const playerFunctions = readFileSync("src/utils/AudioContext/PlayerFunctions.ts", "utf8");
  // Armed after the seek is issued and before the store update that recomputes
  // the lyric index from the new position.
  assert.match(
    playerFunctions,
    /target\.seek\(currentTime\);\s*(?:\/\/[^\n]*\n\s*)*armLyricSeekPresentationHold\(currentTime\);\s*(?:\/\/[^\n]*\n\s*)*music\.setPlaySongTime\(/,
  );
  // The tap itself still hands the raw line time to the seek and to the view;
  // only the presentation conversion knows about the hold.
  const lyricPlayer = readFileSync("src/libs/apple-music-like/LyricPlayer.vue", "utf8");
  assert.match(lyricPlayer, /player\?\.setCurrentTime\(time, true\)/);
  assert.match(lyricPlayer, /emit\("lrcTextClick", time \/ 1000\)/);
  assert.doesNotMatch(lyricPlayer, /armLyricSeekPresentationHold/);
});
