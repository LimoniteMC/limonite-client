//! The game's strings (PLAN.md, UI: no hardcoded strings), read at runtime from
//! the install's `texts/en_US.lang` (CLAUDE.md, Rules).
//!
//! A `.lang` file is lines of `key=value`. A line that starts with `#` is a
//! comment, and a tab ends the value, because pack makers put a note after one.
//! Vanilla's own file has neither, but packs do (milestone 8). Every pack's
//! copy is read, bottom first, so a higher pack overrides a string at a time.

use std::collections::{BTreeSet, HashMap};

use limonite_packs::PackStack;

/// The strings the client shows. Other languages wait for the settings screen.
const FILE: &str = "texts/en_US.lang";

/// The strings, by key.
#[derive(Debug, Default)]
pub struct Lang {
    strings: HashMap<String, String>,
}

impl Lang {
    /// Reads every pack's strings. A pack whose file can't be read is skipped,
    /// and comes back as a problem to log.
    pub fn load(stack: &PackStack) -> (Self, Vec<String>) {
        let mut lang = Self::default();
        let mut problems = Vec::new();
        for (pack, read) in stack.layers(FILE) {
            match read {
                Ok(Some(bytes)) => lang.add(&String::from_utf8_lossy(&bytes)),
                Ok(None) => {}
                Err(err) => problems.push(format!("{}: {err}", pack.name())),
            }
        }
        (lang, problems)
    }

    /// Takes the strings in `text`, over any it already has.
    pub fn add(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.strip_prefix('\u{feff}').unwrap_or(line);
            if line.trim_start().starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.split('\t').next().unwrap_or(value);
            self.strings.insert(key.trim().to_string(), value.trim_end().to_string());
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.strings.get(key).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Which glyph pages the strings need beyond ASCII: a page is the top byte
    /// of a character's code, as `font/glyph_XX` numbers them.
    pub fn glyph_pages(&self) -> BTreeSet<u32> {
        let codes = self.strings.values().flat_map(|value| value.chars()).map(u32::from);
        codes.filter(|code| (0x80..0x1_0000).contains(code)).map(|code| code >> 8).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(text: &str) -> Lang {
        let mut lang = Lang::default();
        lang.add(text);
        lang
    }

    #[test]
    fn keys_values_comments_and_tabs() {
        let lang = lang(
            "## A heading\n\
             # a=comment\n\
             menu.quit=Save & Quit\n\
             menu.note=Options\t#a note for translators\n\
             menu.equals=2 + 2 = 4\n\
             \n\
             not a string\n\
             menu.spaces=Trailing   \r\n",
        );
        assert_eq!(lang.get("menu.quit"), Some("Save & Quit"));
        assert_eq!(lang.get("menu.note"), Some("Options"), "the tab ends the value");
        assert_eq!(lang.get("menu.equals"), Some("2 + 2 = 4"), "only the first = splits");
        assert_eq!(lang.get("menu.spaces"), Some("Trailing"));
        assert_eq!(lang.get("# a"), None, "a comment holds no string");
        assert_eq!(lang.len(), 4);
    }

    #[test]
    fn a_missing_key_is_none() {
        assert_eq!(lang("a=b").get("c"), None);
    }

    #[test]
    fn a_byte_order_mark_doesnt_join_the_first_key() {
        assert_eq!(lang("\u{feff}first=1").get("first"), Some("1"));
    }

    #[test]
    fn a_later_file_overrides_an_earlier_one() {
        let mut lang = lang("a=old\nb=kept");
        lang.add("a=new");
        assert_eq!(lang.get("a"), Some("new"));
        assert_eq!(lang.get("b"), Some("kept"));
    }

    #[test]
    fn glyph_pages_come_from_what_the_strings_use() {
        let lang = lang("a=plain ascii\nb=caf\u{e9}\nc=\u{2019}quoted\u{2019}\nd=\u{1f600} beyond the pages");
        assert_eq!(lang.glyph_pages(), BTreeSet::from([0x00, 0x20]));
    }
}
