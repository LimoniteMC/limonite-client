//! The window's main loop (m4-plan Decision 2).
//!
//! winit owns the main thread and calls [`App`] back: for each window and
//! device event, and in `about_to_wait` once it has handled everything that
//! arrived. The embedded server stays on its own thread, and the
//! `LocalChannel` is still the only thing between them.
//!
//! **One window, world after world** (m5-plan Deviation 63). The window, the GPU and all
//! that was read from the install last as long as the process. A world's server, the host's
//! session with it and the world's meshes last as long as it's played: they're a [`Play`],
//! made when a world opens and dropped when it's left, which saves it. Between worlds the
//! window shows the world list. A world of its own (`--data-dir`) has no list to go back
//! to, so leaving it closes the window.
//!
//! **Ticks don't depend on frames** (worth teaching). The host sends one
//! `PlayerAuthInput` every 50 ms, as the official client does, and the
//! server drops a client it hasn't heard from in 10 s. Frames come at the
//! display's rate while the window is visible, and not at all while it's
//! minimised. So `about_to_wait` runs the ticks that are due, whatever
//! happened to frames, and then asks winit to wake it at the next tick at
//! the latest (`ControlFlow::WaitUntil`).
//!
//! Each tick moves the player once; each frame draws it between its last
//! two ticks' poses, and turns the camera by the mouse (m5-plan Decision 3,
//! `clock.rs`).
//!
//! **While the inventory is open** (m5-plan Step 12), the cursor is free and the mouse
//! is the screen's: the player stands still, nothing is broken or placed, and the keys
//! only close it again. Ticks and their input go on as ever.
//!
//! **While a menu is up,** the keys and the mouse are the UI's (`ui/mod.rs`): Enter and
//! Escape, the arrows and typing go to its screen, and a click to its buttons. Over a world,
//! the pause menu comes up on Escape or when the window loses the focus, and the death screen
//! while the host is dead; the player stands still under either, as with the inventory.
//!
//! **Save & Quit** shows "Saving..." for a frame, then leaves the world: the host leaves and
//! the server stops, which saves it, and the window goes back to the world list.

use std::{
    collections::HashMap,
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
    time::{Duration, Instant},
};

