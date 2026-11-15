//! The command line, and the server's words over the hotbar (m5-plan Decision 15, Step 16).
//!
//! **T opens it to chat, and / with the slash typed** (Deviation 13). Enter sends what's typed:
//! a command if it starts with `/`, and chat otherwise. Escape shuts it. Up and Down go back
//! through what was sent, as Java's do.
//!
//! **The server's words** show at the bottom left, over the hotbar: ten lines at most, the
//! latest at the bottom, each for ten seconds, as Java's chat does. While the command line is
//! open the last ten show, whatever their age. A message is worded from the install's strings
//! as it arrives (`chat.rs`), coloured by its codes and wrapped to the lines' width
//! (`styled.rs`).

use std::time::{Duration, Instant};

use pumpkin_protocol::bedrock::server::text::TextPacketType;

use crate::{
    chat::{Chat, Said},
    ui::{
        atlas::Atlas,
        font::Font,
        lang::Lang,
        layout::{Anchor, Arrange, Axis, Layout, Size},
        screens::worlds::Key,
        styled::{self, Span},
        text_field::{Edit, FieldSprites, TextField},
        tree::{Node, NodeId, Tree},
    },
};

const BACK: &str = "textures/ui/Black";

/// The pictures the screen asks for, besides its field's.
pub const SPRITES: [&str; 1] = [BACK];

/// How many lines show, how tall each is, and how wide.
const LINES: usize = 10;
const LINE: f32 = 9.0;
const WIDTH: f32 = 300.0;
/// How many colours a line can hold.
const SPANS: usize = 8;
/// How far up the screen the lowest line ends, so the lines clear the hotbar and the hearts.
const LINES_UP: f32 = 48.0;
/// How long a line shows, and how dark the black behind the lines and the field is.
const SHOWN_FOR: Duration = Duration::from_secs(10);
const BACK_ALPHA: u8 = 128;
/// The field: how tall, and how far from the bottom.
const FIELD_HEIGHT: f32 = 12.0;
const FIELD_UP: f32 = 2.0;
/// The most characters to type, and how far back Up goes.
const MAX_TYPED: usize = 256;
const HISTORY: usize = 100;
/// The text's colour, before its codes change it.
const TEXT: [u8; 4] = [255; 4];

/// What the command line asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandChoice {
    /// Send this, which a `/` makes a command.
    Send(String),
    /// Shut it, sending nothing.
    Close,
}

/// A line's nodes.
struct LineNodes {
    root: NodeId,
    labels: [NodeId; SPANS],
}

/// The screen's nodes, and its own state.
pub struct CommandScreen {
    root: NodeId,
    lines: Vec<LineNodes>,
    bar: NodeId,
    field: TextField,
    fields: FieldSprites,
    open: bool,
    /// Every kept message's lines, the latest last, each with when its message came.
    wrapped: Vec<(Vec<Span>, Instant)>,
    /// How many messages had arrived when `wrapped` was last made.
    worded: u64,
    /// What was sent, the latest last, and where Up and Down have got to in it.
    history: Vec<String>,
    browsing: Option<usize>,
}

impl CommandScreen {
    /// Builds the screen, hidden.
    pub fn build(tree: &mut Tree, atlas: &Atlas) -> Self {
        let back = atlas.sprite(BACK);
        let fields = FieldSprites::find(atlas);
        let root = tree.add(Tree::ROOT, Node::panel(Layout::default()).hidden());
        let lines = (0..LINES)
            .map(|index| {
                let y = -(LINES_UP + index as f32 * LINE);
                let layout = Layout::px(WIDTH, LINE).at(Anchor::BottomLeft).moved(2.0, y);
                let line = tree.add(root, Node::panel(layout).hidden());
                tree.add(line, Node::image(Layout::default(), back).faded(BACK_ALPHA));
                let stack = Layout::content().at(Anchor::LeftMiddle).moved(1.0, 0.0).arranged(Arrange::Stack(Axis::X));
                let stack = tree.add(line, Node::panel(stack));
                let labels = std::array::from_fn(|_| tree.add(stack, Node::label(Layout::content(), "").hidden()));
                LineNodes { root: line, labels }
            })
            .collect();
        let bar_layout = Layout {
            size: [Size::Percent(100.0), Size::Px(FIELD_HEIGHT)],
            ..Layout::default().at(Anchor::BottomMiddle).moved(0.0, -FIELD_UP)
        };
        let bar = tree.add(root, Node::image(bar_layout, back).faded(BACK_ALPHA).hidden());
        let field = TextField::add(tree, bar, Layout::default(), MAX_TYPED, &fields, false);
        Self {
            root,
            lines,
            bar,
            field,
            fields,
            open: false,
            wrapped: Vec::new(),
            worded: 0,
            history: Vec::new(),
            browsing: None,
        }
    }

