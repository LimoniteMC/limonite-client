//! Screenshots to show people (m5-plan Step 8): the real game, with no window.
//!
//! The server, the client and the renderer run as they do in the window, and only
//! the last step differs: each picture goes to a PNG instead of the screen. So what's
//! in a picture is what the game does: health and hunger are set on the server, and
//! reach the HUD as packets, and bubbles only show on a player who is under water.
//!
//! ```text
//! LIMONITE_SHOTS=run/shots.txt LIMONITE_SHOTS_ARGS="--data-dir run/shots-world --radius 16" \
//!     CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
//!     cargo test --release -p limonite-client --bin limonite-client screenshots -- --ignored --nocapture
//! ```
//!
//! `LIMONITE_SHOTS_ARGS` is the client's own flags, and its paths mustn't hold spaces.
//! Paths are from the repo's root. `--debug-world` works too.
//!
//! The list has a picture to a line, and `#` starts a comment. Each is a name, a
//! gamemode, health, hunger, where to stand, and how many seconds to wait there:
//!
//! ```text
//! jungle  survival 13 7  56.79 73.94 90.71 141.3 33.3  0
//! door    creative 20 20 @oak_door                     0
//! ```
//!
//! Where to stand is what a `/tp` takes, which the `P` key logs: x, y, z, yaw and
//! pitch. In the debug world it can be `@` and what to look for instead, with `+` for
//! a space, and the picture gets the block's name on it. A hotbar slot can follow the
//! seconds, to select it just before the picture, so that the item's name shows.
//!
//! More kinds of line set the scene for the pictures after them, through the server:
//!
//! ```text
//! time 12500                      # the time of day, which then stands still
//! fill 10 64 10 14 66 14 torch    # a box of a block, corner to corner
//! summon cow 10.5 64 12.5         # an entity, its feet there
//! drop torch 3 11 64 12           # some of an item, dropped in that block
//! ```
//!
//! `LIMONITE_SHOTS_HOTBAR` fills the hotbar first, through the server: item names
//! separated by commas, each with `*` and a count if it isn't 1. `LIMONITE_SHOTS_XP`
//! gives a level and how far the bar is filled, also through the server: `7,0.4`.
//!
//! **The pictures go in `run/screenshots`, which is gitignored:** they show Mojang's
//! textures. The owner picks any that go in the README (CLAUDE.md, Rules).

use std::{
    path::PathBuf,
    str::FromStr,
    thread,
    time::{Duration, Instant},
};

use pumpkin_data::{Block, entity::EntityType, item::Item};
use pumpkin_util::{
    GameMode,
    math::{position::BlockPos, vector3::Vector3},
};
use pumpkin_world::world::BlockFlags;

use crate::{
    Args,
    blocks::{self, appearance::AppearanceTable},
    debug_world::{self, Browser},
    edit::aim,
    embedded::{EmbeddedServer, parse_hotbar},
    install, open_home,
    player::{Motion, Pose, EYE_HEIGHT},
    render::{
        self, Gpu, Needs,
        animation::Animator,
        camera::Camera,
        entities::{self, EntityRenderer, Looks},
        meshes::Meshes,
        sky::{self, Eyes, FogSettings},
        terrain::{self, Terrain},
        tint::Tints,
    },
    session::{Session, Stage},
    start_server,
    ui::{self, Ui, render::UiRenderer, sheet::read_back},
    world::BlockTable,
};

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
/// One tick, as in the window.
const TICK: Duration = Duration::from_millis(50);
/// How long meshing can take in a tick. There are no frames to keep up.
const MESHING: Duration = Duration::from_millis(40);
/// How long a place has to stop sending chunks to be taken as loaded.
const QUIET: Duration = Duration::from_secs(3);
/// How long to wait for that, at most.
const LOADING: Duration = Duration::from_secs(120);

/// One picture.
struct Shot {
    name: String,
    gamemode: GameMode,
    health: f32,
    hunger: u8,
    place: Place,
    seconds: f32,
    /// The hotbar slot to select just before the picture.
    slot: Option<u8>,
}

