//! Block textures on the CPU: decoded from the packs into one 2D texture
//! array, one layer per texture, all at one size, with mipmaps (m4-plan
//! Decision 7). Step 10 uploads it to the GPU.
//!
//! **Why an array, not an atlas.** An atlas packs every texture into one big
//! image, and each face gets UVs into it. An array keeps each texture in its
//! own layer instead:
//! - nothing bleeds between neighbouring textures, because a layer has no
//!   neighbours: filtering and mipmapping stay inside it;
//! - each texture gets its own mipmaps, all the way down to 1×1;
//! - a face stores a layer number, and its UVs always run from 0 to 1.
//!
//! The cost: every layer has the same size, so smaller textures are scaled
//! up to the largest (PLAN.md, "Textures").
//!
//! **Animated textures** are strips of frames. A layer holds what's showing now, and
//! `render/animation.rs` keeps the rest and rewrites the layer as they play.

use std::{collections::HashMap, time::Instant};

use image::RgbaImage;
use limonite_packs::{
    ImageFormat, PackStack, TextureFile,
    registry::{FlipbookFrames, Flipbooks},
};

use super::animation::{Animator, Step, Timeline};
use crate::blocks::jar::Jar;

/// Bedrock's own texture for anything that can't be resolved. It's always
/// layer 0.
pub const MISSING_TEXTURE: &str = "textures/misc/missing_texture";
/// A path with this in front names a texture in the Java jar, the way models name
/// it: `java:block/lantern`.
pub const JAVA_TEXTURE: &str = "java:";
/// The largest a texture from the jar is kept. Every layer is as big as the
/// biggest, so the one 64-texel texture in 26.3, on a block few will meet, would
/// make the whole array four times the memory.
const JAVA_TEXTURE_MAX: u32 = 32;

/// What a texture's alpha channel means. It decides how mipmaps average the
/// colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlphaUse {
    /// How much of the texel is there: leaves, glass, plants. Transparent
    /// texels often hold junk colour, so they mustn't darken the average.
    Coverage,
    /// A mask that marks texels to tint: a grass block's sides. The texture
    /// is drawn opaque, so every texel's colour counts.
    Mask,
}

/// The array, ready to upload.
pub struct TextureArray {
    /// Every layer's width and height, in texels.
    pub size: u32,
    pub layers: u32,
    /// The mip levels, largest first. Level n holds every layer at
    /// `size >> n` texels square, one after another, as RGBA bytes.
    pub mips: Vec<Vec<u8>>,
}

impl TextureArray {
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.mips.iter().map(Vec::len).sum()
    }
}

/// Where a texture went in the array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layer {
    pub index: u16,
    /// Some texel of one of its frames isn't fully opaque.
    pub transparent: bool,
}

/// The array, and what happened to each texture asked for.
pub struct Loaded {
    pub array: TextureArray,
    /// For each requested texture, in order, its layer, or `None` if it
    /// couldn't be found or decoded. Those draw as layer 0, the missing
    /// texture.
    pub layers: Vec<Option<Layer>>,
    /// The paths that couldn't be loaded, and why.
    pub failed: Vec<(String, String)>,
    /// How long decoding and building took.
    pub seconds: f64,
    /// The layers that move.
    pub animator: Animator,
}

/// A texture as read: its frames, and how they play if there's more than one.
struct Strip {
    frames: Vec<RgbaImage>,
    timeline: Option<Timeline>,
}

impl Strip {
    /// The frame a layer starts with.
    fn first(&self) -> &RgbaImage {
        let first = self.timeline.as_ref().and_then(|timeline| timeline.steps.first());
        first
            .and_then(|step| self.frames.get(usize::from(step.frame)))
            .unwrap_or(&self.frames[0])
    }

    /// Everything that makes two strips the same picture.
    fn fingerprint(&self, alpha: AlphaUse) -> ((u32, u32), Vec<u8>, Option<Timeline>, AlphaUse) {
        let pixels = self.frames.iter().flat_map(|frame| frame.as_raw()).copied().collect();
        (self.frames[0].dimensions(), pixels, self.timeline.clone(), alpha)
    }
}

