//! Word boundaries for lyric line breaking, served to the WebView as a stand-in for
//! `Intl.Segmenter` (see `src/utils/lyricSegmenter.ts`).
//!
//! A line with only Han characters (and Latin, digits, punctuation) is Chinese and goes through
//! jieba, whose dictionary + HMM cuts Chinese lyrics far closer to how they are sung than ICU's
//! CJ dictionary. What the page needs is not the one right reading but places a line may break,
//! so a Chinese line only breaks where no word jieba knows spans the gap: an ambiguous run such as
//! 杯壁画 (杯壁/画 or 杯/壁画) or 研究生命 stays whole instead of betting on one reading. Anything
//! with kana or Hangul is Japanese or Korean, and everything else goes
//! through ICU4X's dictionary segmenter, so one engine answers for every other script — except
//! Thai, Lao, Khmer and Burmese, which get no answer (an empty result) and are segmented by the
//! browser's own ICU, which carries those dictionaries anyway. Leaving them out of ICU4X's data
//! is most of what keeps this WASM small.

use icu_provider::prelude::*;
use icu_segmenter::{
    provider::{Baked, SegmenterDictionaryAutoV1, SegmenterDictionaryExtendedV1},
    WordSegmenter,
};
use jieba_rs::Jieba;

/// jieba's own dictionary, uncompressed (see `data/README.md`). Shipping it raw costs nothing on
/// the wire: the WASM is served brotli/gzip and Tauri compresses the assets it embeds.
const JIEBA_DICT: &[u8] = include_bytes!("../data/jieba-dict.txt");

/// Words lyrics need and jieba's dictionary lacks (see the file's header). Only their presence
/// matters, since a word only has to be known to keep its span from breaking.
const LYRIC_WORDS: &str = include_str!("../data/lyric-words.txt");

/// Recent vocabulary from cutword's 2024 dictionary that jieba lacks (see `data/README.md`).
const CUTWORD_WORDS: &str = include_str!("../data/cutword-words.txt");

/// ICU's baked dictionaries without the Southeast Asian ones. Requesting them is answered "not
/// found", which ICU accepts (it loads each dictionary as optional), so nothing references that
/// data and the linker drops it.
struct CjDictionaryOnly;

impl DataProvider<SegmenterDictionaryAutoV1> for CjDictionaryOnly {
    fn load(&self, req: DataRequest) -> Result<DataResponse<SegmenterDictionaryAutoV1>, DataError> {
        Baked.load(req)
    }
}

impl DataProvider<SegmenterDictionaryExtendedV1> for CjDictionaryOnly {
    fn load(
        &self,
        req: DataRequest,
    ) -> Result<DataResponse<SegmenterDictionaryExtendedV1>, DataError> {
        Err(DataErrorKind::IdentifierNotFound.with_req(SegmenterDictionaryExtendedV1::INFO, req))
    }
}

pub struct Segmenter {
    jieba: Jieba,
    icu: WordSegmenter,
}

impl Default for Segmenter {
    fn default() -> Self {
        let mut icu =
            WordSegmenter::try_new_for_non_complex_scripts_unstable(&Baked, Default::default())
                .expect("baked word rules");
        icu.load_dictionary_unstable(&CjDictionaryOnly)
            .expect("baked CJ dictionary");
        let mut jieba = Jieba::with_dict(&mut &JIEBA_DICT[..]).expect("bundled jieba dictionary");
        // Added one by one: `Jieba::load_dict` would reset the total frequency to these files'.
        for line in LYRIC_WORDS.lines().chain(CUTWORD_WORDS.lines()) {
            let mut fields = line.split_whitespace();
            if let Some(word) = fields.next().filter(|w| !w.starts_with('#')) {
                jieba.add_word(word, fields.next().and_then(|f| f.parse().ok()), None);
            }
        }
        Self { jieba, icu }
    }
}

impl Segmenter {
    /// One entry per segment: its end offset in UTF-16 code units (a JS string index) shifted
    /// left by one, with bit 0 set when the segment is word-like. Segments are contiguous and
    /// start at 0, so the ends alone describe them.
    ///
    /// Empty for a line in a Southeast Asian script: no answer, the page segments it natively.
    pub fn segments(&self, text: &str) -> Vec<u32> {
        if is_southeast_asian(text) {
            Vec::new()
        } else if is_chinese(text) {
            self.unambiguous_breaks(text)
        } else {
            let utf16: Vec<u16> = text.encode_utf16().collect();
            let mut iter = self.icu.as_borrowed().segment_utf16(&utf16);
            let mut ends = Vec::new();
            while let Some(end) = iter.next() {
                // ICU reports the start of the text as a boundary; it ends no segment.
                if end > 0 {
                    ends.push((end as u32) << 1 | u32::from(iter.is_word_like()));
                }
            }
            ends
        }
    }
}

