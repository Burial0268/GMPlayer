# Generated codec fixtures

These files contain a generated two-second stereo chirp, not recorded music.
The source is 48 kHz signed 16-bit PCM. For sample index `i`:

```python
t = i / 48000
left = round(sin(2 * pi * (440 * t + 110 * t * t)) * 12000)
right = left // 2
```

Write 96,000 interleaved frames into a stereo WAV, then encode with FFmpeg:

```sh
ffmpeg -i tone.wav -map_metadata -1 \
  -metadata title='Codec fixture' -metadata artist='GMPlayer tests' \
  -metadata album='Native codec tests' -metadata track=3 -metadata date=2026 \
  -c:a libopus -b:a 96k -flags:a +bitexact tone.opus
```

Use the same metadata with `-c:a wmav2 -b:a 128k` for `tone.wma`, and
`-c:a wavpack` for `tone.wv`. The committed fixtures were generated with Debian's
FFmpeg 7.1.5. Tests read the committed files and need no FFmpeg executable.

Opus and WavPack decode to exactly 96,000 stereo frames. The WMA encoder has
codec delay/padding; its tests check a bounded duration and valid PCM instead of
pretending that its sample count is identical to the lossless source.
