//! The UI's pictures: sprites from the install, packed into one image at
//! startup and never written to disk (m5-plan Decision 14; CLAUDE.md, Rules).
//!
//! **An atlas here, an array for blocks** (worth teaching). Blocks use a texture
//! array because every face tiles and has mipmaps, so nothing may bleed between
//! textures (`render/textures.rs`). UI sprites come in every size, are drawn at
//! whole-pixel scales with nearest sampling, and never tile, so one big image
//! suits them: one texture, one bind group, and a screen is one draw.
//!
//! **Nine slices** (worth teaching). A panel has to fit any size without its
//! corners smearing. So the sprite is cut by four lines into nine pieces: the
//! corners keep their size, the edges stretch one way, and the middle both ways.
//! The JSON beside a sprite says where the lines are.

use std::collections::HashMap;

use image::RgbaImage;
use limonite_packs::{PackStack, json};

use crate::render::textures::read_image;

/// The empty row and column kept around each sprite, so that nothing beside it
/// can be sampled by accident.
const GAP: u32 = 1;
/// The narrowest atlas tried. It doubles until the sprites fit in a square.
const MIN_WIDTH: u32 = 256;

/// A sprite, by the order it was packed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpriteId(u16);

/// Where the lines of a nine-slice sprite go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NineSlice {
    /// Left, top, right and bottom, in the image's texels.
    pub texels: [u32; 4],
    /// The same, as drawn, in UI units. They differ when a pack's image is larger
    /// than its `base_size`.
    pub units: [f32; 4],
}

impl NineSlice {
    /// From the JSON beside a sprite, such as
    /// `{"nineslice_size":1,"base_size":[4,4]}`. `nineslice_size` is one number
    /// for all four sides, or left, top, right and bottom. `None` if there's none.
    pub fn parse(bytes: &[u8], image: [u32; 2]) -> Option<Self> {
        let value: serde_json::Value = json::parse(bytes).ok()?;
        let units = match value.get("nineslice_size")? {
            serde_json::Value::Array(sides) => {
                let side = |i: usize| sides.get(i).and_then(serde_json::Value::as_f64);
                [side(0)?, side(1)?, side(2)?, side(3)?]
            }
            all => [all.as_f64()?; 4],
        }
        .map(|side| side as f32);
        let base = value.get("base_size").and_then(serde_json::Value::as_array);
        let base_side = |i: usize| {
            let side = base.and_then(|base| base.get(i)).and_then(serde_json::Value::as_f64);
            side.map_or(image[i] as f32, |side| side as f32).max(1.0)
        };
        let per_unit = [image[0] as f32 / base_side(0), image[1] as f32 / base_side(1)];
        let texels = [
            units[0] * per_unit[0],
            units[1] * per_unit[1],
            units[2] * per_unit[0],
            units[3] * per_unit[1],
        ]
        .map(|side| side.round() as u32);
        Some(Self { texels, units })
    }
}

/// One piece of a drawn sprite: where it goes, in UI units, and where it comes
/// from, in texels. Both are left, top, right and bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub dest: [f32; 4],
    pub source: [u32; 4],
}

/// Cuts a sprite drawn into `dest` into its nine pieces. A `dest` too small for
/// two opposite corners squeezes them to fit, keeping their proportions, rather
/// than letting them cross.
#[must_use]
pub fn nine_pieces(dest: [f32; 4], source: [u32; 4], nine: &NineSlice) -> [Piece; 9] {
    let [left, top, right, bottom] = dest;
    let fit = |near: f32, far: f32, room: f32| {
        let total = near + far;
        if total > room && total > 0.0 {
            (near * room / total, far * room / total)
        } else {
            (near, far)
        }
    };
    let (inset_left, inset_right) = fit(nine.units[0], nine.units[2], right - left);
    let (inset_top, inset_bottom) = fit(nine.units[1], nine.units[3], bottom - top);
    let xs = [left, left + inset_left, right - inset_right, right];
    let ys = [top, top + inset_top, bottom - inset_bottom, bottom];
    let us = [source[0], source[0] + nine.texels[0], source[2] - nine.texels[2], source[2]];
    let vs = [source[1], source[1] + nine.texels[1], source[3] - nine.texels[3], source[3]];
    std::array::from_fn(|i| {
        let (column, row) = (i % 3, i / 3);
        Piece {
            dest: [xs[column], ys[row], xs[column + 1], ys[row + 1]],
            source: [us[column], vs[row], us[column + 1], vs[row + 1]],
        }
    })
}

/// A sprite's place in the atlas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    /// Left, top, right and bottom, in the atlas's texels.
    pub source: [u32; 4],
    pub nine_slice: Option<NineSlice>,
}

impl Sprite {
    /// How big it draws by default: one UI unit a texel.
    #[must_use]
    pub fn size(&self) -> [f32; 2] {
        [(self.source[2] - self.source[0]) as f32, (self.source[3] - self.source[1]) as f32]
    }
}

/// A picture on its way into the atlas.
pub struct Input {
    pub name: String,
    pub image: RgbaImage,
    pub nine_slice: Option<NineSlice>,
}

