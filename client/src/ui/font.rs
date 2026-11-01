//! Text in the game's bitmap font, read at runtime from the install (m5-plan
//! Decision 14).
//!
//! **Sixteen cells across, each glyph as wide as its pixels** (worth teaching).
//! `font/default8.png` holds ASCII, so a character's code says which cell it's in. No
//! file says how wide each glyph is: the widths are read from the picture. A glyph runs
//! to the last column with a pixel in it, and then one empty unit keeps it off the next.
//!
//! **Cells taller than the text.** `font/default8.json` says this sheet's cells are 12
//! texels tall, and start 3 above the line, which leaves room for accents and tails
//! around the 8 units a line of text takes up.
//!
//! **Beyond ASCII** come the `font/glyph_XX` pages, one for each top byte of a
//! character's code. Their cells are twice as fine, so each texel is half a unit, and
//! a glyph's blank columns on the left are trimmed as well, because they don't start
//! at the cell's edge. Only the pages the strings need are loaded. `default8` has its
//! own accented letters further down, in an order of its own; the pages stand in for
//! them, a little thinner.

use std::collections::{BTreeSet, HashMap};

use image::RgbaImage;
use limonite_packs::json;

use super::atlas::Atlas;

/// The ASCII sheet.
pub const ASCII_SHEET: &str = "font/default8";
/// What the ASCII sheet's cells are like.
pub const ASCII_CELLS: &str = "font/default8.json";
/// How many cells a sheet has across, and at most down.
const CELLS: u32 = 16;
/// How tall a line of text is, in UI units, whatever the sheet's resolution.
pub const HEIGHT: f32 = 8.0;
/// How far the empty glyphs move on, such as a space.
const SPACE: f32 = 4.0;
/// The empty unit after each glyph.
const GAP: f32 = 1.0;
/// How far right and down the shadow lies.
pub const SHADOW: f32 = 1.0;
/// The character that starts a formatting code, as in `§e`.
const FORMAT: char = '\u{a7}';

/// The path of a glyph page's sheet, without its extension.
#[must_use]
pub fn page_path(page: u32) -> String {
    format!("font/glyph_{page:02X}")
}

/// What `font/default8.json` says about the ASCII sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AsciiCells {
    /// A cell's height, in texels.
    pub height: u32,
    /// Where a cell starts from the top of the line, in UI units.
    pub y_offset: f32,
}

impl AsciiCells {
    /// A sheet with no JSON: square cells that start at the top of the line.
    pub const PLAIN: Self = Self {
        height: 8,
        y_offset: 0.0,
    };

    /// `None` if the JSON isn't the shape it should be.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let value: serde_json::Value = json::parse(bytes).ok()?;
        Some(Self {
            height: u32::try_from(value.get("ascii_character_height")?.as_u64()?).ok()?,
            y_offset: value.get("ascii_character_y_offset").and_then(serde_json::Value::as_f64).unwrap_or(0.0) as f32,
        })
    }
}

/// A glyph, ready to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    /// Left, top, right and bottom, inside the atlas. Empty for a blank.
    pub source: [u32; 4],
    /// Its width and height, in UI units.
    pub size: [f32; 2],
    /// Where it starts from the top of the line, in UI units.
    pub y_offset: f32,
    /// How far on the next one goes.
    pub advance: f32,
}

/// One sheet.
#[derive(Debug)]
struct Page {
    /// The sheet's left and top in the atlas.
    origin: [u32; 2],
    /// A cell's width and height, in texels.
    cell: [u32; 2],
    /// Each cell's first and last column with a pixel in it, if any. As many as fit.
    ink: Vec<Option<(u32, u32)>>,
    /// Whether a glyph starts at its first pixel, rather than the cell's edge.
    trim_left: bool,
    y_offset: f32,
}

impl Page {
    /// A sheet in the atlas, with cells as wide as a sixteenth of it, and `height` tall
    /// or square.
    fn new(
        atlas: &Atlas,
        picture: &RgbaImage,
        name: &str,
        height: Option<u32>,
        y_offset: f32,
        trim_left: bool,
    ) -> Option<Self> {
        let [left, top, right, bottom] = atlas.get(atlas.sprite(name)?).source;
        let width = (right - left) / CELLS;
        let height = height.unwrap_or(width);
        if width == 0 || height == 0 {
            return None;
        }
        let rows = ((bottom - top) / height).min(CELLS);
        let ink = (0..rows * CELLS)
            .map(|index| {
                let (cell_left, cell_top) = (left + index % CELLS * width, top + index / CELLS * height);
                let mut inked = (0..width).filter(|column| {
                    (0..height).any(|row| picture.get_pixel(cell_left + column, cell_top + row)[3] > 0)
                });
                inked.next().map(|first| (first, inked.next_back().unwrap_or(first)))
            })
            .collect();
        Some(Self {
            origin: [left, top],
            cell: [width, height],
            ink,
            trim_left,
            y_offset,
        })
    }

