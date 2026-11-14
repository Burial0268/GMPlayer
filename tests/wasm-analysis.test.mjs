import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import { buildSync } from "esbuild";

import * as bindings from "../src-tauri/crates/audio-backend/pkg/gmplayer_audio_backend_bg.js";

const module = await WebAssembly.compile(
  readFileSync(
    new URL(
      "../src-tauri/crates/audio-backend/pkg/gmplayer_audio_backend_bg.wasm",
      import.meta.url,
    ),
  ),
);
const imports = Object.fromEntries(
  [...new Set(WebAssembly.Module.imports(module).map((entry) => entry.module))].map((name) => [
    name,
    bindings,
  ]),
);
const instance = await WebAssembly.instantiate(module, imports);
bindings.__wbg_set_wasm(instance.exports);

function wav(signal, { rate = 44100, channels = 1, duration = 2 } = {}) {
  const frames = Math.floor(duration * rate);
  const bytes = Buffer.alloc(44 + frames * channels * 2);
  bytes.write("RIFF", 0);
  bytes.writeUInt32LE(bytes.length - 8, 4);
  bytes.write("WAVEfmt ", 8);
  bytes.writeUInt32LE(16, 16);
  bytes.writeUInt16LE(1, 20);
  bytes.writeUInt16LE(channels, 22);
  bytes.writeUInt32LE(rate, 24);
  bytes.writeUInt32LE(rate * channels * 2, 28);
  bytes.writeUInt16LE(channels * 2, 32);
  bytes.writeUInt16LE(16, 34);
  bytes.write("data", 36);
  bytes.writeUInt32LE(frames * channels * 2, 40);
  for (let frame = 0; frame < frames; frame++) {
    const sample = Math.round(signal(frame / rate) * 32767);
    for (let channel = 0; channel < channels; channel++) {
      bytes.writeInt16LE(sample, 44 + (frame * channels + channel) * 2);
    }
  }
  return bytes;
}

function backend(t, bytes, fromFreq) {
  const processor = new bindings.WasmAudioBackend();
  t.after(() => processor.free());
  const send = (data) =>
    JSON.parse(processor.sendMessageJson(JSON.stringify({ callbackId: "", data })));
  send({ type: "setPlaylist", songs: [{ type: "local", filePath: "probe.wav", origOrder: 0 }] });
  const started = send({ type: "jumpToSong", songIndex: 0 });
  const musicId = started.effects.find((effect) => effect.type === "loadTrack").musicId;
  if (fromFreq !== undefined) send({ type: "setFFTRange", fromFreq, toFreq: 2000 });
  const loaded = JSON.parse(processor.loadAnalysisBytes(bytes, "wav", musicId));
  assert.equal(loaded.error, undefined);
  return {
    send,
    frame(ms, delta = 16) {
      return JSON.parse(processor.processAnalysisFrame(ms / 1000, delta)).events.map(
        (event) => event.data,
      );
    },
  };
}

const lowBins = (events) => events.find((event) => event.type === "lowFreqBins")?.data.bins;
const spectrum = (events) => events.find((event) => event.type === "fftData")?.data.data;
const bass = (time) => 0.3 * Math.sin(2 * Math.PI * 55 * time);

const transportBundle = buildSync({
  entryPoints: ["src/utils/tauri/audio/transport/wasm.ts"],
  bundle: true,
  platform: "node",
  format: "cjs",
  packages: "external",
  define: { "import.meta.url": JSON.stringify(import.meta.url) },
  write: false,
}).outputFiles[0].text;