/// Loads every texture in `requests` (a path without its extension, and
/// how its alpha is used) into one array, after the missing texture.
#[must_use]
pub fn load(
    stack: &PackStack,
    mut jar: Option<&mut Jar>,
    flipbooks: &Flipbooks,
    requests: &[(String, AlphaUse)],
) -> Loaded {
    let start = Instant::now();
    let mut failed = Vec::new();
    let mut strips = Vec::new();

    // Layer 0. Should the install lack it, a checkerboard of our own stands
    // in: nothing Mojang's is ever bundled.
    let missing = match read_pack_frame(stack, MISSING_TEXTURE) {
        Ok(frame) => frame,
        Err(reason) => {
            failed.push((MISSING_TEXTURE.to_string(), reason));
            checkerboard()
        }
    };
    let missing = Strip {
        frames: vec![missing],
        timeline: None,
    };
    // Two textures with the same pixels share a layer: Bedrock and Java call many
    // of the same pictures by different names.
    let mut by_pixels = HashMap::from([(missing.fingerprint(AlphaUse::Coverage), 0u16)]);
    strips.push((missing, AlphaUse::Coverage));

    // The same path can be asked for more than once; it gets one layer.
    let mut by_request: HashMap<(&str, AlphaUse), Option<u16>> = HashMap::new();
    let mut layer_of = Vec::with_capacity(requests.len());
    for (path, alpha) in requests {
        let index = *by_request.entry((path.as_str(), *alpha)).or_insert_with(|| {
            match read_strip(stack, jar.as_deref_mut(), flipbooks, path) {
                Ok(strip) => {
                    let next = strips.len() as u16;
                    let index = *by_pixels.entry(strip.fingerprint(*alpha)).or_insert(next);
                    if index == next {
                        strips.push((strip, *alpha));
                    }
                    Some(index)
                }
                Err(reason) => {
                    failed.push((path.clone(), reason));
                    None
                }
            }
        });
        layer_of.push(index);
    }

    let size = strips
        .iter()
        .flat_map(|(strip, _)| &strip.frames)
        .map(|frame| frame.width().max(frame.height()))
        .max()
        .unwrap_or(1)
        .next_power_of_two();
    let level_count = size.ilog2() as usize + 1;
    let mut animator = Animator::new(size, level_count);
    let mut transparent = Vec::with_capacity(strips.len());
    let mut levels: Vec<Vec<u8>> = vec![Vec::new(); level_count];
    for (layer, (strip, alpha)) in (0u16..).zip(&strips) {
        transparent.push(strip.frames.iter().any(|frame| frame.pixels().any(|texel| texel[3] < 255)));
        for (mip, level) in levels.iter_mut().zip(mip_chain(strip.first(), size, *alpha, level_count)) {
            mip.extend_from_slice(&level);
        }
        if let Some(timeline) = &strip.timeline {
            let frames = strip.frames.iter().map(|frame| mip_chain(frame, size, *alpha, level_count).concat());
            animator.add(layer, frames.collect(), timeline.clone());
        }
    }

    let layers = layer_of
        .into_iter()
        .map(|index| {
            index.map(|index| Layer {
                index,
                transparent: transparent[usize::from(index)],
            })
        })
        .collect();
    Loaded {
        array: TextureArray {
            size,
            layers: strips.len() as u32,
            mips: levels,
        },
        layers,
        failed,
        animator,
        seconds: start.elapsed().as_secs_f64(),
    }
}

/// A frame's mip levels at this array's size: the frame scaled to fit, then each level
/// half the last.
fn mip_chain(frame: &RgbaImage, size: u32, alpha: AlphaUse, levels: usize) -> Vec<Vec<u8>> {
    let mut chain = Vec::with_capacity(levels);
    let mut level = scale_nearest(frame, size);
    let mut level_size = size;
    for _ in 0..levels {
        chain.push(level.clone());
        if level_size > 1 {
            level = downsample(&level, level_size, alpha);
            level_size /= 2;
        }
    }
    chain
}

