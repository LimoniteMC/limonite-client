//! Keyboard and mouse input, through a keymap (PLAN.md, "Input"): code asks
//! about actions, never buttons, so every binding can change without touching
//! it. The settings file saves the keymap by name (`settings.rs`).

use std::collections::HashSet;

use winit::{
    event::{ElementState, MouseButton},
    keyboard::KeyCode,
};

use crate::player::movement::Keys;

/// Something the player can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    /// Jumps; rises while flying.
    Jump,
    /// Sneaks; sinks while flying.
    Sneak,
    /// Held: sprints, or flies faster.
    Sprint,
    /// Breaks the block aimed at, or hits what's aimed at.
    Attack,
    /// Places a block, or uses the held item.
    Use,
    /// Picks the block aimed at into the hotbar.
    Pick,
    /// Selects a hotbar slot, 0 to 8 from the left.
    Hotbar(u8),
    /// Selects the slot to the right, wrapping round.
    HotbarNext,
    /// Selects the slot to the left, wrapping round.
    HotbarPrevious,
    Inventory,
    /// Drops the held item.
    Drop,
    /// Opens the pause menu. Until it exists (Step 16), lets go of the cursor.
    Pause,
    /// Opens the command line, empty, to chat.
    Chat,
    /// Opens the command line with a `/` typed.
    Command,
    /// Logs the pose as a `/tp` command for the official client.
    LogPose,
}

impl Action {
    /// Every action with its name in the settings file, in the file's order.
    pub const ALL: [(Self, &'static str); 27] = [
        (Self::Forward, "forward"),
        (Self::Back, "back"),
        (Self::Left, "left"),
        (Self::Right, "right"),
        (Self::Jump, "jump"),
        (Self::Sneak, "sneak"),
        (Self::Sprint, "sprint"),
        (Self::Attack, "attack"),
        (Self::Use, "use"),
        (Self::Pick, "pick"),
        (Self::Hotbar(0), "hotbar_1"),
        (Self::Hotbar(1), "hotbar_2"),
        (Self::Hotbar(2), "hotbar_3"),
        (Self::Hotbar(3), "hotbar_4"),
        (Self::Hotbar(4), "hotbar_5"),
        (Self::Hotbar(5), "hotbar_6"),
        (Self::Hotbar(6), "hotbar_7"),
        (Self::Hotbar(7), "hotbar_8"),
        (Self::Hotbar(8), "hotbar_9"),
        (Self::HotbarNext, "hotbar_next"),
        (Self::HotbarPrevious, "hotbar_previous"),
        (Self::Inventory, "inventory"),
        (Self::Drop, "drop"),
        (Self::Pause, "pause"),
        (Self::Chat, "chat"),
        (Self::Command, "command"),
        (Self::LogPose, "log_pose"),
    ];
}

/// Something the player can press.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    Key(KeyCode),
    Mouse(MouseButton),
    /// One notch of the wheel, away from the player.
    WheelUp,
    /// One notch of the wheel, towards the player.
    WheelDown,
}

/// The mouse's buttons and the wheel, by name.
const MOUSE: [(Button, &str); 7] = [
    (Button::Mouse(MouseButton::Left), "MouseLeft"),
    (Button::Mouse(MouseButton::Right), "MouseRight"),
    (Button::Mouse(MouseButton::Middle), "MouseMiddle"),
    (Button::Mouse(MouseButton::Back), "MouseBack"),
    (Button::Mouse(MouseButton::Forward), "MouseForward"),
    (Button::WheelUp, "WheelUp"),
    (Button::WheelDown, "WheelDown"),
];

/// Lists the keys a binding can name, each under its variant's own name
/// (worth teaching).
///
/// A `macro_rules!` macro writes code from a pattern: here, for each key
/// given, a `(KeyCode::KeyW, "KeyW")` pair. `stringify!` turns the variant's
/// name into the string, so the two can't disagree, and a key winit doesn't
/// have fails to compile.
///
/// The names are the W3C's `code` names, which winit uses for its physical
/// keys. They name a key by where it sits, not what's printed on it: `KeyW`
/// is the key where QWERTY has W, so WASD stays in one place on AZERTY too.
macro_rules! named_keys {
    ($($key:ident),* $(,)?) => {
        const KEYS: &[(KeyCode, &str)] = &[$((KeyCode::$key, stringify!($key))),*];
    };
}