    pub fn set_visible(&self, tree: &mut Tree, visible: bool) {
        tree.set_visible(self.root, visible);
    }

    /// Opens it with `typed` in the field, or shuts it.
    pub fn set_open(&mut self, tree: &mut Tree, open: bool, typed: &str, now: Instant) {
        self.open = open;
        tree.set_visible(self.bar, open);
        self.browsing = None;
        self.field.set(if open { typed } else { "" }, now);
    }

    /// Brings it up to date: the messages, worded and wrapped when new ones have come, the
    /// lines that show now, and the field.
    pub fn update(&mut self, tree: &mut Tree, font: &Font, lang: &Lang, chat: &Chat, screen_width: f32, now: Instant) {
        if chat.received() != self.worded {
            self.worded = chat.received();
            self.wrapped = chat
                .said()
                .flat_map(|said| {
                    let spans = styled::spans(&worded(said, lang), TEXT);
                    styled::wrap(font, &spans, WIDTH - 2.0).into_iter().map(move |line| (line, said.at))
                })
                .collect();
        }
        let showing = self.wrapped.iter().rev().take(LINES);
        let mut count = 0;
        for (nodes, (spans, at)) in self.lines.iter().zip(showing) {
            let fresh = now.saturating_duration_since(*at) < SHOWN_FOR;
            tree.set_visible(nodes.root, self.open || fresh);
            for (index, &label) in nodes.labels.iter().enumerate() {
                let span = spans.get(index);
                tree.set_visible(label, span.is_some());
                if let Some((text, colour)) = span {
                    tree.set_text(label, text);
                    tree.set_color(label, *colour);
                }
            }
            count += 1;
        }
        for nodes in &self.lines[count..] {
            tree.set_visible(nodes.root, false);
        }
        if self.open {
            self.field.show(tree, font, &self.fields, false, true, now);
            self.field.fit(tree, font, screen_width - 8.0);
        }
    }

    /// A key went down while it's open.
    pub fn key(&mut self, key: Key, now: Instant) -> Option<CommandChoice> {
        match key {
            Key::Enter => {
                let typed = self.field.editor.text().to_string();
                if !typed.trim().is_empty() {
                    if self.history.len() == HISTORY {
                        self.history.remove(0);
                    }
                    self.history.push(typed.clone());
                }
                return Some(CommandChoice::Send(typed));
            }
            Key::Escape => return Some(CommandChoice::Close),
            Key::Up => self.browse(true, now),
            Key::Down => self.browse(false, now),
            Key::Edit(edit) => {
                self.field.apply(&edit.edit(), now);
            }
            Key::Tab => {}
        }
        None
    }

    /// Up goes back through what was sent, and Down forward again, to an empty field past
    /// the latest.
    fn browse(&mut self, back: bool, now: Instant) {
        if self.history.is_empty() {
            return;
        }
        let latest = self.history.len() - 1;
        self.browsing = match (self.browsing, back) {
            (None, true) => Some(latest),
            (None, false) => None,
            (Some(at), true) => Some(at.saturating_sub(1)),
            (Some(at), false) if at < latest => Some(at + 1),
            (Some(_), false) => None,
        };
        let text = self.browsing.map_or("", |at| self.history[at].as_str()).to_string();
        self.field.set(&text, now);
    }

    /// Text was typed while it's open.
    pub fn typed(&mut self, text: &str, now: Instant) {
        self.field.apply(&Edit::Insert(text.to_string()), now);
    }

    /// The field's text, for tests.
    #[cfg(test)]
    fn typed_text(&self) -> &str {
        self.field.editor.text()
    }