/// Finds a texture, in the jar if its path says so and otherwise in the stack, with
/// its frames if it's animated.
fn read_strip(
    stack: &PackStack,
    jar: Option<&mut Jar>,
    flipbooks: &Flipbooks,
    path: &str,
) -> Result<Strip, String> {
    if let Some(name) = path.strip_prefix(JAVA_TEXTURE) {
        return read_java_strip(jar.ok_or("there's no Java jar")?, name);
    }
    let image = read_image(stack, path)?;
    let Some(flipbook) = flipbooks.by_path.get(path) else {
        return Ok(Strip {
            frames: vec![first_frame(image)],
            timeline: None,
        });
    };
    let frames = split_frames(&image);
    let order: Vec<usize> = match &flipbook.frames {
        None => (0..frames.len()).collect(),
        Some(FlipbookFrames::Count(count)) => (0..frames.len().min(*count as usize)).collect(),
        Some(FlipbookFrames::List(list)) => list.iter().map(|&frame| frame as usize).filter(|&frame| frame < frames.len()).collect(),
    };
    let timeline = Timeline {
        steps: order
            .into_iter()
            .map(|frame| Step {
                frame: frame as u16,
                ticks: flipbook.ticks_per_frame.max(1),
            })
            .collect(),
        blend: flipbook.blend_frames,
    };
    Ok(Strip {
        frames,
        timeline: Some(timeline),
    })
}

/// A texture from the jar, by the name models give it. A `.mcmeta` file beside it says
/// how it plays, and without one it doesn't.
fn read_java_strip(jar: &mut Jar, name: &str) -> Result<Strip, String> {
    let image = read_java_image(jar, name)?;
    let animation = jar
        .read(&format!("assets/minecraft/textures/{name}.png.mcmeta"))
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|meta| meta.get("animation").cloned());
    let Some(animation) = animation else {
        return Ok(Strip {
            frames: vec![capped(first_frame(image))],
            timeline: None,
        });
    };
    let frames: Vec<RgbaImage> = split_frames(&image).into_iter().map(capped).collect();
    let timeline = java_timeline(&animation, frames.len());
    Ok(Strip {
        frames,
        timeline: Some(timeline),
    })
}

/// How a `.mcmeta` file's `animation` plays a strip of `frames`. Its list of frames can
/// hold a frame's number, or a number and how long it shows.
fn java_timeline(animation: &serde_json::Value, frames: usize) -> Timeline {
    let frametime = animation.get("frametime").and_then(serde_json::Value::as_u64).unwrap_or(1) as u32;
    let blend = animation.get("interpolate").and_then(serde_json::Value::as_bool).unwrap_or(false);
    let Some(list) = animation.get("frames").and_then(serde_json::Value::as_array) else {
        return Timeline::in_order(frames, frametime, blend);
    };
    let steps = list.iter().filter_map(|entry| {
        let frame = entry.as_u64().or_else(|| entry.get("index")?.as_u64())? as usize;
        let ticks = entry.get("time").and_then(serde_json::Value::as_u64).map_or(frametime, |time| time as u32);
        (frame < frames).then_some(Step {
            frame: frame as u16,
            ticks: ticks.max(1),
        })
    });
    Timeline {
        steps: steps.collect(),
        blend,
    }
}

/// Finds a texture in the stack, decodes it, and takes its first frame.
///
/// # Errors
/// If no pack has it, or it can't be decoded.
pub fn read_pack_frame(stack: &PackStack, path: &str) -> Result<RgbaImage, String> {
    read_image(stack, path).map(first_frame)
}

/// Finds a texture in the jar, by the name models give it, decodes it, and takes
/// its first frame.
///
/// # Errors
/// If the jar hasn't got it, or it can't be decoded.
pub fn read_java_frame(jar: &mut Jar, name: &str) -> Result<RgbaImage, String> {
    read_java_image(jar, name).map(|image| capped(first_frame(image)))
}

