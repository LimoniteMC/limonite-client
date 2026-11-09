//! The Pumpkin server the client embeds, running on its own thread.
//!
//! The client's main thread never runs async code. The server gets a
//! dedicated `Pumpkin-Main` thread with its own multi-threaded tokio runtime,
//! just as Pumpkin's own `main` would run it, and the two sides talk only
//! through a `LocalChannel` and a few shared handles.

use std::{fs, io, num::NonZero, path::Path, sync::Arc, sync::mpsc, thread, time::Duration};

use pumpkin_config::{PumpkinConfig, op::Op};
use pumpkin_core::{
    PumpkinServer,
    data::{VanillaData, op::OperatorConfig},
    net::bedrock::local::LocalConnector,
    server::Server,
};
use pumpkin_data::{item::Item, item_stack::ItemStack};
use pumpkin_util::PermissionLvl;
use tokio::runtime::{Handle, Runtime};
use tracing::{error, info};

use crate::{home::save_atomically, inventory::HOTBAR, login, settings::Settings};

/// How long shutdown waits for the runtime's leftover tasks after the world is saved.
const RUNTIME_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
/// Where Pumpkin keeps a world's operators, under its data directory.
const OPERATORS: &str = "data/ops.json";

/// The embedded server's configuration, built in code: no config file is read
/// or written.
///
/// Everything that would open a socket or reach the internet is off. The only
/// way in is the in-process `LocalChannel`.
pub fn config(settings: &Settings) -> PumpkinConfig {
    let mut config = PumpkinConfig::default();
    // Only for players without saved data; a saved host keeps its mode.
    config.basic.default_gamemode = settings.gamemode;
    // Bedrock worlds have no spawn protection. Pumpkin's default, 16 blocks,
    // would stop anyone who isn't an operator building near spawn.
    config.basic.spawn_protection = 0;
    let networking = &mut config.advanced.networking;
    // No UDP or TCP 19132, and no LAN discovery on 7551.
    networking.bedrock.nethernet.enabled = false;
    // No Xbox OIDC key fetch at startup, and the host's unsigned token is accepted.
    networking.bedrock.online_mode = false;
    // Already compiled out (no `java-listener` feature); this keeps the config honest.
    networking.java.enabled = false;
    // The server clamps the radius a Bedrock client asks for to this. Its
    // default, 16, would cap `--radius`; 32 lets M3 measure radius 32.
    networking.bedrock.view_distance = NonZero::new(32).unwrap_or(NonZero::<u8>::MIN);
    // Pumpkin's default sends a heartbeat to market.pumpkinmc.org.
    config.telemetry.enabled = false;
    // Pumpkin's default asks api.github.com for its latest release at every start,
    // and tells operators in chat when there's a newer one.
    config.advanced.update_checker.enabled = false;
    config.advanced.plugins.enabled = false;
    // The client owns the terminal; Pumpkin must not read stdin.
    config.advanced.commands.use_console = false;
    config.advanced.commands.use_tty = false;
    config
}

/// Whether the host had to be added to the operators.
#[derive(Debug, PartialEq, Eq)]
pub enum HostOperator {
    /// Added at level 4.
    Added,
    /// Already listed, at this level. That's left alone: the player may have
    /// chosen it with `/op` or `/deop`.
    Listed(PermissionLvl),
}

/// Makes the host an operator of the world in `data_dir` at level 4: a world
/// owner's power on Bedrock with cheats on, every command (m5-plan
/// Decision 18).
///
/// It must run before the server starts, which reads `data/ops.json` once,
/// at startup. The entry is Pumpkin's own `Op`, under the UUID and the name
/// the server will give the host, so the file stays in Pumpkin's format.
///
/// # Errors
/// If the list can't be read, isn't Pumpkin's format, or can't be written.
/// The file is then left as it was.
pub fn make_host_operator(
    data_dir: &Path,
    config: &PumpkinConfig,
    name: &str,
    xid: &str,
) -> Result<HostOperator, String> {
    let uuid = login::host_uuid(name, xid)?;
    let path = data_dir.join(OPERATORS);
    let mut operators = match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<OperatorConfig>(&text)
            .map_err(|err| format!("{} isn't a list of operators: {err}", path.display()))?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => OperatorConfig::default(),
        Err(err) => return Err(format!("can't read {}: {err}", path.display())),
    };
    if let Some(entry) = operators.get_entry(&uuid) {
        return Ok(HostOperator::Listed(entry.level));
    }
    operators
        .ops
        .push(Op::new(uuid, server_name(config, name), PermissionLvl::Four, false));
    let json = serde_json::to_string_pretty(&operators).map_err(|err| format!("can't write the operators: {err}"))?;
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).map_err(|err| format!("can't create {}: {err}", folder.display()))?;
    }
    save_atomically(&path, json.as_bytes()).map_err(|err| format!("can't write {}: {err}", path.display()))?;
    Ok(HostOperator::Added)
}

