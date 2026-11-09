mod app;
mod bench;
mod blocks;
mod checks;
mod clock;
mod debug_world;
mod edit;
mod embedded;
mod entities;
mod game;
mod home;
mod identity;
mod input;
mod install;
mod inventory;
mod login;
mod map;
mod measure;
mod packets;
mod player;
mod render;
mod session;
mod settings;
#[cfg(test)]
mod shots;
#[cfg(test)]
mod test_util;
mod ui;
mod world;

use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::PoisonError,
    thread,
    time::{Duration, Instant},
};

use glam::DVec3;
use pumpkin_core::{entity::player::Player as ServerPlayer, world::World as ServerWorld};
use pumpkin_data::{
    Block,
    entity::EntityType,
    item::{BedrockItem, Item},
    item_stack::ItemStack,
};
use pumpkin_protocol::bedrock::network_item::NetworkItemStackDescriptor;
use pumpkin_util::{
    GameMode,
    math::{position::BlockPos, vector2::Vector2, vector3::Vector3},
};
use pumpkin_world::{CURRENT_BEDROCK_MC_PROTOCOL, CURRENT_BEDROCK_MC_VERSION, world::BlockFlags};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{EnvFilter, filter::LevelFilter};

use crate::{
    checks::Check,
    clock::TICK,
    edit::{
        UsePlan,
        aim::{self, Hit},
    },
    embedded::{EmbeddedServer, HostOperator},
    home::Home,
    identity::Identity,
    inventory::{items, requests::Button as ClickButton, slots::SlotRef},
    render::camera::look_direction,
    session::{Session, Stage},
    settings::{MAX_RADIUS, MIN_RADIUS, Overrides, Settings},
    world::{BlockTable, World},
};

const USAGE: &str = "usage: limonite-client [--home <path>] [--bedrock <path>] [--java-jar <path>] [--data-dir <path>] \
                     [--debug-world] [--goto <text>] \
                     [--name <name>] [--gamemode <mode>] [--radius <chunks>] [--hotbar <items>] \
                     [--time <ticks>] [--bench <seconds>]\n       \
                     limonite-client --headless [--home <path>] [--data-dir <path>] [--name <name>] \
                     [--gamemode <mode>] [--radius <chunks>] [--hotbar <items>] [--linger <seconds>] [--measure] \
                     [--map <path.png>] [--stream <blocks>] [--edits <blocks>] [--inventory]";

/// Login and spawn must finish within this.
const SPAWN_DEADLINE: Duration = Duration::from_secs(120);
/// The walk: 40 ticks at 0.2 blocks a tick is 8 blocks east.
const WALK_TICKS: u32 = 40;
/// How far the scripted edits reach, which is in reach in any gamemode.
const EDIT_REACH: f64 = 4.4;
/// How high the scripted edits' floor is, and how far out it goes each way.
const EDIT_FLOOR_Y: i32 = 200;
const EDIT_FLOOR: i32 = 5;
/// How far off it looks for something to click, so that the middle of a block put
/// there is in reach too.
const CLICK_REACH: f64 = 3.4;
/// How long to wait for the server to apply the walk before checking it.
const SETTLE: Duration = Duration::from_secs(5);
/// How far up the teleport check moves the host, in blocks.
const TELEPORT_UP: f64 = 5.0;
/// How long the client has to arrive and report a teleport (m5-plan Step 5).
const TELEPORT_DEADLINE: Duration = Duration::from_secs(1);
/// The join is over once no chunk has arrived for this long...
const CHUNKS_QUIET: Duration = Duration::from_secs(2);
/// ...or after this, whichever comes first. A radius-32 join into a fresh
/// world is about 3,200 chunks of worldgen, so this is generous.
const CHUNKS_MAX_WAIT: Duration = Duration::from_secs(180);
/// How long the world comparison keeps retrying while block updates land.
const WORLD_SETTLE: Duration = Duration::from_secs(2);
/// How long the headless run lights chunks between ticks: well inside the 10 s after which the
/// server drops a client it hasn't heard from.
const LIGHT_SLICE: Duration = Duration::from_millis(500);
/// How many ticks a still-differing chunk gets for a fresh encode to decode
/// equal to the server.
const FRESH_DECODE_ATTEMPTS: u32 = 5;