    /// How many lines show now, for tests.
    #[cfg(test)]
    fn shown(&self, tree: &Tree) -> usize {
        self.lines.iter().filter(|nodes| tree.node(nodes.root).visible).count()
    }

    /// A line's first span's text, for tests: 0 is the latest.
    #[cfg(test)]
    fn line_text(&self, tree: &Tree, index: usize) -> String {
        let nodes = &self.lines[index];
        nodes
            .labels
            .iter()
            .filter(|&&label| tree.node(label).visible)
            .map(|&label| match &tree.node(label).kind {
                crate::ui::tree::Kind::Label(label) => label.text.clone(),
                _ => String::new(),
            })
            .collect()
    }

}

/// What a message says, worded from the strings where it's a key into them: chat as
/// `<name> text`, an announcement as `[name] text`, a translation from its key and parameters,
/// a JSON message from its `rawtext`, and anything else as it came.
#[must_use]
pub fn worded(said: &Said, lang: &Lang) -> String {
    let params: Vec<&str> = said.params.iter().map(String::as_str).collect();
    match said.kind {
        TextPacketType::Chat | TextPacketType::Whisper => lang.format("chat.type.text", &[&said.source, &said.message]),
        TextPacketType::Announcement => lang.format("chat.type.announcement", &[&said.source, &said.message]),
        TextPacketType::Json | TextPacketType::JsonWhisper | TextPacketType::JsonAnnouncement => {
            raw_text(&said.message, lang).unwrap_or_else(|| said.message.clone())
        }
        _ if said.translate => translated(&said.message, &params, lang),
        _ => said.message.clone(),
    }
}

/// A translation: a key, which may follow colour codes and a `%`, as in
/// `§e%multiplayer.player.joined`, with its parameters put in. The codes stay in front.
fn translated(message: &str, params: &[&str], lang: &Lang) -> String {
    // Each code is a `§` and the character after it.
    let mut rest = message;
    while let Some(after) = rest.strip_prefix('\u{a7}') {
        let mut chars = after.chars();
        chars.next();
        rest = chars.as_str();
    }
    let prefix = &message[..message.len() - rest.len()];
    format!("{prefix}{}", lang.format(rest.strip_prefix('%').unwrap_or(rest), params))
}