named_keys![
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM,
    KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    Space, Enter, Tab, Backspace, Escape, CapsLock,
    ShiftLeft, ShiftRight, ControlLeft, ControlRight, AltLeft, AltRight, SuperLeft, SuperRight,
    ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
    Insert, Delete, Home, End, PageUp, PageDown,
    Backquote, Minus, Equal, BracketLeft, BracketRight, Backslash, IntlBackslash,
    Semicolon, Quote, Comma, Period, Slash,
    Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
    NumpadAdd, NumpadSubtract, NumpadMultiply, NumpadDivide, NumpadDecimal, NumpadEnter,
];

impl Button {
    /// The button's name in the settings file, if it has one. Every button
    /// the keymap can hold has: they're read from names.
    #[must_use]
    pub fn name(self) -> Option<&'static str> {
        let key = match self {
            Self::Key(key) => key,
            other => return MOUSE.iter().find(|&&(button, _)| button == other).map(|&(_, name)| name),
        };
        KEYS.iter().find(|&&(known, _)| known == key).map(|&(_, name)| name)
    }

    /// The button a settings-file name stands for.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        KEYS.iter()
            .find(|&&(_, known)| known == name)
            .map(|&(key, _)| Self::Key(key))
            .or_else(|| MOUSE.iter().find(|&&(_, known)| known == name).map(|&(button, _)| button))
    }
}

/// Which buttons do what. An action can have several buttons, or none, and a
/// button can do several actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keymap {
    /// Each action's buttons, one entry per action in [`Action::ALL`]'s
    /// order. One order for every keymap makes two with the same bindings
    /// equal, however they were made.
    bindings: Vec<(Action, Vec<Button>)>,
}

impl Default for Keymap {
    /// M4's keys, changed where M5 needs them (m5-plan Decision 16): Ctrl
    /// sprints, Shift sneaks, and Esc pauses.
    fn default() -> Self {
        use Button::{Key, Mouse};
        let mut keymap = Self {
            bindings: Action::ALL.iter().map(|&(action, _)| (action, Vec::new())).collect(),
        };
        let defaults = [
            (Action::Forward, Key(KeyCode::KeyW)),
            (Action::Back, Key(KeyCode::KeyS)),
            (Action::Left, Key(KeyCode::KeyA)),
            (Action::Right, Key(KeyCode::KeyD)),
            (Action::Jump, Key(KeyCode::Space)),
            (Action::Sneak, Key(KeyCode::ShiftLeft)),
            (Action::Sprint, Key(KeyCode::ControlLeft)),
            (Action::Attack, Mouse(MouseButton::Left)),
            (Action::Use, Mouse(MouseButton::Right)),
            (Action::Pick, Mouse(MouseButton::Middle)),
            (Action::Hotbar(0), Key(KeyCode::Digit1)),
            (Action::Hotbar(1), Key(KeyCode::Digit2)),
            (Action::Hotbar(2), Key(KeyCode::Digit3)),
            (Action::Hotbar(3), Key(KeyCode::Digit4)),
            (Action::Hotbar(4), Key(KeyCode::Digit5)),
            (Action::Hotbar(5), Key(KeyCode::Digit6)),
            (Action::Hotbar(6), Key(KeyCode::Digit7)),
            (Action::Hotbar(7), Key(KeyCode::Digit8)),
            (Action::Hotbar(8), Key(KeyCode::Digit9)),
            // Scrolling towards you moves right along the hotbar, as in the
            // official client.
            (Action::HotbarNext, Button::WheelDown),
            (Action::HotbarPrevious, Button::WheelUp),
            (Action::Inventory, Key(KeyCode::KeyE)),
            (Action::Drop, Key(KeyCode::KeyQ)),
            (Action::Pause, Key(KeyCode::Escape)),
            (Action::Chat, Key(KeyCode::KeyT)),
            (Action::Command, Key(KeyCode::Slash)),
            (Action::LogPose, Key(KeyCode::KeyP)),
        ];
        for (action, button) in defaults {
            keymap.bind(action, [button]);
        }
        keymap
    }
}

