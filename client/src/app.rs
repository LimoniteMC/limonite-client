//! The window's main loop (m4-plan Decision 2).
//!
//! winit owns the main thread and calls [`App`] back: for each window and
//! device event, and in `about_to_wait` once it has handled everything that
//! arrived. The embedded server stays on its own thread, and the
//! `LocalChannel` is still the only thing between them.
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

use std::{
    collections::HashMap,
    process::ExitCode,
    sync::Arc,
    time::{Duration, Instant},
};

use pumpkin_data::item::Item;
use pumpkin_util::math::vector3::Vector3;
use tracing::{debug, error, info, warn};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

use crate::{
    bench::{Bench, Sample},
    blocks::{self, appearance::AppearanceTable},
    clock::Ticks,
    debug_world::Browser,
    edit::aim::{self, Hit},
    embedded::EmbeddedServer,
    input::{Action, Button, Input, PIXELS_PER_NOTCH},
    inventory::requests::Button as ClickButton,
    player::{EYE_HEIGHT, Motion, Pose, movement::Keys},
    render::{
        Frame, Gpu, Needs,
        animation::Animator,
        camera::{Camera, FOV_Y},
        crack::Cracked,
        meshes::{self, Meshes},
        terrain::Terrain,
        textures::TextureArray,
        tint::Tints,
    },
    session::{Session, Stage},
    settings::Settings,
    ui::{self, Ui, render::UiRenderer, screens::inventory::Pressed},
    world::light,
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
}

/// The longest step the bench's camera takes in one frame, so a hitch doesn't
/// fling it far away.
const MAX_FRAME_SECONDS: f64 = 0.25;

