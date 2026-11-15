//! The crash net: a small first process that runs the window as a child, and starts it again
//! on the world list if it crashes (m5-plan Decision 17, as Deviation 63 left it).
//!
//! **Why a second process** (worth teaching): a crash ends a process, and nothing inside it can
//! act on that any more. So the first process never opens a window or a server. It starts the
//! client again as a child, waits, and reads how the child ended from its exit status: 0 is the
//! player quitting, and anything else a crash, such as a panic, which Rust ends with 101, or the
//! system stopping it. Worlds no longer need a process each, so a child only ends to quit or
//! because it crashed.
//!
//! **A child that can't start** would be started again and again. So each child says when it's
//! up: it creates a file the supervisor names, once it has drawn its first frame. One that ends
//! before that died at startup. The first such death is treated as any crash, but a second in a
//! row stops the supervisor, with a message, rather than loop.
//!
//! [`next`] is the choice, as a pure function of how the children ended, so it's tested without
//! starting any.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use tracing::{error, info, warn};

/// Tells a child it's supervised, and which file to create once it's up.
pub const SUPERVISED: &str = "--supervised";
/// Tells a child the last one crashed, and how, for the world list to say.
pub const CRASHED: &str = "--crashed";

/// How a child ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ending {
    /// How it crashed, as the system words it, or `None` if it quit.
    pub crashed: Option<String>,
    /// Whether it had drawn a frame.
    pub was_up: bool,
}

/// What the supervisor does next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// The player quit, and so does the supervisor.
    Quit,
    /// A new child, on the world list, telling the player how the last one crashed.
    Restart { crashed: String },
    /// Two deaths at startup in a row: the client can't start, and starting it again won't
    /// help.
    GiveUp { message: String },
}

/// What follows the endings so far, the last one last.
#[must_use]
pub fn next(endings: &[Ending]) -> Next {
    let Some(last) = endings.last() else {
        return Next::Quit;
    };
    let Some(crashed) = &last.crashed else {
        return Next::Quit;
    };
    let before = endings.len().checked_sub(2).map(|index| &endings[index]);
    let died_at_startup = |ending: &Ending| ending.crashed.is_some() && !ending.was_up;
    if died_at_startup(last) && before.is_some_and(died_at_startup) {
        return Next::GiveUp {
            message: format!(
                "The client can't start: it stopped twice in a row before its window was up, the last time \
                 with {crashed}. What went wrong is in the log above."
            ),
        };
    }
    Next::Restart { crashed: crashed.clone() }
}

/// The next child's arguments: the player's own, then where to say it's up, then how the last
/// child crashed, if it did.
#[must_use]
pub fn child_args(args: &[String], ready: &Path, crashed: Option<&str>) -> Vec<String> {
    let mut child = args.to_vec();
    child.push(SUPERVISED.to_string());
    child.push(ready.display().to_string());
    if let Some(crashed) = crashed {
        child.push(CRASHED.to_string());
        child.push(crashed.to_string());
    }
    child
}

/// Where a child says it's up: a file in the system's temporary folder, named after this
/// process, so that two supervisors can't mix theirs up.
fn ready_file() -> PathBuf {
    std::env::temp_dir().join(format!("limonite-client-{}-up", std::process::id()))
}

/// Runs children until one quits, or the client can't start.
pub fn run(args: &[String]) -> ExitCode {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => {
            error!("Can't find this program's own file to start the client: {err}");
            return ExitCode::FAILURE;
        }
    };
    let ready = ready_file();
    let mut endings: Vec<Ending> = Vec::new();
    let mut crashed: Option<String> = None;
    loop {
        let _ = fs::remove_file(&ready);
        let child_args = child_args(args, &ready, crashed.as_deref());
        let status = Command::new(&exe).args(&child_args).status();
        let was_up = ready.exists();
        let _ = fs::remove_file(&ready);
        let status = match status {
            Ok(status) => status,
            Err(err) => {
                error!("Couldn't start the client: {err}");
                return ExitCode::FAILURE;
            }
        };
        let ending = Ending {
            crashed: (!status.success()).then(|| status.to_string()),
            was_up,
        };
        endings.push(ending);
        match next(&endings) {
            Next::Quit => return ExitCode::SUCCESS,
            Next::Restart { crashed: how } => {
                warn!("The client stopped unexpectedly ({how}); starting it again on the world list");
                crashed = Some(how);
            }
            Next::GiveUp { message } => {
                error!("{message}");
                return ExitCode::FAILURE;
            }
        }
        info!("Supervisor: starting child {}", endings.len() + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quit(was_up: bool) -> Ending {
        Ending { crashed: None, was_up }
    }

    fn crash(was_up: bool) -> Ending {
        Ending {
            crashed: Some("exit code: 101".to_string()),
            was_up,
        }
    }

    #[test]
    fn a_quit_ends_it() {
        assert_eq!(next(&[quit(true)]), Next::Quit);
        assert_eq!(next(&[crash(true), quit(true)]), Next::Quit, "after a crash too");
        assert_eq!(next(&[]), Next::Quit);
    }

    #[test]
    fn a_crash_goes_back_to_the_list_with_an_error() {
        let restart = Next::Restart { crashed: "exit code: 101".to_string() };
        assert_eq!(next(&[crash(true)]), restart);
        assert_eq!(next(&[crash(true), crash(true), crash(true)]), restart, "a crash in play always does");
    }

    #[test]
    fn one_death_at_startup_is_a_crash_and_two_in_a_row_stop_it() {
        assert!(matches!(next(&[crash(false)]), Next::Restart { .. }));
        assert!(matches!(next(&[crash(false), crash(false)]), Next::GiveUp { .. }));
        assert!(matches!(next(&[crash(true), crash(false), crash(false)]), Next::GiveUp { .. }));
    }

    #[test]
    fn a_death_at_startup_after_a_good_run_is_only_a_crash() {
        assert!(matches!(next(&[crash(true), crash(false)]), Next::Restart { .. }));
        assert!(matches!(next(&[crash(false), crash(true), crash(false)]), Next::Restart { .. }));
    }

    #[test]
    fn the_message_says_how_it_ended() {
        let Next::GiveUp { message } = next(&[crash(false), crash(false)]) else {
            panic!("it gives up");
        };
        assert!(message.contains("exit code: 101"), "{message}");
    }

    #[test]
    fn a_child_gets_the_players_arguments_then_its_own() {
        let args = ["--home".to_string(), "run/home".to_string()];
        let ready = Path::new("up");
        assert_eq!(child_args(&args, ready, None), ["--home", "run/home", "--supervised", "up"]);
        assert_eq!(
            child_args(&args, ready, Some("exit code: 101")),
            ["--home", "run/home", "--supervised", "up", "--crashed", "exit code: 101"]
        );
        assert_eq!(child_args(&[], ready, None), ["--supervised", "up"]);
    }
}
