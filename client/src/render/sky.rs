//! The time of day on screen (m5-plan Decision 12): the light map, the sky's colour and fog.
//!
//! **The time of day** comes from the server's `SetTime`, counted on a tick at a time between
//! (`clock.rs`). Java's curves turn it into how bright the sky is ([`daylight`]), its colours, and a
//! glow towards the sun as it rises and sets. Bedrock's own live in its engine.
//!
//! **Fog** is Bedrock's, read from the install's `fogs/default_fog_setting.json` at startup
//! ([`FogSettings`]): in air it starts at 92% of the view distance and ends at its edge, just inside
//! the meshed area, so the edge of the world fades into the sky instead of stopping, and so does the
//! dark band M4 saw where far water showed the sky through it. Under water it's 60 blocks, and
//! closes in to a quarter of that for a moment when the eyes go under. Lava shows next to nothing.
//!
//! **The sky** is drawn first (`terrain.wgsl`), as Java draws it: a plane 16 blocks up in the sky's
//! colour, fogged by its distance, so the fog's colour shows at the horizon and the sky's a little way
//! up; and a glow 50° across on the horizon below a rising or setting sun.
//!
//! # The light map (worth teaching)
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

use std::{
    f32::consts::{PI, TAU},
    time::Instant,
};

use glam::{DVec3, Vec3};
use limonite_packs::PackStack;
use serde_json::Value;

use crate::{
    blocks::appearance::{AppearanceTable, Liquid},
    world::World,
};

/// The light map's side: 16 levels.
pub const SIZE: usize = 16;

/// How bright the sky is at noon, as Java's `getSkyDarken` puts it: 1, down to 0.2 at night.
pub const NOON: f32 = 1.0;
/// Noon, in ticks of the day: what the sky shows before the server says what time it is.
pub const NOON_TICKS: i64 = 6_000;

/// Java's plains sky, `#78A7FF`: Bedrock's sky colours live in its engine.
const SKY: [u8; 3] = [0x78, 0xA7, 0xFF];

/// Java's `DimensionType.timeOfDay`: how far round the sun has gone, as a fraction of a turn, from
/// 0 at noon to 0.5 at midnight. The curve keeps the sun up a little longer than it's down.
#[must_use]
pub fn sun_angle(time: i64) -> f32 {
    let along = (time as f64 / 24_000.0 - 0.25).rem_euclid(1.0);
    let bent = 0.5 - (along * std::f64::consts::PI).cos() / 2.0;
    ((along * 2.0 + bent) / 3.0) as f32
}

/// Java's `getSkyDarken`, with no rain: how bright the sky lights the world, 1 by day and 0.2 by
/// night. The light map takes it.
#[must_use]
pub fn daylight(time: i64) -> f32 {
    let dark = (1.0 - ((sun_angle(time) * TAU).cos() * 2.0 + 0.2)).clamp(0.0, 1.0);
    (1.0 - dark) * 0.8 + 0.2
}

/// How bright the sky's and the fog's colours are: Java's, from 1 by day to 0 at night.
fn colour_brightness(time: i64) -> f32 {
    ((sun_angle(time) * TAU).cos() * 2.0 + 0.5).clamp(0.0, 1.0)
}

/// Java's sunrise and sunset glow: its sRGB colour and how strong it is, while the sun is near the
/// horizon.
#[must_use]
pub fn glow(time: i64) -> Option<[f32; 4]> {
    let height = (sun_angle(time) * TAU).cos();
    if !(-0.4..=0.4).contains(&height) {
        return None;
    }
    let along = height / 0.4 * 0.5 + 0.5;
    let strength = 1.0 - (1.0 - (along * PI).sin()) * 0.99;
    Some([along * 0.3 + 0.7, along * along * 0.7 + 0.2, 0.2, strength * strength])
}

/// Which way along x the sun is: 1 to the east as it rises, -1 to the west as it sets.
fn sun_side(time: i64) -> f32 {
    if (sun_angle(time) * TAU).sin() > 0.0 { -1.0 } else { 1.0 }
}

/// One kind of fog, as Bedrock's files give it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    pub start: f32,
    pub end: f32,
    /// sRGB.
    pub colour: [u8; 3],
    /// Whether `start` and `end` are fractions of the view distance, or blocks.
    pub of_view: bool,
}

impl Fog {
    fn read(value: &Value) -> Option<Self> {
        let colour = value.get("fog_color")?.as_str()?.strip_prefix('#')?;
        let colour = u32::from_str_radix(colour, 16).ok()?;
        Some(Self {
            start: value.get("fog_start")?.as_f64()? as f32,
            end: value.get("fog_end")?.as_f64()? as f32,
            colour: [(colour >> 16) as u8, (colour >> 8) as u8, colour as u8],
            of_view: value.get("render_distance_type")?.as_str()? == "render",
        })
    }

