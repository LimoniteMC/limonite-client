mod app;
mod bench;
mod blocks;
mod checks;
mod clock;
mod debug_world;
mod edit;
mod embedded;
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
    io,
    path::{Path, PathBuf},
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};

use pumpkin_util::{
    GameMode,
    math::{vector2::Vector2, vector3::Vector3},
};
use pumpkin_world::{CURRENT_BEDROCK_MC_PROTOCOL, CURRENT_BEDROCK_MC_VERSION};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{EnvFilter, filter::LevelFilter};

use crate::{
    checks::Check,
    clock::TICK,
    embedded::{EmbeddedServer, HostOperator},
    home::Home,
    identity::Identity,
    session::{Session, Stage},
    settings::{MAX_RADIUS, MIN_RADIUS, Overrides, Settings},
    world::{BlockTable, World},
};

const USAGE: &str = "usage: limonite-client [--home <path>] [--bedrock <path>] [--java-jar <path>] [--data-dir <path>] \
                     [--debug-world] [--goto <text>] \
                     [--name <name>] [--gamemode <mode>] [--radius <chunks>] [--bench <seconds>]\n       \
                     limonite-client --headless [--home <path>] [--data-dir <path>] [--name <name>] \
                     [--gamemode <mode>] [--radius <chunks>] [--linger <seconds>] [--measure] \
                     [--map <path.png>] [--stream <blocks>]";

/// Login and spawn must finish within this.
const SPAWN_DEADLINE: Duration = Duration::from_secs(120);
/// The walk: 40 ticks at 0.2 blocks a tick is 8 blocks east.
const WALK_TICKS: u32 = 40;
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
    /// How long to stay in the world after the checks, still sending input,
    /// so the process can be inspected from outside.
    linger: Duration,
    /// Log what the `LocalChannel` cost (meaningful in release builds only).
    measure: bool,
    /// Where to write a top-down PNG map of the loaded chunks.
    map: Option<PathBuf>,
    /// After the join, fly this many blocks east and log how chunks stream.
    stream: Option<u32>,
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
        let mut linger = Duration::ZERO;
        let mut measure = false;
        let mut map = None;
        let mut stream = None;
        let mut bench = None;
        // The scripted session's flags, which mean nothing to the window.
        let mut headless_only = Vec::new();
        while let Some(arg) = args.next() {
            if matches!(arg.as_str(), "--linger" | "--measure" | "--map" | "--stream") {
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
            linger,
            measure,
            map: map.as_deref().map(absolute).transpose()?,
            stream,
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
    let assets = app::Assets {
        table: looks.table,
        textures: looks.textures,
        animator: looks.animator,
        tints,
        approximate,
        ui: ui_assets,
    };
    let browser = args.debug_world.then(|| {
        let layout = debug_world::Layout::build(&BlockTable::build());
        debug_world::place(&server, &layout);
        debug_world::Browser::new(layout, args.goto.as_deref())
    });
    let result = app::run(&server, settings, identity.xid(), needs, assets, args.bench, browser);
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
    EmbeddedServer::start(config).map_err(|err| {
        error!("{err}");
        ExitCode::FAILURE
    })
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
            if args.measure {
                measure(&mut session, server, join_chunks);
            }
            if let Some(blocks) = args.stream {
                measure::stream(&mut session, &server.server, blocks, join_chunks);
            }
            true
        }
        _ => {
            for name in ["movement", "operator", "teleport"] {
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
        linger(&mut session, args.linger);
    } else {
        checks.push(Check::new(
            "world matches server",
            false,
            "not checked: never spawned".to_string(),
        ));
    }
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
    checks::teleport(session.teleport_report(), eyes, asked.elapsed())
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

    // Pumpkin runs worldgen and chunk encoding on Rayon's global pool. Naming
    // its threads makes them recognisable in a debugger or profiler.
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