    /// The glyph in this cell, if the sheet goes that far down.
    fn glyph(&self, index: u32) -> Option<Glyph> {
        let [width, height] = self.cell;
        let units_per_texel = HEIGHT / width as f32;
        let (cell_left, cell_top) = (
            self.origin[0] + index % CELLS * width,
            self.origin[1] + index / CELLS * height,
        );
        let size_y = height as f32 * units_per_texel;
        let Some((first, last)) = *self.ink.get(index as usize)? else {
            return Some(Glyph {
                source: [cell_left, cell_top, cell_left, cell_top],
                size: [0.0, size_y],
                y_offset: self.y_offset,
                advance: SPACE,
            });
        };
        let first = if self.trim_left { first } else { 0 };
        let size_x = (last + 1 - first) as f32 * units_per_texel;
        Some(Glyph {
            source: [cell_left + first, cell_top, cell_left + last + 1, cell_top + height],
            size: [size_x, size_y],
            y_offset: self.y_offset,
            advance: size_x + GAP,
        })
    }
}

/// The font's sheets.
#[derive(Debug, Default)]
pub struct Font {
    ascii: Option<Page>,
    pages: HashMap<u32, Page>,
}

impl Font {
    /// Reads the widths from the packed `picture`, for the sheets that made it into the
    /// atlas: ASCII, and `pages`.
    #[must_use]
    pub fn new(atlas: &Atlas, picture: &RgbaImage, ascii: AsciiCells, pages: &BTreeSet<u32>) -> Self {
        Self {
            ascii: Page::new(atlas, picture, ASCII_SHEET, Some(ascii.height), ascii.y_offset, false),
            pages: pages
                .iter()
                .filter_map(|&page| Some((page, Page::new(atlas, picture, &page_path(page), None, 0.0, true)?)))
                .collect(),
        }
    }

    /// The glyph for `c`, if a sheet has it.
    #[must_use]
    pub fn glyph(&self, c: char) -> Option<Glyph> {
        let code = u32::from(c);
        match code {
            0x20..0x80 => self.ascii.as_ref()?.glyph(code),
            0x80.. => self.pages.get(&(code >> 8))?.glyph(code & 0xFF),
            _ => None,
        }
    }

    /// How far `c` moves the text on. A character no sheet has takes a space.
    #[must_use]
    pub fn advance(&self, c: char) -> f32 {
        self.glyph(c).map_or(SPACE, |glyph| glyph.advance)
    }

    /// How wide `text` is, in UI units.
    #[must_use]
    pub fn width(&self, text: &str) -> f32 {
        shown(text).map(|c| self.advance(c)).sum()
    }
}

/// The characters of `text` that show: all but its formatting codes, `§` and the
/// letter after it. Colours arrive with chat (Step 16).
pub fn shown(text: &str) -> impl Iterator<Item = char> {
    let mut skip = false;
    text.chars().filter(move |&c| {
        let is_code = skip;
        skip = !is_code && c == FORMAT;
        !is_code && !skip
    })
}

#[cfg(test)]
pub mod tests {
    use image::Rgba;

    use super::*;
    use crate::ui::atlas::Input;

    /// A sheet 16 cells across, and `rows` down, with `glyphs`: each a cell's index, and
    /// its first and last inked column.
    pub fn sheet(cell: [u32; 2], rows: u32, glyphs: &[(u32, u32, u32)]) -> RgbaImage {
        let mut picture = RgbaImage::new(cell[0] * CELLS, cell[1] * rows);
        for &(index, first, last) in glyphs {
            let (left, top) = (index % CELLS * cell[0], index / CELLS * cell[1]);
            for column in first..=last {
                picture.put_pixel(left + column, top + cell[1] / 2, Rgba([255; 4]));
            }
        }
        picture
    }

    pub fn ascii_input(cell: [u32; 2], rows: u32, glyphs: &[(u32, u32, u32)]) -> Input {
        Input {
            name: ASCII_SHEET.to_string(),
            image: sheet(cell, rows, glyphs),
            nine_slice: None,
        }
    }

    fn font(glyphs: &[(u32, u32, u32)]) -> Font {
        let (atlas, picture) = Atlas::pack(vec![ascii_input([8, 8], 16, glyphs)]);
        Font::new(&atlas, &picture, AsciiCells::PLAIN, &BTreeSet::new())
    }

    const A: u32 = 'A' as u32;

    #[test]
    fn a_glyph_is_as_wide_as_its_pixels_and_a_gap() {
        let font = font(&[(A, 0, 4), ('i' as u32, 0, 0), ('.' as u32, 2, 2)]);
        let glyph = font.glyph('A').unwrap();
        assert_eq!(glyph.size, [5.0, HEIGHT]);
        assert_eq!(glyph.advance, 6.0);
        assert_eq!(glyph.source[2] - glyph.source[0], 5);
        assert_eq!(glyph.source[3] - glyph.source[1], 8);
        assert_eq!(font.glyph('i').unwrap().advance, 2.0);
        // ASCII keeps the blank columns on its left.
        assert_eq!(font.glyph('.').unwrap().size[0], 3.0);
    }