use glam::{IVec3, Vec3};
use pumpkin_data::item::Item;
use pumpkin_util::math::vector3::Vector3;
use tracing::{debug, error, info, warn};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{DeviceEvent, DeviceId, ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key as Logical, KeyCode, NamedKey, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

use crate::{
    bench::{Bench, Sample},
    blocks::{self, appearance::AppearanceTable},
    clock::Ticks,
    debug_world::Browser,
    edit::aim::{self, Hit},
    embedded::{self, EmbeddedServer},
    input::{Action, Button, Input, PIXELS_PER_NOTCH},
    inventory::requests::Button as ClickButton,
    player::{EYE_HEIGHT, Motion, Pose, movement::Keys},
    render::{
        Frame, Gpu, Needs,
        animation::Animator,
        camera::{Camera, FOV_Y},
        crack::Cracked,
        entities::{self, EntityRenderer, Looks},
        meshes::{self, Meshes},
        sky::{self, Eyes, FogSettings, Medium},
        terrain::{self, Terrain},
        textures::TextureArray,
        tint::Tints,
    },
    session::{Session, Stage},
    settings::Settings,
    ui::{
        self, Clicked, Menu, Ui,
        render::UiRenderer,
        screens::{
            inventory::Pressed,
            overlay::OverlayChoice,
            worlds::{Choice, EditKey, Key, Message},
        },
    },
    world::{
        World,
        light::{self, Kind},
    },
    worlds::{self, WorldEntry, Worlds},
};

/// What the window draws with, prepared before it opens.
pub struct Assets {
    pub table: AppearanceTable,
    /// Uploaded when the GPU comes up, then freed.
    pub textures: TextureArray,
    /// The layers that move.
    pub animator: Animator,
    pub tints: Tints,
    /// The ids drawn as approximate shapes, for the census once the join settles.
    pub approximate: HashMap<u32, String>,
    pub ui: ui::Assets,
    /// Bedrock's fogs, from the install.
    pub fog: FogSettings,
}

/// The longest step the bench's camera takes in one frame, so a hitch doesn't
/// fling it far away.
const MAX_FRAME_SECONDS: f64 = 0.25;
/// How often a menu is drawn while nothing happens: often enough for a caret to blink.
const MENU_FRAME: Duration = Duration::from_millis(50);

/// What a world of its own does besides playing, from the command line.
#[derive(Default)]
pub struct Extras {
    /// `--bench`: fly for this long, log the frame times, and leave.
    pub bench: Option<Duration>,
    /// `--debug-world`: where you are in it.
    pub browser: Option<Browser>,
    /// `--hotbar`: what the host gets on joining.
    pub hotbar: Vec<(&'static Item, u8)>,
}

/// What the window opens on.
pub enum Start {
    /// The world list, saying how the last child crashed, if one did (`supervisor.rs`).
    List { worlds: Worlds, crashed: Option<String> },
    /// A world of its own (`--data-dir`), already started. Leaving it closes the window.
    World { server: EmbeddedServer, extras: Extras },
}

/// Opens the window on `start`, as the host with this install's `xid`, and runs until the
/// window closes, a world of its own is left, or a bench is done. A world still open then is
/// left, which saves it.
///
/// `ready` is the supervisor's file, created once a frame has been drawn (`supervisor.rs`).
pub fn run(start: Start, settings: Settings, xid: &str, needs: Needs, assets: Assets, ready: Option<PathBuf>) -> ExitCode {
    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(err) => {
            error!("Couldn't start the window system: {err}");
            if let Start::World { server, .. } = start {
                server.stop();
            }
            return ExitCode::FAILURE;
        }
    };
    let mut app = App::new(settings, xid, needs, assets, ready);
    match start {
        Start::List { worlds, crashed } => {
            let message = crashed.map(|how| Message {
                text: app.ui.lang().format("limonite.crashed", &[&how]),
                error: true,
            });
            app.worlds = Some(worlds);
            app.show_list(message.as_ref());
        }
        Start::World { server, extras } => {
            if let Err(err) = app.join(server, None, extras) {
                error!("{err}");
                return ExitCode::FAILURE;
            }
        }
    }
    if let Err(err) = event_loop.run_app(&mut app) {
        error!("The window's event loop failed: {err}");
        app.failure.get_or_insert_with(|| "the event loop failed".to_string());
    }
    // The window has closed. A world still open is left now, before the process ends.
    app.leave();
    match app.failure {
        Some(err) => {
            error!("{err}");
            ExitCode::FAILURE
        }
        None => ExitCode::SUCCESS,
    }
}

/// A world being played, and all that lasts only as long as it does.
struct Play {
    server: EmbeddedServer,
    session: Session,
    /// Its place in the world list, or `None` for a world of its own.
    entry: Option<WorldEntry>,
    meshes: Meshes,
    /// Placed where `StartGame` put the host, once it has spawned. It draws
    /// the player's eyes, except during the bench, which flies it.
    camera: Option<Camera>,
    /// What the camera was aimed at, the last frame.
    aimed: Option<Hit>,
    /// What the camera's eyes are in.
    eyes: Eyes,
    ticks: Ticks,
    /// Whether the inventory was open at the last look, to free the cursor when it opens
    /// and capture it again when it shuts.
    inventory_open: bool,
    /// Whether the player paused, and the menu over the world at the last look.
    paused: bool,
    menu: Menu,
    /// `--bench`: it flies the camera instead of the keys and the mouse.
    bench: Option<Bench>,
    /// `--debug-world`: where you are in it.
    browser: Option<Browser>,
    /// `--hotbar`: what the host gets once it has spawned.
    hotbar: Vec<(&'static Item, u8)>,
}

/// Work that waits until a frame has shown it's under way, since it holds the window still
/// while it runs.
enum Pending {
    /// Opening a world from the list.
    Open(WorldEntry),
    /// Save & Quit: leaving the world, which saves it.
    Leave,
}

pub struct App {
    settings: Settings,
    xid: String,
    needs: Needs,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    table: AppearanceTable,
    /// The CPU copy of the texture array, until it's uploaded.
    textures: Option<TextureArray>,
    animator: Animator,
    /// Ticks so far, and the tick the textures show.
    animation_tick: u64,
    animated_tick: Option<u64>,
    tints: Tints,
    /// Until the census is logged, once, after the first world's join settles.
    approximate: Option<HashMap<u32, String>>,
    terrain: Option<Terrain>,
    fog: FogSettings,
    ui: Ui,
    /// The CPU copy of the UI's atlas, until it's uploaded.
    ui_picture: Option<image::RgbaImage>,
    ui_renderer: Option<UiRenderer>,
    input: Input,
    cursor_grabbed: bool,
    /// Whether shift is held, for a shift-click.
    shift: bool,
    /// Whether frames can be seen: not minimised or covered.
    visible: bool,
    last_frame: Instant,
    /// Counting frames for the title, which updates once a second.
    second_started: Instant,
    frames_this_second: u32,
    fps: f64,
    /// The world list's worlds, when the run has a list.
    worlds: Option<Worlds>,
    /// The world being played, if one is.
    play: Option<Play>,
    /// Work to do once a frame has shown it's under way, and whether that frame was drawn.
    pending: Option<Pending>,
    pending_shown: bool,
    /// The supervisor's file, until a frame is drawn (`supervisor.rs`).
    ready: Option<PathBuf>,
    /// Set when the run is over: the loop ends at its next pass.
    quit: bool,
    /// Why the loop ended early, if it did.
    failure: Option<String>,
}

impl App {
    fn new(settings: Settings, xid: &str, needs: Needs, assets: Assets, ready: Option<PathBuf>) -> Self {
        let now = Instant::now();
        let input = Input::new(settings.keymap.clone());
        let mut ui = Ui::new(assets.ui.atlas, assets.ui.font, assets.ui.lang, assets.ui.icons, settings.ui_scale);
        ui.set_default_gamemode(settings.gamemode);
        Self {
            settings,
            xid: xid.to_string(),
            needs,
            window: None,
            gpu: None,
            table: assets.table,
            textures: Some(assets.textures),
            animator: assets.animator,
            animation_tick: 0,
            animated_tick: None,
            tints: assets.tints,
            approximate: Some(assets.approximate),
            terrain: None,
            fog: assets.fog,
            ui,
            ui_picture: Some(assets.ui.picture),
            ui_renderer: None,
            input,
            cursor_grabbed: false,
            shift: false,
            visible: true,
            last_frame: now,
            second_started: now,
            frames_this_second: 0,
            fps: 0.0,
            worlds: None,
            play: None,
            pending: None,
            pending_shown: false,
            ready,
            quit: false,
            failure: None,
        }
    }

    /// Joins `server`'s world as the host, and shows it. A world of its own has no `entry`.
    ///
    /// # Errors
    /// If the session can't connect, after which the server has been stopped.
    fn join(&mut self, server: EmbeddedServer, entry: Option<WorldEntry>, extras: Extras) -> Result<(), String> {
        let radius = self.settings.radius;
        let mut session = match Session::connect(&server.connector, &self.settings.name, &self.xid, radius) {
            Ok(session) => session,
            Err(err) => {
                server.stop();
                return Err(format!("Couldn't connect to the embedded server: {err}"));
            }
        };
        // Before any chunk arrives, so the meshes hear about every one.
        session.world_mut().track_changes();
        self.play = Some(Play {
            server,
            session,
            entry,
            meshes: Meshes::new(!self.settings.smooth_lighting),
            camera: None,
            aimed: None,
            eyes: Eyes::default(),
            ticks: Ticks::new(Instant::now()),
            inventory_open: false,
            paused: false,
            menu: Menu::None,
            bench: extras.bench.map(|duration| Bench::new(duration, radius)),
            browser: extras.browser,
            hotbar: extras.hotbar,
        });
        // Until the host spawns, there's nothing to see but this.
        self.ui.show_busy("menu.generatingTerrain");
        Ok(())
    }

    /// Leaves the world being played, if one is: the host leaves, then its server stops,
    /// which saves the world. Says which world it was, if it came from the list.
    fn leave(&mut self) -> Option<WorldEntry> {
        let Play {
            server,
            session,
            entry,
            ..
        } = self.play.take()?;
        session.log_edits();
        session.log_light();
        session.log_entities();
        // The host leaves before the server stops, as a player would.
        drop(session);
        server.stop();
        info!("Left the world, and its server saved it");
        self.release_cursor();
        // Nothing of that world is drawn again.
        if let (Some(gpu), Some(terrain)) = (&self.gpu, &mut self.terrain) {
            terrain.outline.show(gpu, glam::DVec3::ZERO, &[]);
            terrain.crack.show(gpu, &self.table, &self.tints, None);
            if let Some(renderer) = &mut terrain.entities {
                renderer.upload(gpu, &[]);
            }
        }
        entry
    }

    /// Shows the world list, as the files have it now, with `message` over it.
    fn show_list(&mut self, message: Option<&Message>) {
        let list = self.worlds.as_ref().map(Worlds::list).unwrap_or_default();
        self.ui.show_worlds(&list, message);
        self.release_cursor();
        self.input.clear();
    }

    /// A message for the list, from the strings, with `detail` put in.
    fn message(&self, key: &str, detail: &str, error: bool) -> Message {
        Message {
            text: self.ui.lang().format(key, &[detail]),
            error,
        }
    }

    /// The world in the list with this id, as the files have it now.
    fn entry(&self, id: &str) -> Option<WorldEntry> {
        self.worlds.as_ref()?.list().into_iter().find(|entry| entry.id == id)
    }

    /// Does what a menu asked for.
    fn clicked(&mut self, clicked: Clicked) {
        match clicked {
            Clicked::Worlds(choice) => self.choose(choice),
            Clicked::Overlay(choice) => self.overlay_choice(choice),
        }
    }

    /// Does what the pause menu or the death screen asked for.
    fn overlay_choice(&mut self, choice: OverlayChoice) {
        let Some(play) = &mut self.play else {
            return;
        };
        match choice {
            OverlayChoice::Resume => play.paused = false,
            OverlayChoice::Respawn => play.session.respawn(),
            OverlayChoice::Quit => {
                self.ui.show_busy("menu.saving");
                self.pending = Some(Pending::Leave);
                self.pending_shown = false;
            }
        }
    }

    /// Does what the world list asked for, and shows the list again with what came of it,
    /// unless a world is opening.
    fn choose(&mut self, choice: Choice) {
        let Some(worlds) = &self.worlds else {
            return;
        };
        let now = worlds::now();
        let message = match choice {
            Choice::Play(id) => match self.entry(&id) {
                Some(entry) => return self.start_opening(entry),
                None => None,
            },
            Choice::Create { name, gamemode, seed } => match worlds.create(&name, gamemode, &seed, now) {
                Ok(entry) => {
                    info!("Created the world {:?} in {}", entry.meta.name, entry.dir.display());
                    return self.start_opening(entry);
                }
                Err(err) => Some(self.message("limonite.fileError", &err.to_string(), true)),
            },
            Choice::Copy(id) => self.entry(&id).map(|entry| match worlds.back_up(&entry, now) {
                Ok(copy) => {
                    info!("Copied the world {:?} to {}", entry.meta.name, copy.display());
                    self.message("limonite.copied", &copy.display().to_string(), false)
                }
                Err(err) => self.message("limonite.fileError", &err.to_string(), true),
            }),
            Choice::Delete(id) => self.entry(&id).map(|entry| match worlds.delete(&entry, now) {
                Ok(moved) => {
                    info!("Moved the world {:?} to {}", entry.meta.name, moved.display());
                    self.message("limonite.deleted", &moved.display().to_string(), false)
                }
                Err(err) => self.message("limonite.fileError", &err.to_string(), true),
            }),
        };
        self.show_list(message.as_ref());
    }

    /// Says a world is loading, and opens it once a frame has said so.
    fn start_opening(&mut self, entry: WorldEntry) {
        self.ui.show_busy("menu.loadingLevel");
        self.pending = Some(Pending::Open(entry));
        self.pending_shown = false;
    }

    /// Opens a world from the list: notes that it's being played, starts its server, and
    /// joins it. If that fails, the list says why.
    fn open(&mut self, mut entry: WorldEntry) {
        if let Some(worlds) = &self.worlds
            && let Err(err) = worlds.played(&mut entry, worlds::now())
        {
            warn!("Couldn't note when {} was played: {err}", entry.dir.display());
        }
        info!("Opening the world {:?}", entry.meta.name);
        let config = entry.config(&self.settings);
        let opened = embedded::open_world(&entry.dir, config, &self.settings.name, &self.xid)
            .and_then(|server| self.join(server, Some(entry), Extras::default()));
        if let Err(err) = opened {
            error!("{err}");
            let message = self.message("limonite.worldFailed", &err, true);
            self.show_list(Some(&message));
        }
    }

    /// Runs the pending work, once a frame has shown it's under way, or at once if no frame
    /// can be seen.
    fn run_pending(&mut self) {
        if !self.pending_shown && self.visible {
            return;
        }
        match self.pending.take() {
            Some(Pending::Open(entry)) => self.open(entry),
            Some(Pending::Leave) => {
                // A world from the list goes back to it; a world of its own ends the run.
                if self.leave().is_some() {
                    self.show_list(None);
                } else {
                    self.quit = true;
                }
            }
            None => {}
        }
    }

    /// The world was lost: its server ended the connection. A world from the list goes back
    /// to the list, saying so; a world of its own ends the run.
    fn lost_world(&mut self, reason: &str) {
        let from_list = self.play.as_ref().is_some_and(|play| play.entry.is_some());
        self.leave();
        if from_list {
            let message = self.message("limonite.worldClosed", reason, true);
            self.show_list(Some(&message));
        } else {
            self.failure = Some(format!("The embedded server ended the connection: {reason}"));
            self.quit = true;
        }
    }

    /// Runs the ticks that are due: each moves the player once and sends one
    /// `PlayerAuthInput`.
    fn run_ticks(&mut self) {
        let Some(play) = &mut self.play else {
            return;
        };
        for _ in 0..play.ticks.due(Instant::now()) {
            let motion = motion(play, &self.input);
            if play.bench.is_none() {
                let hit = play.camera.as_ref().and_then(|camera| aim::from_camera(play.session.game(), camera));
                let playing = !play.inventory_open && play.menu == Menu::None;
                let attack = playing && self.input.is_held(Action::Attack);
                let use_item = playing && self.input.is_held(Action::Use);
                play.session.edit_tick(attack, use_item, hit.as_ref());
            }
            play.session.step(motion);
            self.animation_tick += 1;
        }
    }

    /// When the server turns the player (a teleport), the camera turns with
    /// it, before the next tick reads the camera's angles.
    fn follow_turns(&mut self) {
        let Some(play) = &mut self.play else {
            return;
        };
        let turned = play.session.game_mut().player_mut().and_then(|player| player.take_turned());
        if let (Some((pitch, yaw)), Some(camera)) = (turned, &mut play.camera) {
            camera.pitch = pitch;
            camera.yaw = yaw;
        }
    }

    /// Places the camera where the host spawned, once, and captures the cursor.
    fn place_camera(&mut self) {
        let Some(play) = &mut self.play else {
            return;
        };
        if play.camera.is_some() || play.session.stage() != Stage::Spawned {
            return;
        }
        let Some(player) = play.session.game().player() else {
            return;
        };
        let pose = player.pose();
        let mut camera = Camera::new(pose.eyes(), pose.pitch, pose.yaw);
        if let Some(window) = &self.window {
            camera.aspect = aspect(window.inner_size());
        }
        // The bench keeps M4's view, so its numbers compare with the
        // baseline whatever the settings say.
        camera.fov_y = if play.bench.is_some() { FOV_Y } else { self.settings.fov as f32 };
        play.camera = Some(camera);
        self.ui.show_game();
        info!("The camera is at the host's spawn");
        play.session.check_gamemode(self.settings.gamemode);
        if !play.hotbar.is_empty() {
            play.server.fill_hotbar(&play.hotbar);
        }
        let bench = play.bench.is_some();
        if play.browser.is_some() {
            info!(
                "Debug world: ] and [ go to the next and last block, . and , to the next and last state, \
                 and typing in this terminal goes to what matches"
            );
            self.jump_to_browser();
        }
        if !bench && self.window.as_ref().is_some_and(|window| window.has_focus()) {
            self.grab_cursor();
        }
    }

    /// Moves the host to what the debug world's browser is at, hovering to look at
    /// it, and says what that is.
    fn jump_to_browser(&mut self) {
        let Some(play) = &self.play else {
            return;
        };
        let Some(browser) = &play.browser else {
            return;
        };
        let (feet, yaw, pitch) = browser.pose();
        let label = browser.label();
        let players = play.server.server.get_all_players();
        let Some(player) = players.first() else {
            return;
        };
        player.set_flying(true);
        player.request_teleport(Vector3::new(feet[0], feet[1], feet[2]), yaw, pitch);
        info!("Debug world: {label}");
        self.ui.set_note(Some(&label));
        self.set_title();
    }

    /// A key the debug world answers to. The keymap has no say: these only exist here.
    fn debug_key(&mut self, key: KeyCode) {
        let Some(browser) = self.play.as_mut().and_then(|play| play.browser.as_mut()) else {
            return;
        };
        match key {
            KeyCode::BracketRight => browser.step_block(true),
            KeyCode::BracketLeft => browser.step_block(false),
            KeyCode::Period => browser.step_state(true),
            KeyCode::Comma => browser.step_state(false),
            _ => return,
        }
        self.jump_to_browser();
    }

    /// Goes to what was typed in the terminal, if anything was.
    fn debug_typed(&mut self) {
        let Some(browser) = self.play.as_mut().and_then(|play| play.browser.as_mut()) else {
            return;
        };
        let Some(line) = browser.take_typed() else {
            return;
        };
        if browser.find(&line) {
            self.jump_to_browser();
        } else {
            info!("Debug world: nothing matches `{}`", line.trim());
        }
    }

    fn grab_cursor(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        // Locked keeps the cursor still; where it's unavailable (Windows),
        // Confined keeps it in the window. Found out by trying, not by
        // checking the OS.
        let grabbed = window
            .set_cursor_grab(CursorGrabMode::Locked)
            .map(|()| CursorGrabMode::Locked)
            .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined).map(|()| CursorGrabMode::Confined));
        match grabbed {
            Ok(mode) => {
                window.set_cursor_visible(false);
                self.cursor_grabbed = true;
                debug!("Cursor captured ({mode:?})");
            }
            Err(err) => warn!("Couldn't capture the cursor: {err}"),
        }
    }

    fn release_cursor(&mut self) {
        if let Some(window) = &self.window {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            window.set_cursor_visible(true);
        }
        self.cursor_grabbed = false;
    }

    /// Shows the menu the world needs: the death screen while the host is dead, and the pause
    /// menu while the player has paused. The cursor is freed while one is up, and captured
    /// again after, as long as the window has the focus.
    fn follow_menu(&mut self) {
        let Some(play) = &mut self.play else {
            return;
        };
        let menu = if play.session.game().is_dead() {
            Menu::Death
        } else if play.paused {
            Menu::Pause
        } else {
            Menu::None
        };
        if menu == play.menu {
            return;
        }
        play.menu = menu;
        // Dying ends a pause: after a respawn the game goes on.
        if menu != Menu::Pause {
            play.paused = false;
        }
        self.ui.set_menu(menu);
        if menu != Menu::None {
            self.release_cursor();
            // The menu has the keys now, so what was held when it came up is let go: a key
            // that comes up under the menu would otherwise stay held in the game.
            self.input.clear();
        } else if !play.inventory_open && self.window.as_ref().is_some_and(|window| window.has_focus()) {
            self.grab_cursor();
        }
    }

    /// Pauses the world being played, unless a bench is flying it.
    fn pause(&mut self) {
        if let Some(play) = &mut self.play
            && play.bench.is_none()
            && play.camera.is_some()
        {
            play.paused = true;
        }
    }

    /// Frees the cursor when the server opens the inventory, and captures it again when
    /// the screen shuts, as long as the window has the focus.
    fn follow_inventory(&mut self) {
        let Some(play) = &mut self.play else {
            return;
        };
        let open = play.session.game().inventory().is_open();
        if open == play.inventory_open {
            return;
        }
        play.inventory_open = open;
        if open {
            self.release_cursor();
        } else if self.window.as_ref().is_some_and(|window| window.has_focus()) {
            self.grab_cursor();
        }
    }

    /// A mouse button while the inventory is open: a press on what's under the pointer,
    /// which acts at once, and a release, which ends a drag.
    fn inventory_click(&mut self, state: ElementState, button: MouseButton) {
        let Some(play) = &mut self.play else {
            return;
        };
        if state == ElementState::Released {
            play.session.pointer_released();
            return;
        }
        let button = match button {
            MouseButton::Left => ClickButton::Left,
            MouseButton::Right => ClickButton::Right,
            _ => return,
        };
        match self.ui.inventory_pressed() {
            Pressed::Slot(slot) => play.session.click_slot(slot, button, self.shift),
            Pressed::Creative(entry) => play.session.click_creative(entry),
            Pressed::Tab(tab) => play.session.creative_tab(tab),
            Pressed::Nothing => {}
        }
    }

    /// A key while a menu has the keys: one the menu answers to, or typing.
    fn menu_key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        match menu_key(&event.logical_key) {
            // Holding an editing key repeats it; holding Enter doesn't.
            Some(key) if !event.repeat || matches!(key, Key::Edit(_)) => {
                if let Some(clicked) = self.ui.key(key) {
                    self.clicked(clicked);
                }
            }
            Some(_) => {}
            None => {
                if let Some(text) = &event.text {
                    self.ui.typed(text);
                }
            }
        }
    }

    /// The keys pressed since the last frame, while playing.
    fn play_keys(&mut self) -> (bool, bool) {
        let (mut attack_pressed, mut use_pressed) = (false, false);
        for action in self.input.take_pressed() {
            let Some(play) = &mut self.play else {
                break;
            };
            // With the inventory open, the keys only shut it.
            if play.inventory_open && !matches!(action, Action::Inventory | Action::Pause | Action::LogPose) {
                continue;
            }
            match action {
                Action::Attack => attack_pressed = true,
                Action::Use => use_pressed = true,
                Action::Inventory => {
                    if play.session.game().inventory().is_open() {
                        play.session.close_inventory();
                    } else {
                        play.session.open_inventory();
                    }
                }
                Action::Pause if play.inventory_open => play.session.close_inventory(),
                Action::Pause => play.paused = true,
                Action::LogPose => log_pose(play.camera.as_ref()),
                Action::Hotbar(slot) => play.session.select_slot(slot),
                Action::HotbarNext => {
                    let slot = play.session.game().inventory().next_slot();
                    play.session.select_slot(slot);
                }
                Action::HotbarPrevious => {
                    let slot = play.session.game().inventory().previous_slot();
                    play.session.select_slot(slot);
                }
                Action::Drop => play.session.drop_held(),
                Action::Pick => {
                    if let Some(hit) = &play.aimed {
                        play.session.pick_block(hit.block);
                    }
                }
                _ => {}
            }
        }
        (attack_pressed, use_pressed)
    }

    fn redraw(&mut self) {
        self.debug_typed();
        let now = Instant::now();
        let frame_time = now - self.last_frame;
        let seconds = frame_time.as_secs_f64().min(MAX_FRAME_SECONDS);
        self.last_frame = now;
        self.ui.update_menus(now);
        let drawn = if self.play.is_some() {
            self.redraw_world(now, frame_time, seconds)
        } else {
            self.redraw_menu()
        };
        if drawn == Frame::Drawn {
            self.frames_this_second += 1;
            self.pending_shown = self.pending.is_some();
            if let Some(ready) = self.ready.take()
                && let Err(err) = std::fs::write(&ready, b"")
            {
                warn!("Couldn't tell the supervisor the window is up ({}): {err}", ready.display());
            }
        }
        self.once_a_second(now);
    }

    /// A frame of the world list: the sky's clear colour, and the UI over it.
    fn redraw_menu(&mut self) -> Frame {
        let sky_frame = sky::frame(&self.fog, sky::NOON_TICKS, Medium::Air, self.view_depth(), Vec3::Z);
        let (Some(gpu), Some(terrain), Some(ui_renderer)) = (&mut self.gpu, &mut self.terrain, &mut self.ui_renderer)
        else {
            return Frame::Skipped;
        };
        if let Some(drawing) = self.ui.drawing() {
            ui_renderer.upload(gpu, drawing);
        }
        let ui_renderer = &*ui_renderer;
        let nothing = Meshes::new(false);
        gpu.frame(|gpu, views, encoder| {
            terrain.draw(gpu, &views.world, encoder, None, &nothing, &sky_frame);
            ui_renderer.draw(&views.ui, encoder);
        })
    }

    /// A frame of the world being played.
    fn redraw_world(&mut self, now: Instant, frame_time: Duration, seconds: f64) -> Frame {
        let (attack_pressed, use_pressed) = self.play_keys();
        let (dx, dy) = self.input.take_mouse();
        let sensitivity = self.settings.mouse_sensitivity;
        let view = self.view_depth();
        let Some(play) = &mut self.play else {
            return Frame::Skipped;
        };
        let alpha = play.ticks.alpha(now);
        if let Some(camera) = &mut play.camera {
            match &play.bench {
                Some(bench) => camera.fly(bench.movement(), seconds),
                None => {
                    // Looking around turns the camera every frame; moving
                    // shows the player between its last two ticks.
                    if self.cursor_grabbed {
                        camera.turn(dx * sensitivity, dy * sensitivity);
                    }
                    if let Some(player) = play.session.game().player() {
                        camera.position = player.eyes_at(alpha);
                    }
                }
            }
        }

        // The bench keeps M4's picture, so its numbers compare with the baseline.
        play.aimed = play
            .camera
            .as_ref()
            .filter(|_| play.bench.is_none())
            .and_then(|camera| aim::from_camera(play.session.game(), camera));
        if attack_pressed {
            play.session.attack_pressed(play.aimed.as_ref());
        }
        if use_pressed {
            play.session.use_pressed(play.aimed.as_ref());
        }
        // Shows the player's edits in this very frame, lit: a zero budget, only the changed
        // blocks, before their sub-chunks are meshed.
        play.session.world_mut().update_light(Duration::ZERO, (0, 0));
        let changes = play.session.world_mut().take_changes();
        play.meshes.apply(changes, play.session.world());
        if let Some(gpu) = &self.gpu {
            play.meshes.build_urgent(&gpu.device, play.session.world(), &self.table, &self.tints);
        }

        self.ui.update(play.session.game());

        // The sky and fog: the time of day, what the eyes are in, and where they look.
        let time = play.session.game().time().get().unwrap_or(sky::NOON_TICKS);
        let sky_frame = match &play.camera {
            Some(camera) => {
                let medium = play.eyes.medium(play.session.world(), &self.table, camera.position, now);
                sky::frame(&self.fog, time, medium, view, camera.forward())
            }
            None => sky::frame(&self.fog, time, Medium::Air, view, Vec3::Z),
        };

        // Separate fields, borrowed separately: the frame needs the GPU
        // mutably while the terrain and meshes are read.
        let (Some(gpu), Some(terrain), Some(ui_renderer)) = (&mut self.gpu, &mut self.terrain, &mut self.ui_renderer)
        else {
            return Frame::Skipped;
        };
        if let Some(drawing) = self.ui.drawing() {
            ui_renderer.upload(gpu, drawing);
        }
        if let Some(camera) = &play.camera {
            let boxes = play.aimed.as_ref().map_or(&[][..], |hit| &hit.boxes);
            terrain.outline.show(gpu, camera.position, boxes);
        }
        let cracked = play.session.crack().and_then(|(block, stage)| {
            let id = play.session.world().block(block.x, block.y, block.z, 0)?;
            Some(Cracked { block, id, stage })
        });
        terrain.crack.show(gpu, &self.table, &self.tints, cracked);
        // The entities, as they were a moment ago (`entities/track.rs`). The bench keeps
        // M4's picture, which had none.
        if let (Some(camera), Some(renderer)) = (&play.camera, &mut terrain.entities) {
            let drawn = if play.bench.is_none() { play.session.game().entities().drawn(now) } else { Vec::new() };
            let world = play.session.world();
            let looks = Looks {
                icons: self.ui.icons(),
                atlas: self.ui.atlas(),
            };
            let vertices = entities::vertices(&drawn, camera.position, &|at| light_byte(world, at), Some(looks));
            renderer.upload(gpu, &vertices);
        }
        if self.animated_tick != Some(self.animation_tick) {
            self.animated_tick = Some(self.animation_tick);
            terrain.animate(gpu, &mut self.animator, self.animation_tick);
        }
        terrain.set_daylight(gpu, sky::daylight(time));
        let (camera, meshes) = (play.camera.as_ref(), &play.meshes);
        let ui_renderer = &*ui_renderer;
        let drawn = gpu.frame(|gpu, views, encoder| {
            terrain.draw(gpu, &views.world, encoder, camera, meshes, &sky_frame);
            ui_renderer.draw(&views.ui, encoder);
        });
        if drawn == Frame::Drawn {
            play.session.frame_drawn();
            if let Some(bench) = &mut play.bench {
                let sample = Sample {
                    time: frame_time,
                    draws: terrain.draws,
                    visible: terrain.visible,
                    meshes: meshes.len(),
                    quads: meshes.stats.gpu_quads,
                    gpu_bytes: meshes.stats.gpu_bytes,
                    chunks: play.session.world().chunk_count(),
                };
                self.quit |= bench.record(sample, &meshes.stats);
            }
        }
        drawn
    }

    /// How far the view reaches, in blocks: what the fog ends at.
    fn view_depth(&self) -> f32 {
        self.settings.radius as f32 * 16.0
    }

    fn set_title(&self) {
        let Some(window) = &self.window else {
            return;
        };
        let Some(play) = &self.play else {
            window.set_title("Limonite");
            return;
        };
        let Some(camera) = &play.camera else {
            return;
        };
        let p = camera.position;
        let fps = self.fps;
        let (draws, visible) = self.terrain.as_ref().map_or((0, 0), |t| (t.draws, t.visible));
        let debug = play.browser.as_ref().map_or(String::new(), |browser| format!("{}; ", browser.label()));
        // The light in the block at the player's feet, as Java's debug screen shows it.
        let world = play.session.world();
        let light = world.light();
        let (x, y, z) = (p.x.floor() as i32, (p.y - EYE_HEIGHT).floor() as i32, p.z.floor() as i32);
        let here = match (light.level(Kind::Sky, x, y, z), light.level(Kind::Block, x, y, z)) {
            (Some(sky), Some(block)) => format!("; light {sky} sky, {block} block"),
            _ => String::new(),
        };
        window.set_title(&format!(
            "Limonite: {debug}{fps:.0} fps, {:.1} ms; {} chunks ({} unlit), {} meshes ({} waiting), {visible} \
             visible, {draws} draws; {} entities; at {:.1}, {:.1}, {:.1}{here}",
            1000.0 / fps.max(0.001),
            world.chunk_count(),
            light.waiting(),
            play.meshes.len(),
            play.meshes.waiting(),
            play.session.game().entities().len(),
            p.x,
            p.y,
            p.z
        ));
    }

    /// The title, and at debug level the camera beside the server's own
    /// copy of the host, which should follow it a tick or two behind.
    fn once_a_second(&mut self, now: Instant) {
        let elapsed = now - self.second_started;
        if elapsed < Duration::from_secs(1) {
            return;
        }
        self.fps = f64::from(self.frames_this_second) / elapsed.as_secs_f64();
        self.second_started = now;
        self.frames_this_second = 0;
        self.set_title();
        let Some(play) = &self.play else {
            return;
        };
        let Some(camera) = &play.camera else {
            return;
        };
        let p = camera.position;
        let stats = &play.meshes.stats;
        debug!(
            "Meshes: {} on the GPU, {:.1} MB, {} waiting; {} meshed so far at {:.0} µs each, {} quads",
            play.meshes.len(),
            stats.gpu_bytes as f64 / 1e6,
            play.meshes.waiting(),
            stats.meshed,
            stats.micros_each(),
            stats.quads_built,
        );
        if let Some(player) = play.server.server.get_all_players().first() {
            let server = player.position();
            debug!(
                "Camera eyes at ({:.2}, {:.2}, {:.2}); the server's player feet at ({:.2}, {:.2}, {:.2}); \
                 {} LevelChunks so far; time of day {:?}",
                p.x,
                p.y,
                p.z,
                server.x,
                server.y,
                server.z,
                play.session.level_chunks(),
                play.session.game().time().get(),
            );
        }
    }

    /// The world's work between frames: what arrived, the ticks that are due, and light and
    /// meshing within their budgets. Says whether a world is still being played.
    fn step_world(&mut self) -> bool {
        let Some(play) = &mut self.play else {
            return false;
        };
        play.session.drain();
        if play.session.stage() == Stage::Disconnected {
            self.lost_world("the server ended the connection");
            return self.play.is_some();
        }
        self.place_camera();
        self.follow_turns();
        self.follow_inventory();
        self.follow_menu();
        self.run_ticks();
        let Some(play) = &mut self.play else {
            return false;
        };

        // Light, then meshing, each within its budget: what changed in the
        // world, nearest the camera first.
        let around = play.camera.as_ref().map_or((0, 0), |camera| {
            let p = camera.position;
            ((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4)
        });
        play.session.world_mut().update_light(light::BUDGET, around);
        let changes = play.session.world_mut().take_changes();
        play.meshes.apply(changes, play.session.world());
        if let (Some(gpu), Some(camera)) = (&self.gpu, &play.camera) {
            play.meshes.build(
                &gpu.device,
                play.session.world(),
                &self.table,
                &self.tints,
                camera.position,
                meshes::BUDGET,
            );
        }
        // Once, when the first join's meshing has settled: the approximate shapes it loaded.
        if play.meshes.settled()
            && let Some(approximate) = self.approximate.take()
        {
            blocks::log_approximate_census(play.session.world().chunks(), &approximate);
        }
        if let (Some(bench), Some(camera), Some(gpu)) = (&mut play.bench, &mut play.camera, &self.gpu)
            && bench.waiting()
            && play.meshes.settled()
        {
            let chunks = play.session.world().chunk_count();
            bench.start(camera, chunks, &play.meshes.stats, gpu.config.present_mode);
        }
        true
    }
}