    /// Where it starts and ends, in blocks, for a view `view` blocks deep.
    fn distances(&self, view: f32) -> (f32, f32) {
        let scale = if self.of_view { view } else { 1.0 };
        (self.start * scale, self.end * scale)
    }
}

/// How the water's fog closes in when the eyes go under (Bedrock's `transition_fog`): to `least`
/// of its distance at first, `middle` after `middle_seconds`, and all of it after `most_seconds`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    pub least: f32,
    pub middle_seconds: f32,
    pub middle: f32,
    pub most_seconds: f32,
}

impl Transition {
    fn read(value: &Value) -> Option<Self> {
        let number = |key: &str| value.get(key)?.as_f64().map(|value| value as f32);
        Some(Self {
            least: number("min_percent")?,
            middle_seconds: number("mid_seconds")?,
            middle: number("mid_percent")?,
            most_seconds: number("max_seconds")?,
        })
    }

    /// How much of the fog's distance shows, `seconds` after the eyes went under.
    fn fraction(&self, seconds: f32) -> f32 {
        if seconds < self.middle_seconds {
            self.least + (self.middle - self.least) * seconds / self.middle_seconds
        } else if seconds < self.most_seconds {
            self.middle + (1.0 - self.middle) * (seconds - self.middle_seconds) / (self.most_seconds - self.middle_seconds)
        } else {
            1.0
        }
    }
}

/// Bedrock's default fogs, from the install.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogSettings {
    pub air: Fog,
    pub water: Fog,
    pub water_transition: Option<Transition>,
    pub lava: Fog,
}

/// Where the default fogs are.
const FOG_FILE: &str = "fogs/default_fog_setting.json";

impl FogSettings {
    /// Bedrock 1.26.60's, for an install that hasn't got them: what the file says.
    pub const FALLBACK: Self = Self {
        air: Fog {
            start: 0.92,
            end: 1.0,
            colour: [0xAB, 0xD2, 0xFF],
            of_view: true,
        },
        water: Fog {
            start: 0.0,
            end: 60.0,
            colour: [0x44, 0xAF, 0xF5],
            of_view: false,
        },
        water_transition: Some(Transition {
            least: 0.25,
            middle_seconds: 5.0,
            middle: 0.6,
            most_seconds: 30.0,
        }),
        lava: Fog {
            start: 0.0,
            end: 0.64,
            colour: [0x99, 0x1A, 0x00],
            of_view: false,
        },
    };

    /// Reads the default fogs from the vanilla packs. What's missing or unreadable is the fallback's,
    /// and said in the error.
    pub fn load(stack: &PackStack) -> (Self, Result<(), String>) {
        let read = || -> Result<Self, String> {
            let found = stack
                .read(FOG_FILE)
                .map_err(|err| err.to_string())?
                .ok_or_else(|| format!("{FOG_FILE} is in no pack"))?;
            let root: Value = serde_json::from_slice(&found.bytes).map_err(|err| err.to_string())?;
            let distance = &root["minecraft:fog_settings"]["distance"];
            let fog = |name: &str| Fog::read(&distance[name]).ok_or_else(|| format!("{FOG_FILE} has no {name} fog"));
            Ok(Self {
                air: fog("air")?,
                water: fog("water")?,
                water_transition: Transition::read(&distance["water"]["transition_fog"]),
                lava: fog("lava")?,
            })
        };
        match read() {
            Ok(settings) => (settings, Ok(())),
            Err(err) => (Self::FALLBACK, Err(err)),
        }
    }
}

/// What the eyes are in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Medium {
    Air,
    /// Water, since this many seconds ago.
    Water(f32),
    Lava,
}

/// What the eyes are in, frame to frame: it counts how long they've been under water.
#[derive(Debug, Default)]
pub struct Eyes {
    under_water_since: Option<Instant>,
}

impl Eyes {
    /// What eyes at `eye` are in at `now`: the liquid in their block, on either layer.
    pub fn medium(&mut self, world: &World, table: &AppearanceTable, eye: DVec3, now: Instant) -> Medium {
        let [x, y, z] = eye.floor().to_array().map(|value| value as i32);
        let liquid = (0..2).find_map(|layer| table.get(world.block(x, y, z, layer)?).liquid());
        if liquid != Some(Liquid::Water) {
            self.under_water_since = None;
        }
        match liquid {
            Some(Liquid::Water) => Medium::Water((now - *self.under_water_since.get_or_insert(now)).as_secs_f32()),
            Some(Liquid::Lava) => Medium::Lava,
            None => Medium::Air,
        }
    }
}

/// One frame's sky and fog, in linear light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyFrame {
    pub sky: [f32; 3],
    pub fog: [f32; 3],
    /// The glow towards the sun, and how strong it is.
    pub glow: [f32; 4],
    /// Which way along x the sun is.
    pub sun_side: f32,
    /// Where the fog starts and ends, in blocks.
    pub fog_start: f32,
    pub fog_end: f32,
}

