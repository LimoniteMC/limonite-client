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
/// The client's own strings, which vanilla lacks: the only lang file in the repo.
const OWN: &str = include_str!("limonite.lang");

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
        lang.add(OWN);
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

    /// `key`'s string, or the key itself if there's none, which is how a missing string
    /// shows.
    pub fn text<'a>(&'a self, key: &'a str) -> &'a str {
        self.get(key).unwrap_or(key)
    }

    /// `key`'s string with `params` put in (see [`fill`]). A parameter that's `%` and a key is
    /// that key's string, as the server sends an entity's name. A missing key comes back as
    /// itself, with its parameters after it, so nothing the server said is lost.
    pub fn format(&self, key: &str, params: &[&str]) -> String {
        let params: Vec<&str> = params
            .iter()
            .map(|param| param.strip_prefix('%').and_then(|key| self.get(key)).unwrap_or(param))
            .collect();
        match self.get(key) {
            Some(pattern) => fill(pattern, &params),
            None if params.is_empty() => key.to_string(),
            None => format!("{key} {}", params.join(" ")),
        }
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

/// Puts `params` into a string as the game's own strings expect (worth teaching): `%s` and `%d`
/// take them in turn, `%1$s`, `%1$d` and Bedrock's bare `%1` take one by its number from 1, and
/// `%%` is a `%`. A parameter that isn't there is left out, rather than panicking over a string
/// a pack got wrong.
#[must_use]
pub fn fill(pattern: &str, params: &[&str]) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut next = 0;
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('%') => {
                chars.next();
                out.push('%');
            }
            Some('s' | 'd') => {
                chars.next();
                out.push_str(params.get(next).copied().unwrap_or_default());
                next += 1;
            }
            Some(digit) if digit.is_ascii_digit() => {
                let mut number = 0_usize;
                while let Some(digit) = chars.peek().and_then(|c| c.to_digit(10)) {
                    number = number * 10 + digit as usize;
                    chars.next();
                }
                if chars.peek() == Some(&'$') {
                    chars.next();
                    if matches!(chars.peek(), Some('s' | 'd')) {
                        chars.next();
                    }
                }
                let param = number.checked_sub(1).and_then(|index| params.get(index));
                out.push_str(param.copied().unwrap_or_default());
            }
            _ => out.push('%'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_go_in_in_turn_or_by_number() {
        assert_eq!(fill("<%s> %s", &["Steve", "hi"]), "<Steve> hi");
        assert_eq!(fill("Set the time to %1$d", &["6000"]), "Set the time to 6000");
        assert_eq!(fill("%2$s before %1$s", &["a", "b"]), "b before a");
        assert_eq!(fill("bare %1 and %2", &["x", "y"]), "bare x and y");
        assert_eq!(fill("100%% sure, %s", &["yes"]), "100% sure, yes");
        assert_eq!(fill("missing %s and %3$s", &[]), "missing  and ");
        assert_eq!(fill("a lone % stays", &[]), "a lone % stays");
    }

    #[test]
    fn a_formatted_string_translates_what_the_server_names() {
        let lang = lang("commands.kill.successful=Killed %1$s\nentity.cow.name=Cow");
        assert_eq!(lang.format("commands.kill.successful", &["%entity.cow.name"]), "Killed Cow");
        assert_eq!(lang.format("commands.kill.successful", &["Steve"]), "Killed Steve");
        assert_eq!(lang.format("no.such.key", &["a", "b"]), "no.such.key a b", "nothing is lost");
        assert_eq!(lang.text("no.such.key"), "no.such.key");
    }

    #[test]
    fn the_clients_own_strings_are_read_before_any_packs() {
        let mut lang = Lang::default();
        lang.add(OWN);
        assert!(lang.get("limonite.crashed").is_some_and(|text| text.contains("%s")));
    }

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