impl Keymap {
    /// The actions bound to a button.
    pub fn actions(&self, button: Button) -> impl Iterator<Item = Action> + '_ {
        self.bindings
            .iter()
            .filter(move |(_, buttons)| buttons.contains(&button))
            .map(|&(action, _)| action)
    }

    /// The buttons bound to an action, in the order they were bound.
    pub fn buttons(&self, action: Action) -> impl Iterator<Item = Button> + '_ {
        self.bindings
            .iter()
            .filter(move |&&(bound, _)| bound == action)
            .flat_map(|(_, buttons)| buttons.iter().copied())
    }

    /// Binds `action` to exactly `buttons`, replacing what it had.
    pub fn bind(&mut self, action: Action, buttons: impl IntoIterator<Item = Button>) {
        let buttons = buttons.into_iter().collect();
        match self.bindings.iter_mut().find(|(bound, _)| *bound == action) {
            Some((_, old)) => *old = buttons,
            None => self.bindings.push((action, buttons)),
        }
    }
}

/// How far a touchpad's pixel scrolling goes for one notch of a wheel.
/// Windows reports touchpads in notches as well, so this is only used
/// elsewhere, and hasn't been tuned there.
pub const PIXELS_PER_NOTCH: f64 = 40.0;

/// The state of every action, and the mouse motion since it was last read.
#[derive(Default)]
pub struct Input {
    keymap: Keymap,
    held: HashSet<Action>,
    /// Actions pressed since [`Input::take_pressed`] was last called.
    pressed: Vec<Action>,
    /// Raw mouse motion since [`Input::take_mouse`], in counts.
    mouse: (f64, f64),
    /// Wheel motion short of a whole notch, + away from the player.
    wheel: f64,
}

impl Input {
    pub fn new(keymap: Keymap) -> Self {
        Self {
            keymap,
            ..Self::default()
        }
    }

    /// A key or mouse button changed. Auto-repeat presses arrive as more
    /// presses, and are ignored: an action is pressed once and held until
    /// released.
    pub fn button(&mut self, button: Button, state: ElementState) {
        let actions: Vec<Action> = self.keymap.actions(button).collect();
        for action in actions {
            match state {
                ElementState::Pressed => {
                    if self.held.insert(action) {
                        self.pressed.push(action);
                    }
                }
                ElementState::Released => {
                    self.held.remove(&action);
                }
            }
        }
    }

    /// Whether `action` is held down.
    #[must_use]
    pub fn is_held(&self, action: Action) -> bool {
        self.held.contains(&action)
    }

    /// The wheel turned by `notches`, + away from the player. Each whole
    /// notch presses what `WheelUp` or `WheelDown` is bound to, once: a wheel
    /// is never held.
    pub fn wheel(&mut self, notches: f64) {
        self.wheel += notches;
        while self.wheel.abs() >= 1.0 {
            let (button, step) = if self.wheel > 0.0 {
                (Button::WheelUp, 1.0)
            } else {
                (Button::WheelDown, -1.0)
            };
            self.wheel -= step;
            self.pressed.extend(self.keymap.actions(button));
        }
    }

    pub fn mouse_motion(&mut self, dx: f64, dy: f64) {
        self.mouse.0 += dx;
        self.mouse.1 += dy;
    }

    #[must_use]
    pub fn held(&self, action: Action) -> bool {
        self.held.contains(&action)
    }

