mod tone_generator;

use crate::types::*;
use tone_generator::ToneGenerator;

use interception as ic;
use vigem::*;

use serde::{Deserialize, Serialize};

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::spin_loop_hint;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Hash, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bind {
    Keyboard(ic::ScanCode),
    Mouse(MouseButton),
}

#[derive(Serialize, Deserialize, Hash, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DodgeAction {
    Jump,
    Forwards,
    Backwards,
    Left,
    Right,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    sensitivity: f64,

    sample_window: Duration,
    dodge_lock_duration: Duration,

    oversteer_alert_enabled: bool,
    oversteer_alert_threshold: f64,
    oversteer_alert: tone_generator::Config,

    binds: HashMap<Bind, ControllerButton>,
    dodge_binds: HashMap<DodgeAction, Bind>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            sensitivity: 1.0,

            sample_window: Duration::from_millis(20),
            dodge_lock_duration: Duration::from_millis(50),

            oversteer_alert_enabled: false,
            oversteer_alert_threshold: 1.5,
            oversteer_alert: tone_generator::Config::default(),

            binds: HashMap::new(),
            dodge_binds: HashMap::new(),
        }
    }
}

pub struct EventHandler {
    config: Config,

    rx: mpsc::Receiver<Event>,

    vigem: Vigem,
    target: Target,
    report: XUSBReport,

    tone_generator: Option<ToneGenerator>,

    mouse_samples: VecDeque<(i32, i32, Instant)>,

    analog_locked: bool,
    analog_lock_start: Instant,

    iteration_count: i32,
    iteration_total: Duration,
    iteration_window_start: Instant,
}

impl EventHandler {
    const ANALOG_MAX: f64 = i16::MAX as f64;

    pub fn new(rx: mpsc::Receiver<Event>, config: Config) -> Result<Self, anyhow::Error> {
        let mut vigem = Vigem::new();
        vigem.connect()?;

        let mut target = Target::new(TargetType::Xbox360);
        vigem.target_add(&mut target)?;

        info!("ViGEm connected, controller index: {}", target.index());

        info!(
            "sensitivity: {}, sample_window: {:#?}, dodge_lock_duration: {:#?}",
            config.sensitivity, config.sample_window, config.dodge_lock_duration
        );

        let tone_generator = match config.oversteer_alert_enabled {
            true => Some(ToneGenerator::new(config.oversteer_alert)?),
            false => None,
        };

        Ok(EventHandler {
            config: config,

            rx: rx,

            vigem: vigem,
            target: target,
            report: XUSBReport::default(),

            tone_generator: tone_generator,

            mouse_samples: VecDeque::new(),

            analog_locked: false,
            analog_lock_start: Instant::now(),

            iteration_count: 0,
            iteration_total: Duration::from_secs(0),
            iteration_window_start: Instant::now(),
        })
    }

    pub fn run(&mut self) -> Result<(), anyhow::Error> {
        loop {
            let iteration_start = Instant::now();

            let mut event = self.rx.try_recv();
            while event.is_err() && iteration_start.elapsed() < Duration::from_micros(2000) {
                spin_loop_hint();
                event = self.rx.try_recv();
            }

            if let Ok(event) = event {
                match event {
                    Event::MouseMove(x, y) => self.handle_mouse_move(x, y),

                    Event::MouseButton(button, state) => {
                        self.handle_bind(Bind::Mouse(button), state)
                    }

                    Event::Keyboard(scancode, state) => {
                        self.handle_bind(Bind::Keyboard(scancode), state)
                    }

                    Event::Reset => self.report = XUSBReport::default(),
                }
            }

            self.update_analog();
            self.vigem.update(&self.target, &self.report)?;

            if log_enabled!(log::Level::Info) {
                self.iteration_count += 1;
                self.iteration_total += iteration_start.elapsed();

                if self.iteration_window_start.elapsed() > Duration::from_secs(2) {
                    debug!(
                        "{} loops, {} per sec, avg = {:#?}",
                        self.iteration_count,
                        self.iteration_count as f64 / 2.0,
                        self.iteration_total.div_f64(self.iteration_count.into())
                    );

                    self.iteration_count = 0;
                    self.iteration_total = Duration::from_secs(0);
                    self.iteration_window_start = Instant::now();
                }
            }
        }
    }

