use interception as ic;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Up,
    Down,
}

impl From<ic::KeyState> for KeyState {
    fn from(key_state: ic::KeyState) -> Self {
        if key_state.contains(ic::KeyState::UP) {
            KeyState::Up
        } else {
            KeyState::Down
        }
    }
}

#[derive(Serialize, Deserialize, Hash, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Button4,
    Button5,
}

#[derive(Serialize, Deserialize, Hash, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ControllerButton {
    DpadUp = 1,
    DpadDown = 2,
    DpadLeft = 4,
    DpadRight = 8,

    Start = 16,
    Back = 32,

    LeftThumb = 64,
    RightThumb = 128,

    LeftShoulder = 256,
    RightShoulder = 512,

    Guide = 1024,

    A = 4096,
    B = 8192,
    X = 16384,
    Y = 32768,

    LeftTrigger,
    RightTrigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    MouseMove(i32, i32),
    MouseButton(MouseButton, KeyState),
    Keyboard(ic::ScanCode, KeyState),
    Reset,
}
