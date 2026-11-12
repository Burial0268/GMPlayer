//! Word boundaries for lyric line breaking, served to the WebView as a stand-in for
//! `Intl.Segmenter` (see `src/utils/lyricSegmenter.ts`).
//!
//! A line with only Han characters (and Latin, digits, punctuation) is Chinese and goes through
//! jieba, whose dictionary + HMM cuts Chinese lyrics far closer to how they are sung than ICU's
//! CJ dictionary. Anything with kana or Hangul is Japanese or Korean, and everything else goes
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
        Self {
            jieba: Jieba::with_dict(&mut &JIEBA_DICT[..]).expect("bundled jieba dictionary"),
            icu,
        }
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
    fn southeast_asian_lines_are_left_to_the_page() {
        let s = Segmenter::default();
        assert!(s.segments("ฉันรักเธอ").is_empty());
        assert!(s.segments("ខ្ញុំស្រលាញ់អ្នក").is_empty());
        assert!(!s.segments("我爱你 I love you").is_empty());
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