/// What's in the atlas, and where. The picture itself goes to the GPU once and
/// is then freed.
#[derive(Debug)]
pub struct Atlas {
    size: [u32; 2],
    sprites: Vec<Sprite>,
    names: HashMap<String, SpriteId>,
}

impl Atlas {
    #[must_use]
    pub fn sprite(&self, name: &str) -> Option<SpriteId> {
        self.names.get(name).copied()
    }

    #[must_use]
    pub fn get(&self, id: SpriteId) -> &Sprite {
        &self.sprites[usize::from(id.0)]
    }

    /// The whole atlas's width and height, in texels.
    #[must_use]
    pub const fn size(&self) -> [u32; 2] {
        self.size
    }

    /// How many sprites it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sprites.len()
    }

    /// Packs `inputs` into one picture.
    #[must_use]
    pub fn pack(inputs: Vec<Input>) -> (Self, RgbaImage) {
        let sizes: Vec<[u32; 2]> = inputs.iter().map(|input| [input.image.width(), input.image.height()]).collect();
        let (corners, size) = place(&sizes);
        let mut picture = RgbaImage::new(size[0], size[1]);
        let mut sprites = Vec::with_capacity(inputs.len());
        let mut names = HashMap::with_capacity(inputs.len());
        for (index, (input, corner)) in inputs.into_iter().zip(corners).enumerate() {
            image::imageops::replace(&mut picture, &input.image, i64::from(corner[0]), i64::from(corner[1]));
            sprites.push(Sprite {
                source: [
                    corner[0],
                    corner[1],
                    corner[0] + input.image.width(),
                    corner[1] + input.image.height(),
                ],
                nine_slice: input.nine_slice,
            });
            names.insert(input.name, SpriteId(index as u16));
        }
        (Self { size, sprites, names }, picture)
    }
}

/// Where each picture's top-left corner goes, and the atlas's size. The tallest
/// go first, in shelves, which wastes little when most sprites are small.
fn place(sizes: &[[u32; 2]]) -> (Vec<[u32; 2]>, [u32; 2]) {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&index| std::cmp::Reverse(sizes[index][1]));
    let widest = sizes.iter().map(|size| size[0]).max().unwrap_or(1);
    let mut width = MIN_WIDTH.max((widest + 2 * GAP).next_power_of_two());
    loop {
        let mut corners = vec![[0; 2]; sizes.len()];
        let (mut x, mut y, mut shelf) = (GAP, GAP, 0);
        for &index in &order {
            let [picture_width, picture_height] = sizes[index];
            if x + picture_width + GAP > width {
                x = GAP;
                y += shelf + GAP;
                shelf = 0;
            }
            corners[index] = [x, y];
            x += picture_width + GAP;
            shelf = shelf.max(picture_height);
        }
        let height = y + shelf + GAP;
        if height <= width {
            return (corners, [width, height]);
        }
        width *= 2;
    }
}

/// Reads a sprite (a path without its extension), with the nine-slice JSON beside
/// it if it has one.
///
/// # Errors
/// If no pack has it, or it can't be decoded.
pub fn read_sprite(stack: &PackStack, path: &str) -> Result<Input, String> {
    let image = read_image(stack, path)?;
    let nine_slice = stack
        .read(&format!("{path}.json"))
        .ok()
        .flatten()
        .and_then(|found| NineSlice::parse(&found.bytes, [image.width(), image.height()]));
    Ok(Input {
        name: path.to_string(),
        image,
        nine_slice,
    })
}