/// sRGB to linear light, for the frame, which blends in linear light.
fn linear(srgb: f32) -> f32 {
    if srgb <= 0.040_45 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    }
}

fn unit(colour: [u8; 3]) -> [f32; 3] {
    colour.map(|channel| f32::from(channel) / 255.0)
}

/// The sky and fog at `time`, for eyes in `medium` that look along `look`, with a view `view`
/// blocks deep.
#[must_use]
pub fn frame(settings: &FogSettings, time: i64, medium: Medium, view: f32, look: Vec3) -> SkyFrame {
    let brightness = colour_brightness(time);
    let side = sun_side(time);
    let glow = glow(time);
    let sky = unit(SKY).map(|channel| channel * brightness);
    // Java darkens the fog's colour, but keeps a little blue in it at night.
    let darken = [brightness * 0.94 + 0.06, brightness * 0.94 + 0.06, brightness * 0.91 + 0.09];
    let (fog, start, end, glows) = match medium {
        Medium::Air => {
            let mut fog = [0, 1, 2].map(|i| unit(settings.air.colour)[i] * darken[i]);
            // Looking towards a rising or setting sun, the fog takes its colour.
            if let Some([r, g, b, strength]) = glow {
                let towards = (look.x * side).max(0.0) * strength;
                fog = [0, 1, 2].map(|i| fog[i] * (1.0 - towards) + [r, g, b][i] * towards);
            }
            let (start, end) = settings.air.distances(view);
            (fog, start, end, true)
        }
        Medium::Water(seconds) => {
            let fog = [0, 1, 2].map(|i| unit(settings.water.colour)[i] * darken[i]);
            let (start, end) = settings.water.distances(view);
            let closer = settings.water_transition.map_or(1.0, |transition| transition.fraction(seconds));
            (fog, start, end * closer, false)
        }
        Medium::Lava => {
            let (start, end) = settings.lava.distances(view);
            (unit(settings.lava.colour), start, end, false)
        }
    };
    SkyFrame {
        // In anything but air, the fog is all there is to see past it.
        sky: if glows { sky } else { fog }.map(linear),
        fog: fog.map(linear),
        glow: match glow {
            Some([r, g, b, strength]) if glows => [linear(r), linear(g), linear(b), strength],
            _ => [0.0; 4],
        },
        sun_side: side,
        fog_start: start,
        fog_end: end,
    }
}

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

    #[test]
    fn the_day_is_bright_at_noon_dark_at_midnight_and_glows_at_either_end() {
        assert_eq!(daylight(NOON_TICKS), 1.0);
        assert!((daylight(18_000) - 0.2).abs() < 1e-6, "midnight");
        // Sunrise is at 0 and sunset at 12,000; the light's halfway as the sun crosses the horizon.
        assert!((0.5..0.8).contains(&daylight(0)), "{}", daylight(0));
        assert!((0.5..0.8).contains(&daylight(12_000)), "{}", daylight(12_000));
        assert!(glow(NOON_TICKS).is_none() && glow(18_000).is_none());
        let sunset = glow(12_500).expect("a glow at sunset");
        assert!(sunset[0] > sunset[1] && sunset[1] > sunset[2], "orange: {sunset:?}");
        assert_eq!((sun_side(500), sun_side(12_500)), (1.0, -1.0), "east at sunrise, west at sunset");
    }

    #[test]
    fn the_sky_and_fog_follow_the_time_and_what_the_eyes_are_in() {
        let settings = FogSettings::FALLBACK;
        let north = Vec3::Z;
        let noon = frame(&settings, NOON_TICKS, Medium::Air, 128.0, north);
        assert_eq!((noon.fog_start, noon.fog_end), (0.92 * 128.0, 128.0), "at the view's edge");
        let night = frame(&settings, 18_000, Medium::Air, 128.0, north);
        assert!(night.sky.iter().all(|&channel| channel == 0.0), "Java's night sky is black");
        assert!(night.fog[2] > night.fog[0] && night.fog[2] < noon.fog[2], "the fog keeps a little blue");
        // A sunset seen from the east, and looked at from the west.
        let away = frame(&settings, 12_500, Medium::Air, 128.0, Vec3::X);
        let towards = frame(&settings, 12_500, Medium::Air, 128.0, -Vec3::X);
        assert!(towards.fog[0] > away.fog[0], "the fog takes the glow's colour towards the sun");
        // Under water: 60 blocks, closing in to 15 as the eyes go under.
        let under = frame(&settings, NOON_TICKS, Medium::Water(0.0), 128.0, north);
        assert_eq!((under.fog_start, under.fog_end), (0.0, 15.0));
        assert_eq!(frame(&settings, NOON_TICKS, Medium::Water(60.0), 128.0, north).fog_end, 60.0);
        assert_eq!(under.sky, under.fog, "nothing but the fog past it");
    }

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