/// What a run does besides playing, from the command line.
pub struct Extras {
    /// `--bench`: fly for this long, log the frame times, and leave.
    pub bench: Option<Duration>,
    /// `--debug-world`: where you are in it.
    pub browser: Option<Browser>,
    /// `--hotbar`: what the host gets on joining.
    pub hotbar: Vec<(&'static Item, u8)>,
}

/// Opens the window, joins the embedded server as the host with this
/// install's `xid`, and runs until the window closes, the server ends the
/// connection, or a bench is done.
pub fn run(
    server: &EmbeddedServer,
    settings: Settings,
    xid: &str,
    needs: Needs,
    assets: Assets,
    extras: Extras,
) -> ExitCode {
    let radius = settings.radius;
    let mut session = match Session::connect(&server.connector, &settings.name, xid, radius) {
        Ok(session) => session,
        Err(err) => {
            error!("Couldn't connect to the embedded server: {err}");
            return ExitCode::FAILURE;
        }
    };
    // Before any chunk arrives, so the meshes hear about every one.
    session.world_mut().track_changes();
    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(err) => {
            error!("Couldn't start the window system: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut app = App::new(server, session, settings, needs, assets);
    app.bench = extras.bench.map(|duration| Bench::new(duration, radius));
    app.browser = extras.browser;
    app.hotbar = extras.hotbar;
    if let Err(err) = event_loop.run_app(&mut app) {
        error!("The window's event loop failed: {err}");
        return ExitCode::FAILURE;
    }
    app.session.log_edits();
    app.session.log_light();
    // Dropping the app drops the session, so the host leaves before the
    // caller stops the server.
    match app.failure {
        Some(err) => {
            error!("{err}");
            ExitCode::FAILURE
        }
        None => ExitCode::SUCCESS,
    }
}

pub struct App<'a> {
    server: &'a EmbeddedServer,
    session: Session,
    settings: Settings,
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
    /// Until the census is logged.
    approximate: Option<HashMap<u32, String>>,
    meshes: Meshes,
    terrain: Option<Terrain>,
    ui: Ui,
    /// The CPU copy of the UI's atlas, until it's uploaded.
    ui_picture: Option<image::RgbaImage>,
    ui_renderer: Option<UiRenderer>,
    /// Placed where `StartGame` put the host, once it has spawned. It draws
    /// the player's eyes, except during the bench, which flies it.
    camera: Option<Camera>,
    /// What the camera was aimed at, the last frame.
    aimed: Option<Hit>,
    input: Input,
    cursor_grabbed: bool,
    /// Whether shift is held, for a shift-click.
    shift: bool,
    /// Whether the inventory was open at the last look, to free the cursor when it opens
    /// and capture it again when it shuts.
    inventory_open: bool,
    /// Whether frames can be seen: not minimised or covered.
    visible: bool,
    ticks: Ticks,
    last_frame: Instant,
    /// Counting frames for the title, which updates once a second.
    second_started: Instant,
    frames_this_second: u32,
    fps: f64,
    /// `--bench`: it flies the camera instead of the keys and the mouse.
    bench: Option<Bench>,
    /// `--debug-world`: where you are in it.
    browser: Option<Browser>,
    /// `--hotbar`: what the host gets once it has spawned.
    hotbar: Vec<(&'static Item, u8)>,
    /// Set when the bench is done: the loop ends at its next pass.
    quit: bool,
    /// Why the loop ended early, if it did.
    failure: Option<String>,
}

impl<'a> App<'a> {
    fn new(
        server: &'a EmbeddedServer,
        session: Session,
        settings: Settings,
        needs: Needs,
        assets: Assets,
    ) -> Self {
        let now = Instant::now();
        let input = Input::new(settings.keymap.clone());
        let ui = Ui::new(assets.ui.atlas, assets.ui.font, assets.ui.lang, assets.ui.icons, settings.ui_scale);
        Self {
            server,
            session,
            settings,
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
            meshes: Meshes::default(),
            terrain: None,
            ui,
            ui_picture: Some(assets.ui.picture),
            ui_renderer: None,
            camera: None,
            aimed: None,
            input,
            cursor_grabbed: false,
            shift: false,
            inventory_open: false,
            visible: true,
            ticks: Ticks::new(now),
            last_frame: now,
            second_started: now,
            frames_this_second: 0,
            fps: 0.0,
            bench: None,
            browser: None,
            hotbar: Vec::new(),
            quit: false,
            failure: None,
        }
    }

    /// Runs the ticks that are due: each moves the player once and sends one
    /// `PlayerAuthInput`.
    fn run_ticks(&mut self) {
        for _ in 0..self.ticks.due(Instant::now()) {
            let motion = self.motion();
            if self.bench.is_none() {
                let hit = self.camera.as_ref().and_then(|camera| aim::from_camera(self.session.game(), camera));
                let playing = !self.inventory_open;
                let attack = playing && self.input.is_held(Action::Attack);
                let use_item = playing && self.input.is_held(Action::Use);
                self.session.edit_tick(attack, use_item, hit.as_ref());
            }
            self.session.step(motion);
            self.animation_tick += 1;
        }
    }

    /// What moves the player this tick: the keys, under physics, facing the
    /// camera's way. The bench flies the camera itself, frame by frame as in
    /// M4, so its numbers compare with M4's, and the player just follows it.
    fn motion(&self) -> Motion {
        let Some(camera) = &self.camera else {
            return Motion::Still;
        };
        if self.bench.is_some() {
            Motion::To(Pose::from_eyes(camera.position, camera.pitch, camera.yaw))
        } else {
            // The player stands still while the inventory is open.
            let keys = if self.inventory_open { Keys::default() } else { self.input.keys() };
            Motion::Keys {
                keys,
                pitch: camera.pitch,
                yaw: camera.yaw,
            }
        }
    }

    /// When the server turns the player (a teleport), the camera turns with
    /// it, before the next tick reads the camera's angles.
    fn follow_turns(&mut self) {
        let turned = self.session.game_mut().player_mut().and_then(|player| player.take_turned());
        if let (Some((pitch, yaw)), Some(camera)) = (turned, &mut self.camera) {
            camera.pitch = pitch;
            camera.yaw = yaw;
        }
    }

    /// Places the camera where the host spawned, once.
    fn place_camera(&mut self) {
        if self.camera.is_some() || self.session.stage() != Stage::Spawned {
            return;
        }
        let Some(player) = self.session.game().player() else {
            return;
        };
        let pose = player.pose();
        let mut camera = Camera::new(pose.eyes(), pose.pitch, pose.yaw);
        if let Some(window) = &self.window {
            let size = window.inner_size();
            camera.aspect = aspect(size);
        }
        // The bench keeps M4's view, so its numbers compare with the
        // baseline whatever the settings say.
        camera.fov_y = if self.bench.is_some() {
            FOV_Y
        } else {
            self.settings.fov as f32
        };
        self.camera = Some(camera);
        info!("The camera is at the host's spawn; click in the window to look around");
        self.session.check_gamemode(self.settings.gamemode);
        if !self.hotbar.is_empty() {
            self.server.fill_hotbar(&self.hotbar);
        }
        if self.browser.is_some() {
            info!(
                "Debug world: ] and [ go to the next and last block, . and , to the next and last state, \
                 and typing in this terminal goes to what matches"
            );
            self.jump_to_browser();
        }
    }

    /// Moves the host to what the debug world's browser is at, hovering to look at
    /// it, and says what that is.
    fn jump_to_browser(&mut self) {
        let Some(browser) = &self.browser else {
            return;
        };
        let (feet, yaw, pitch) = browser.pose();
        let label = browser.label();
        let players = self.server.server.get_all_players();
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
        let Some(browser) = &mut self.browser else {
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
        let Some(browser) = &mut self.browser else {
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

    /// Frees the cursor when the server opens the inventory, and captures it again when
    /// the screen shuts, as long as the window has the focus.
    fn follow_inventory(&mut self) {
        let open = self.session.game().inventory().is_open();
        if open == self.inventory_open {
            return;
        }
        self.inventory_open = open;
        if open {
            self.release_cursor();
        } else if self.window.as_ref().is_some_and(|window| window.has_focus()) {
            self.grab_cursor();
        }
    }

    /// E: the inventory opens, once the server says so, or shuts.
    fn toggle_inventory(&mut self) {
        if self.session.game().inventory().is_open() {
            self.session.close_inventory();
        } else {
            self.session.open_inventory();
        }
    }

    /// A mouse button while the inventory is open: a press on what's under the pointer,
    /// which acts at once, and a release, which ends a drag.
    fn inventory_click(&mut self, state: ElementState, button: MouseButton) {
        if state == ElementState::Released {
            self.session.pointer_released();
            return;
        }
        let button = match button {
            MouseButton::Left => ClickButton::Left,
            MouseButton::Right => ClickButton::Right,
            _ => return,
        };
        match self.ui.inventory_pressed() {
            Pressed::Slot(slot) => self.session.click_slot(slot, button, self.shift),
            Pressed::Creative(entry) => self.session.click_creative(entry),
            Pressed::Tab(tab) => self.session.creative_tab(tab),
            Pressed::Nothing => {}
        }
    }

    /// Logs the camera's pose as a command to type into the official client,
    /// to look from the same place.
    fn log_pose(&self) {
        if let Some(camera) = &self.camera {
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

    /// Shows the player's edits in this very frame, lit.
    fn mesh_edits(&mut self) {
        // A zero budget: only the changed blocks, before their sub-chunks are meshed.
        self.session.world_mut().update_light(Duration::ZERO, (0, 0));
        let changes = self.session.world_mut().take_changes();
        self.meshes.apply(changes, self.session.world());
        if let Some(gpu) = &self.gpu {
            self.meshes.build_urgent(&gpu.device, self.session.world(), &self.table, &self.tints);
        }
    }

    fn redraw(&mut self) {
        self.debug_typed();
        let now = Instant::now();
        let frame_time = now - self.last_frame;
        let seconds = frame_time.as_secs_f64().min(MAX_FRAME_SECONDS);
        self.last_frame = now;

        let (mut attack_pressed, mut use_pressed) = (false, false);
        for action in self.input.take_pressed() {
            // With the inventory open, the keys only shut it.
            if self.inventory_open && !matches!(action, Action::Inventory | Action::Pause | Action::LogPose) {
                continue;
            }
            match action {
                Action::Attack => attack_pressed = true,
                Action::Use => use_pressed = true,
                Action::Inventory => self.toggle_inventory(),
                // Until the pause menu (Step 16), Esc lets go of the cursor.
                Action::Pause if self.inventory_open => self.session.close_inventory(),
                Action::Pause => self.release_cursor(),
                Action::LogPose => self.log_pose(),
                Action::Hotbar(slot) => self.session.select_slot(slot),
                Action::HotbarNext => {
                    let slot = self.session.game().inventory().next_slot();
                    self.session.select_slot(slot);
                }
                Action::HotbarPrevious => {
                    let slot = self.session.game().inventory().previous_slot();
                    self.session.select_slot(slot);
                }
                Action::Drop => self.session.drop_held(),
                Action::Pick => {
                    if let Some(hit) = &self.aimed {
                        self.session.pick_block(hit.block);
                    }
                }
                _ => {}
            }
        }
        let (dx, dy) = self.input.take_mouse();
        let sensitivity = self.settings.mouse_sensitivity;
        let alpha = self.ticks.alpha(now);
        if let Some(camera) = &mut self.camera {
            match &self.bench {
                Some(bench) => camera.fly(bench.movement(), seconds),
                None => {
                    // Looking around turns the camera every frame; moving
                    // shows the player between its last two ticks.
                    if self.cursor_grabbed {
                        camera.turn(dx * sensitivity, dy * sensitivity);
                    }
                    if let Some(player) = self.session.game().player() {
                        camera.position = player.eyes_at(alpha);
                    }
                }
            }
        }

        // The bench keeps M4's picture, so its numbers compare with the baseline.
        self.aimed = self
            .camera
            .as_ref()
            .filter(|_| self.bench.is_none())
            .and_then(|camera| aim::from_camera(self.session.game(), camera));
        if attack_pressed {
            self.session.attack_pressed(self.aimed.as_ref());
        }
        if use_pressed {
            self.session.use_pressed(self.aimed.as_ref());
        }
        self.mesh_edits();

        self.ui.update(self.session.game());

        // Separate fields, borrowed separately: the frame needs the GPU
        // mutably while the terrain and meshes are read.
        let (Some(gpu), Some(terrain), Some(ui_renderer)) = (&mut self.gpu, &mut self.terrain, &mut self.ui_renderer)
        else {
            return;
        };
        if let Some(drawing) = self.ui.drawing() {
            ui_renderer.upload(gpu, drawing);
        }
        if let Some(camera) = &self.camera {
            let boxes = self.aimed.as_ref().map_or(&[][..], |hit| &hit.boxes);
            terrain.outline.show(gpu, camera.position, boxes);
        }
        let cracked = self.session.crack().and_then(|(block, stage)| {
            let id = self.session.world().block(block.x, block.y, block.z, 0)?;
            Some(Cracked { block, id, stage })
        });
        terrain.crack.show(gpu, &self.table, &self.tints, cracked);
        if self.animated_tick != Some(self.animation_tick) {
            self.animated_tick = Some(self.animation_tick);
            terrain.animate(gpu, &mut self.animator, self.animation_tick);
        }
        let (camera, meshes) = (self.camera.as_ref(), &self.meshes);
        let ui_renderer = &*ui_renderer;
        let drawn = gpu.frame(|gpu, views, encoder| {
            terrain.draw(gpu, &views.world, encoder, camera, meshes);
            ui_renderer.draw(&views.ui, encoder);
        });
        if drawn == Frame::Drawn {
            self.session.frame_drawn();
            self.frames_this_second += 1;
            if let Some(bench) = &mut self.bench {
                let sample = Sample {
                    time: frame_time,
                    draws: terrain.draws,
                    visible: terrain.visible,
                    meshes: meshes.len(),
                    quads: meshes.stats.gpu_quads,
                    gpu_bytes: meshes.stats.gpu_bytes,
                    chunks: self.session.world().chunk_count(),
                };
                self.quit |= bench.record(sample, &meshes.stats);
            }
        }
        self.once_a_second(now);
    }

    fn set_title(&self) {
        let (Some(window), Some(camera)) = (&self.window, &self.camera) else {
            return;
        };
        let p = camera.position;
        let fps = self.fps;
        let (draws, visible) = self.terrain.as_ref().map_or((0, 0), |t| (t.draws, t.visible));
        let debug = self.browser.as_ref().map_or(String::new(), |browser| format!("{}; ", browser.label()));
        window.set_title(&format!(
            "Limonite: {debug}{fps:.0} fps, {:.1} ms; {} chunks ({} unlit), {} meshes ({} waiting), {visible} \
             visible, {draws} draws; at {:.1}, {:.1}, {:.1}",
            1000.0 / fps.max(0.001),
            self.session.world().chunk_count(),
            self.session.world().light().waiting(),
            self.meshes.len(),
            self.meshes.waiting(),
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
        let Some(camera) = &self.camera else {
            return;
        };
        let p = camera.position;
        self.set_title();
        let stats = &self.meshes.stats;
        debug!(
            "Meshes: {} on the GPU, {:.1} MB, {} waiting; {} meshed so far at {:.0} µs each, {} quads",
            self.meshes.len(),
            stats.gpu_bytes as f64 / 1e6,
            self.meshes.waiting(),
            stats.meshed,
            stats.micros_each(),
            stats.quads_built,
        );
        if let Some(player) = self.server.server.get_all_players().first() {
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
                self.session.level_chunks(),
                self.session.game().time().get(),
            );
        }
    }
}

/// Width over height, never dividing by zero.
fn aspect(size: PhysicalSize<u32>) -> f32 {
    size.width.max(1) as f32 / size.height.max(1) as f32
}

impl ApplicationHandler for App<'_> {
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
                if let (Some(picture), Some(terrain)) = (self.ui_picture.take(), &self.terrain) {
                    self.ui_renderer = Some(UiRenderer::new(&gpu, &picture, &terrain.texture_view));
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
                if self.bench.as_ref().is_some_and(|bench| !bench.is_done()) {
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
                if let Some(camera) = &mut self.camera {
                    camera.aspect = aspect(size);
                }
            }
            WindowEvent::Occluded(occluded) => self.visible = !occluded,
            WindowEvent::Focused(false) => {
                self.release_cursor();
                self.input.clear();
            }
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
            WindowEvent::MouseInput { state, button, .. } if self.inventory_open => self.inventory_click(state, button),
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
            WindowEvent::MouseWheel { delta, .. } if self.cursor_grabbed || self.inventory_open => {
                let notches = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    MouseScrollDelta::PixelDelta(position) => position.y / PIXELS_PER_NOTCH,
                };
                if self.inventory_open {
                    self.ui.scroll(notches);
                } else {
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
        self.session.drain();
        if self.session.stage() == Stage::Disconnected {
            self.failure = Some("The embedded server ended the connection".to_string());
            event_loop.exit();
            return;
        }
        self.place_camera();
        self.follow_turns();
        self.follow_inventory();
        self.run_ticks();

        // Light, then meshing, each within its budget: what changed in the
        // world, nearest the camera first.
        let around = self.camera.as_ref().map_or((0, 0), |camera| {
            let p = camera.position;
            ((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4)
        });
        self.session.world_mut().update_light(light::BUDGET, around);
        let changes = self.session.world_mut().take_changes();
        self.meshes.apply(changes, self.session.world());
        if let (Some(gpu), Some(camera)) = (&self.gpu, &self.camera) {
            self.meshes.build(
                &gpu.device,
                self.session.world(),
                &self.table,
                &self.tints,
                camera.position,
                meshes::BUDGET,
            );
        }
        // Once, when the join's meshing has settled: the approximate shapes it loaded.
        if self.meshes.settled()
            && let Some(approximate) = self.approximate.take()
        {
            blocks::log_approximate_census(self.session.world().chunks(), &approximate);
        }
        if let (Some(bench), Some(camera), Some(gpu)) = (&mut self.bench, &mut self.camera, &self.gpu)
            && bench.waiting()
            && self.meshes.settled()
        {
            let chunks = self.session.world().chunk_count();
            bench.start(camera, chunks, &self.meshes.stats, gpu.config.present_mode);
        }

        if self.visible
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.ticks.next()));
    }
}