/// Command-line options. Every path is absolute by the time parsing returns.
struct Args {
    /// Run the scripted session and its checks instead of opening a window.
    /// It needs no GPU and no Bedrock install, so it stays the regression
    /// suite.
    headless: bool,
    /// The Bedrock install: the folder that holds `data/resource_packs`.
    bedrock: Option<PathBuf>,
    /// The user's own Java client jar, for its block models. Without one, the
    /// usual places are looked in, and it's fine if they're empty.
    java_jar: Option<PathBuf>,
    /// Open the debug world, with every block state in it (`debug_world.rs`).
    debug_world: bool,
    /// Where to start in it.
    goto: Option<String>,
    /// The home folder: the settings and this install's identity.
    home: PathBuf,
    /// Where the embedded server keeps its world and data files.
    data_dir: PathBuf,
    /// What `--name`, `--radius` and `--gamemode` change in the settings for
    /// this run.
    overrides: Overrides,
    /// What the host gets in its hotbar on joining: the way to get blocks until there's
    /// an inventory screen.
    hotbar: Vec<(&'static Item, u8)>,
    /// The world's time of day to start at, in ticks, until the command line can set it.
    time: Option<i64>,
    /// How long to stay in the world after the checks, still sending input,
    /// so the process can be inspected from outside.
    linger: Duration,
    /// Log what the `LocalChannel` cost (meaningful in release builds only).
    measure: bool,
    /// Where to write a top-down PNG map of the loaded chunks.
    map: Option<PathBuf>,
    /// After the join, fly this many blocks east and log how chunks stream.
    stream: Option<u32>,
    /// Place and break this many blocks near the host with the real packets, and
    /// check that the server applied them.
    edits: Option<u32>,
    /// Open the inventory and move items with the real packets, and check that the
    /// server agreed and holds what the client shows.
    inventory: bool,
    /// Fly the window's camera east for this long, log the frame times, and
    /// exit (`bench.rs`).
    bench: Option<Duration>,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut headless = false;
        let mut bedrock = None;
        let mut java_jar = None;
        let mut debug_world = false;
        let mut goto = None;
        let mut home = PathBuf::from("run/home");
        let mut data_dir = None;
        let mut overrides = Overrides::default();
        let mut hotbar = Vec::new();
        let mut time = None;
        let mut linger = Duration::ZERO;
        let mut measure = false;
        let mut map = None;
        let mut stream = None;
        let mut edits = None;
        let mut inventory = false;
        let mut bench = None;
        // The scripted session's flags, which mean nothing to the window.
        let mut headless_only = Vec::new();
        while let Some(arg) = args.next() {
            if matches!(arg.as_str(), "--linger" | "--measure" | "--map" | "--stream" | "--edits" | "--inventory") {
                headless_only.push(arg.clone());
            }
            match arg.as_str() {
                "--headless" => headless = true,
                "--bedrock" => {
                    bedrock = Some(PathBuf::from(args.next().ok_or("--bedrock needs a path")?));
                }
                "--java-jar" => {
                    java_jar = Some(PathBuf::from(args.next().ok_or("--java-jar needs a path")?));
                }
                "--debug-world" => debug_world = true,
                "--goto" => goto = Some(args.next().ok_or("--goto needs what to look for")?),
                "--home" => home = args.next().ok_or("--home needs a path")?.into(),
                "--data-dir" => {
                    data_dir = Some(PathBuf::from(args.next().ok_or("--data-dir needs a path")?));
                }
                "--name" => {
                    let name = args.next().ok_or("--name needs a name")?;
                    if !settings::is_valid_name(&name) {
                        return Err(format!(
                            "--name: `{name}` isn't 1 to 16 characters without control characters"
                        ));
                    }
                    overrides.name = Some(name);
                }
                "--gamemode" => {
                    let mode = args.next().ok_or("--gamemode needs a mode")?;
                    overrides.gamemode = Some(mode.parse().map_err(|_| {
                        format!("--gamemode: `{mode}` isn't survival, creative, adventure or spectator")
                    })?);
                }
                "--hotbar" => {
                    let list = args.next().ok_or("--hotbar needs a list of items")?;
                    hotbar = embedded::parse_hotbar(&list).map_err(|err| format!("--hotbar: {err}"))?;
                }
                "--time" => {
                    let ticks = args.next().ok_or("--time needs a time of day in ticks")?;
                    time = Some(
                        ticks
                            .parse::<i64>()
                            .ok()
                            .filter(|ticks| (0..24_000).contains(ticks))
                            .ok_or(format!("--time: `{ticks}` isn't a time of day, 0 to 23999 ticks"))?,
                    );
                }
                "--linger" => {
                    let secs = args.next().ok_or("--linger needs a number of seconds")?;
                    let secs = secs
                        .parse()
                        .map_err(|_| format!("--linger: `{secs}` isn't a number of seconds"))?;
                    linger = Duration::from_secs(secs);
                }
                "--measure" => measure = true,
                "--map" => map = Some(PathBuf::from(args.next().ok_or("--map needs a path")?)),
                "--radius" => {
                    let chunks = args.next().ok_or("--radius needs a number of chunks")?;
                    overrides.radius = Some(
                        chunks
                            .parse()
                            .ok()
                            .filter(|r| (MIN_RADIUS..=MAX_RADIUS).contains(r))
                            .ok_or(format!("--radius: `{chunks}` isn't {MIN_RADIUS} to {MAX_RADIUS} chunks"))?,
                    );
                }
                "--stream" => {
                    let blocks = args.next().ok_or("--stream needs a number of blocks")?;
                    stream = Some(
                        blocks
                            .parse()
                            .map_err(|_| format!("--stream: `{blocks}` isn't a number of blocks"))?,
                    );
                }
                "--edits" => {
                    let blocks = args.next().ok_or("--edits needs a number of blocks")?;
                    edits = Some(
                        blocks
                            .parse()
                            .map_err(|_| format!("--edits: `{blocks}` isn't a number of blocks"))?,
                    );
                }
                "--inventory" => inventory = true,
                "--bench" => {
                    let secs = args.next().ok_or("--bench needs a number of seconds")?;
                    let secs = secs
                        .parse()
                        .ok()
                        .filter(|&secs| secs > 0)
                        .ok_or(format!("--bench: `{secs}` isn't a whole number of seconds above 0"))?;
                    bench = Some(Duration::from_secs(secs));
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        if !headless && !headless_only.is_empty() {
            return Err(format!(
                "the scripted session's flags need --headless: {}",
                headless_only.join(", ")
            ));
        }
        if headless && bench.is_some() {
            return Err("--bench measures the window's frames, so it can't run with --headless".to_string());
        }
        // Resolve now, against the directory we were started in: the working
        // directory changes later, and a relative path would change meaning.
        let absolute = |path: &Path| {
            std::path::absolute(path).map_err(|err| format!("can't resolve {}: {err}", path.display()))
        };
        // A debug world has a folder of its own, so a normal one isn't flattened, and
        // it always opens in creative, to fly around it.
        let default_data_dir = if debug_world { "run/debug-world" } else { "run/client" };
        let data_dir = data_dir.unwrap_or_else(|| PathBuf::from(default_data_dir));
        if debug_world {
            overrides.gamemode = Some(GameMode::Creative);
        }
        Ok(Self {
            headless,
            bedrock: bedrock.as_deref().map(absolute).transpose()?,
            java_jar: java_jar.as_deref().map(absolute).transpose()?,
            debug_world,
            goto,
            home: absolute(&home)?,
            data_dir: absolute(&data_dir)?,
            overrides,
            hotbar,
            time,
            linger,
            measure,
            map: map.as_deref().map(absolute).transpose()?,
            stream,
            edits,
            inventory,
            bench,
        })
    }
}

fn main() -> ExitCode {
    // `RUST_LOG` picks what gets logged, e.g. `RUST_LOG=debug` or
    // `RUST_LOG=info,pumpkin_core::net::bedrock=debug`. Without it, `info` and above.
    // Pumpkin logs through the same global subscriber.
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt().with_env_filter(filter).init();

    info!(
        "Limonite Client {} starting; embedded Pumpkin targets Bedrock \
         {CURRENT_BEDROCK_MC_VERSION} (protocol {CURRENT_BEDROCK_MC_PROTOCOL})",
        env!("CARGO_PKG_VERSION"),
    );

    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(err) => {
            error!("{err}\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let (settings, identity) = match open_home(&args) {
        Ok(home) => home,
        Err(err) => {
            error!("{err}");
            return ExitCode::FAILURE;
        }
    };

    if args.headless {
        if args.bedrock.is_some() || args.java_jar.is_some() {
            warn!("--bedrock and --java-jar are unused: the scripted session reads no assets");
        }
        if args.debug_world {
            warn!("--debug-world is unused: it's for the window");
        }
        headless(&args, &settings, &identity)
    } else {
        windowed(&args, settings, &identity)
    }
}

/// Opens the home folder, and reads the settings, with this run's flags
/// applied, and this install's identity.
fn open_home(args: &Args) -> Result<(Settings, Identity), String> {
    let home = Home::open(args.home.clone())
        .map_err(|err| format!("Can't use the home folder {}: {err}", args.home.display()))?;
    info!("Home folder: {}", home.root().display());
    let mut settings = Settings::load(&home.settings());
    settings.apply(&args.overrides);
    let identity = Identity::load_or_create(&home.identity())?;
    Ok((settings, identity))
}

/// The default path: the window.
///
/// The assets load first, before the server starts, so a missing install
/// fails at once.
fn windowed(args: &Args, settings: Settings, identity: &Identity) -> ExitCode {
    let install = match install::find(args.bedrock.as_deref()) {
        Ok(install) => install,
        Err(err) => {
            error!("{err}");
            return ExitCode::FAILURE;
        }
    };
    let vanilla = match install::load_vanilla(&install) {
        Ok(vanilla) => vanilla,
        Err(err) => {
            error!("Couldn't load the vanilla packs: {err}");
            return ExitCode::FAILURE;
        }
    };
    let palette = match blocks::load_palette() {
        Ok(palette) => palette,
        Err(err) => {
            error!("{err}");
            return ExitCode::FAILURE;
        }
    };
    let mut jar = match blocks::find_jar(args.java_jar.as_deref()) {
        Ok(jar) => jar,
        Err(err) => {
            error!("{err}");
            return ExitCode::FAILURE;
        }
    };
    let looks = blocks::build_appearance(&palette, &vanilla, jar.as_mut());
    let approximate = blocks::approximate_ids(&palette, &looks.report);
    let tints = render::load_tints(&vanilla.stack);
    let ui_assets = ui::load(&vanilla.stack, &looks.table, &tints);

    let server = match start_server(args, &settings, identity) {
        Ok(server) => server,
        Err(code) => return code,
    };
    if args.bench.is_some() && cfg!(debug_assertions) {
        warn!("This is a debug build: the bench's frame times mean little. Use --release");
    }
    let needs = render::Needs {
        texture_layers: looks.textures.layers,
        uncapped: args.bench.is_some(),
    };
    let (fog, fog_read) = render::sky::FogSettings::load(&vanilla.stack);
    if let Err(err) = fog_read {
        warn!("The install's fog didn't load, so it's Bedrock 1.26.60's: {err}");
    }
    let assets = app::Assets {
        table: looks.table,
        textures: looks.textures,
        animator: looks.animator,
        tints,
        approximate,
        ui: ui_assets,
        fog,
    };
    let browser = args.debug_world.then(|| {
        let layout = debug_world::Layout::build(&BlockTable::build());
        debug_world::place(&server, &layout);
        debug_world::Browser::new(layout, args.goto.as_deref())
    });
    let extras = app::Extras {
        bench: args.bench,
        browser,
        hotbar: args.hotbar.clone(),
    };
    let result = app::run(&server, settings, identity.xid(), needs, assets, extras);
    server.stop();
    result
}

/// The scripted session (`--headless`): the M2 and M3 acceptance run, which
/// needs no GPU and no Bedrock install.
fn headless(args: &Args, settings: &Settings, identity: &Identity) -> ExitCode {
    let server = match start_server(args, settings, identity) {
        Ok(server) => server,
        Err(code) => return code,
    };
    let result = play(&server, args, settings, identity);
    server.stop();
    result
}

/// Sets up the process, changes into the data directory, makes the host an
/// operator of its world, and starts the embedded server.
fn start_server(args: &Args, settings: &Settings, identity: &Identity) -> Result<EmbeddedServer, ExitCode> {
    set_up_process();
    if let Err(err) = enter_data_dir(&args.data_dir) {
        error!(
            "Can't use the data directory {}: {err}",
            args.data_dir.display()
        );
        return Err(ExitCode::FAILURE);
    }
    info!("Data directory: {}", args.data_dir.display());
    info!(
        "New players get gamemode {} (the settings, or --gamemode)",
        settings.gamemode.name()
    );
    let config = embedded::config(settings);
    match embedded::make_host_operator(&args.data_dir, &config, &settings.name, identity.xid()) {
        Ok(HostOperator::Added) => info!("The host is now an operator of this world, at level 4"),
        Ok(HostOperator::Listed(level)) => {
            info!("The host is an operator of this world at level {}, left as it is", level as u8);
        }
        Err(err) => warn!("Couldn't make the host an operator, so it has no commands: {err}"),
    }
    if args.debug_world
        && let Err(err) = debug_world::prepare(&config.basic.get_world_path(), config.basic.seed)
    {
        error!("{err}");
        return Err(ExitCode::FAILURE);
    }
    let server = EmbeddedServer::start(config).map_err(|err| {
        error!("{err}");
        ExitCode::FAILURE
    })?;
    if let Some(time) = args.time {
        server.set_time(time, false);
        info!("The world's time of day is now {time} (--time); the day goes on from there");
    }
    Ok(server)
}

/// The scripted host session: log in, spawn, walk east, check the move on the
/// server, optionally linger, and report.
fn play(server: &EmbeddedServer, args: &Args, settings: &Settings, identity: &Identity) -> ExitCode {
    let mut session = match Session::connect(&server.connector, &settings.name, identity.xid(), settings.radius) {
        Ok(session) => session,
        Err(err) => {
            error!("Couldn't connect to the embedded server: {err}");
            return ExitCode::FAILURE;
        }
    };

    let spawned = session.run_until(Instant::now() + SPAWN_DEADLINE, |s| {
        s.stage() == Stage::Spawned
    });
    let mut checks = vec![match &spawned {
        Ok(()) => Check::new(
            "spawn",
            true,
            "PlayerSpawn received; SetLocalPlayerAsInitialized sent".to_string(),
        ),
        Err(err) => Check::new("spawn", false, err.to_string()),
    }];

    let spawned = match (spawned, session.spawn()) {
        (Ok(()), Some(spawn)) => {
            session.check_gamemode(settings.gamemode);
            checks.push(walk_and_check(&mut session, server, &spawn));
            checks.push(checks::operator(&server.server, login::host_uuid(&settings.name, identity.xid())));
            checks.push(teleport_and_check(&mut session, server));
            // The server probes once a second, from when the host has loaded.
            let _ = session.run_until(Instant::now() + SETTLE, |_| checks::latency(&server.server).passed());
            checks.push(checks::latency(&server.server));
            if args.measure || args.stream.is_some() {
                // These runs stay a minute or more. A host left standing
                // with its head in terrain dies in about 10 s, and the server
                // then ignores its input. Rising is a vertical move, so the
                // chunks the server sends don't change.
                info!("Rising to eye height {} for the measurements", measure::SAFE_EYE_Y);
                session.rise_to(measure::SAFE_EYE_Y);
            }
            // `PlayerSpawn` only means the server has queued enough chunks,
            // not that they've arrived, so wait for them before checking.
            let join_chunks = wait_for_chunks(&mut session);
            checks.push(entities_and_check(&mut session, server));
            if !args.hotbar.is_empty() {
                checks.push(hotbar_and_check(&mut session, server, &args.hotbar));
            }
            if let Some(blocks) = args.edits {
                checks.push(edits_and_check(&mut session, server, blocks));
            }
            if args.inventory {
                checks.push(inventory_and_check(&mut session, server));
            }
            if args.measure {
                measure(&mut session, server, join_chunks);
            }
            if let Some(blocks) = args.stream {
                measure::stream(&mut session, &server.server, blocks, join_chunks);
            }
            true
        }
        _ => {
            for name in ["movement", "operator", "teleport", "latency", "entities"] {
                checks.push(Check::new(name, false, "not checked: never spawned".to_string()));
            }
            false
        }
    };

    let chunks = session.level_chunks();
    checks.push(Check::new(
        "chunks",
        chunks > 4,
        format!("{chunks} LevelChunk packets received (needs more than 4)"),
    ));
    checks.push(checks::chunks_decoded(&session));
    if spawned {
        checks.push(check_world(&mut session, server));
        checks.push(check_light(&mut session, server));
        linger(&mut session, args.linger);
    } else {
        for name in ["world matches server", "light matches server"] {
            checks.push(Check::new(name, false, "not checked: never spawned".to_string()));
        }
    }
    session.log_edits();
    session.log_light();
    session.log_entities();
    let mut passed = checks::report(&checks);
    if let Some(path) = &args.map {
        passed &= write_map(&session, path);
    }
    // Dropping the session closes the connection: the host leaves the game
    // before the server stops.
    drop(session);

    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Writes the map of the loaded chunks, marking the host. Returns false if it
/// couldn't.
fn write_map(session: &Session, path: &Path) -> bool {
    let host = session.spawn().map(|_| session.position());
    if let Some(host) = host {
        info!(
            "Host at x {:.1}, z {:.1} (eyes at y {:.1})",
            host.x, host.z, host.y
        );
    }
    match map::write(session.world(), host, path) {
        Ok(map) => {
            let (width, height) = map.image.dimensions();
            info!(
                "Map: {} chunks, {width}×{height} px, {} unknown ids -> {}",
                map.chunks,
                map.unknown_ids.len(),
                path.display()
            );
            for id in &map.unknown_ids {
                debug!("Unknown block id on the map: {id:#010x}");
            }
            true
        }
        Err(err) => {
            error!("Couldn't write the map: {err}");
            false
        }
    }
}

/// Walks east, then waits until the server's copy of the player has moved
/// (or [`SETTLE`] passes) and checks it.
fn walk_and_check(session: &mut Session, server: &EmbeddedServer, spawn: &session::Spawn) -> Check {
    info!("Walking east for {WALK_TICKS} ticks");
    session.walk(WALK_TICKS);
    let walk_deadline = Instant::now() + TICK * (WALK_TICKS + 20);
    if let Err(err) = session.run_until(walk_deadline, |s| !s.is_walking()) {
        return Check::new("movement", false, format!("walk interrupted: {err}"));
    }
    // The server applies input on its own tick thread, a tick or more later.
    // Polling its state each tick, rather than sleeping a fixed time, keeps
    // a slow (debug) server from failing the check. Timing out here is fine:
    // the check below reports what the server has.
    let _ = session.run_until(Instant::now() + SETTLE, |_| {
        checks::movement(&server.server, spawn).passed()
    });
    checks::movement(&server.server, spawn)
}

/// Asks the server to move the host [`TELEPORT_UP`] blocks up, as `/tp` does,
/// then runs ticks until an input reports the teleport, for up to
/// [`TELEPORT_DEADLINE`], and checks the report.
fn hotbar_and_check(session: &mut Session, server: &EmbeddedServer, stacks: &[(&'static Item, u8)]) -> Check {
    let held = |session: &Session| -> Vec<u16> {
        session.game().inventory().hotbar().iter().flatten().map(|item| item.count).collect()
    };
    let wanted: Vec<u16> = stacks.iter().map(|&(_, count)| u16::from(count)).collect();
    server.fill_hotbar(stacks);
    let _ = session.run_until(Instant::now() + SETTLE, |session| held(session) == wanted);
    let got = held(session);
    Check::new(
        "hotbar",
        got == wanted,
        format!("the client holds stacks of {got:?}, and {wanted:?} went in through the server"),
    )
}

/// What the entity check summons, so there's always something to compare: animals,
/// which leave the host alone in survival too. They go where the host stands, which is
/// open air, since one put into the ground suffocates; they push each other apart.
const SUMMONS: [(&EntityType, [f64; 3]); 3] = [
    (&EntityType::COW, [0.0, 0.0, 0.0]),
    (&EntityType::PIG, [0.0, 0.0, 0.0]),
    (&EntityType::CHICKEN, [0.0, 0.0, 0.0]),
];

/// Summons [`SUMMONS`] through the server, then runs ticks until the client has every
/// entity the server shows the host, for up to [`SETTLE`], and checks it.
fn entities_and_check(session: &mut Session, server: &EmbeddedServer) -> Check {
    info!("Summoning {} animals beside the host", SUMMONS.len());
    if let Some(feet) = server.host_feet() {
        for (entity_type, offset) in SUMMONS {
            server.summon(entity_type, [feet[0] + offset[0], feet[1] + offset[1], feet[2] + offset[2]]);
        }
    }
    let _ = session.run_until(Instant::now() + SETTLE, |s| checks::entities(s, &server.server).passed());
    checks::entities(session, &server.server)
}

fn teleport_and_check(session: &mut Session, server: &EmbeddedServer) -> Check {
    let players = server.server.get_all_players();
    let Some(player) = players.first() else {
        return Check::new("teleport", false, "the server has no player".to_string());
    };
    let entity = &player.living_entity.entity;
    let from = player.position();
    let to = Vector3::new(from.x, from.y + TELEPORT_UP, from.z);
    // Where the server puts the eyes: it sends the feet plus its eye height.
    let eyes = Vector3::new(to.x, to.y + f64::from(entity.entity_type.eye_height), to.z);
    info!("Teleporting the host {TELEPORT_UP} blocks up");
    session.forget_teleport_report();
    let asked = Instant::now();
    player.request_teleport(to, entity.yaw.load(), entity.pitch.load());
    let _ = session.run_until(asked + TELEPORT_DEADLINE, |s| s.teleport_report().is_some());
    let waited = asked.elapsed();
    // The server applies the input on its own tick, so give it a moment to see it.
    let server_waits = || {
        player
            .awaiting_teleport
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    };
    let _ = session.run_until(Instant::now() + SETTLE, |_| !server_waits());
    checks::teleport(session.teleport_report(), eyes, waited, server_waits())
}

/// What the host can click on from where it stands: looking around, each block a ray
/// hits, once, that a block can go against, and not so close that it'd be in the way.
fn clicks_in_reach(session: &Session) -> Vec<Hit> {
    let Some(player) = session.game().player() else {
        return Vec::new();
    };
    let eyes = player.pose().eyes();
    let feet = player.pose().feet.floor().as_ivec3();
    let world = session.game().world();
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for pitch in (10..=80).step_by(5) {
        for yaw in (0..360).step_by(15) {
            let look = look_direction(pitch as f32, yaw as f32).as_dvec3();
            let Some(hit) = aim::in_world(world, eyes, look, CLICK_REACH) else {
                continue;
            };
            let near = (hit.block - feet).abs();
            if near.x.max(near.z) > 1 && seen.insert(hit.block) {
                found.push(hit);
            }
        }
    }
    found
}

/// Runs `ticks` ticks at most with neither button held, until `done` says so.
fn edit_ticks(session: &mut Session, ticks: u32, mut done: impl FnMut(&Session) -> bool) -> bool {
    for _ in 0..ticks {
        if done(session) {
            return true;
        }
        session.edit_tick(false, false, None);
        session.tick();
        thread::sleep(TICK);
    }
    done(session)
}

/// What the client expects a click on `hit` to do, and the block in hand.
fn plan_for(session: &Session, hit: &Hit) -> Result<(UsePlan, &'static Block), String> {
    let player = session.game().player().ok_or("no player")?;
    let held = session.game().inventory().held();
    let item = items::placed_block(held.id, held.aux_value.0).ok_or("nothing to place in hand")?;
    let plan = edit::plan_use(session.game().world(), player.pose().feet, player.abilities.build, Some(item), hit.block, hit.face);
    if plan.places {
        Ok((plan, item))
    } else {
        Err(format!("the client says {} won't take a block", hit.block))
    }
}

/// One click and one break, of whatever's in the hand, checked on both sides.
fn place_and_break(session: &mut Session, world: &ServerWorld, hit: &Hit) -> Result<(), String> {
    let (plan, item) = plan_for(session, hit)?;
    let eyes = session.game().player().ok_or("no player")?.pose().eyes();
    let at = plan.result;
    let pos = BlockPos::new(at.x, at.y, at.z);
    let shown = |session: &Session| session.game().world().block(at.x, at.y, at.z, 0);
    let client_has_it = |session: &Session| {
        shown(session)
            .and_then(|id| session.game().world().blocks().java_state(id))
            .is_some_and(|state| Block::from_state_id(state).id == item.id)
    };
    let before = shown(session);

    session.use_pressed(Some(hit));
    let first_look = shown(session);
    match plan.shows {
        Some(id) if first_look != Some(id) => {
            return Err(format!("at {at} the client shows {first_look:?} right after the click, not {id}"));
        }
        None if first_look != before => return Err(format!("at {at} the client guessed a block it can't be sure of")),
        _ => {}
    }
    if !edit_ticks(session, 40, |_| world.get_block(&pos).id == item.id) {
        return Err(format!("the server has {} at {at}, not {}", world.get_block(&pos).name, item.name));
    }
    if !edit_ticks(session, 20, client_has_it) {
        return Err(format!("the client doesn't have what the server has at {at}: {}", item.name));
    }

    // Now to break it, holding the button until it goes.
    let middle = at.as_dvec3() + DVec3::splat(0.5);
    let aimed = aim::in_world(session.game().world(), eyes, (middle - eyes).normalize(), EDIT_REACH)
        .filter(|aimed| aimed.block == at)
        .ok_or_else(|| format!("nothing to aim at, at {at}"))?;
    session.attack_pressed(Some(&aimed));
    let start = session.game().ticks();
    let mut broke = false;
    for _ in 0..300 {
        session.edit_tick(true, false, Some(&aimed));
        session.tick();
        thread::sleep(TICK);
        if !client_has_it(session) {
            broke = true;
            break;
        }
    }
    let took = session.game().ticks() - start;
    session.edit_tick(false, false, None);
    if !broke {
        return Err(format!("{} at {at} never broke, in {took} ticks", item.name));
    }
    if !edit_ticks(session, 40, |_| world.get_block(&pos).id != item.id) {
        return Err(format!("the server still has {} at {at}", item.name));
    }
    info!("Placed and broke {} at {at}; it took {took} ticks to break", item.name);
    Ok(())
}

/// Places and breaks `blocks` blocks near the host, and one log, with the real packets,
/// and checks that the server applied each. Then a click the server refuses, which has
/// to snap back (m5-plan Step 11).
fn edits_and_check(session: &mut Session, server: &EmbeddedServer, blocks: u32) -> Check {
    const NAME: &str = "edits";
    let players = server.server.get_all_players();
    let Some(player) = players.first() else {
        return Check::new(NAME, false, "the server has no player".to_string());
    };
    let world = player.world().clone();
    // Through the server, as anything gets into a slot.
    let _runtime = server.runtime.enter();
    player.inventory().insert_stack(0, &mut ItemStack::new(64, &Item::DIRT));
    player.inventory().insert_stack(1, &mut ItemStack::new(64, &Item::OAK_LOG));
    player.sync_inventory_to_client();
    // A floor of its own, high up in clear air, so that this goes the same in any world:
    // a host can just as well spawn over the sea, with nothing in reach.
    let at = player.position();
    let (x, z) = (at.x.floor() as i32, at.z.floor() as i32);
    for dx in -EDIT_FLOOR..=EDIT_FLOOR {
        for dz in -EDIT_FLOOR..=EDIT_FLOOR {
            let stone = Block::STONE.default_state.id;
            world.set_block_state(&BlockPos::new(x + dx, EDIT_FLOOR_Y, z + dz), stone, BlockFlags::NOTIFY_ALL);
        }
    }
    let on_it = Vector3::new(f64::from(x) + 0.5, f64::from(EDIT_FLOOR_Y) + 1.0, f64::from(z) + 0.5);
    player.request_teleport(on_it, 0.0, 0.0);
    // The stacks, the floor and the teleport all take a tick or two.
    let _ = session.run_until(Instant::now() + SETTLE, |session| {
        let on_the_floor = session
            .game()
            .player()
            .is_some_and(|player| player.on_ground && player.pose().feet.y >= f64::from(EDIT_FLOOR_Y));
        on_the_floor && session.game().inventory().hotbar()[1].is_some()
    });
    let gamemode = session.game().player().map(|player| player.gamemode);
    let mut ground = clicks_in_reach(session).into_iter();

    let mut problems = Vec::new();
    let mut done = 0;
    while done < blocks {
        let Some(hit) = ground.next() else {
            problems.push(format!("only {done} places in reach"));
            break;
        };
        // What's been edited already may not take a block any more, which is no fault.
        if plan_for(session, &hit).is_ok() {
            match place_and_break(session, &world, &hit) {
                Ok(()) => done += 1,
                Err(problem) => problems.push(problem),
            }
        }
    }
    session.select_slot(1);
    let log = ground.find(|hit| plan_for(session, hit).is_ok()).map(|hit| place_and_break(session, &world, &hit));
    session.select_slot(0);
    match log {
        Some(Ok(())) => {}
        Some(Err(problem)) => problems.push(problem),
        None => problems.push("nothing left in reach for the log".to_string()),
    }

    // The server takes the right to build away, and the client hasn't heard yet.
    let refused = match ground.find(|hit| plan_for(session, hit).is_ok_and(|(plan, _)| plan.shows.is_some())) {
        Some(hit) => {
            let at = plan_for(session, &hit).map_or(hit.block, |(plan, _)| plan.result);
            let shown = |session: &Session| session.game().world().block(at.x, at.y, at.z, 0);
            let before = shown(session);
            player.set_gamemode(GameMode::Adventure);
            session.use_pressed(Some(&hit));
            let predicted = shown(session);
            let settled = edit_ticks(session, 40, |session| shown(session) == before);
            player.set_gamemode(gamemode.unwrap_or(GameMode::Creative));
            let _ = session.run_until(Instant::now() + SETTLE, |session| session.game().player().is_some_and(|player| player.abilities.build));
            if predicted == before {
                problems.push(format!("the refused click at {at} showed nothing"));
            }
            if !settled {
                problems.push(format!("the refused click at {at} didn't snap back"));
            }
            settled
        }
        None => {
            problems.push("nothing left in reach for the refused click".to_string());
            false
        }
    };
    Check::new(
        NAME,
        problems.is_empty(),
        if problems.is_empty() {
            format!(
                "placed and broke {done} blocks and a log in {gamemode:?} with the real packets, the server applied each, and a refused click snapped back ({refused})"
            )
        } else {
            problems.join("; ")
        },
    )
}

/// Opens the inventory with the real packets, makes the capture's survival moves and then
/// the creative ones, and after each answer checks that the server agreed and holds what
/// the client shows (m5-plan Step 12).
fn inventory_and_check(session: &mut Session, server: &EmbeddedServer) -> Check {
    const NAME: &str = "inventory";
    let players = server.server.get_all_players();
    let Some(player) = players.first() else {
        return Check::new(NAME, false, "the server has no player".to_string());
    };
    let _runtime = server.runtime.enter();
    let gamemode = session.game().player().map_or(GameMode::Creative, |player| player.gamemode);
    let mut problems = Vec::new();
    let mut requests = 0;

    // The capture's survival session: three stacks on the hotbar, the rest empty.
    player.set_gamemode(GameMode::Survival);
    for slot in 0..inventory::MAIN {
        player.inventory().set_slot(slot, ItemStack::EMPTY.clone());
    }
    player.inventory().set_slot(0, ItemStack::new(16, &Item::DIRT));
    player.inventory().set_slot(1, ItemStack::new(16, &Item::OAK_PLANKS));
    player.inventory().set_slot(2, ItemStack::new(4, &Item::TORCH));
    player.sync_inventory_to_client();
    let _ = session.run_until(Instant::now() + SETTLE, |session| {
        let survival = session.game().player().is_some_and(|player| player.gamemode == GameMode::Survival);
        survival && session.game().inventory().hotbar()[2].is_some_and(|item| item.count == 4)
    });

    let moves = [
        (SlotRef::Main(0), ClickButton::Left, false),
        (SlotRef::Main(27), ClickButton::Left, false),
        (SlotRef::Main(1), ClickButton::Right, false),
        (SlotRef::Main(28), ClickButton::Left, false),
        (SlotRef::Main(2), ClickButton::Left, false),
        (SlotRef::Main(27), ClickButton::Left, false),
        (SlotRef::Main(0), ClickButton::Left, false),
        (SlotRef::Main(0), ClickButton::Left, true),
    ];
    if open_inventory(session) {
        for (slot, button, shift) in moves {
            requests += inventory_move(session, |session| session.click_slot(slot, button, shift));
            compare_inventory(session, player, &mut problems);
        }
        close_inventory(session);
    } else {
        problems.push("the server didn't open the inventory in survival".to_string());
    }

    // Creative: a stack of stone from the list, put in the last hotbar slot, then
    // shift-clicked away.
    player.set_gamemode(GameMode::Creative);
    let _ = session.run_until(Instant::now() + SETTLE, |session| {
        session.game().player().is_some_and(|player| player.gamemode == GameMode::Creative)
    });
    let list = session.game().creative();
    let stone = (0..list.len()).find(|&index| list.entry(index).stack.id == BedrockItem::STONE.id);
    match stone {
        _ if list.is_empty() => problems.push("the server sent no creative list".to_string()),
        None => problems.push("the creative list has no stone".to_string()),
        Some(stone) => {
            if open_inventory(session) {
                requests += inventory_move(session, |session| session.click_creative(Some(stone)));
                requests += inventory_move(session, |session| session.click_slot(SlotRef::Main(8), ClickButton::Left, false));
                compare_inventory(session, player, &mut problems);
                let placed = session.game().inventory().hotbar()[8].map(|item| (item.id, item.count));
                if placed != Some((BedrockItem::STONE.id, 64)) {
                    problems.push(format!("the creative take put {placed:?} in the hotbar, not 64 stone"));
                }
                requests += inventory_move(session, |session| session.click_slot(SlotRef::Main(8), ClickButton::Left, true));
                compare_inventory(session, player, &mut problems);
                close_inventory(session);
            } else {
                problems.push("the server didn't open the inventory in creative".to_string());
            }
        }
    }
    player.set_gamemode(gamemode);
    let refused = session.game().inventory().refused();
    if refused > 0 {
        problems.push(format!("the server refused {refused} of the {requests} requests"));
    }
    Check::new(
        NAME,
        problems.is_empty(),
        if problems.is_empty() {
            format!(
                "opened the inventory in survival and creative, made the capture's moves in {requests} requests, \
                 and the server agreed to each and holds what the client shows"
            )
        } else {
            problems.join("; ")
        },
    )
}

/// Asks the server to open the inventory, and says whether it did.
fn open_inventory(session: &mut Session) -> bool {
    session.open_inventory();
    session
        .run_until(Instant::now() + SETTLE, |session| session.game().inventory().is_open())
        .is_ok()
}

/// Shuts the inventory, and waits for the server to say so and send the inventory again.
fn close_inventory(session: &mut Session) {
    session.close_inventory();
    for _ in 0..4 {
        session.tick();
        thread::sleep(TICK);
    }
}

/// One click, then a release, and the ticks until the server has answered. Says how
/// many requests that was.
fn inventory_move(session: &mut Session, click: impl FnOnce(&mut Session)) -> usize {
    let before = session.game().inventory().sent();
    click(session);
    session.pointer_released();
    let _ = session.run_until(Instant::now() + SETTLE, |session| session.game().inventory().waiting() == 0);
    // The inventory the server sends after an answer.
    session.tick();
    session.game().inventory().sent() - before
}

/// Notes each slot where what the client shows isn't what the server holds.
fn compare_inventory(session: &Session, player: &ServerPlayer, problems: &mut Vec<String>) {
    let shown = session.game().inventory().slots();
    for slot in 0..inventory::MAIN {
        let server = NetworkItemStackDescriptor::from(&player.inventory().get_slot(slot));
        let ours = shown.item(SlotRef::Main(slot as u8)).map(|item| (item.id, item.count));
        let theirs = (server.id != 0 && server.stack_size > 0).then_some((server.id, server.stack_size));
        if ours != theirs {
            problems.push(format!("slot {slot} shows {ours:?}, and the server has {theirs:?}"));
        }
    }
}

/// Compares every decoded chunk the server still has loaded with the
/// server's copy, block for block on both layers and biome for biome.
///
/// The server keeps ticking while we compare, and a block change (a sheep
/// eating grass, a mob lighting redstone ore) reaches us as an `UpdateBlock`
/// a tick or more later. A full pass takes several ticks, longer than a whole
/// second in a dev build, so some chunk will usually be caught mid-change.
/// So after the full pass, the chunks that differed are compared again each
/// tick for up to [`WORLD_SETTLE`]. Each chunk must still equal the server's
/// exactly at one moment, so a real decode error can never pass.
fn check_world(session: &mut Session, server: &EmbeddedServer) -> Check {
    const NAME: &str = "world matches server";
    let Some(level) = server.server.worlds.load().first().map(|world| world.level.clone()) else {
        return Check::new(NAME, false, "the server has no world".to_string());
    };
    // Compares one of our chunks: `None` if the server no longer has it,
    // otherwise how many values differ.
    let compare = |world: &World, (x, z): (i32, i32), examples: &mut Vec<String>| {
        let ours = world.chunk(x, z)?;
        let server_chunk = level
            .loaded_chunks
            .get(&Vector2::new(x, z))
            .map(|entry| entry.value().clone())?;
        Some(checks::compare_chunk(&server_chunk, ours, world.blocks(), examples, 10))
    };

    let start = Instant::now();
    let mut examples = Vec::new();
    let mut compared = 0usize;
    let mut differing = Vec::new();
    let positions: Vec<_> = session.world().chunks().map(|c| (c.x, c.z)).collect();
    for &pos in &positions {
        match compare(session.world(), pos, &mut examples) {
            Some(0) => compared += 1,
            Some(_) => {
                compared += 1;
                differing.push(pos);
            }
            None => {}
        }
    }
    let full_pass = start.elapsed().as_secs_f64();
    let mut first_differing = differing.len();
    debug!(
        "World comparison: {compared} chunks in {full_pass:.2} s, {first_differing} differ; \
         re-checking those"
    );
    for example in &examples {
        debug!("Full-pass difference: {example}");
    }

    let mut differ = 0;
    if !differing.is_empty() {
        let _ = session.run_until(Instant::now() + WORLD_SETTLE, |s| {
            examples.clear();
            differ = 0;
            differing.retain(|&pos| match compare(s.world(), pos, &mut examples) {
                Some(0) => false,
                None => {
                    // Unloaded by the server meanwhile: no longer comparable.
                    compared -= 1;
                    first_differing -= 1;
                    false
                }
                Some(n) => {
                    differ += n;
                    true
                }
            });
            differing.is_empty()
        });
    }
    // A chunk that still differs is either a decoder bug or a change the
    // server never sent. A fresh encode of the server's chunk tells them
    // apart: if our decoder reads it back equal to the server, the decoder is
    // right and the difference is drift. Retried on later ticks, since the
    // server can change the chunk between the encode and the comparison.
    let mut drifted = 0;
    let mut fresh_examples = Vec::new();
    differing.retain(|&(x, z)| {
        for _ in 0..FRESH_DECODE_ATTEMPTS {
            let Some(server_chunk) = level
                .loaded_chunks
                .get(&Vector2::new(x, z))
                .map(|entry| entry.value().clone())
            else {
                return true;
            };
            let mut attempt = Vec::new();
            if checks::fresh_decode_matches(&server_chunk, session.world().blocks(), &mut attempt) {
                drifted += 1;
                return false;
            }
            fresh_examples = attempt;
            session.tick();
            thread::sleep(TICK);
        }
        true
    });

    let took = start.elapsed().as_secs_f64();
    let not_loaded = positions.len() - compared;
    if differing.is_empty() && compared > 0 {
        let mut detail = format!(
            "{compared} chunks equal block for block on both layers and biome for biome: \
             {} on the full pass ({full_pass:.2} s), {} on a re-check",
            compared - first_differing,
            first_differing - drifted,
        );
        if drifted > 0 {
            for example in &examples {
                warn!("Changed by the server without an UpdateBlock: {example}");
            }
            detail += &format!(
                "; {drifted} hold {differ} values the server changed without sending an \
                 UpdateBlock (logged above), and a fresh encode of each decodes equal to the \
                 server"
            );
        }
        detail += &format!("; {not_loaded} no longer loaded on the server; {took:.2} s");
        return Check::new(NAME, true, detail);
    }
    for example in &examples {
        warn!("Difference: {example}");
    }
    for example in &fresh_examples {
        warn!("Difference in a fresh encode: {example}");
    }
    Check::new(
        NAME,
        false,
        format!(
            "{} of {compared} chunks still differ after {:.1} s of re-checks, {differ} values, \
             and a fresh encode doesn't decode equal to the server either (logged above); \
             {took:.2} s",
            differing.len(),
            WORLD_SETTLE.as_secs_f64(),
        ),
    )
}

/// Lights every chunk that has arrived, in slices with a tick between, so the server keeps
/// hearing from the host.
fn light_everything(session: &mut Session, around: (i32, i32)) {
    session.world_mut().update_light(Duration::ZERO, around);
    while session.world().light().waiting() > 0 {
        session.world_mut().update_light(LIGHT_SLICE, around);
        session.tick();
    }
    session.world_mut().update_light(Duration::ZERO, around);
}

/// Compares the client's light with the server's own light engine, level for level, on every chunk
/// whose 5 × 5 chunks are lit on our side, so its 3 × 3 has all its light (m5-plan Step 13).
///
/// Where they differ, each group of differences has to be explained: by a block where the server's
/// light breaks its own rules, or by a block that reads differently on the two sides
/// ([`checks::compare_light`]). The server keeps ticking, so a chunk with a group nothing explains
/// is compared again each tick for up to [`WORLD_SETTLE`], as the world check does.
fn check_light(session: &mut Session, server: &EmbeddedServer) -> Check {
    const NAME: &str = "light matches server";
    let Some(level) = server.server.worlds.load().first().map(|world| world.level.clone()) else {
        return Check::new(NAME, false, "the server has no world".to_string());
    };
    let eyes = session.position();
    let around = ((eyes.x.floor() as i32) >> 4, (eyes.z.floor() as i32) >> 4);
    let start = Instant::now();
    light_everything(session, around);
    let lit_in = start.elapsed().as_secs_f64();
    let lit = session.world().light().stats.lit;

    let world = session.world();
    let compared: Vec<(i32, i32)> = world
        .chunks()
        .map(|chunk| (chunk.x, chunk.z))
        .filter(|&(x, z)| (-2..=2).all(|dx| (-2..=2).all(|dz| world.is_lit(x + dx, z + dz))))
        .collect();
    let around_each = |chunks: &[(i32, i32)]| -> Vec<(i32, i32)> {
        chunks
            .iter()
            .flat_map(|&(x, z)| (-2..=2).flat_map(move |dx| (-2..=2).map(move |dz| (x + dx, z + dz))))
            .collect()
    };
    let compare_start = Instant::now();
    let server_light = checks::ServerLight::gather(&level, around_each(&compared));
    let mut report = checks::LightReport::default();
    for &chunk in &compared {
        checks::compare_light(world, &server_light, chunk, &mut report);
    }
    let full_pass = compare_start.elapsed().as_secs_f64();
    for example in &report.explained {
        debug!("Explained by the server's rules: {example}");
    }

    // The chunks that failed, again each tick: the server may have changed a block since.
    let mut failing = report.failing.clone();
    let first_failing = failing.len();
    let mut last = checks::LightReport::default();
    let deadline = Instant::now() + WORLD_SETTLE;
    while !failing.is_empty() && Instant::now() < deadline {
        session.tick();
        session.world_mut().update_light(Duration::ZERO, around);
        let server_light = checks::ServerLight::gather(&level, around_each(&failing));
        last = checks::LightReport::default();
        for &chunk in &failing {
            checks::compare_light(session.world(), &server_light, chunk, &mut last);
        }
        failing.clone_from(&last.failing);
        thread::sleep(TICK);
    }

    let groups = report.server_rule + report.blocks + report.ours_wrong + report.unexplained;
    let detail = format!(
        "{} chunks compared, {} levels; {} equal level for level; in the rest {} sky and {} block levels \
         differ, in {groups} groups: {} where the server's light breaks its own rules, {} where a block \
         reads differently; lit {lit} chunks in {lit_in:.1} s, compared in {full_pass:.1} s",
        report.chunks,
        report.levels,
        report.equal_chunks,
        report.differ[0],
        report.differ[1],
        report.server_rule,
        report.blocks,
    );
    if report.chunks > 0 && failing.is_empty() {
        let rechecked = if first_failing > 0 {
            format!("; {first_failing} chunks matched on a re-check")
        } else {
            String::new()
        };
        return Check::new(NAME, true, detail + &rechecked);
    }
    for failure in last.failures.iter().chain(&report.failures).take(10) {
        warn!("Light difference: {failure}");
    }
    Check::new(
        NAME,
        false,
        format!(
            "{detail}; {} chunks still hold a group nothing explains, or where our light breaks the \
             rules, after {:.1} s of re-checks (logged above)",
            failing.len(),
            WORLD_SETTLE.as_secs_f64()
        ),
    )
}

/// Keeps playing until the join's chunks stop arriving, so the checks and
/// measurements cover a whole join.
fn wait_for_chunks(session: &mut Session) -> u32 {
    let start = Instant::now();
    let mut seen = session.level_chunks();
    let mut last_new = Instant::now();
    let _ = session.run_until(Instant::now() + CHUNKS_MAX_WAIT, |s| {
        if s.level_chunks() != seen {
            seen = s.level_chunks();
            last_new = Instant::now();
        }
        last_new.elapsed() >= CHUNKS_QUIET
    });
    info!(
        "{seen} LevelChunk packets in this join (waited {:.1} s)",
        start.elapsed().as_secs_f64()
    );
    seen
}

/// Logs the per-packet table, then the chunk cost phases and the numbers the
/// "special-case chunks with `Arc`" decision needs, then runs the block
/// change probe.
fn measure(session: &mut Session, server: &EmbeddedServer, join_chunks: u32) {
    if cfg!(debug_assertions) {
        warn!("This is a debug build: the timings below are meaningless");
    }
    session.stats().log_table();
    let start_game_throughput = session
        .start_game()
        .map(|start_game| measure::start_game_throughput(start_game, 1000));
    match measure::chunk_costs(&server.server, 10, || session.tick()) {
        Some(costs) => {
            measure::log_costs(session.stats(), &costs, join_chunks, start_game_throughput);
        }
        None => warn!("No loaded chunks to measure"),
    }
    measure::block_change_probe(session, &server.server);
}

/// Stays in the world for `duration`, still sending input, so the process can
/// be inspected from outside (e.g. for open sockets).
fn linger(session: &mut Session, duration: Duration) {
    if duration.is_zero() {
        return;
    }
    info!(
        "Lingering for {} s; process id {}",
        duration.as_secs(),
        std::process::id()
    );
    let until = Instant::now() + duration;
    if let Err(err) = session.run_until(until + TICK, |_| Instant::now() >= until) {
        warn!("Session ended while lingering: {err}");
    }
}

/// Process-wide setup that Pumpkin's own `main` does before anything else.
fn set_up_process() {
    // Pumpkin's HTTP client is built without a TLS crypto provider, so the
    // process picks one before any client can be created.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Pumpkin reads saved chunks on Rayon's global pool, and the client makes
    // item icons on it: the tick, worldgen and chunk encoding have pools of their
    // own. Naming its threads makes them recognisable in a debugger or profiler.
    if let Err(err) = rayon::ThreadPoolBuilder::new()
        .thread_name(|i| format!("Rayon-Worker-{i}"))
        .build_global()
    {
        warn!("Couldn't configure the Rayon pool: {err}");
    }
}

/// Creates `dir` and makes it the process's working directory.
///
/// Pumpkin reads and writes its world and data files relative to the working
/// directory. That directory belongs to the whole process, which is why every
/// path the client holds was made absolute before this runs.
fn enter_data_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::env::set_current_dir(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(flags: &[&str]) -> Result<Args, String> {
        Args::parse(flags.iter().map(|flag| (*flag).to_string()))
    }

    #[test]
    fn time_is_a_time_of_day_in_ticks() {
        assert_eq!(parse(&["--time", "13000"]).unwrap().time, Some(13_000));
        assert_eq!(parse(&[]).unwrap().time, None);
        for bad in ["24000", "-1", "noon"] {
            assert!(parse(&["--time", bad]).is_err(), "{bad}");
        }
        assert!(parse(&["--time"]).is_err());
    }
}