/// A JSON message's words: its `rawtext`'s `text` parts, and its `translate` parts worded with
/// their `with`. `None` if it isn't such JSON.
fn raw_text(message: &str, lang: &Lang) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(message).ok()?;
    let parts = value.get("rawtext")?.as_array()?;
    let mut out = String::new();
    for part in parts {
        if let Some(text) = part.get("text").and_then(serde_json::Value::as_str) {
            out.push_str(text);
        } else if let Some(key) = part.get("translate").and_then(serde_json::Value::as_str) {
            let with: Vec<String> = match part.get("with") {
                Some(serde_json::Value::Array(items)) => {
                    items.iter().filter_map(serde_json::Value::as_str).map(str::to_string).collect()
                }
                Some(nested) => raw_text(&nested.to_string(), lang).into_iter().collect(),
                None => Vec::new(),
            };
            let with: Vec<&str> = with.iter().map(String::as_str).collect();
            out.push_str(&lang.format(key, &with));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use pumpkin_protocol::bedrock::server::text::SText;

    use super::*;
    use crate::ui::{
        atlas::SpriteId,
        layout::{self, Measure},
    };

    struct Fake;

    impl Measure for Fake {
        fn image(&self, _: SpriteId) -> [f32; 2] {
            [0.0, 0.0]
        }

        fn text(&self, text: &str) -> [f32; 2] {
            [text.chars().count() as f32 * 4.0, 8.0]
        }
    }

    fn said(kind: TextPacketType, source: &str, message: &str, params: &[&str], translate: bool) -> Said {
        Said {
            kind,
            source: source.to_string(),
            message: message.to_string(),
            params: params.iter().map(|param| (*param).to_string()).collect(),
            translate,
            at: Instant::now(),
        }
    }

    fn lang() -> Lang {
        let mut lang = Lang::default();
        lang.add(
            "chat.type.text=<%s> %s\n\
             commands.time.set=Set the time to %1$d\n\
             multiplayer.player.joined=%s joined the game\n\
             entity.cow.name=Cow\n\
             commands.kill.successful=Killed %1$s",
        );
        lang
    }

    #[test]
    fn messages_are_worded_from_the_strings() {
        let lang = lang();
        assert_eq!(worded(&said(TextPacketType::Chat, "Steve", "hi", &[], false), &lang), "<Steve> hi");
        let set = said(TextPacketType::Translation, "", "commands.time.set", &["6000"], true);
        assert_eq!(worded(&set, &lang), "Set the time to 6000");
        let joined = said(TextPacketType::Translation, "", "\u{a7}e%multiplayer.player.joined", &["Steve"], true);
        assert_eq!(worded(&joined, &lang), "\u{a7}eSteve joined the game", "the colour stays in front");
        let killed = said(TextPacketType::Translation, "", "commands.kill.successful", &["%entity.cow.name"], true);
        assert_eq!(worded(&killed, &lang), "Killed Cow");
        let system = said(TextPacketType::System, "", "\u{a7}cplain", &[], false);
        assert_eq!(worded(&system, &lang), "\u{a7}cplain");
        let json = said(TextPacketType::Json, "", r#"{"rawtext":[{"text":"a "},{"translate":"commands.time.set","with":["5"]}]}"#, &[], false);
        assert_eq!(worded(&json, &lang), "a Set the time to 5");
    }

    fn screen() -> (Tree, CommandScreen) {
        let mut tree = Tree::new();
        let screen = CommandScreen::build(&mut tree, &Atlas::pack(Vec::new()).0);
        screen.set_visible(&mut tree, true);
        (tree, screen)
    }

    fn chat_of(messages: &[&str], at: Instant) -> Chat {
        let mut chat = Chat::default();
        for message in messages {
            let text = SText {
                needs_translation: false,
                r#type: TextPacketType::System,
                source_name: Cow::Borrowed(""),
                message: Cow::Owned((*message).to_string()),
                parameters: Vec::new(),
                xuid: Cow::Borrowed(""),
                platform_chat_id: Cow::Borrowed(""),
                filtered_message: None,
            };
            chat.receive(&text, at);
        }
        chat
    }

    #[test]
    fn lines_show_for_ten_seconds_or_while_its_open() {
        let (mut tree, mut screen) = screen();
        let start = Instant::now();
        let chat = chat_of(&["first", "second"], start);
        let font = Font::default();
        screen.update(&mut tree, &font, &lang(), &chat, 426.0, start);
        assert_eq!(screen.shown(&tree), 2);
        assert_eq!(screen.line_text(&tree, 0), "second", "the latest at the bottom");
        let later = start + SHOWN_FOR + Duration::from_secs(1);
        screen.update(&mut tree, &font, &lang(), &chat, 426.0, later);
        assert_eq!(screen.shown(&tree), 0, "gone after ten seconds");
        screen.set_open(&mut tree, true, "", later);
        screen.update(&mut tree, &font, &lang(), &chat, 426.0, later);
        assert_eq!(screen.shown(&tree), 2, "back while it's open");
        layout::run(&mut tree, [426.0, 240.0], &Fake);
    }

    #[test]
    fn enter_sends_escape_shuts_and_up_goes_back() {
        let (mut tree, mut screen) = screen();
        let now = Instant::now();
        screen.set_open(&mut tree, true, "/", now);
        screen.typed("time set noon", now);
        assert_eq!(screen.key(Key::Enter, now), Some(CommandChoice::Send("/time set noon".to_string())));
        screen.set_open(&mut tree, true, "", now);
        screen.typed("hello", now);
        assert_eq!(screen.key(Key::Enter, now), Some(CommandChoice::Send("hello".to_string())));
        screen.set_open(&mut tree, true, "", now);
        screen.key(Key::Up, now);
        assert_eq!(screen.typed_text(), "hello");
        screen.key(Key::Up, now);
        assert_eq!(screen.typed_text(), "/time set noon");
        screen.key(Key::Up, now);
        assert_eq!(screen.typed_text(), "/time set noon", "no further back than the first");
        screen.key(Key::Down, now);
        screen.key(Key::Down, now);
        assert_eq!(screen.typed_text(), "", "past the latest, an empty field");
        assert_eq!(screen.key(Key::Escape, now), Some(CommandChoice::Close));
    }
}