impl Segmenter {
    /// Splits a Chinese line only where no candidate word spans the gap. Candidates are every
    /// dictionary word of two or more characters (`cut_all`) plus the words of the best cut, which
    /// adds what the HMM recognised and keeps Latin words whole.
    fn unambiguous_breaks(&self, text: &str) -> Vec<u32> {
        let chars: Vec<char> = text.chars().collect();
        // spanned[i]: some candidate covers the gap before character i.
        let mut spanned = vec![false; chars.len() + 1];
        let all = self
            .jieba
            .cut_all(text)
            .into_iter()
            .filter(|t| t.end - t.start >= 2);
        for token in all.chain(self.jieba.cut(text, true)) {
            spanned[token.start + 1..token.end].fill(true);
        }
        // 之 + a bound postposition (千里之外, 我们之间) leans on the word before it, also when the
        // segment runs on past it (现实之间作斗争, where 间作 is a dictionary word).
        let mut start = 0;
        for i in 1..=chars.len() {
            if i < chars.len() && spanned[i] {
                continue;
            }
            let segment: String = chars[start..i].iter().collect();
            if start > 0
                && is_han(chars[start - 1])
                && LEANS_BACK.iter().any(|p| segment.starts_with(p))
            {
                spanned[start] = true;
            }
            start = i;
        }
        let mut ends = Vec::new();
        let (mut end, mut word_like) = (0u32, false);
        for (i, c) in chars.iter().enumerate() {
            if i > 0 && !spanned[i] {
                ends.push(end << 1 | u32::from(word_like));
                word_like = false;
            }
            end += c.len_utf16() as u32;
            word_like |= c.is_alphanumeric();
        }
        if !chars.is_empty() {
            ends.push(end << 1 | u32::from(word_like));
        }
        ends
    }
}

/// Postpositions built on 之 that cannot start a phrase of their own: a line break before one
/// strands half a phrase (情话飘散成千里 | 之外). 之 as a plain possessive (爱之深) is left alone.
const LEANS_BACK: &[&str] = &[
    "之外", "之内", "之中", "之间", "之后", "之前", "之上", "之下", "之一", "之余", "之际", "之初",
    "之时", "之久", "之多", "之类", "之处",
];

fn is_han(c: char) -> bool {
    matches!(c,
        '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}'
            | '\u{20000}'..='\u{3134F}')
}

/// Thai, Lao, Myanmar (and its extensions), Khmer (and its symbols).
fn is_southeast_asian(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c,
            '\u{0E00}'..='\u{0EFF}'
                | '\u{1000}'..='\u{109F}'
                | '\u{A9E0}'..='\u{A9FF}'
                | '\u{AA60}'..='\u{AA7F}'
                | '\u{1780}'..='\u{17FF}'
                | '\u{19E0}'..='\u{19FF}')
    })
}