    fn handle_bind(&mut self, bind: Bind, state: KeyState) {
        let controller_button = match self.config.binds.get(&bind) {
            Some(controller_button) => controller_button,
            None => return,
        };

        match *controller_button {
            ControllerButton::LeftTrigger => match state {
                KeyState::Down => self.report.b_left_trigger = u8::MAX,
                KeyState::Up => self.report.b_left_trigger = 0,
            },

            ControllerButton::RightTrigger => match state {
                KeyState::Down => self.report.b_right_trigger = u8::MAX,
                KeyState::Up => self.report.b_right_trigger = 0,
            },

            button => {
                let button_flag = XButton::from_bits(button as u16).unwrap();

                match state {
                    KeyState::Down => self.report.w_buttons |= button_flag,
                    KeyState::Up => self.report.w_buttons &= !button_flag,
                }
            }
        }

        if state == KeyState::Up {
            return;
        }

        if let Some(jump_bind) = self.config.dodge_binds.get(&DodgeAction::Jump) {
            if *jump_bind == bind {
                self.handle_jump();
            }
        }
    }

    fn handle_jump(&mut self) {
        self.analog_locked = true;
        self.analog_lock_start = Instant::now();

        let mut analog = [0.0, 0.0];

        let actions = [
            (DodgeAction::Forwards, 1, 1.0),
            (DodgeAction::Backwards, 1, -1.0),
            (DodgeAction::Left, 0, -1.0),
            (DodgeAction::Right, 0, 1.0),
        ];

        for (action, idx, diff) in actions.iter() {
            if self.dodge_action_pressed(*action) {
                analog[*idx] += *diff;
            }
        }

        self.set_analog(analog[0], analog[1]);
    }

    fn dodge_action_pressed(&self, action: DodgeAction) -> bool {
        let bind = match self.config.dodge_binds.get(&action) {
            Some(bind) => bind,
            None => return false,
        };

        let button = match self.config.binds.get(&bind) {
            Some(button) => button,
            None => return false,
        };

        match *button {
            ControllerButton::LeftTrigger => return self.report.b_left_trigger > 0,
            ControllerButton::RightTrigger => return self.report.b_right_trigger > 0,
            button => {
                let button_flag = XButton::from_bits(button as u16).unwrap();
                return self.report.w_buttons.contains(button_flag);
            }
        }
    }

    fn handle_mouse_move(&mut self, x: i32, y: i32) {
        let now = Instant::now();
        self.mouse_samples.push_back((x, y, now));
    }

    fn update_analog(&mut self) {
        let now = Instant::now();

        loop {
            let sample = match self.mouse_samples.front() {
                Some(sample) => sample,
                None => break,
            };

            if now - sample.2 > self.config.sample_window {
                self.mouse_samples.pop_front();
            } else {
                break;
            }
        }

        if !self.analog_locked || now - self.analog_lock_start > self.config.dodge_lock_duration {
            self.analog_locked = false;

            // let window = self.config.sample_window.as_secs_f64();
            let mut mouse_vel = (0.0, 0.0);

            /*
            let dt_offset = if self.mouse_samples.len() > 0 {
                let sample = self.mouse_samples[0];
                if (now - sample.2).as_secs_f64() * 1000.0 < 1.0 {
                    (now - sample.2).as_secs_f64()
                } else {
                    0.0005
                }
            } else {
                0.0
            };
            */

            for &(x, y, _) in self.mouse_samples.iter() {
                // let dt = ((now - t).as_secs_f64() - dt_offset) / window;

                mouse_vel.0 += x as f64;
                mouse_vel.1 += y as f64;
            }

            let multiplier =
                self.config.sensitivity / (1e4 * self.config.sample_window.as_secs_f64());

            self.set_analog(
                mouse_vel.0 as f64 * multiplier,
                -mouse_vel.1 as f64 * multiplier,
            );
        }
    }

    fn set_analog(&mut self, x: f64, y: f64) {
        let alert = x.abs().max(y.abs()) >= self.config.oversteer_alert_threshold;
        self.tone_generator.as_mut().map(|tg| tg.enable(alert));

        if x.abs() <= 1.0 && y.abs() <= 1.0 {
            self.report.s_thumb_lx = (x * Self::ANALOG_MAX) as i16;
            self.report.s_thumb_ly = (y * Self::ANALOG_MAX) as i16;

            return;
        }

        let overshoot = x.abs().max(y.abs());

        let angle = y.atan2(x);
        let radius = (x.powi(2) + y.powi(2)).sqrt();

        let new_radius = radius / overshoot;

        self.report.s_thumb_lx = (angle.cos() * new_radius * Self::ANALOG_MAX) as i16;
        self.report.s_thumb_ly = (angle.sin() * new_radius * Self::ANALOG_MAX) as i16;
    }
}