/// A line of the list.
enum Line {
    Shot(Shot),
    /// The time of day from now on, standing still.
    Time(i64),
    /// A box of a block, from one corner to the other.
    Fill([i32; 3], [i32; 3], &'static Block),
    /// An entity, with its feet there.
    Summon(&'static EntityType, [f64; 3]),
    /// Some of an item, dropped in a block.
    Drop(&'static Item, u8, [i32; 3]),
}

impl Line {
    fn parse(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        match *parts.first()? {
            "time" => Some(Self::Time(parts.get(1)?.parse().ok()?)),
            "fill" => {
                let number = |index: usize| parts.get(index).and_then(|part| part.parse::<i32>().ok());
                let corner = |at: usize| Some([number(at)?, number(at + 1)?, number(at + 2)?]);
                Some(Self::Fill(corner(1)?, corner(4)?, Block::from_name(parts.get(7)?)?))
            }
            "summon" => {
                let number = |index: usize| parts.get(index).and_then(|part| part.parse::<f64>().ok());
                let at = [number(2)?, number(3)?, number(4)?];
                Some(Self::Summon(EntityType::from_name(parts.get(1)?)?, at))
            }
            "drop" => {
                let number = |index: usize| parts.get(index).and_then(|part| part.parse::<i32>().ok());
                let count = parts.get(2)?.parse().ok()?;
                Some(Self::Drop(Item::from_registry_key(parts.get(1)?)?, count, [number(3)?, number(4)?, number(5)?]))
            }
            _ => Shot::parse(line).map(Self::Shot),
        }
    }
}

enum Place {
    /// As a `/tp`: x, y and z of the feet, yaw and pitch.
    At([f64; 3], f32, f32),
    /// What to look for in the debug world.
    Find(String),
}

impl Shot {
    fn parse(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let (name, gamemode, health, hunger) = (*parts.first()?, *parts.get(1)?, *parts.get(2)?, *parts.get(3)?);
        let (place, rest) = if let Some(text) = parts.get(4)?.strip_prefix('@') {
            (Place::Find(text.replace('+', " ")), 5)
        } else {
            let number = |index: usize| parts.get(index).and_then(|part| part.parse::<f64>().ok());
            let feet = [number(4)?, number(5)?, number(6)?];
            (Place::At(feet, number(7)? as f32, number(8)? as f32), 9)
        };
        Some(Self {
            name: name.to_string(),
            gamemode: GameMode::from_str(gamemode).ok()?,
            health: health.parse().ok()?,
            hunger: hunger.parse().ok()?,
            place,
            seconds: parts.get(rest).and_then(|part| part.parse().ok()).unwrap_or(0.0),
            slot: parts.get(rest + 1).and_then(|part| part.parse().ok()),
        })
    }
}

/// The game, with a texture for a window.
struct Studio {
    server: EmbeddedServer,
    session: Session,
    table: AppearanceTable,
    tints: Tints,
    animator: Animator,
    tick: u64,
    next_tick: Instant,
    meshes: Meshes,
    gpu: Gpu,
    terrain: Terrain,
    ui: Ui,
    ui_renderer: UiRenderer,
    target: wgpu::Texture,
    camera: Camera,
    pose: Pose,
    browser: Option<Browser>,
    fog: FogSettings,
    eyes: Eyes,
    /// How far the view reaches, in blocks.
    view: f32,
}

impl Studio {
    /// One tick, in real time: what the server sent, the player staying put, and some
    /// meshing.
    fn step(&mut self) {
        if let Some(wait) = self.next_tick.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
        self.next_tick = Instant::now() + TICK;
        self.session.drain();
        assert_ne!(self.session.stage(), Stage::Disconnected, "the server ended the connection");
        self.session.step(Motion::To(self.pose));
        self.tick += 1;
        let around = ((self.pose.feet.x.floor() as i32) >> 4, (self.pose.feet.z.floor() as i32) >> 4);
        self.session.world_mut().update_light(MESHING, around);
        let changes = self.session.world_mut().take_changes();
        self.meshes.apply(changes, self.session.world());
        self.meshes.build(
            &self.gpu.device,
            self.session.world(),
            &self.table,
            &self.tints,
            self.camera.position,
            MESHING,
        );
    }

    /// Moves the player, and waits until what's around it has loaded and been meshed.
    fn go(&mut self, feet: [f64; 3], yaw: f32, pitch: f32) {
        let feet_vector = glam::DVec3::from_array(feet);
        self.pose = Pose {
            feet: feet_vector,
            pitch,
            yaw,
        };
        self.camera.position = feet_vector + glam::DVec3::Y * EYE_HEIGHT;
        self.camera.pitch = pitch;
        self.camera.yaw = yaw;
        if let Some(player) = self.server.server.get_all_players().first() {
            player.request_teleport(Vector3::new(feet[0], feet[1], feet[2]), yaw, pitch);
        }
        let here = (feet[0].floor() as i32 >> 4, feet[2].floor() as i32 >> 4);
        let start = Instant::now();
        let mut chunks = self.session.level_chunks();
        let mut quiet_since = Instant::now();
        while start.elapsed() < LOADING {
            self.step();
            if self.session.level_chunks() != chunks {
                chunks = self.session.level_chunks();
                quiet_since = Instant::now();
            }
            // A long way off, nothing arrives for a while, which looks just like having
            // everything. So the chunk underfoot has to be here.
            let arrived = self.session.world().chunk(here.0, here.1).is_some();
            if arrived && quiet_since.elapsed() > QUIET && self.meshes.settled() {
                return;
            }
        }
        println!("Still loading after {} s; taking it anyway", LOADING.as_secs());
    }

    /// Fills a box with a block, through the server.
    fn fill(&self, from: [i32; 3], to: [i32; 3], block: &Block) {
        let _runtime = self.server.runtime.enter();
        let world = self.server.server.worlds.load().first().expect("a world").clone();
        let range = |axis: usize| from[axis].min(to[axis])..=from[axis].max(to[axis]);
        for x in range(0) {
            for y in range(1) {
                for z in range(2) {
                    world.set_block_state(&BlockPos::new(x, y, z), block.default_state.id, BlockFlags::NOTIFY_ALL);
                }
            }
        }
    }

    /// Fills the hotbar through the server.
    fn fill_hotbar(&mut self, list: &str) {
        self.server.fill_hotbar(&parse_hotbar(list).expect("a list of items"));
    }

    /// Gives the player a level and a bar through the server: `7,0.4`.
    ///
    /// The level first, and then the bar, which works out the points that go with it.
    /// The server keeps points and the bar both, and works the bar out from the points
    /// again whenever it adds any, even none, as an advancement's reward can.
    fn set_experience(&self, text: &str) {
        let (level, progress) = text.split_once(',').expect("a level and how far the bar is filled");
        let _runtime = self.server.runtime.enter();
        if let Some(player) = self.server.server.get_all_players().first() {
            player.set_experience_level(level.parse().expect("a level"), false);
            player.set_experience_progress(progress.parse().expect("a fraction"));
        }
    }

    /// Sets what the server knows of the player.
    fn set_player(&self, shot: &Shot) {
        let _runtime = self.server.runtime.enter();
        if let Some(player) = self.server.server.get_all_players().first() {
            player.set_gamemode(shot.gamemode);
            player.set_health(shot.health);
            player.set_food_level(shot.hunger);
            player.send_health();
        }
    }

    /// Goes where `shot` says, and takes its picture.
    fn shoot(&mut self, shot: &Shot, out: &std::path::Path) {
        self.set_player(shot);
        let mut note = None;
        match &shot.place {
            Place::At(feet, yaw, pitch) => self.go(*feet, *yaw, *pitch),
            Place::Find(text) => {
                let browser = self.browser.as_mut().expect("`@` is for the debug world");
                assert!(browser.find(text), "nothing in the debug world matches `{text}`");
                let (feet, yaw, pitch) = browser.pose();
                note = Some(browser.label());
                self.go(feet, yaw, pitch);
            }
        }
        // Moving can cost health, and a new gamemode takes a moment.
        self.set_player(shot);
        for _ in 0..(shot.seconds * 20.0) as u32 + 10 {
            self.step();
        }
        if let Some(slot) = shot.slot {
            self.session.select_slot(slot);
            // One tick for the HUD to see it, and a few for the server.
            for _ in 0..3 {
                self.step();
            }
        }
        self.ui.set_note(note.as_deref());
        self.ui.update(self.session.game());
        if let Some(drawing) = self.ui.drawing() {
            self.ui_renderer.upload(&self.gpu, drawing);
        }
        self.terrain.animate(&self.gpu, &mut self.animator, self.tick);
        let time = self.session.game().time().get().unwrap_or(sky::NOON_TICKS);
        let medium = self.eyes.medium(self.session.world(), &self.table, self.camera.position, Instant::now());
        let sky_frame = sky::frame(&self.fog, time, medium, self.view, self.camera.forward());
        self.terrain.set_daylight(&self.gpu, sky::daylight(time));
        let aimed = aim::from_camera(self.session.game(), &self.camera);
        let boxes = aimed.as_ref().map_or(&[][..], |hit| &hit.boxes);
        self.terrain.outline.show(&self.gpu, self.camera.position, boxes);
        let drawn = self.session.game().entities().drawn(Instant::now());
        let looks = Looks {
            icons: self.ui.icons(),
            atlas: self.ui.atlas(),
        };
        let world_ref = self.session.world();
        let vertices = entities::vertices(&drawn, self.camera.position, &|at| crate::app::light_byte(world_ref, at), Some(looks));
        if let Some(renderer) = &mut self.terrain.entities {
            renderer.upload(&self.gpu, &vertices);
        }

        let world = self.target.create_view(&wgpu::TextureViewDescriptor::default());
        let over = self.target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.gpu.ui_format),
            ..Default::default()
        });
        let mut encoder = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.terrain.draw(&self.gpu, &world, &mut encoder, Some(&self.camera), &self.meshes, &sky_frame);
        self.ui_renderer.draw(&over, &mut encoder);
        let picture = read_back(&self.gpu, &self.target, encoder);
        let file = out.join(format!("{}.png", shot.name));
        picture.save(&file).expect("the picture saves");

        let player = self.session.game().player().expect("the player");
        println!(
            "{}: {:?}, health {}, hunger {}, air {}, level {} and {}; {} meshes, {} in view; {} entities; aimed at {:?}",
            shot.name,
            player.gamemode,
            player.attributes.health,
            player.attributes.hunger,
            player.attributes.air,
            player.attributes.level,
            player.attributes.experience,
            self.meshes.len(),
            self.terrain.visible,
            drawn.len(),
            aimed.as_ref().map(|hit| hit.block),
        );
    }
}

