//! The light map: how bright each pair of sky and block levels is drawn (m5-plan Decision 12;
//! worth teaching).
//!
//! **Levels aren't brightness.** A block at sky level 15 by day is fully lit, but the same level at
//! night is dim and blue, while a torch's level looks the same at any hour, and warm. So a vertex
//! carries its levels (`mesher.rs`), and the shader looks up what they mean in a 16 × 16 picture,
//! block levels across and sky levels down, sampled smoothly between its texels. This is the picture.
//!
//! **Java's curve** (`LightTexture.updateLightTexture`), as the plan says: Bedrock's own lives in its
//! engine, so the official client's frames judge it.
//! - **a level's brightness** is `l / (4 - 3 l)` of `l = level / 15`, so light falls off fast in the
//!   dark: level 7 is a fifth as bright as 15;
//! - **the sky's** is scaled by how bright the sky is, and tinted from grey towards blue as it
//!   darkens; **the blocks'** is warm, as red runs ahead of green and green of blue;
//! - the two add, and then a gamma lift brightens the dark end, as Java's default brightness, 50%,
//!   does. Nothing is ever quite black: the darkest level is 10% grey.

/// The light map's side: 16 levels.
pub const SIZE: usize = 16;

/// How bright the sky is at noon, as Java's `getSkyDarken` puts it: 1, down to 0.2 at night.
pub const NOON: f32 = 1.0;

/// Java's brightness of a light level in the overworld, which has no ambient light.
fn brightness(level: usize) -> f32 {
    let fraction = level as f32 / 15.0;
    fraction / (4.0 - 3.0 * fraction)
}

fn lerp(from: [f32; 3], to: [f32; 3], by: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| from[i] + (to[i] - from[i]) * by)
}

/// The light map for a sky `daylight` bright, from 0.2 at night to [`NOON`]: sRGB colours, block
/// level across and sky level down.
#[must_use]
pub fn light_map(daylight: f32) -> [[u8; 4]; SIZE * SIZE] {
    // No lightning, so the sky's light is this, and its colour goes from grey to blue as it fades.
    let sky_light = daylight * 0.95 + 0.05;
    let sky_colour = lerp([daylight, daylight, 1.0], [1.0; 3], 0.35);
    // Java adds a flicker to the blocks' red; this is its resting value.
    let flicker = 1.5;
    let mut map = [[0; 4]; SIZE * SIZE];
    for sky in 0..SIZE {
        for block in 0..SIZE {
            let sky_brightness = brightness(sky) * sky_light;
            let red = brightness(block) * flicker;
            let green = red * ((red * 0.6 + 0.4) * 0.6 + 0.4);
            let blue = red * (red * red * 0.6 + 0.4);
            let mut colour = [0, 1, 2].map(|i| [red, green, blue][i] + sky_colour[i] * sky_brightness);
            colour = lerp(colour, [0.75; 3], 0.04).map(|c| c.clamp(0.0, 1.0));
            // Java's gamma, at its default of 0.5: halfway to a curve that lifts the dark.
            let lifted = colour.map(|c| 1.0 - (1.0 - c).powi(4));
            colour = lerp(lerp(colour, lifted, 0.5), [0.75; 3], 0.04).map(|c| c.clamp(0.0, 1.0));
            let [r, g, b] = colour.map(|c| (c * 255.0) as u8);
            map[sky * SIZE + block] = [r, g, b, 255];
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(map: &[[u8; 4]; SIZE * SIZE], sky: usize, block: usize) -> [u8; 3] {
        let [r, g, b, _] = map[sky * SIZE + block];
        [r, g, b]
    }

    #[test]
    fn noon_is_nearly_white_and_the_dark_is_never_black() {
        let noon = light_map(NOON);
        assert_eq!(at(&noon, 15, 0), [251, 251, 251], "open sky at noon");
        assert_eq!(at(&noon, 0, 0), [25, 25, 25], "no light at all");
        // Brighter with every level of the sky's, and of the blocks' until it whites out.
        for level in 1..SIZE {
            assert!(at(&noon, level, 0)[1] > at(&noon, level - 1, 0)[1], "sky level {level}");
            assert!(at(&noon, 0, level)[1] >= at(&noon, 0, level - 1)[1], "block level {level}");
        }
        assert_eq!(at(&noon, 0, 14), [252, 252, 252], "a torch's own level is white");
    }

    #[test]
    fn the_night_sky_is_dim_and_blue_and_torchlight_warm() {
        let night = light_map(0.2);
        assert_eq!(at(&night, 15, 0), [80, 80, 125], "open sky at midnight");
        // A torch's light five blocks off, at night: as bright as by day, red first and blue last.
        let torch = at(&night, 0, 9);
        assert_eq!(torch, at(&light_map(NOON), 0, 9));
        assert!(torch[0] > torch[1] && torch[1] > torch[2], "{torch:?}");
    }
}