/// A texture from the jar, whole.
fn read_java_image(jar: &mut Jar, name: &str) -> Result<RgbaImage, String> {
    let bytes = jar
        .read(&format!("assets/minecraft/textures/{name}.png"))
        .ok_or("not in the jar")?;
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map(|image| image.to_rgba8())
        .map_err(|err| format!("can't decode it as png: {err}"))
}

/// A frame from the jar, no bigger than [`JAVA_TEXTURE_MAX`].
fn capped(frame: RgbaImage) -> RgbaImage {
    if frame.width().max(frame.height()) > JAVA_TEXTURE_MAX {
        image::imageops::resize(&frame, JAVA_TEXTURE_MAX, JAVA_TEXTURE_MAX, image::imageops::FilterType::Nearest)
    } else {
        frame
    }
}

/// Finds an image in the stack (a path without its extension) and decodes
/// it whole.
///
/// # Errors
/// If no pack has it, or it can't be decoded.
pub fn read_image(stack: &PackStack, path: &str) -> Result<RgbaImage, String> {
    let file = stack
        .read_texture(path)
        .map_err(|err| err.to_string())?
        .ok_or("in no pack")?;
    decode(&file)
}

/// Decodes a PNG or TGA. TGA has no magic number, so the extension it was
/// found under says which. `image` flips TGAs stored bottom-up.
fn decode(file: &TextureFile) -> Result<RgbaImage, String> {
    let format = match file.format {
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Tga => image::ImageFormat::Tga,
    };
    image::load_from_memory_with_format(&file.bytes, format)
        .map(|image| image.to_rgba8())
        .map_err(|err| format!("can't decode it as {}: {err}", file.format.extension()))
}

/// A strip taller than it is wide is a flipbook: its frames are squares
/// stacked top to bottom. This is its first.
fn first_frame(image: RgbaImage) -> RgbaImage {
    let (width, height) = image.dimensions();
    if height > width {
        image::imageops::crop_imm(&image, 0, 0, width, width).to_image()
    } else {
        image
    }
}

/// All the frames of a strip, top to bottom. A picture that isn't taller than it's
/// wide is one frame.
fn split_frames(image: &RgbaImage) -> Vec<RgbaImage> {
    let (width, height) = image.dimensions();
    if width == 0 || height <= width {
        return vec![image.clone()];
    }
    (0..height / width)
        .map(|frame| image::imageops::crop_imm(image, 0, frame * width, width, width).to_image())
        .collect()
}

/// Scales `frame` to `size` × `size` by nearest neighbour, returning RGBA
/// bytes. A 16×16 texture in a 32 array becomes each texel doubled, so pixel
/// art stays sharp instead of blurring.
fn scale_nearest(frame: &RgbaImage, size: u32) -> Vec<u8> {
    let (width, height) = frame.dimensions();
    if (width, height) == (size, size) {
        return frame.as_raw().clone();
    }
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        let source_y = y * height / size;
        for x in 0..size {
            let source_x = x * width / size;
            out.extend_from_slice(&frame.get_pixel(source_x, source_y).0);
        }
    }
    out
}

/// The next mip level: each 2×2 block of texels averaged into one (a box
/// filter).
///
/// Alpha is always the plain average. For [`AlphaUse::Coverage`], colour is
/// weighted by alpha, so the junk colour in see-through texels can't bleed
/// a dark fringe into a leaf's edge.
fn downsample(level: &[u8], size: u32, alpha_use: AlphaUse) -> Vec<u8> {
    let half = size / 2;
    let size = size as usize;
    let texel = |x: usize, y: usize| &level[(y * size + x) * 4..(y * size + x) * 4 + 4];
    let mut out = Vec::with_capacity(half as usize * half as usize * 4);
    for y in 0..half as usize {
        for x in 0..half as usize {
            let block = [
                texel(2 * x, 2 * y),
                texel(2 * x + 1, 2 * y),
                texel(2 * x, 2 * y + 1),
                texel(2 * x + 1, 2 * y + 1),
            ];
            let alpha_sum: u32 = block.iter().map(|t| u32::from(t[3])).sum();
            for channel in 0..3 {
                let value = match alpha_use {
                    AlphaUse::Coverage if alpha_sum > 0 => {
                        let weighted: u32 = block.iter().map(|t| u32::from(t[channel]) * u32::from(t[3])).sum();
                        (weighted + alpha_sum / 2) / alpha_sum
                    }
                    _ => (block.iter().map(|t| u32::from(t[channel])).sum::<u32>() + 2) / 4,
                };
                out.push(value as u8);
            }
            out.push(((alpha_sum + 2) / 4) as u8);
        }
    }
    out
}