/// What moves the player this tick: the keys, under physics, facing the
/// camera's way. The bench flies the camera itself, frame by frame as in
/// M4, so its numbers compare with M4's, and the player just follows it.
fn motion(play: &Play, input: &Input) -> Motion {
    let Some(camera) = &play.camera else {
        return Motion::Still;
    };
    if play.bench.is_some() {
        Motion::To(Pose::from_eyes(camera.position, camera.pitch, camera.yaw))
    } else {
        // The player stands still while the inventory or a menu is open.
        let keys = if play.inventory_open || play.menu != Menu::None { Keys::default() } else { input.keys() };
        Motion::Keys {
            keys,
            pitch: camera.pitch,
            yaw: camera.yaw,
        }
    }
}

/// Logs the camera's pose as a command to type into the official client,
/// to look from the same place.
fn log_pose(camera: Option<&Camera>) {
    if let Some(camera) = camera {
        let p = camera.position;
        info!(
            "Pose: /tp @s {:.2} {:.2} {:.2} {:.1} {:.1}",
            p.x,
            p.y - EYE_HEIGHT,
            p.z,
            camera.yaw,
            camera.pitch
        );
    }
}

/// The key a menu answers to, if it's one: Enter, Escape, Tab, the arrows, and the keys
/// that edit a field. Anything else may be typing.
fn menu_key(key: &Logical) -> Option<Key> {
    let Logical::Named(named) = key else {
        return None;
    };
    Some(match named {
        NamedKey::Enter => Key::Enter,
        NamedKey::Escape => Key::Escape,
        NamedKey::Tab => Key::Tab,
        NamedKey::ArrowUp => Key::Up,
        NamedKey::ArrowDown => Key::Down,
        NamedKey::ArrowLeft => Key::Edit(EditKey::Left),
        NamedKey::ArrowRight => Key::Edit(EditKey::Right),
        NamedKey::Home => Key::Edit(EditKey::Home),
        NamedKey::End => Key::Edit(EditKey::End),
        NamedKey::Backspace => Key::Edit(EditKey::Backspace),
        NamedKey::Delete => Key::Edit(EditKey::Delete),
        _ => return None,
    })
}