    pub fn take_pressed(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.pressed)
    }

    pub fn take_mouse(&mut self) -> (f64, f64) {
        std::mem::take(&mut self.mouse)
    }

    /// Lets go of everything, e.g. when the window loses focus and stops
    /// hearing about releases.
    pub fn clear(&mut self) {
        self.held.clear();
        self.pressed.clear();
        self.mouse = (0.0, 0.0);
        self.wheel = 0.0;
    }

    /// The movement keys held, for physics.
    #[must_use]
    pub fn keys(&self) -> Keys {
        Keys {
            forward: self.held(Action::Forward),
            back: self.held(Action::Back),
            left: self.held(Action::Left),
            right: self.held(Action::Right),
            jump: self.held(Action::Jump),
            sneak: self.held(Action::Sneak),
            sprint: self.held(Action::Sprint),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> Button {
        Button::Key(code)
    }

    #[test]
    fn keys_drive_actions_through_the_keymap() {
        let mut input = Input::default();
        input.button(key(KeyCode::KeyW), ElementState::Pressed);
        input.button(key(KeyCode::KeyW), ElementState::Pressed); // auto-repeat
        input.button(key(KeyCode::KeyA), ElementState::Pressed);
        input.button(key(KeyCode::ControlLeft), ElementState::Pressed);
        assert_eq!(
            input.keys(),
            Keys {
                forward: true,
                left: true,
                sprint: true,
                ..Keys::default()
            }
        );
        assert_eq!(input.take_pressed(), [Action::Forward, Action::Left, Action::Sprint]);
        assert!(input.take_pressed().is_empty());

        input.button(key(KeyCode::KeyW), ElementState::Released);
        input.button(key(KeyCode::KeyS), ElementState::Pressed);
        assert!(input.keys().back && !input.keys().forward);
        input.button(key(KeyCode::ShiftLeft), ElementState::Pressed);
        assert!(input.keys().sneak, "Shift sneaks");

        input.mouse_motion(3.0, -1.0);
        input.mouse_motion(1.0, 0.5);
        assert_eq!(input.take_mouse(), (4.0, -0.5));
        assert_eq!(input.take_mouse(), (0.0, 0.0));

        input.clear();
        assert_eq!(input.keys(), Keys::default());
        // An unbound key does nothing.
        input.button(key(KeyCode::KeyZ), ElementState::Pressed);
        assert!(input.take_pressed().is_empty());
    }

    #[test]
    fn mouse_buttons_and_the_wheel_drive_actions_too() {
        let mut input = Input::default();
        input.button(Button::Mouse(MouseButton::Left), ElementState::Pressed);
        assert!(input.held(Action::Attack));
        input.button(Button::Mouse(MouseButton::Left), ElementState::Released);
        assert!(!input.held(Action::Attack));

        // Two notches towards the player, then a touchpad's worth of small
        // steps away from them: one notch's worth, and a bit left over.
        input.wheel(-2.0);
        for _ in 0..5 {
            input.wheel(0.25);
        }
        assert_eq!(
            input.take_pressed(),
            [Action::Attack, Action::HotbarNext, Action::HotbarNext, Action::HotbarPrevious]
        );
        assert!(!input.held(Action::HotbarNext), "a wheel is never held");
    }

    #[test]
    fn a_binding_can_change() {
        let mut keymap = Keymap::default();
        keymap.bind(Action::Jump, [key(KeyCode::KeyJ), Button::Mouse(MouseButton::Back)]);
        keymap.bind(Action::Drop, []);
        let mut input = Input::new(keymap);
        input.button(key(KeyCode::Space), ElementState::Pressed);
        input.button(key(KeyCode::KeyQ), ElementState::Pressed);
        assert!(input.take_pressed().is_empty());
        input.button(Button::Mouse(MouseButton::Back), ElementState::Pressed);
        assert_eq!(input.take_pressed(), [Action::Jump]);
    }

    #[test]
    fn every_name_reads_back_as_its_button() {
        for &(code, name) in KEYS {
            assert_eq!(Button::from_name(name), Some(key(code)));
            assert_eq!(key(code).name(), Some(name));
        }
        for (button, name) in MOUSE {
            assert_eq!(Button::from_name(name), Some(button));
            assert_eq!(button.name(), Some(name));
        }
        assert_eq!(Button::from_name("Spacebar"), None);
        assert_eq!(key(KeyCode::MediaPlayPause).name(), None);
    }

    #[test]
    fn the_names_are_unique_and_every_default_has_one() {
        let mut names = HashSet::new();
        for name in KEYS.iter().map(|&(_, name)| name).chain(MOUSE.map(|(_, name)| name)) {
            assert!(names.insert(name), "{name} names two buttons");
        }
        let actions: HashSet<_> = Action::ALL.iter().map(|&(action, _)| action).collect();
        let action_names: HashSet<_> = Action::ALL.iter().map(|&(_, name)| name).collect();
        assert_eq!((actions.len(), action_names.len()), (Action::ALL.len(), Action::ALL.len()));
        let keymap = Keymap::default();
        assert_eq!(keymap.bindings.len(), Action::ALL.len(), "an action was bound outside Action::ALL");
        for (action, buttons) in &keymap.bindings {
            assert_eq!(buttons.len(), 1, "{action:?} has {buttons:?}");
            assert!(buttons[0].name().is_some(), "{:?} has no name", buttons[0]);
        }
    }

    #[test]
    fn keymaps_with_the_same_bindings_are_equal() {
        let mut one = Keymap::default();
        one.bind(Action::Jump, [key(KeyCode::KeyJ)]);
        one.bind(Action::Drop, [key(KeyCode::KeyX)]);
        let mut other = Keymap::default();
        other.bind(Action::Drop, [key(KeyCode::KeyX)]);
        other.bind(Action::Jump, [key(KeyCode::KeyJ)]);
        assert_eq!(one, other);
    }
}