/// A 16×16 magenta and black checkerboard, drawn here.
fn checkerboard() -> RgbaImage {
    RgbaImage::from_fn(16, 16, |x, y| {
        if (x / 8 + y / 8) % 2 == 0 {
            image::Rgba([255, 0, 255, 255])
        } else {
            image::Rgba([0, 0, 0, 255])
        }
    })
}

#[cfg(test)]
mod tests {
    use image::Rgba;
    use limonite_packs::Pack;

    use super::*;
    use crate::test_util::{TempDir, png, tga};

    fn stack(dir: &TempDir) -> PackStack {
        PackStack::new(vec![Pack::new(dir.path().to_path_buf())])
    }

    #[test]
    fn a_flipbook_shows_its_first_frame_scaled_up() {
        let dir = TempDir::new("flipbook");
        // A 2-frame strip of 16×16: red on top, blue below.
        let strip = RgbaImage::from_fn(16, 32, |_, y| {
            if y < 16 { Rgba([255, 0, 0, 255]) } else { Rgba([0, 0, 255, 255]) }
        });
        dir.write("textures/blocks/water_still.png", &png(&strip));
        // A 32×32 texture sets the layer size.
        let big = RgbaImage::from_fn(32, 32, |x, _| Rgba([x as u8, 0, 0, 255]));
        dir.write("textures/blocks/shelf.png", &png(&big));
        dir.write("textures/misc/missing_texture.png", &png(&checkerboard()));

        let loaded = load(
            &stack(&dir),
            None,
            &Flipbooks::default(),
            &[
                ("textures/blocks/water_still".to_string(), AlphaUse::Coverage),
                ("textures/blocks/shelf".to_string(), AlphaUse::Coverage),
                ("textures/blocks/nothing".to_string(), AlphaUse::Coverage),
            ],
        );
        assert_eq!(loaded.array.size, 32);
        assert_eq!(loaded.array.layers, 3, "missing, water, shelf");
        assert_eq!(loaded.layers[0].map(|l| l.index), Some(1));
        assert_eq!(loaded.layers[2], None);
        assert_eq!(loaded.failed.len(), 1);

        // Layer 1, level 0: the red first frame, every texel doubled.
        let layer = &loaded.array.mips[0][32 * 32 * 4..2 * 32 * 32 * 4];
        assert!(layer.as_chunks::<4>().0.iter().all(|t| *t == [255, 0, 0, 255]));
        // The shelf's columns 2x and 2x+1 come from source columns 2x, 2x+1:
        // a 32-wide texture isn't scaled.
        let shelf = &loaded.array.mips[0][2 * 32 * 32 * 4..];
        assert_eq!(shelf[5 * 4], 5);
    }

    #[test]
    fn the_mip_chain_reaches_one_texel() {
        let dir = TempDir::new("mips");
        let grey = RgbaImage::from_pixel(16, 16, Rgba([100, 150, 200, 255]));
        dir.write("textures/blocks/a.tga", &tga(&grey));
        let loaded = load(&stack(&dir), None, &Flipbooks::default(), &[("textures/blocks/a".to_string(), AlphaUse::Coverage)]);
        // The missing texture isn't in this pack, so the checkerboard stands in.
        assert_eq!(loaded.failed[0].0, MISSING_TEXTURE);
        assert_eq!(loaded.array.size, 16);
        let sizes: Vec<usize> = loaded.array.mips.iter().map(Vec::len).collect();
        assert_eq!(sizes, [16 * 16 * 4 * 2, 8 * 8 * 4 * 2, 4 * 4 * 4 * 2, 2 * 2 * 4 * 2, 4 * 2]);
        // A flat colour stays that colour all the way down.
        let last = &loaded.array.mips[4];
        assert_eq!(&last[4..8], [100, 150, 200, 255]);
        assert!(!loaded.layers[0].unwrap().transparent);
    }