#[test]
#[ignore = "needs a GPU and a Bedrock install; set LIMONITE_SHOTS and LIMONITE_SHOTS_ARGS"]
fn screenshots() {
    // So that paths are from the repo's root. The server moves it on when it starts, by
    // when the flags' paths have all been made absolute.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    std::env::set_current_dir(&root).unwrap();
    let list = std::fs::read_to_string(std::env::var("LIMONITE_SHOTS").expect("LIMONITE_SHOTS is set")).unwrap();
    let lines: Vec<Line> = list
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .filter(|line| !line.trim().is_empty())
        .map(|line| Line::parse(line).unwrap_or_else(|| panic!("can't read `{line}`")))
        .collect();
    let flags = std::env::var("LIMONITE_SHOTS_ARGS").unwrap_or_default();
    let args = Args::parse(flags.split_whitespace().map(str::to_string)).unwrap();
    let out = std::path::absolute(std::env::var("LIMONITE_SHOTS_OUT").unwrap_or_else(|_| "run/screenshots".to_string())).unwrap();
    std::fs::create_dir_all(&out).unwrap();

    let (settings, identity) = open_home(&args).unwrap();
    let install = install::find(args.bedrock.as_deref()).unwrap();
    let vanilla = install::load_vanilla(&install).unwrap();
    let palette = blocks::load_palette().unwrap();
    let mut jar = blocks::find_jar(args.java_jar.as_deref()).unwrap();
    let looks = blocks::build_appearance(&palette, &vanilla, jar.as_mut());
    let tints = render::load_tints(&vanilla.stack);
    let ui_assets = ui::load(&vanilla.stack, &looks.table, &tints);
    let (fog, fog_read) = FogSettings::load(&vanilla.stack);
    fog_read.unwrap();
    let server = start_server(&args, &settings, &identity).unwrap();
    let browser = args.debug_world.then(|| {
        let layout = debug_world::Layout::build(&BlockTable::build());
        debug_world::place(&server, &layout);
        Browser::new(layout, None)
    });

    let needs = Needs {
        texture_layers: looks.textures.layers,
        uncapped: false,
    };
    let gpu = Gpu::offscreen(WIDTH, HEIGHT, &needs).expect("a GPU");
    let mut terrain = Terrain::new(&gpu, &looks.textures);
    let ui_renderer = UiRenderer::new(&gpu, &ui_assets.picture, &terrain.texture_view);
    terrain.entities = Some(EntityRenderer::new(
        &gpu,
        terrain::DEPTH_FORMAT,
        terrain.frame(),
        &terrain.texture_view,
        &terrain.light_map_view(),
        ui_renderer.atlas_view(),
    ));
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("screenshot"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: gpu.config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[gpu.ui_format],
    });
    let mut ui = Ui::new(ui_assets.atlas, ui_assets.font, ui_assets.lang, ui_assets.icons, settings.ui_scale);
    ui.resize(WIDTH, HEIGHT);

    let mut session = Session::connect(&server.connector, &settings.name, identity.xid(), settings.radius).unwrap();
    session.world_mut().track_changes();
    let start = Instant::now();
    while session.stage() != Stage::Spawned {
        assert!(start.elapsed() < LOADING, "the host didn't spawn");
        thread::sleep(TICK);
        session.drain();
        session.step(Motion::Still);
    }
    let pose = session.game().player().expect("the player").pose();
    let mut camera = Camera::new(pose.eyes(), pose.pitch, pose.yaw);
    camera.aspect = WIDTH as f32 / HEIGHT as f32;
    camera.fov_y = settings.fov as f32;

    let mut studio = Studio {
        server,
        session,
        table: looks.table,
        tints,
        animator: looks.animator,
        tick: 0,
        next_tick: Instant::now(),
        meshes: Meshes::default(),
        gpu,
        terrain,
        ui,
        ui_renderer,
        target,
        camera,
        pose,
        browser,
        fog,
        eyes: Eyes::default(),
        view: settings.radius as f32 * 16.0,
    };
    if let Ok(experience) = std::env::var("LIMONITE_SHOTS_XP") {
        studio.set_experience(&experience);
    }
    if let Ok(list) = std::env::var("LIMONITE_SHOTS_HOTBAR") {
        studio.fill_hotbar(&list);
    }
    let mut pictures = 0;
    for line in &lines {
        match line {
            Line::Shot(shot) => {
                studio.shoot(shot, &out);
                pictures += 1;
            }
            Line::Time(time) => studio.server.set_time(*time, true),
            Line::Fill(from, to, block) => studio.fill(*from, *to, block),
            Line::Summon(entity_type, at) => studio.server.summon(entity_type, *at),
            Line::Drop(item, count, at) => studio.server.drop_item(item, *count, *at),
        }
    }
    println!("Wrote {pictures} pictures to {}", out.display());
    let Studio { server, session, .. } = studio;
    drop(session);
    server.stop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_line_says_what_to_take() {
        let shot = Shot::parse("jungle survival 13 7 56.79 73.94 90.71 141.3 33.3 8").unwrap();
        assert_eq!(shot.name, "jungle");
        assert_eq!(shot.gamemode, GameMode::Survival);
        assert_eq!((shot.health, shot.hunger, shot.seconds), (13.0, 7, 8.0));
        assert!(matches!(shot.place, Place::At(feet, yaw, pitch) if feet == [56.79, 73.94, 90.71] && (yaw, pitch) == (141.3, 33.3)));

        let shot = Shot::parse("door creative 20 20 @oak_door+upper").unwrap();
        assert!(matches!(&shot.place, Place::Find(text) if text == "oak_door upper"));
        assert_eq!(shot.seconds, 0.0);

        assert!(matches!(Line::parse("time 12500"), Some(Line::Time(12_500))));
        assert!(matches!(
            Line::parse("fill 1 2 3 4 5 6 torch"),
            Some(Line::Fill([1, 2, 3], [4, 5, 6], block)) if block.name == "torch"
        ));
        assert!(Line::parse("fill 1 2 3 torch").is_none());
        assert!(Shot::parse("too few").is_none());
        assert!(Shot::parse("wrong flying 20 20 @door").is_none());
        assert!(Shot::parse("short survival 20 20 1 2 3").is_none());
    }
}
