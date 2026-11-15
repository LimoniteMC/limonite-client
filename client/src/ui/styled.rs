//! Text in colours: the game's formatting codes (m5-plan Step 16).
//!
//! **A `§` and a letter change what follows** (worth teaching). The server's messages carry
//! their colours inline: `§c` turns what follows red, `§e` yellow, and `§r` back to the line's
//! own colour. The font never draws the codes (`font::shown`). Here a line is cut into spans,
//! each a run of one colour, and each span becomes a label of its own, side by side in a stack:
//! so a label stays one colour, and the renderer never has to know about codes.
//!
//! Bedrock adds material colours to Java's sixteen, `§g` to `§v`. Bold, italic and obfuscated,
//! `§l`, `§o` and `§k`, are left plain: the font has no bold or slanted glyphs to use.

use super::font::Font;

/// A run of text in one colour.
pub type Span = (String, [u8; 4]);

/// What starts a code.
const FORMAT: char = '\u{a7}';

/// The colour a code names, as Bedrock draws it, or `None` for one that isn't a colour.
#[must_use]
pub const fn code_colour(code: char) -> Option<[u8; 3]> {
    Some(match code {
        '0' => [0, 0, 0],
        '1' => [0, 0, 170],
        '2' => [0, 170, 0],
        '3' => [0, 170, 170],
        '4' => [170, 0, 0],
        '5' => [170, 0, 170],
        '6' => [255, 170, 0],
        '7' => [170, 170, 170],
        '8' => [85, 85, 85],
        '9' => [85, 85, 255],
        'a' => [85, 255, 85],
        'b' => [85, 255, 255],
        'c' => [255, 85, 85],
        'd' => [255, 85, 255],
        'e' => [255, 255, 85],
        'f' => [255, 255, 255],
        'g' => [221, 214, 5],
        'h' => [227, 212, 209],
        'i' => [206, 202, 202],
        'j' => [68, 58, 59],
        'm' => [151, 22, 7],
        'n' => [180, 104, 77],
        'p' => [222, 177, 45],
        'q' => [71, 160, 54],
        's' => [44, 186, 168],
        't' => [33, 73, 123],
        'u' => [154, 92, 198],
        'v' => [235, 113, 20],
        _ => return None,
    })
}

/// `text` cut into runs of one colour, starting in `base`. The codes themselves are gone.
#[must_use]
pub fn spans(text: &str, base: [u8; 4]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut colour = base;
    let mut current = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != FORMAT {
            current.push(c);
            continue;
        }
        let Some(code) = chars.next() else {
            break;
        };
        let next = match code.to_ascii_lowercase() {
            'r' => base,
            code => code_colour(code).map_or(colour, |[r, g, b]| [r, g, b, base[3]]),
        };
        if next != colour {
            if !current.is_empty() {
                spans.push((std::mem::take(&mut current), colour));
            }
            colour = next;
        }
    }
    if !current.is_empty() {
        spans.push((current, colour));
    }
    spans
}

/// Adds `text` to the end of `line`, joining the last span if it's the same colour.
fn push(line: &mut Vec<Span>, text: &str, colour: [u8; 4]) {
    match line.last_mut() {
        Some((last, last_colour)) if *last_colour == colour => last.push_str(text),
        _ => line.push((text.to_string(), colour)),
    }
}

/// The byte where `text` stops fitting in `room`, a character at least if `must` is set.
fn cut(font: &Font, text: &str, room: f32, must: bool) -> usize {
    let mut taken = 0.0;
    for (at, c) in text.char_indices() {
        taken += font.advance(c);
        if taken > room {
            return if at == 0 && must { c.len_utf8() } else { at };
        }
    }
    text.len()
}

/// A word, as its runs of colour, and the colour of the space before it, if one came.
struct Word {
    space: Option<[u8; 4]>,
    pieces: Vec<Span>,
}

/// The words of a line. A colour code inside a word doesn't end it, so it's kept whole.
fn words(spans: &[Span]) -> Vec<Word> {
    let mut words = Vec::new();
    let mut current = Word { space: None, pieces: Vec::new() };
    let mut space = None;
    for (text, colour) in spans {
        for (index, piece) in text.split(' ').enumerate() {
            if index > 0 {
                if !current.pieces.is_empty() {
                    words.push(std::mem::replace(&mut current, Word { space: None, pieces: Vec::new() }));
                }
                space = Some(*colour);
            }
            if !piece.is_empty() {
                if current.pieces.is_empty() {
                    current.space = space.take();
                }
                push(&mut current.pieces, piece, *colour);
            }
        }
    }
    if !current.pieces.is_empty() {
        words.push(current);
    }
    words
}