    #[test]
    fn mipmaps_weigh_colour_by_coverage_but_not_masks() {
        // One opaque white texel, three see-through black ones.
        let level = [
            255, 255, 255, 255, 0, 0, 0, 0, //
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert_eq!(downsample(&level, 2, AlphaUse::Coverage), [255, 255, 255, 64]);
        assert_eq!(downsample(&level, 2, AlphaUse::Mask), [64, 64, 64, 64]);
        // All see-through: no weight, so a plain average.
        assert_eq!(downsample(&[8, 8, 8, 0].repeat(4), 2, AlphaUse::Coverage), [8, 8, 8, 0]);
    }

    #[test]
    fn textures_with_the_same_pixels_share_a_layer() {
        let dir = TempDir::new("same_pixels");
        let grey = RgbaImage::from_pixel(16, 16, Rgba([100, 150, 200, 255]));
        let red = RgbaImage::from_pixel(16, 16, Rgba([255, 0, 0, 255]));
        dir.write("textures/blocks/a.tga", &tga(&grey));
        dir.write("textures/blocks/b.tga", &tga(&grey));
        dir.write("textures/blocks/c.tga", &tga(&red));
        let requests = ["a", "b", "c"].map(|name| (format!("textures/blocks/{name}"), AlphaUse::Coverage));
        let loaded = load(&stack(&dir), None, &Flipbooks::default(), &requests);
        let layer = |i: usize| loaded.layers[i].unwrap().index;
        assert_eq!(layer(0), layer(1));
        assert_ne!(layer(0), layer(2));
        assert_eq!(loaded.array.layers, 3, "with the missing texture");
        // A mask averages differently, so it isn't the same layer.
        let requests = [
            ("textures/blocks/a".to_string(), AlphaUse::Coverage),
            ("textures/blocks/b".to_string(), AlphaUse::Mask),
        ];
        let loaded = load(&stack(&dir), None, &Flipbooks::default(), &requests);
        assert_ne!(loaded.layers[0], loaded.layers[1]);
    }

    #[test]
    fn a_java_texture_with_no_jar_is_a_failure_not_a_panic() {
        let dir = TempDir::new("no_jar");
        let loaded = load(&stack(&dir), None, &Flipbooks::default(), &[(format!("{JAVA_TEXTURE}block/a"), AlphaUse::Coverage)]);
        assert_eq!(loaded.layers[0], None);
        assert!(loaded.failed.iter().any(|(path, _)| path == "java:block/a"));
    }

    /// A strip of flat frames, each one texel a side wider than the last's colour.
    fn strip(colors: &[[u8; 4]]) -> RgbaImage {
        RgbaImage::from_fn(16, 16 * colors.len() as u32, |_, y| Rgba(colors[(y / 16) as usize]))
    }

    fn flipbooks(text: &[u8]) -> Flipbooks {
        let dir = TempDir::new("flipbook_file");
        dir.write("textures/flipbook_textures.json", text);
        Flipbooks::load(&stack(&dir)).0
    }

    #[test]
    fn a_flipbook_plays_and_a_strip_without_one_shows_its_first_frame() {
        let dir = TempDir::new("flipbook");
        let (red, green, blue) = ([255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]);
        dir.write("textures/blocks/lava.png", &png(&strip(&[red, green, blue])));
        dir.write("textures/blocks/still.png", &png(&strip(&[red, green])));
        let books = flipbooks(
            br#"[{ "flipbook_texture": "textures/blocks/lava", "ticks_per_frame": 2, "frames": [2, 0], "blend_frames": false }]"#,
        );
        let requests = ["lava", "still"].map(|name| (format!("textures/blocks/{name}"), AlphaUse::Coverage));
        let mut loaded = load(&stack(&dir), None, &books, &requests);
        assert_eq!(loaded.animator.layers(), 1);
        assert_eq!(loaded.animator.frames().0, 3, "the strip's frames, kept");
        // The layer starts on the first frame the list names.
        let at = |layer: usize| &loaded.array.mips[0][layer * 16 * 16 * 4..][..4];
        assert_eq!(at(usize::from(loaded.layers[0].unwrap().index)), blue);
        assert_eq!(at(usize::from(loaded.layers[1].unwrap().index)), red);
        let lava = u32::from(loaded.layers[0].unwrap().index);
        let mut written = Vec::new();
        for tick in 0..5 {
            loaded.animator.advance(tick, &mut |layer, level, _, texels| {
                if level == 0 {
                    written.push((tick, layer, [texels[0], texels[1], texels[2]]));
                }
            });
        }
        assert_eq!(
            written,
            [(0, lava, [0, 0, 255]), (2, lava, [255, 0, 0]), (4, lava, [0, 0, 255])],
            "two ticks of each, in the order listed"
        );
    }

    #[test]
    fn strips_share_a_layer_only_if_they_play_the_same() {
        let dir = TempDir::new("flipbook_share");
        let (red, green) = ([255, 0, 0, 255], [0, 255, 0, 255]);
        for name in ["a", "b", "c"] {
            dir.write(&format!("textures/blocks/{name}.png"), &png(&strip(&[red, green])));
        }
        dir.write("textures/blocks/flat.png", &png(&strip(&[red])));
        let books = flipbooks(
            br#"[
                { "flipbook_texture": "textures/blocks/a" },
                { "flipbook_texture": "textures/blocks/b" },
                { "flipbook_texture": "textures/blocks/c", "ticks_per_frame": 9 }
            ]"#,
        );
        let requests = ["a", "b", "c", "flat"].map(|name| (format!("textures/blocks/{name}"), AlphaUse::Coverage));
        let loaded = load(&stack(&dir), None, &books, &requests);
        let layer = |i: usize| loaded.layers[i].unwrap().index;
        assert_eq!(layer(0), layer(1), "the same pixels, playing the same");
        assert_ne!(layer(0), layer(2), "the same pixels at another speed");
        assert_ne!(layer(0), layer(3), "the same first frame, but this one doesn't move");
    }