async function transportEnvironment(t) {
  const timers = new Map();
  const frames = [];
  const audioElements = [];
  const workerRequests = [];
  const events = [];
  let nextTimer = 0;
  let now = 100;
  const flush = () => new Promise((resolve) => setImmediate(resolve));

  class FakeWorker {
    constructor(_url, options) {
      this.name = options.name;
    }
    terminate() {}
    postMessage(request) {
      workerRequests.push({ name: this.name, ...request });
      const reply = (value = {}) =>
        this.onmessage?.({ data: { id: request.id, ok: true, reply: value } });
      if (request.type === "process") {
        frames.push((bins) =>
          reply({ events: [{ data: { type: "lowFreqBins", data: { bins } } }] }),
        );
        return;
      }
      const msg = request.envelope?.data;
      let effects = [];
      if (msg?.type === "jumpToSong") {
        effects = [
          {
            type: "loadTrack",
            src: "probe.wav",
            musicId: "local:probe.wav",
            currentPlayIndex: 0,
            initialPosition: 0,
          },
        ];
      } else if (msg?.type === "resumeAudio") {
        effects = [{ type: "play" }];
      } else if (msg?.type === "pauseAudio") {
        effects = [{ type: "pause" }];
      } else if (msg?.type === "seekAudio") {
        effects = [{ type: "seek", position: msg.position }];
      }
      queueMicrotask(() => reply({ effects }));
    }
  }

  class FakeAudio extends EventTarget {
    paused = true;
    ended = false;
    currentTime = 0;
    volume = 1;
    constructor() {
      super();
      audioElements.push(this);
    }
    load() {}
    removeAttribute() {}
    play() {
      this.paused = false;
      this.dispatchEvent(new Event("play"));
      return Promise.resolve();
    }
    pause() {
      this.paused = true;
      this.dispatchEvent(new Event("pause"));
    }
  }

  const cjs = { exports: {} };
  runInNewContext(transportBundle, {
    module: cjs,
    exports: cjs.exports,
    require: (name) => {
      assert.equal(name, "@tauri-apps/api/core");
      return {};
    },
    URL,
    Worker: FakeWorker,
    Audio: FakeAudio,
    console,
    queueMicrotask,
    performance: { now: () => now },
    setInterval: (callback, ms) => {
      const id = ++nextTimer;
      timers.set(id, { callback, ms });
      return id;
    },
    clearInterval: (id) => timers.delete(id),
  });
  const transport = cjs.exports.createWasmAudioBackendTransport();
  t.after(() => transport.shutdown());
  transport.subscribe((event) => events.push(JSON.parse(JSON.stringify(event))));
  await transport.connect();
  transport.sendOrQueue({ type: "jumpToSong", songIndex: 0 });
  await flush();
  transport.sendOrQueue({ type: "resumeAudio" });
  await flush();
  return {
    transport,
    events,
    workerRequests,
    frames,
    flush,
    tick() {
      now += 16;
      audioElements.at(-1).currentTime += 0.016;
      const timer = [...timers.values()].find((entry) => entry.ms === 16);
      assert.ok(timer, "low-frequency analysis must run every 16 ms");
      timer.callback();
    },
  };
}

test("late worker frames cannot restore old bins after a seek, pause or analysis toggle", async (t) => {
  const env = await transportEnvironment(t);
  env.frames.shift()([10, 20]);
  await env.flush();
  assert.deepEqual(lowBins(env.events), [10, 20]);

  for (const command of [
    { type: "seekAudio", position: 1 },
    { type: "pauseAudio" },
    { type: "setAnalysis", enabled: false },
  ]) {
    env.tick();
    const lateFrame = env.frames.shift();
    assert.ok(lateFrame);
    env.transport.sendOrQueue(command);
    await env.flush();
    const count = env.events.length;
    lateFrame([999, 999]);
    await env.flush();
    assert.equal(env.events.length, count, `stale frame leaked after ${command.type}`);
    if (command.type === "seekAudio") {
      assert.deepEqual(env.events.at(-1).data.bins, [0, 0]);
      assert.ok(
        env.workerRequests.some((request) => request.type === "seek" && request.position === 1),
      );
    }
    if (command.type === "pauseAudio") env.transport.sendOrQueue({ type: "resumeAudio" });
    if (command.type === "setAnalysis")
      env.transport.sendOrQueue({ type: "setAnalysis", enabled: true });
    await env.flush();
    if (env.frames.length === 0) env.tick();
    env.frames.shift()([30, 40]);
    await env.flush();
    assert.deepEqual(env.events.at(-1).data.bins, [30, 40]);
  }
});

