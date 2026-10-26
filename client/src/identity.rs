//! Who this install logs in as (m5-plan Decision 16).
//!
//! The embedded server runs offline, so nobody checks the Xbox user id
//! (`xid`) the host's login carries, and the server derives the player's UUID
//! from it. That UUID is what a world remembers the player by: their
//! inventory, their position, whether they're an operator. So each install
//! makes a random `xid` once, keeps it in `identity.toml`, and is the same
//! player in every world from then on.

use std::{fmt::Write as _, fs, path::Path};

use toml::{Table, Value};
use tracing::info;

use crate::home::save_atomically;

/// How many digits an `xid` has, as Xbox user ids do.
const DIGITS: usize = 16;

/// This install's identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    xid: String,
}

impl Identity {
    /// Reads the identity at `path`, or makes one and saves it there if
    /// there's no file yet.
    ///
    /// # Errors
    /// If the file exists but can't be read or holds no valid `xid`, or a new
    /// one can't be saved. A damaged file isn't quietly replaced: every world
    /// would then forget the player. The message says how to put it right.
    pub fn load_or_create(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            let identity = Self::random();
            save_atomically(path, identity.to_toml().as_bytes())
                .map_err(|err| format!("Couldn't save a new identity to {}: {err}", path.display()))?;
            info!("Made this install's identity, saved in {}", path.display());
            return Ok(identity);
        }
        let damaged = |why: String| {
            format!(
                "{} is damaged: {why}. Fix it, or delete it to start as a new player; your worlds \
                 won't know the new one",
                path.display()
            )
        };
        let text = fs::read_to_string(path).map_err(|err| damaged(err.to_string()))?;
        let table: Table = text.parse().map_err(|err: toml::de::Error| damaged(err.to_string()))?;
        match table.get("xid") {
            Some(Value::String(xid)) if is_valid_xid(xid) => Ok(Self { xid: xid.clone() }),
            Some(other) => Err(damaged(format!("`xid = {other}` isn't {DIGITS} digits"))),
            None => Err(damaged("it has no `xid`".to_string())),
        }
    }

    /// The Xbox user id the host logs in with.
    #[must_use]
    pub fn xid(&self) -> &str {
        &self.xid
    }

    /// A new identity: 16 random digits, the first not a 0.
    fn random() -> Self {
        let xid: u64 = rand::random_range(1_000_000_000_000_000..10_000_000_000_000_000);
        Self { xid: xid.to_string() }
    }

    fn to_toml(&self) -> String {
        let mut out = String::from(
            "# Who this install plays as. Your worlds remember you by it, so keep this file.\n\
             # Deleting it makes a new identity, a stranger to your worlds.\n",
        );
        let _ = writeln!(out, "xid = {}", Value::String(self.xid.clone()));
        out
    }
}

fn is_valid_xid(xid: &str) -> bool {
    xid.len() == DIGITS && xid.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    #[test]
    fn an_identity_is_made_once_and_then_kept() {
        let dir = TempDir::new("identity");
        let path = dir.path().join("identity.toml");
        let made = Identity::load_or_create(&path).unwrap();
        assert!(is_valid_xid(made.xid()), "{}", made.xid());
        assert!(!made.xid().starts_with('0'));
        for _ in 0..3 {
            assert_eq!(Identity::load_or_create(&path).unwrap(), made);
        }
        let other = TempDir::new("identity-other");
        let another = Identity::load_or_create(&other.path().join("identity.toml")).unwrap();
        assert_ne!(another, made, "each install gets its own");
    }

    #[test]
    fn a_damaged_identity_stops_the_client_and_is_left_alone() {
        let dir = TempDir::new("identity-damaged");
        let path = dir.path().join("identity.toml");
        for (text, why) in [
            ("xid = \"12345\"\n", "`xid = \"12345\"` isn't 16 digits"),
            ("xid = 1000000000000001\n", "isn't 16 digits"),
            ("name = \"me\"\n", "it has no `xid`"),
            ("xid = \n", "TOML parse error"),
        ] {
            fs::write(&path, text).unwrap();
            let err = Identity::load_or_create(&path).unwrap_err();
            assert!(err.contains(why) && err.contains("delete it"), "{err}");
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
        }
    }

    #[test]
    fn a_hand_written_identity_is_used() {
        // M4's fixed development identity, which keeps M4's player data.
        let dir = TempDir::new("identity-m4");
        dir.write("identity.toml", b"xid = \"1000000000000001\"\n");
        let identity = Identity::load_or_create(&dir.path().join("identity.toml")).unwrap();
        assert_eq!(identity.xid(), "1000000000000001");
    }
}