/// The name the server will know the host by, built as Pumpkin's Bedrock
/// login builds it: the configured prefix, then the name with its spaces made
/// underscores if the config says so. `/deop <name>` finds an operator who
/// isn't online by it.
fn server_name(config: &PumpkinConfig, name: &str) -> String {
    let bedrock = &config.advanced.networking.bedrock;
    let name = if bedrock.replace_username_spaces {
        name.replace(' ', "_")
    } else {
        name.to_string()
    };
    format!("{}{name}", bedrock.username_prefix)
}

/// Reads what to put in the hotbar: item names separated by commas, each with `*` and a
/// count if it isn't 1. It's `--hotbar`, for getting blocks until there's an inventory
/// screen, and the screenshots' `LIMONITE_SHOTS_HOTBAR`.
///
/// # Errors
/// If a name isn't an item's, a count isn't 1 to 64, or a hotbar can't hold them all.
pub fn parse_hotbar(list: &str) -> Result<Vec<(&'static Item, u8)>, String> {
    let stacks = list
        .split(',')
        .map(|entry| {
            let (name, count) = entry.split_once('*').unwrap_or((entry, "1"));
            let name = name.trim();
            let item = Item::from_registry_key(name).ok_or(format!("no item called `{name}`"))?;
            let count = count
                .parse()
                .ok()
                .filter(|count| (1..=64).contains(count))
                .ok_or(format!("`{count}` isn't a count from 1 to 64"))?;
            Ok((item, count))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if stacks.len() > HOTBAR {
        return Err(format!("that's {} stacks, and a hotbar holds {HOTBAR}", stacks.len()));
    }
    Ok(stacks)
}

/// A running embedded server.
pub struct EmbeddedServer {
    /// Opens in-process connections to the server.
    pub connector: LocalConnector,
    /// The server's own state, shared with its threads. Read-only use from the
    /// client: checks and measurements.
    pub server: Arc<Server>,
    /// For waiting on the server's async calls from the client's thread.
    pub runtime: Handle,
    thread: thread::JoinHandle<()>,
}

/// What the server thread sends back once the server exists.
type Ready = Result<(LocalConnector, Arc<Server>, Handle), String>;

impl EmbeddedServer {
    /// Starts the server and waits until it can accept connections.
    ///
    /// Pumpkin resolves its files (world, `ops.json`, ...) from the working
    /// directory, so the caller must have changed into the data directory first.
    ///
    /// # Errors
    /// If the thread or runtime can't be created, or the world fails to load.
    pub fn start(config: PumpkinConfig) -> Result<Self, String> {
        let (ready_tx, ready_rx) = mpsc::channel::<Ready>();
        let thread = thread::Builder::new()
            .name("Pumpkin-Main".into())
            .spawn(move || run_server(config, &ready_tx))
            .map_err(|err| format!("failed to spawn the server thread: {err}"))?;

        // `recv` fails only if the thread ended without reporting, e.g. a panic.
        match ready_rx.recv() {
            Ok(Ok((connector, server, runtime))) => Ok(Self {
                connector,
                server,
                runtime,
                thread,
            }),
            Ok(Err(err)) => {
                let _ = thread.join();
                Err(err)
            }
            Err(_) => {
                let _ = thread.join();
                Err("the server thread ended before the server started".to_string())
            }
        }
    }

    /// Puts these in the host's hotbar, from the first slot, through the server. They
    /// reach the client as they do in play, as `InventorySlot`s.
    pub fn fill_hotbar(&self, stacks: &[(&'static Item, u8)]) {
        let _runtime = self.runtime.enter();
        let players = self.server.get_all_players();
        let Some(player) = players.first() else {
            return;
        };
        for (slot, &(item, count)) in (0_i16..).zip(stacks) {
            player.inventory().insert_stack(slot, &mut ItemStack::new(count, item));
        }
        player.sync_inventory_to_client();
    }

    /// Sets every world's time of day, in ticks: 0 is sunrise, 6,000 noon, 12,000 sunset
    /// and 18,000 midnight. It reaches clients as a `SetTime`, and the world keeps it
    /// when it saves. With `stand_still`, the day stops there, as a picture needs.
    pub fn set_time(&self, time: i64, stand_still: bool) {
        let _runtime = self.runtime.enter();
        for world in self.server.worlds.load().iter() {
            let mut level_time = world.level_time.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            level_time.time_of_day = time;
            level_time.paused = stand_still;
            level_time.send_time(world);
        }
    }

    /// Stops the server, which kicks any players and saves the world, and waits
    /// for its thread to finish.
    ///
    /// Only this server stops: its own stop token is cancelled, and Pumpkin's
    /// process-wide stop flags are left alone, since an embedded server doesn't
    /// own the process.
    pub fn stop(self) {
        self.server.stop();
        if self.thread.join().is_err() {
            error!("The embedded server thread panicked while stopping");
        }
    }
}

/// The body of the `Pumpkin-Main` thread.
fn run_server(config: PumpkinConfig, ready: &mpsc::Sender<Ready>) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("Pumpkin-Tokio")
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = ready.send(Err(format!("failed to build the server runtime: {err}")));
            return;
        }
    };
    runtime.block_on(serve(config, ready));
    shut_down(runtime);
}

async fn serve(config: PumpkinConfig, ready: &mpsc::Sender<Ready>) {
    // Pumpkin's `init_logger` is never called: the client already installed
    // the global `tracing` subscriber, and Pumpkin's events reach it.
    let vanilla_data = VanillaData::load();
    // Not standalone: the server reads no console, never marks the process as
    // stopping, and ends only its own world when stopped.
    let standalone = false;
    let server = match PumpkinServer::new(
        config.basic,
        config.advanced,
        config.telemetry,
        vanilla_data,
        Vec::new(),
        standalone,
    )
    .await
    {
        Ok(server) => server,
        Err(err) => {
            let _ = ready.send(Err(format!("failed to start the embedded server: {err}")));
            return;
        }
    };

    let started = (server.local_listener.connector(), server.server.clone(), Handle::current());
    if ready.send(Ok(started)).is_err() {
        return;
    }
    info!("Embedded server started");
    server.start().await;
    info!("Embedded server stopped");
}

fn shut_down(runtime: Runtime) {
    // Tasks still parked on the runtime (e.g. timers) are dropped; the world
    // has already been saved by `start` returning.
    runtime.shutdown_timeout(RUNTIME_SHUTDOWN_TIMEOUT);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    const XID: &str = "1000000000000001";

    fn operators(dir: &TempDir) -> OperatorConfig {
        serde_json::from_str(&fs::read_to_string(dir.path().join(OPERATORS)).unwrap()).unwrap()
    }

    #[test]
    fn the_host_is_added_once_and_then_kept() {
        let dir = TempDir::new("ops-added");
        let config = config(&Settings::default());
        assert_eq!(
            make_host_operator(dir.path(), &config, "Limonite", XID),
            Ok(HostOperator::Added)
        );
        let list = operators(&dir);
        assert_eq!(list.ops.len(), 1);
        let entry = &list.ops[0];
        assert_eq!(entry.uuid, login::host_uuid("Limonite", XID).unwrap());
        assert_eq!((entry.name.as_str(), entry.level), ("Limonite", PermissionLvl::Four));
        assert!(!entry.bypasses_player_limit);

        // Again, and after the player lowered their own level: kept as is.
        let first = fs::read_to_string(dir.path().join(OPERATORS)).unwrap();
        assert_eq!(
            make_host_operator(dir.path(), &config, "Limonite", XID),
            Ok(HostOperator::Listed(PermissionLvl::Four))
        );
        let lowered = first.replace("\"level\": 4", "\"level\": 2");
        assert_ne!(lowered, first);
        fs::write(dir.path().join(OPERATORS), &lowered).unwrap();
        assert_eq!(
            make_host_operator(dir.path(), &config, "Limonite", XID),
            Ok(HostOperator::Listed(PermissionLvl::Two))
        );
        assert_eq!(fs::read_to_string(dir.path().join(OPERATORS)).unwrap(), lowered);
    }

    #[test]
    fn other_operators_stay() {
        let dir = TempDir::new("ops-others");
        dir.write(
            OPERATORS,
            br#"[{"uuid":"00000000-0000-0000-0000-000000000001","name":"Friend","level":3,"bypassesPlayerLimit":true}]"#,
        );
        let config = config(&Settings::default());
        assert_eq!(
            make_host_operator(dir.path(), &config, "Mr Q", XID),
            Ok(HostOperator::Added)
        );
        let list = operators(&dir);
        let names: Vec<_> = list.ops.iter().map(|op| (op.name.as_str(), op.level)).collect();
        // Spaces become underscores, as in the name the server gives the host.
        assert_eq!(names, [("Friend", PermissionLvl::Three), ("Mr_Q", PermissionLvl::Four)]);
        assert!(list.ops[0].bypasses_player_limit);
    }

    #[test]
    fn a_damaged_list_is_left_alone() {
        let dir = TempDir::new("ops-damaged");
        dir.write(OPERATORS, b"not json");
        let err = make_host_operator(dir.path(), &config(&Settings::default()), "Limonite", XID).unwrap_err();
        assert!(err.contains("isn't a list of operators"), "{err}");
        assert_eq!(fs::read(dir.path().join(OPERATORS)).unwrap(), b"not json");
    }

    #[test]
    fn a_hotbar_list_names_items_with_their_counts() {
        let stacks = parse_hotbar("dirt*64, oak_log*12,torch").unwrap();
        let stacks: Vec<_> = stacks.iter().map(|&(item, count)| (item.id, count)).collect();
        assert_eq!(stacks, [(Item::DIRT.id, 64), (Item::OAK_LOG.id, 12), (Item::TORCH.id, 1)]);
    }

    #[test]
    fn a_bad_hotbar_list_says_what_is_wrong() {
        let ten = ["dirt"; 10].join(",");
        for (list, wrong) in [
            ("dirt,cheese", "no item called `cheese`"),
            ("dirt*0", "`0` isn't a count"),
            ("dirt*65", "`65` isn't a count"),
            ("dirt*many", "`many` isn't a count"),
            (ten.as_str(), "that's 10 stacks"),
        ] {
            let Err(err) = parse_hotbar(list) else {
                panic!("`{list}` should have failed");
            };
            assert!(err.contains(wrong), "{err}");
        }
        assert!(parse_hotbar(&["dirt"; HOTBAR].join(",")).is_ok());
    }

    #[test]
    fn spawn_protection_is_off() {
        assert_eq!(config(&Settings::default()).basic.spawn_protection, 0);
    }

    /// `config`'s promise: nothing opens a socket or reaches the internet. Some of these
    /// are off by Pumpkin's own default, and are checked so that an upstream sync that
    /// turns one on fails here. One that adds a new way out, as the update checker did
    /// (2026-11-10), needs a line in `config` and one here.
    #[test]
    fn nothing_opens_a_socket_or_reaches_the_internet() {
        let config = config(&Settings::default());
        let networking = &config.advanced.networking;
        assert!(!networking.bedrock.nethernet.enabled);
        assert!(!networking.bedrock.online_mode);
        assert!(!networking.java.enabled);
        assert!(!networking.query.enabled);
        assert!(!networking.management.enabled);
        assert!(!networking.proxy.enabled);
        assert!(!networking.lan_broadcast.enabled);
        assert!(!config.telemetry.enabled);
        assert!(!config.advanced.update_checker.enabled);
        assert!(!config.advanced.plugins.enabled);
    }
}