    #[test]
    fn a_frame_further_down_the_strip_can_make_a_texture_see_through() {
        let dir = TempDir::new("flipbook_alpha");
        dir.write("textures/blocks/a.png", &png(&strip(&[[255, 0, 0, 255], [255, 0, 0, 0]])));
        let books = flipbooks(br#"[{ "flipbook_texture": "textures/blocks/a" }]"#);
        let loaded = load(&stack(&dir), None, &books, &[("textures/blocks/a".to_string(), AlphaUse::Coverage)]);
        assert!(loaded.layers[0].unwrap().transparent);
    }

    #[test]
    fn a_mcmeta_file_says_how_a_jar_texture_plays() {
        let plays = |text: &str, frames| java_timeline(&serde_json::from_str(text).unwrap(), frames);
        assert_eq!(plays("{}", 2), Timeline::in_order(2, 1, false));
        assert_eq!(plays(r#"{"frametime": 5, "interpolate": true}"#, 2), Timeline::in_order(2, 5, true));
        let listed = plays(r#"{"frametime": 3, "frames": [1, {"index": 0, "time": 7}, 9]}"#, 2);
        assert_eq!(
            listed.steps,
            [Step { frame: 1, ticks: 3 }, Step { frame: 0, ticks: 7 }],
            "a frame that isn't in the strip is left out"
        );
    }

    #[test]
    fn transparency_is_reported_per_texture() {
        let dir = TempDir::new("alpha");
        let leaves = RgbaImage::from_fn(16, 16, |x, _| Rgba([0, 200, 0, if x % 2 == 0 { 255 } else { 0 }]));
        dir.write("textures/blocks/leaves.tga", &tga(&leaves));
        let loaded = load(&stack(&dir), None, &Flipbooks::default(), &[("textures/blocks/leaves".to_string(), AlphaUse::Coverage)]);
        assert!(loaded.layers[0].unwrap().transparent);
    }
}
