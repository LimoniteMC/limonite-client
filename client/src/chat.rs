//! What the server says in words: chat, its replies to commands, and its notices (m5-plan
//! Decision 15, Step 16).
//!
//! **Kept as the server said it, worded by the UI** (worth teaching). A `Text` packet can be a
//! finished line, or a key into the strings with parameters to put in, such as
//! `commands.time.set` and `6000`. Wording it needs the install's strings, which the UI holds,
//! and the game never reads (PLAN.md, UI). So the game keeps each message as it came, and when,
//! and the UI words it when it shows it (`ui/screens/command.rs`).

use std::{collections::VecDeque, time::Instant};

use pumpkin_protocol::bedrock::server::text::{SText, TextPacketType};

/// How many messages are kept: enough for the command line to look back over, which shows
/// ten.
const KEPT: usize = 100;

/// One message from the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Said {
    pub kind: TextPacketType,
    /// Who said it, for chat.
    pub source: String,
    /// The text, or the key of the strings to word it from.
    pub message: String,
    pub params: Vec<String>,
    /// Whether `message` is a key into the strings.
    pub translate: bool,
    pub at: Instant,
}

/// What the server has said, the latest last.
#[derive(Debug, Default)]
pub struct Chat {
    said: VecDeque<Said>,
    /// Messages ever received, so the UI can tell when there's a new one.
    received: u64,
}

impl Chat {
    /// Keeps what a `Text` packet says.
    pub fn receive(&mut self, text: &SText<'_>, at: Instant) {
        if self.said.len() == KEPT {
            self.said.pop_front();
        }
        self.said.push_back(Said {
            kind: text.r#type,
            source: text.source_name.to_string(),
            message: text.message.to_string(),
            params: text.parameters.iter().map(ToString::to_string).collect(),
            translate: text.needs_translation,
            at,
        });
        self.received += 1;
    }

    /// The messages kept, the oldest first.
    pub fn said(&self) -> impl DoubleEndedIterator<Item = &Said> + ExactSizeIterator {
        self.said.iter()
    }

    /// How many messages have arrived since the world opened.
    #[must_use]
    pub const fn received(&self) -> u64 {
        self.received
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    fn text(message: &str) -> SText<'static> {
        SText {
            needs_translation: false,
            r#type: TextPacketType::System,
            source_name: Cow::Borrowed(""),
            message: Cow::Owned(message.to_string()),
            parameters: Vec::new(),
            xuid: Cow::Borrowed(""),
            platform_chat_id: Cow::Borrowed(""),
            filtered_message: None,
        }
    }

    #[test]
    fn the_latest_hundred_are_kept_in_order() {
        let mut chat = Chat::default();
        let now = Instant::now();
        for n in 0..150 {
            chat.receive(&text(&n.to_string()), now);
        }
        assert_eq!(chat.received(), 150);
        assert_eq!(chat.said().len(), KEPT);
        assert_eq!(chat.said().next().map(|said| said.message.as_str()), Some("50"));
        assert_eq!(chat.said().last().map(|said| said.message.as_str()), Some("149"));
    }

    #[test]
    fn a_translation_keeps_its_key_and_parameters() {
        let mut chat = Chat::default();
        let mut set = SText::translation("commands.time.set".to_string(), vec!["6000".to_string()]);
        set.source_name = Cow::Borrowed("");
        chat.receive(&set, Instant::now());
        let said = chat.said().next().unwrap();
        assert!(said.translate);
        assert_eq!((said.message.as_str(), said.params.as_slice()), ("commands.time.set", &["6000".to_string()][..]));
    }
}