/// Reads part of a bigger picture, as the sprite `name`: the left, top, width and
/// height of it, in texels.
///
/// # Errors
/// If no pack has the picture, it can't be decoded, or it's too small for the part.
pub fn read_part(stack: &PackStack, name: &str, path: &str, part: [u32; 4]) -> Result<Input, String> {
    let image = read_image(stack, path)?;
    let [x, y, width, height] = part;
    if x + width > image.width() || y + height > image.height() {
        return Err(format!("{}×{} has no room for {part:?}", image.width(), image.height()));
    }
    Ok(Input {
        name: name.to_string(),
        image: image::imageops::crop_imm(&image, x, y, width, height).to_image(),
        nine_slice: None,
    })
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    fn flat(name: &str, width: u32, height: u32, shade: u8) -> Input {
        Input {
            name: name.to_string(),
            image: RgbaImage::from_pixel(width, height, Rgba([shade, shade, shade, 255])),
            nine_slice: None,
        }
    }

    #[test]
    fn sprites_never_touch_and_keep_their_pixels() {
        let inputs: Vec<Input> = (1..60).map(|i| flat(&format!("s{i}"), i % 17 + 1, i % 23 + 1, i as u8)).collect();
        let (atlas, picture) = Atlas::pack(inputs);
        assert_eq!(atlas.len(), 59);
        let rects: Vec<[u32; 4]> = (1..60)
            .map(|i| atlas.get(atlas.sprite(&format!("s{i}")).unwrap()).source)
            .collect();
        for (i, a) in rects.iter().enumerate() {
            assert!(a[2] < atlas.size()[0] && a[3] < atlas.size()[1], "{a:?} is inside, with a gap to spare");
            assert_eq!(picture.get_pixel(a[0], a[1])[0], (i + 1) as u8, "its own pixels");
            for b in &rects[i + 1..] {
                let apart = a[2] + GAP <= b[0] || b[2] + GAP <= a[0] || a[3] + GAP <= b[1] || b[3] + GAP <= a[1];
                assert!(apart, "{a:?} and {b:?} touch");
            }
        }
    }

    #[test]
    fn a_wide_sprite_widens_the_atlas() {
        let (atlas, _) = Atlas::pack(vec![flat("wide", 500, 2, 1), flat("small", 3, 3, 2)]);
        assert_eq!(atlas.size()[0], 512);
        assert_eq!(atlas.get(atlas.sprite("wide").unwrap()).size(), [500.0, 2.0]);
    }

    #[test]
    fn many_sprites_grow_the_atlas_until_they_fit_a_square() {
        let inputs: Vec<Input> = (0..40).map(|i| flat(&format!("s{i}"), 100, 100, 1)).collect();
        let (atlas, _) = Atlas::pack(inputs);
        assert!(atlas.size()[1] <= atlas.size()[0], "{:?}", atlas.size());
        assert_eq!(atlas.size()[0], 1024);
    }

    #[test]
    fn nothing_to_pack_is_still_a_picture() {
        let (atlas, picture) = Atlas::pack(Vec::new());
        assert_eq!(atlas.len(), 0);
        assert!(picture.width() > 0 && picture.height() > 0);
    }

    #[test]
    fn nine_slice_json_in_both_shapes() {
        let one = NineSlice::parse(br#"{"nineslice_size":1,"base_size":[4,4]}"#, [4, 4]).unwrap();
        assert_eq!(one.texels, [1; 4]);
        assert_eq!(one.units, [1.0; 4]);

        let four = NineSlice::parse(br#"{"nineslice_size":[1,2,3,4],"base_size":[16,16]}"#, [16, 16]).unwrap();
        assert_eq!(four.texels, [1, 2, 3, 4]);

        // A pack's image twice the base size: twice the texels, the same units.
        let doubled = NineSlice::parse(br#"{"nineslice_size":2,"base_size":[8,8]}"#, [16, 16]).unwrap();
        assert_eq!(doubled.texels, [4; 4]);
        assert_eq!(doubled.units, [2.0; 4]);

        // No base size: the image's own.
        let plain = NineSlice::parse(br#"{"nineslice_size":3}"#, [9, 9]).unwrap();
        assert_eq!(plain.texels, [3; 4]);

        assert_eq!(NineSlice::parse(br#"{"base_size":[4,4]}"#, [4, 4]), None);
        assert_eq!(NineSlice::parse(br#"{"nineslice_size":[1,2]}"#, [4, 4]), None);
        assert_eq!(NineSlice::parse(b"not json", [4, 4]), None);
    }

    #[test]
    fn corners_keep_their_size_and_the_rest_stretches() {
        let nine = NineSlice {
            texels: [1; 4],
            units: [1.0; 4],
        };
        let pieces = nine_pieces([10.0, 20.0, 50.0, 40.0], [100, 200, 104, 204], &nine);
        // The top-left corner, 1 × 1 at both ends.
        assert_eq!(pieces[0].dest, [10.0, 20.0, 11.0, 21.0]);
        assert_eq!(pieces[0].source, [100, 200, 101, 201]);
        // The top edge stretches across.
        assert_eq!(pieces[1].dest, [11.0, 20.0, 49.0, 21.0]);
        assert_eq!(pieces[1].source, [101, 200, 103, 201]);
        // The middle stretches both ways.
        assert_eq!(pieces[4].dest, [11.0, 21.0, 49.0, 39.0]);
        assert_eq!(pieces[4].source, [101, 201, 103, 203]);
        // The bottom-right corner.
        assert_eq!(pieces[8].dest, [49.0, 39.0, 50.0, 40.0]);
        assert_eq!(pieces[8].source, [103, 203, 104, 204]);

        // Together the pieces cover it, whatever the sizes.
        let area: f32 = pieces
            .iter()
            .map(|piece| (piece.dest[2] - piece.dest[0]) * (piece.dest[3] - piece.dest[1]))
            .sum();
        assert_eq!(area, 40.0 * 20.0);
    }

    #[test]
    fn a_rect_too_small_squeezes_the_corners() {
        let nine = NineSlice {
            texels: [3, 3, 1, 1],
            units: [3.0, 3.0, 1.0, 1.0],
        };
        let pieces = nine_pieces([0.0, 0.0, 2.0, 8.0], [0, 0, 8, 8], &nine);
        // 3 and 1 in a width of 2: 1.5 and 0.5, and no middle.
        assert_eq!(pieces[0].dest, [0.0, 0.0, 1.5, 3.0]);
        assert_eq!(pieces[1].dest[0], pieces[1].dest[2]);
        assert_eq!(pieces[2].dest, [1.5, 0.0, 2.0, 3.0]);
    }
}
