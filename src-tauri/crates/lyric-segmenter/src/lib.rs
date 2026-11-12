//! Word boundaries for lyric line breaking, served to the WebView as a stand-in for
//! `Intl.Segmenter` (see `src/utils/lyricSegmenter.ts`).
//!
//! A line with only Han characters (and Latin, digits, punctuation) is Chinese and goes through
//! jieba, whose dictionary + HMM cuts Chinese lyrics far closer to how they are sung than ICU's
//! CJ dictionary. Anything with kana or Hangul is Japanese or Korean, and everything else goes
//! through ICU4X's dictionary segmenter, so one engine answers for every other script.

use icu_segmenter::{options::WordBreakInvariantOptions, WordSegmenter, WordSegmenterBorrowed};
use jieba_rs::Jieba;

pub struct Segmenter {
    jieba: Jieba,
    icu: WordSegmenterBorrowed<'static>,
}

impl Default for Segmenter {
    fn default() -> Self {
        Self {
            jieba: Jieba::new(),
            icu: WordSegmenter::new_dictionary(WordBreakInvariantOptions::default()),
        }
    }
}

impl Segmenter {
    /// One entry per segment: its end offset in UTF-16 code units (a JS string index) shifted
    /// left by one, with bit 0 set when the segment is word-like. Segments are contiguous and
    /// start at 0, so the ends alone describe them.
    pub fn segments(&self, text: &str) -> Vec<u32> {
        if is_chinese(text) {
            let mut end = 0u32;
            self.jieba
                .cut(text, true)
                .into_iter()
                .map(|token| {
                    end += token.word.encode_utf16().count() as u32;
                    end << 1 | u32::from(token.word.chars().any(char::is_alphanumeric))
                })
                .collect()
        } else {
            let utf16: Vec<u16> = text.encode_utf16().collect();
            let mut iter = self.icu.segment_utf16(&utf16);
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
            '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' => {
                han = true
            }
            '\u{20000}'..='\u{3134F}' => han = true,
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
    fn kana_keeps_a_line_away_from_jieba() {
        let s = Segmenter::default();
        assert!(!is_chinese("東京に行きたい"));
        assert!(is_chinese("东京"));
        // ICU's dictionary keeps the place name whole.
        assert_eq!(words(&split(&s, "東京に行きたい"))[0], "東京");
    }
}
