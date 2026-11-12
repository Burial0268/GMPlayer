# Bundled data

`jieba-dict.txt` is jieba's default dictionary from `jieba-rs` 0.11.0 (`src/data/dict.txt`), with
the part-of-speech column removed (`sed -E 's/^([^ ]+ [0-9]+) [^ ]+$/\1/'`): `cut` reads only
the word and its frequency, and the column was 0.8 MB of the WASM. jieba and jieba-rs are
MIT-licensed.

It is vendored because `jieba-rs` only embeds it zstd-compressed (its `default-dict` feature),
and zstd builds C sources for `wasm32-unknown-unknown`, which needs clang. Build hosts without
clang (Vercel's image among them) then fail in `cc-rs`. Re-copy it when upgrading `jieba-rs`.