    #[test]
    fn a_blank_glyph_takes_a_space() {
        let font = font(&[(A, 0, 4)]);
        let space = font.glyph(' ').unwrap();
        assert_eq!(space.advance, SPACE);
        assert_eq!(space.source[0], space.source[2], "nothing to draw");
    }

    #[test]
    fn a_character_no_sheet_has_takes_a_space_too() {
        let font = font(&[(A, 0, 4)]);
        assert_eq!(font.glyph('\u{2019}'), None);
        assert_eq!(font.advance('\u{2019}'), SPACE);
        // Control characters hold other things on the sheet, and don't show.
        assert_eq!(font.glyph('\t'), None);
    }

    #[test]
    fn the_sprite_lands_where_the_atlas_put_it() {
        let (atlas, picture) = Atlas::pack(vec![
            Input {
                name: "other".to_string(),
                image: RgbaImage::new(200, 200),
                nine_slice: None,
            },
            ascii_input([8, 8], 16, &[(A, 0, 4)]),
        ]);
        let font = Font::new(&atlas, &picture, AsciiCells::PLAIN, &BTreeSet::new());
        let sheet = atlas.get(atlas.sprite(ASCII_SHEET).unwrap()).source;
        let glyph = font.glyph('A').unwrap();
        // 'A' is 0x41: row 4, column 1.
        assert_eq!(glyph.source[0], sheet[0] + 8);
        assert_eq!(glyph.source[1], sheet[1] + 32);
        assert_eq!(font.width("A"), 6.0);
    }

    #[test]
    fn taller_cells_as_the_json_says() {
        let cells = AsciiCells::parse(br#"{"ascii_character_height":12,"ascii_character_y_offset":-3,"ascii_extended":true}"#);
        assert_eq!(
            cells,
            Some(AsciiCells {
                height: 12,
                y_offset: -3.0,
            })
        );
        // Ten rows of twelve fit a sheet 128 tall, with a few texels left over.
        let (atlas, picture) = Atlas::pack(vec![ascii_input([8, 12], 10, &[(A, 0, 4)])]);
        let font = Font::new(&atlas, &picture, cells.unwrap(), &BTreeSet::new());
        let sheet = atlas.get(atlas.sprite(ASCII_SHEET).unwrap()).source;
        let glyph = font.glyph('A').unwrap();
        assert_eq!(glyph.size, [5.0, 12.0]);
        assert_eq!(glyph.y_offset, -3.0);
        assert_eq!(glyph.source[1], sheet[1] + 4 * 12, "row 4, at 12 texels a row");
        assert_eq!(glyph.source[3] - glyph.source[1], 12);
    }

    #[test]
    fn the_cells_json_can_be_wrong() {
        assert_eq!(AsciiCells::parse(b"{}"), None);
        assert_eq!(AsciiCells::parse(b"not json"), None);
        // With no offset, it starts at the top of the line.
        assert_eq!(AsciiCells::parse(br#"{"ascii_character_height":8}"#), Some(AsciiCells::PLAIN));
    }

    #[test]
    fn a_character_past_the_end_of_the_sheet_isnt_there() {
        // Eight rows: the last cell is 0x7F, and ASCII ends there. Six rows: it doesn't.
        let (atlas, picture) = Atlas::pack(vec![ascii_input([8, 8], 6, &[(A, 0, 4)])]);
        let font = Font::new(&atlas, &picture, AsciiCells::PLAIN, &BTreeSet::new());
        assert!(font.glyph('A').is_some());
        assert_eq!(font.glyph('z'), None);
        assert_eq!(font.advance('z'), SPACE);
    }

    #[test]
    fn a_glyph_page_is_twice_as_fine_and_trims_its_left() {
        let (atlas, picture) = Atlas::pack(vec![Input {
            name: page_path(0x20),
            image: sheet([16, 16], 16, &[(0x19, 3, 10)]),
            nine_slice: None,
        }]);
        let font = Font::new(&atlas, &picture, AsciiCells::PLAIN, &BTreeSet::from([0x20]));
        // U+2019 is page 0x20, cell 0x19. Eight texels are four units.
        let glyph = font.glyph('\u{2019}').unwrap();
        assert_eq!(glyph.size, [4.0, HEIGHT]);
        assert_eq!(glyph.advance, 5.0);
        assert_eq!(glyph.source[2] - glyph.source[0], 8);
    }

    #[test]
    fn formatting_codes_dont_show_or_take_room() {
        assert_eq!(shown("\u{a7}eGold").collect::<String>(), "Gold");
        assert_eq!(shown("a\u{a7}rb\u{a7}").collect::<String>(), "ab");
        assert_eq!(shown("\u{a7}\u{a7}x").collect::<String>(), "x");
        let font = font(&[(A, 0, 4)]);
        assert_eq!(font.width("\u{a7}cA"), font.width("A"));
    }
}