/// Width over height, never dividing by zero.
fn aspect(size: PhysicalSize<u32>) -> f32 {
    size.width.max(1) as f32 / size.height.max(1) as f32
}

impl ApplicationHandler for App {
    /// The window and the GPU come up here, once: some platforms only allow
    /// creating windows after the loop has started.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Limonite")
            .with_inner_size(PhysicalSize::new(1280, 720));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                self.failure = Some(format!("Couldn't open a window: {err}"));
                event_loop.exit();
                return;
            }
        };
        match Gpu::new(window.clone(), event_loop.owned_display_handle(), &self.needs) {
            Ok(gpu) => {
                if let Some(textures) = self.textures.take() {
                    self.terrain = Some(Terrain::new(&gpu, &textures));
                }
                if let (Some(picture), Some(terrain)) = (self.ui_picture.take(), &mut self.terrain) {
                    let ui_renderer = UiRenderer::new(&gpu, &picture, &terrain.texture_view);
                    terrain.entities = Some(EntityRenderer::new(
                        &gpu,
                        terrain::DEPTH_FORMAT,
                        terrain.frame(),
                        &terrain.texture_view,
                        &terrain.light_map_view(),
                        ui_renderer.atlas_view(),
                    ));
                    self.ui_renderer = Some(ui_renderer);
                }
                let size = window.inner_size();
                self.ui.resize(size.width, size.height);
                self.gpu = Some(gpu);
            }
            Err(err) => {
                self.failure = Some(format!("Can't draw: {err}"));
                event_loop.exit();
                return;
            }
        }
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                let bench_unfinished = self
                    .play
                    .as_ref()
                    .and_then(|play| play.bench.as_ref())
                    .is_some_and(|bench| !bench.is_done());
                if bench_unfinished {
                    self.failure = Some("The bench didn't finish: the window was closed first".to_string());
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                self.visible = size.width > 0 && size.height > 0;
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
                self.ui.resize(size.width, size.height);
                if let Some(camera) = self.play.as_mut().and_then(|play| play.camera.as_mut()) {
                    camera.aspect = aspect(size);
                }
            }
            WindowEvent::Occluded(occluded) => self.visible = !occluded,
            WindowEvent::Focused(false) => {
                self.release_cursor();
                self.input.clear();
                self.pause();
            }
            WindowEvent::KeyboardInput { event, .. } if self.ui.wants_keys() => self.menu_key(&event),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    self.input.button(Button::Key(key), event.state);
                    if event.state == ElementState::Pressed && !event.repeat {
                        self.debug_key(key);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => self.ui.pointer_moved(position.x, position.y),
            WindowEvent::ModifiersChanged(modifiers) => self.shift = modifiers.state().shift_key(),
            // A menu's buttons act when the button comes up over what it went down on.
            WindowEvent::MouseInput { state, button, .. } if self.ui.wants_keys() => {
                if button == MouseButton::Left {
                    if state == ElementState::Pressed {
                        self.ui.pointer_pressed();
                    } else if let Some(clicked) = self.ui.pointer_released() {
                        self.clicked(clicked);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. }
                if self.play.as_ref().is_some_and(|play| play.inventory_open) =>
            {
                self.inventory_click(state, button);
            }
            // While the cursor is free, a left click goes to the UI, and if the UI
            // doesn't want it, it captures the cursor. A release always counts, so a
            // button held when the cursor was let go isn't left held.
            WindowEvent::MouseInput { state, button, .. } => {
                if self.cursor_grabbed || state == ElementState::Released {
                    self.input.button(Button::Mouse(button), state);
                    if button == MouseButton::Left {
                        self.ui.pointer_released();
                    }
                } else if button == MouseButton::Left && !self.ui.pointer_pressed() {
                    self.grab_cursor();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let notches = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    MouseScrollDelta::PixelDelta(position) => position.y / PIXELS_PER_NOTCH,
                };
                let inventory_open = self.play.as_ref().is_some_and(|play| play.inventory_open);
                if self.ui.wants_keys() || inventory_open {
                    self.ui.scroll(notches);
                } else if self.cursor_grabbed {
                    self.input.wheel(notches);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    /// Raw mouse motion: unaffected by pointer acceleration, and still
    /// reported when the cursor is locked in place.
    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event
            && self.cursor_grabbed
        {
            self.input.mouse_motion(dx, dy);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.quit {
            event_loop.exit();
            return;
        }
        self.run_pending();
        let playing = self.step_world();
        if self.quit {
            event_loop.exit();
            return;
        }
        if self.visible
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }
        let wake = match &self.play {
            Some(play) if playing => play.ticks.next(),
            _ => Instant::now() + MENU_FRAME,
        };
        event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
    }
}

/// The light in a block, as the terrain's vertices pack it: the sky's level high and the
/// blocks' low. Where nothing's lit yet, the open sky.
pub fn light_byte(world: &World, at: IVec3) -> u8 {
    let light = world.light();
    let sky = light.level(Kind::Sky, at.x, at.y, at.z).unwrap_or(15);
    let block = light.level(Kind::Block, at.x, at.y, at.z).unwrap_or(0);
    sky << 4 | block
}
