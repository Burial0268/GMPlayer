# Bundled data

`jieba-dict.txt` is jieba's default dictionary from `jieba-rs` 0.11.0 (`src/data/dict.txt`), with
the part-of-speech column removed (`sed -E 's/^([^ ]+ [0-9]+) [^ ]+$/\1/'`): `cut` reads only
the word and its frequency, and the column was 0.8 MB of the WASM. jieba and jieba-rs are
MIT-licensed.

It is vendored because `jieba-rs` only embeds it zstd-compressed (its `default-dict` feature),
and zstd builds C sources for `wasm32-unknown-unknown`, which needs clang. Build hosts without
clang (Vercel's image among them) then fail in `cc-rs`. Re-copy it when upgrading `jieba-rs`.

`lyric-words.txt` is ours: words lyrics use that jieba's dictionary lacks, each with the
smallest frequency that fixed a reported line. See its header before adding one.

`cutword-words.txt` is generated from [cutword](https://github.com/liwenju0/cutword)'s
`cutword/dict.txt` (Apache-2.0, `LICENSE-cutword`; dictionary built from data up to January
2024): the words jieba's dictionary lacks that are Han only, 2-4 characters and have a cutword
frequency of at least 20, with the frequency rescaled to jieba's by the median ratio over the
words both list (2.62). The threshold was chosen on 2285 chart lyric lines: at 20, breakable
segments drop by under 1% and the changed lines mostly stop splitting words (掉进, 想过, 不对等);
going down to 5 adds 7,000 words, mostly transliteration fragments, for ten more changed lines.
To regenerate, unpack `cutword/dict.txt` from the cutword wheel and run, in this directory:

```sh
tr -d '\r' < dict.txt > cutword.txt
cut -d' ' -f1 jieba-dict.txt | LC_ALL=C sort -u > jieba.words
awk -F'\t' '{print $1}' cutword.txt | LC_ALL=C sort -u | LC_ALL=C comm -13 jieba.words - > new.words
awk -F'\t' 'NR==FNR{n[$1]=1;next} ($1 in n){print $1"\t"$2}' new.words cutword.txt \
  | grep -P '^\p{Han}{2,4}\t' \
  | awk -F'\t' '$2>=20{f=int($2*2.62+0.5); if(f<1)f=1; print $1, f}' | LC_ALL=C sort
```

(prefixed with the four header lines in the current file).
