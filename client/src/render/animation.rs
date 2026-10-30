//! Textures that move: water, lava, fire, lanterns, kelp and the rest (m5-plan
//! Step 7d).
//!
//! **Each animated layer is rewritten as it changes** (worth teaching). Giving every
//! frame its own layer would ask less of the CPU, but there are over a thousand of
//! them and the array has room for 2,048 layers, so a pack or two could use it all.
//! So a block keeps its one layer, and this keeps the frames on the CPU, with their
//! mipmaps, and writes the right one into that layer when it's due. Java does the
//! same with its atlas. No vertex changes, and neither does the shader.
//!
//! **Blending:** most of Bedrock's flipbooks fade from frame to frame. A blended layer
//! is a mix of two frames, worked out here, and only as often as the mix changes
//! visibly: 16 steps a frame, so a slow one writes far less than once a tick.

/// How finely the mix between two frames is split.
const BLEND_STEPS: u32 = 16;

/// How long one frame shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Step {
    /// Which frame of the strip.
    pub frame: u16,
    /// For how many ticks: 20 to a second.
    pub ticks: u32,
}

/// How a strip plays, once it's been read from a flipbook or a `.mcmeta` file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Timeline {
    pub steps: Vec<Step>,
    /// Whether frames fade into the next.
    pub blend: bool,
}

impl Timeline {
    /// One step of every frame in order, for a strip that doesn't say otherwise.
    #[must_use]
    pub fn in_order(frames: usize, ticks: u32, blend: bool) -> Self {
        Self {
            steps: (0..frames)
                .map(|frame| Step {
                    frame: frame as u16,
                    ticks: ticks.max(1),
                })
                .collect(),
            blend,
        }
    }
}

/// One animated layer.
struct Animation {
    layer: u32,
    /// Each frame's mip levels, one after another.
    frames: Vec<Vec<u8>>,
    timeline: Timeline,
    /// All its steps' ticks.
    length: u64,
    /// What's in the layer now: which frames, and how far between them.
    shown: Option<(u16, u16, u32)>,
}

/// One mip level's place in a frame's bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Level {
    size: u32,
    offset: usize,
    len: usize,
}

/// Every animated layer.
pub struct Animator {
    animations: Vec<Animation>,
    levels: Vec<Level>,
    /// A blended frame, kept to save allocating one each tick.
    mixed: Vec<u8>,
}

impl Animator {
    /// With nothing to animate yet, for layers of `size` texels and `levels` mips.
    #[must_use]
    pub fn new(size: u32, levels: usize) -> Self {
        let mut offset = 0;
        let levels = (0..levels)
            .map(|level| {
                let size = (size >> level).max(1);
                let len = (size * size * 4) as usize;
                let at = Level { size, offset, len };
                offset += len;
                at
            })
            .collect();
        Self {
            animations: Vec::new(),
            levels,
            mixed: Vec::new(),
        }
    }

    /// Adds a layer that plays `timeline`, where each frame holds its mip levels one
    /// after another. A timeline with nothing to change does nothing, so it's left out.
    pub fn add(&mut self, layer: u16, frames: Vec<Vec<u8>>, timeline: Timeline) {
        let length: u64 = timeline.steps.iter().map(|step| u64::from(step.ticks)).sum();
        if timeline.steps.len() < 2 || length == 0 {
            return;
        }
        self.animations.push(Animation {
            layer: u32::from(layer),
            frames,
            timeline,
            length,
            shown: None,
        });
    }

    /// How many layers are animated.
    #[must_use]
    pub fn layers(&self) -> usize {
        self.animations.len()
    }

    /// All the frames held, and their bytes.
    #[must_use]
    pub fn frames(&self) -> (usize, usize) {
        let frames = self.animations.iter().map(|animation| animation.frames.len()).sum();
        let bytes = self.animations.iter().flat_map(|animation| &animation.frames).map(Vec::len).sum();
        (frames, bytes)
    }

    /// Moves everything to `tick`, writing what changed: a layer, a mip level, that
    /// level's size, and its texels.
    pub fn advance(&mut self, tick: u64, write: &mut impl FnMut(u32, u32, u32, &[u8])) {
        let Self {
            animations,
            levels,
            mixed,
        } = self;
        for animation in animations {
            let (from, to, step) = animation.at(tick);
            if animation.shown == Some((from, to, step)) {
                continue;
            }
            animation.shown = Some((from, to, step));
            let (Some(first), Some(second)) = (
                animation.frames.get(usize::from(from)),
                animation.frames.get(usize::from(to)),
            ) else {
                continue;
            };
            let bytes: &[u8] = if step == 0 {
                first
            } else {
                mix(first, second, step, mixed);
                mixed
            };
            for (index, level) in (0u32..).zip(levels.iter()) {
                if let Some(texels) = bytes.get(level.offset..level.offset + level.len) {
                    write(animation.layer, index, level.size, texels);
                }
            }
        }
    }
}

