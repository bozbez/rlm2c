use crate::types::*;

use interception as ic;
use serde::{Deserialize, Serialize};

use std::convert::TryFrom;
use std::sync::mpsc;

#[derive(Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    toggle_key: ic::ScanCode,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            toggle_key: ic::ScanCode::Grave,
        }
    }
}

pub struct EventDispatcher {
    config: Config,

    tx: mpsc::Sender<Event>,
    interception: ic::Interception,

    active: bool,
    key_states: [KeyState; 128],
}

impl EventDispatcher {
    pub fn new(tx: mpsc::Sender<Event>, config: Config) -> Option<Self> {
        let interception = match ic::Interception::new() {
            Some(interception) => interception,
            None => {
                error!("could not create interception context");
                return None;
            }
        };

        interception.set_filter(
            ic::is_mouse,
            ic::Filter::MouseFilter(ic::MouseFilter::all()),
        );

        interception.set_filter(
            ic::is_keyboard,
            ic::Filter::KeyFilter(ic::KeyFilter::UP | ic::KeyFilter::DOWN),
        );

        info!("toggle_key: {:?}", config.toggle_key);

        Some(EventDispatcher {
            config: config,

            tx: tx,
            interception: interception,

            active: false,
            key_states: [KeyState::Up; 128],
        })
    }

    pub fn run(&mut self) {
        let mut strokes = [ic::Stroke::Keyboard {
            code: ic::ScanCode::Esc,
            state: ic::KeyState::empty(),
            information: 0,
        }; 10];

        loop {
            let device = self.interception.wait();

            let num_strokes = self.interception.receive(device, &mut strokes);
            let num_strokes = num_strokes as usize;

            for i in 0..num_strokes {
                let send = self.process_stroke(strokes[i]);
                if send {
                    self.interception.send(device, &strokes[i..i + 1]);
                }
            }
        }
    }

    fn process_stroke(&mut self, stroke: ic::Stroke) -> bool {
        match stroke {
            ic::Stroke::Keyboard {
                code,
                state,
                information: _,
            } => self.process_key(code, state.into()),

            ic::Stroke::Mouse {
                state,
                flags: _,
                rolling: _,
                x,
                y,
                information: _,
            } => {
                if !self.active {
                    return true;
                }

                if state != ic::MouseState::empty() {
                    self.process_mouse_state(state);
                }

                if x != 0 || y != 0 {
                    self.tx.send(Event::MouseMove(x, y)).unwrap();
                }

                false
            }
        }
    }

    fn toggle_active(&mut self) {
        self.active = !self.active;

        if !self.active {
            self.tx.send(Event::Reset).unwrap();
            return;
        }

        let mut strokes = Vec::new();

        for (index, &state) in self.key_states.iter().enumerate() {
            if state == KeyState::Up {
                continue;
            }

            let code = match ic::ScanCode::try_from(index as u16) {
                Ok(code) => code,
                Err(_) => continue,
            };

            strokes.push(ic::Stroke::Keyboard {
                code: code,
                state: ic::KeyState::UP,
                information: 0,
            });
        }

        self.interception.send(1, &strokes);

        let stroke = [ic::Stroke::Mouse {
            state: ic::MouseState::LEFT_BUTTON_UP
                | ic::MouseState::RIGHT_BUTTON_UP
                | ic::MouseState::MIDDLE_BUTTON_UP
                | ic::MouseState::BUTTON_4_UP
                | ic::MouseState::BUTTON_5_UP,
            flags: ic::MouseFlags::empty(),
            rolling: 0,
            x: 0,
            y: 0,
            information: 0,
        }];

        self.interception.send(11, &stroke);
    }

    fn process_key(&mut self, code: ic::ScanCode, state: KeyState) -> bool {
        let mut changed_state = false;
        if state != self.key_states[code as usize] {
            self.key_states[code as usize] = state;
            changed_state = true;
        }

        if code == self.config.toggle_key {
            if changed_state && state == KeyState::Down {
                self.toggle_active();
            }

            return false;
        }

        if self.active {
            if changed_state {
                self.tx.send(Event::Keyboard(code, state)).unwrap();
            }

            false
        } else {
            true
        }
    }

    fn process_mouse_state(&mut self, state: ic::MouseState) {
        let table = [
            (
                ic::MouseState::LEFT_BUTTON_DOWN,
                ic::MouseState::LEFT_BUTTON_UP,
                MouseButton::Left,
            ),
            (
                ic::MouseState::RIGHT_BUTTON_DOWN,
                ic::MouseState::RIGHT_BUTTON_UP,
                MouseButton::Right,
            ),
            (
                ic::MouseState::MIDDLE_BUTTON_DOWN,
                ic::MouseState::MIDDLE_BUTTON_UP,
                MouseButton::Middle,
            ),
            (
                ic::MouseState::BUTTON_4_DOWN,
                ic::MouseState::BUTTON_4_UP,
                MouseButton::Button4,
            ),
            (
                ic::MouseState::BUTTON_5_DOWN,
                ic::MouseState::BUTTON_5_UP,
                MouseButton::Button5,
            ),
        ];

        for (flag_down, flag_up, button) in table.iter() {
            if state.contains(*flag_down) && state.contains(*flag_up) {
                continue;
            }

            if state.contains(*flag_down) {
                self.tx
                    .send(Event::MouseButton(*button, KeyState::Down))
                    .unwrap();
            } else if state.contains(*flag_up) {
                self.tx
                    .send(Event::MouseButton(*button, KeyState::Up))
                    .unwrap();
            }
        }
    }
}