test("16 ms analysis detects short bass pulses at every phase, without reading future audio", (t) => {
  for (const rate of [44100, 48000]) {
    for (const offsetMs of [1, 10, 20, 30, 40, 50, 60, 65]) {
      const onset = (200 + offsetMs) / 1000;
      const audio = wav((time) => (time >= onset && time < onset + 0.01 ? bass(time - onset) : 0), {
        rate,
        duration: 0.5,
      });
      const analysis = backend(t, audio);
      let peak = 0;
      for (let ms = 0; ms < 450; ms += 16) {
        const bins = lowBins(analysis.frame(ms));
        if (!bins) continue;
        if (ms / 1000 < onset) assert.deepEqual(bins, [0, 0], "must not anticipate the beat");
        peak = Math.max(peak, ...bins);
      }
      assert.ok(peak > 100, `pulse missed: rate=${rate}, offset=${offsetMs}, peak=${peak}`);
    }
  }
});

test("WASM publishes native raw bins and throttles only the full spectrum", (t) => {
  const analysis = backend(t, wav(bass));
  let lowFrames = 0;
  let spectrumFrames = 0;
  for (let ms = 0; ms <= 1000; ms += 16) {
    const events = analysis.frame(ms);
    const bins = lowBins(events);
    const raw = spectrum(events);
    assert.ok(events.every((event) => event.type !== "lowFrequencyVolume"));
    if (bins) lowFrames++;
    if (raw) {
      spectrumFrames++;
      // The display frame is 512 peaks over runs of 4 raw bins; the low bins are
      // raw bins 0 and 16, so each sits inside the run that must cover it.
      assert.equal(raw.length, 512);
      assert.ok(raw[0] >= bins[0] && raw[4] >= bins[1]);
      assert.ok(Math.max(...raw) > 255, "raw spectrum must not be normalized twice");
    }
  }
  assert.ok(lowFrames >= 58, `only ${lowFrames} low-frequency frames`);
  assert.ok(spectrumFrames >= 19 && spectrumFrames <= 21, `${spectrumFrames} spectrum frames`);
});

test("turning spectrum bars off leaves low-frequency analysis and smoothing untouched", (t) => {
  const audio = wav((time) => bass(time) * Math.exp(-(time % 0.5) / 0.055));
  const baseline = backend(t, audio);
  const toggled = backend(t, audio);
  for (let ms = 0; ms < 1000; ms += 16) {
    if (ms === 160) toggled.send({ type: "setFFT", enabled: false });
    if (ms === 640) toggled.send({ type: "setFFT", enabled: true });
    const reference = baseline.frame(ms);
    const actual = toggled.frame(ms);
    assert.deepEqual(lowBins(actual), lowBins(reference), `low bins changed at ${ms} ms`);
    if (ms >= 160 && ms < 640) assert.equal(spectrum(actual), undefined);
    if (ms === 640) assert.ok(spectrum(actual), "reenabling should publish the current spectrum");
  }
});

test("default frequency range includes kick fundamentals and stereo matches native gain", (t) => {
  const monoBytes = wav(bass);
  const mono = backend(t, monoBytes);
  const explicit40 = backend(t, monoBytes, 40);
  const old80 = backend(t, monoBytes, 80);
  const stereo = backend(t, wav(bass, { channels: 2 }));
  let actual;
  let old;
  let amplified;
  for (let ms = 0; ms < 300; ms += 16) {
    actual = lowBins(mono.frame(ms));
    old = lowBins(old80.frame(ms));
    amplified = lowBins(stereo.frame(ms));
    assert.deepEqual(actual, lowBins(explicit40.frame(ms)));
  }
  assert.ok(actual[0] > old[0] * 2, "55 Hz kick should reach the low-frequency bins");
  for (let index = 0; index < 2; index++) {
    assert.ok(Math.abs(amplified[index] / actual[index] - Math.SQRT2) < 0.001);
  }
});

test("seeks clear the previous window and repeated media positions do not reanalyze it", (t) => {
  const analysis = backend(
    t,
    wav((time) => (time >= 0.2 && time < 0.3 ? bass(time) : 0)),
  );
  for (let ms = 0; ms <= 256; ms += 16) analysis.frame(ms);
  assert.equal(analysis.frame(256).length, 0);
  analysis.send({ type: "seekAudio", position: 0.4 });
  assert.deepEqual(lowBins(analysis.frame(400)), [0, 0]);
  analysis.send({ type: "seekAudio", position: 0.1 });
  assert.deepEqual(lowBins(analysis.frame(100)), [0, 0]);
  assert.deepEqual(lowBins(analysis.frame(2000)), [0, 0]);
  assert.equal(analysis.frame(2000).length, 0);
});