impl Animation {
    /// What shows at `tick`: a frame, the one it's fading into, and how far in
    /// sixteenths. A frame that isn't blending fades into itself.
    fn at(&self, tick: u64) -> (u16, u16, u32) {
        let mut left = tick % self.length;
        let steps = &self.timeline.steps;
        for (index, step) in steps.iter().enumerate() {
            let ticks = u64::from(step.ticks);
            if left < ticks {
                if !self.timeline.blend {
                    return (step.frame, step.frame, 0);
                }
                let next = steps[(index + 1) % steps.len()].frame;
                let mix = (left * u64::from(BLEND_STEPS) / ticks) as u32;
                return (step.frame, next, if next == step.frame { 0 } else { mix });
            }
            left -= ticks;
        }
        (steps[0].frame, steps[0].frame, 0)
    }
}

/// `step` sixteenths of the way from `from` to `to`, into `out`.
fn mix(from: &[u8], to: &[u8], step: u32, out: &mut Vec<u8>) {
    out.clear();
    out.extend(from.iter().zip(to).map(|(&from, &to)| {
        let (from, to) = (u32::from(from), u32::from(to));
        ((from * (BLEND_STEPS - step) + to * step + BLEND_STEPS / 2) / BLEND_STEPS) as u8
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes down what an advance wrote: a layer, a level, and its first byte.
    fn wrote(animator: &mut Animator, tick: u64) -> Vec<(u32, u32, u8)> {
        let mut out = Vec::new();
        animator.advance(tick, &mut |layer, level, _, texels| out.push((layer, level, texels[0])));
        out
    }

    /// Frames of a flat value, on a 2×2 layer with two mip levels: 20 bytes each.
    fn flat(values: &[u8]) -> Vec<Vec<u8>> {
        values.iter().map(|&value| vec![value; 20]).collect()
    }

    #[test]
    fn a_layer_is_written_when_its_frame_changes_and_not_otherwise() {
        let mut animator = Animator::new(2, 2);
        animator.add(5, flat(&[10, 20]), Timeline::in_order(2, 3, false));
        assert_eq!(wrote(&mut animator, 0), [(5, 0, 10), (5, 1, 10)], "the first time, every level");
        assert_eq!(wrote(&mut animator, 1), [], "the same frame");
        assert_eq!(wrote(&mut animator, 2), []);
        assert_eq!(wrote(&mut animator, 3), [(5, 0, 20), (5, 1, 20)]);
        assert_eq!(wrote(&mut animator, 6), [(5, 0, 10), (5, 1, 10)], "and it loops");
    }

    #[test]
    fn levels_get_their_own_bytes_and_sizes() {
        let mut animator = Animator::new(2, 2);
        // Level 0 is 16 bytes of 1, and level 1 is 4 bytes of 2.
        let frame = [vec![1; 16], vec![2; 4]].concat();
        animator.add(0, vec![frame.clone(), frame], Timeline::in_order(2, 1, false));
        let mut seen = Vec::new();
        animator.advance(0, &mut |_, level, size, texels| seen.push((level, size, texels.len(), texels[0])));
        assert_eq!(seen, [(0, 2, 16, 1), (1, 1, 4, 2)]);
    }

    #[test]
    fn blended_frames_fade_into_the_next_and_wrap_around() {
        let mut animator = Animator::new(2, 2);
        animator.add(0, flat(&[0, 160]), Timeline::in_order(2, 4, true));
        assert_eq!(wrote(&mut animator, 0)[0].2, 0);
        assert_eq!(wrote(&mut animator, 1)[0].2, 40, "a quarter of the way");
        assert_eq!(wrote(&mut animator, 2)[0].2, 80);
        assert_eq!(wrote(&mut animator, 4)[0].2, 160);
        assert_eq!(wrote(&mut animator, 6)[0].2, 80, "the last frame fades back into the first");
    }

    #[test]
    fn a_slow_blend_writes_only_when_the_mix_changes() {
        let mut animator = Animator::new(2, 2);
        animator.add(0, flat(&[0, 160]), Timeline::in_order(2, 160, true));
        assert_eq!(wrote(&mut animator, 0).len(), 2);
        let writes: usize = (1..10).map(|tick| wrote(&mut animator, tick).len()).sum();
        assert_eq!(writes, 0, "a sixteenth of a frame is 10 ticks");
        assert_eq!(wrote(&mut animator, 10).len(), 2);
    }

    #[test]
    fn a_listed_timeline_shows_its_frames_for_as_long_as_it_says() {
        let mut animator = Animator::new(2, 2);
        let timeline = Timeline {
            steps: vec![
                Step { frame: 1, ticks: 2 },
                Step { frame: 0, ticks: 1 },
                Step { frame: 1, ticks: 1 },
            ],
            blend: false,
        };
        animator.add(0, flat(&[10, 20]), timeline);
        let shown: Vec<u16> = (0..5)
            .map(|tick| animator.animations[0].at(tick).0)
            .collect();
        assert_eq!(shown, [1, 1, 0, 1, 1]);
    }

    #[test]
    fn nothing_to_change_is_left_out_and_a_missing_frame_is_skipped() {
        let mut animator = Animator::new(2, 2);
        animator.add(0, flat(&[10]), Timeline::in_order(1, 1, true));
        assert_eq!(animator.layers(), 0);
        let timeline = Timeline {
            steps: vec![Step { frame: 7, ticks: 1 }, Step { frame: 0, ticks: 1 }],
            blend: false,
        };
        animator.add(0, flat(&[10]), timeline);
        assert_eq!(wrote(&mut animator, 0), [], "there is no frame 7");
        assert_eq!(wrote(&mut animator, 1), [(0, 0, 10), (0, 1, 10)]);
    }
}