fn is_chinese(text: &str) -> bool {
    let mut han = false;
    for c in text.chars() {
        match c {
            // Kana (incl. halfwidth) or Hangul: Japanese or Korean, which jieba would mangle.
            '\u{3040}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}' => {
                return false
            }
            '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' | '\u{AC00}'..='\u{D7AF}' => {
                return false
            }
            c if is_han(c) => han = true,
            _ => {}
        }
    }
    han
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub struct LyricSegmenter(super::Segmenter);

    #[wasm_bindgen]
    impl LyricSegmenter {
        #[wasm_bindgen(constructor)]
        #[allow(clippy::new_without_default)]
        pub fn new() -> Self {
            Self(super::Segmenter::default())
        }

        /// One entry per segment: its end as a JS string index, shifted left by one, with bit 0
        /// set when the segment is word-like.
        pub fn segments(&self, text: &str) -> Vec<u32> {
            self.0.segments(text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decodes `segments` back into (text, word-like) pairs.
    fn split(segmenter: &Segmenter, text: &str) -> Vec<(String, bool)> {
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let mut start = 0;
        segmenter
            .segments(text)
            .into_iter()
            .map(|packed| {
                let end = (packed >> 1) as usize;
                let piece = String::from_utf16(&utf16[start..end]).unwrap();
                start = end;
                (piece, packed & 1 == 1)
            })
            .collect()
    }

    /// Whether `word` sits in `line` with no break inside it.
    fn kept_whole(s: &Segmenter, line: &str, word: &str) -> bool {
        let start = line[..line.find(word).expect(word)].encode_utf16().count() as u32;
        let end = start + word.encode_utf16().count() as u32;
        s.segments(line)
            .into_iter()
            .all(|packed| !(start < packed >> 1 && packed >> 1 < end))
    }

    fn words(pieces: &[(String, bool)]) -> Vec<&str> {
        pieces
            .iter()
            .filter(|(_, w)| *w)
            .map(|(s, _)| s.as_str())
            .collect()
    }

    #[test]
    fn chinese_goes_through_jieba() {
        let s = Segmenter::default();
        assert_eq!(
            words(&split(&s, "我爱北京天安门")),
            ["我", "爱", "北京", "天安门"]
        );
    }

    #[test]
    fn segments_cover_the_whole_line_in_utf16_units() {
        let s = Segmenter::default();
        for text in [
            "😀我爱你",
            "hello world",
            "東京に行きたい",
            "사랑해요 너를",
            "晴天 Jay Chou，2003",
        ] {
            let joined: String = split(&s, text).into_iter().map(|(p, _)| p).collect();
            assert_eq!(joined, text);
            let last = s.segments(text).last().copied().unwrap() >> 1;
            assert_eq!(last as usize, text.encode_utf16().count(), "{text}");
        }
    }

    #[test]
    fn spaces_and_punctuation_are_not_word_like() {
        let s = Segmenter::default();
        let pieces = split(&s, "hello, world");
        assert_eq!(words(&pieces), ["hello", "world"]);
        let chinese = split(&s, "你好，世界");
        assert!(chinese.iter().any(|(p, w)| p == "，" && !w));
    }

    #[test]
    fn southeast_asian_lines_are_left_to_the_page() {
        let s = Segmenter::default();
        assert!(s.segments("ฉันรักเธอ").is_empty());
        assert!(s.segments("ខ្ញុំស្រលាញ់អ្នក").is_empty());
        assert!(!s.segments("我爱你 I love you").is_empty());
    }

    #[test]
    fn ambiguous_runs_stay_whole() {
        let s = Segmenter::default();
        // (line, the run either reading needs unbroken)
        for (line, run) in [
            ("研究生命的起源", "研究生命"),       // 研究生/命 or 研究/生命
            ("结婚的和尚未结婚的", "和尚未"),     // 和尚/未 or 和/尚未
            ("冷凝水在杯壁画出了迷宫", "杯壁画"), // 杯壁/画 or 杯/壁画 (杯壁 from lyric-words)
            // Not ambiguous, but 之外/之间 lean on the word before them.
            ("情话飘散成千里之外一曲东风破", "千里之外"),
            ("偷偷想着我们之间关系的远近", "我们之间"),
            ("在浪漫还有现实之间作斗争", "现实之间"),
        ] {
            assert!(kept_whole(&s, line, run), "{line}: {:?}", split(&s, line));
        }
    }

    #[test]
    fn reported_lines_do_not_break_inside_a_word() {
        let s = Segmenter::default();
        for (line, word) in [
            ("冷凝水在杯壁画出了迷宫", "杯壁"),
            ("蒸发成辞职信里两行鞠躬", "辞职信"),
            ("眼前的未来也并没有想通", "想通"),
            // From cutword's recent vocabulary; jieba alone cut 掉/进 and 想/过.
            ("主动掉进陷阱还是往你脸上贴金", "掉进"),
            ("也曾想过以后生活没有人牵挂", "想过"),
        ] {
            assert!(kept_whole(&s, line, word), "{line}: {:?}", split(&s, line));
        }
    }

    #[test]
    fn kana_keeps_a_line_away_from_jieba() {
        let s = Segmenter::default();
        assert!(!is_chinese("東京に行きたい"));
        assert!(is_chinese("东京"));
        // ICU's dictionary keeps the place name whole.
        assert_eq!(words(&split(&s, "東京に行きたい"))[0], "東京");
    }
}