/// Spans broken into lines no wider than `width`, between words where they can be, and inside
/// a word only when it's wider than a line on its own. The colours carry over a break.
#[must_use]
pub fn wrap(font: &Font, spans: &[Span], width: f32) -> Vec<Vec<Span>> {
    let mut lines = Vec::new();
    let mut line: Vec<Span> = Vec::new();
    let mut used = 0.0;
    for word in words(spans) {
        let wide: f32 = word.pieces.iter().map(|(text, _)| font.width(text)).sum();
        let space = font.advance(' ');
        let spaced = word.space.filter(|_| !line.is_empty());
        let needs = if spaced.is_some() { space + wide } else { wide };
        // A word that fits a line of its own starts the next one, rather than being cut.
        if !line.is_empty() && used + needs > width && wide <= width {
            lines.push(std::mem::take(&mut line));
            used = 0.0;
        } else if let Some(colour) = spaced {
            push(&mut line, " ", colour);
            used += space;
        }
        for (text, colour) in &word.pieces {
            let mut rest = text.as_str();
            while !rest.is_empty() {
                let room = width - used;
                let fits = font.width(rest);
                if fits <= room {
                    push(&mut line, rest, *colour);
                    used += fits;
                    break;
                }
                let at = cut(font, rest, room, line.is_empty());
                if at > 0 {
                    push(&mut line, &rest[..at], *colour);
                    rest = &rest[at..];
                }
                lines.push(std::mem::take(&mut line));
                used = 0.0;
            }
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [u8; 4] = [255; 4];
    const RED: [u8; 4] = [255, 85, 85, 255];
    const YELLOW: [u8; 4] = [255, 255, 85, 255];

    fn span(text: &str, colour: [u8; 4]) -> Span {
        (text.to_string(), colour)
    }

    #[test]
    fn codes_cut_a_line_into_colours_and_reset_back() {
        assert_eq!(spans("plain", WHITE), [span("plain", WHITE)]);
        assert_eq!(
            spans("a \u{a7}cred\u{a7}r and \u{a7}eyellow", WHITE),
            [span("a ", WHITE), span("red", RED), span(" and ", WHITE), span("yellow", YELLOW)]
        );
        assert_eq!(spans("\u{a7}C\u{a7}lbold red", WHITE), [span("bold red", RED)], "upper case, and bold kept plain");
        assert_eq!(spans("end\u{a7}", WHITE), [span("end", WHITE)], "a code cut off at the end");
        assert!(spans("", WHITE).is_empty());
    }

    #[test]
    fn a_material_colour_is_bedrocks() {
        assert_eq!(code_colour('g'), Some([221, 214, 5]), "minecoin gold");
        assert_eq!(code_colour('k'), None, "obfuscated isn't a colour");
    }

    #[test]
    fn lines_break_at_spaces_and_keep_their_colours() {
        // With no sheets, each character is a space wide: 4 units, so 10 to a line of 40.
        let font = Font::default();
        let lines = wrap(&font, &[span("<Steve> ", WHITE), span("hello there world", RED)], 52.0);
        assert_eq!(lines, [vec![span("<Steve> ", WHITE), span("hello", RED)], vec![span("there world", RED)]]);
        // A colour code inside a word doesn't make a place to break it.
        let lines = wrap(&font, &[span("ab cd", WHITE), span("ef gh", RED)], 24.0);
        assert_eq!(lines, [vec![span("ab", WHITE)], vec![span("cd", WHITE), span("ef", RED)], vec![span("gh", RED)]]);
        assert_eq!(wrap(&font, &[], 40.0), [Vec::<Span>::new()], "never no lines");
    }

    #[test]
    fn a_word_wider_than_a_line_is_cut() {
        let font = Font::default();
        let lines = wrap(&font, &[span("abcdefghijklmnop", WHITE)], 24.0);
        assert_eq!(lines, [vec![span("abcdef", WHITE)], vec![span("ghijkl", WHITE)], vec![span("mnop", WHITE)]]);
    }
}
