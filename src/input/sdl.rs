use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use sdl3::{
    EventPump, GamepadSubsystem, Sdl,
    event::Event,
    gamepad::{Axis, Button, Gamepad},
    joystick::PowerLevel,
};
use slint::{Timer, TimerMode};
use thiserror::Error;
use tracing::{debug, info, warn};

use super::actions::{UiAction, UiActionEvent};

const POLL_INTERVAL: Duration = Duration::from_millis(8);
// Battery can become available shortly after hotplug; keep this inexpensive
// status query responsive without putting it in the 8ms input hot path.
const CONTROLLER_BATTERY_INTERVAL: Duration = Duration::from_secs(2);

// Left-stick navigation uses hysteresis: a stronger threshold enters a
// direction and a lower threshold releases it. This prevents small stick drift
// from moving the UI while still making deliberate analog navigation feel
// immediate.
const ANALOG_ENTER_THRESHOLD: i32 = 18_000;
const ANALOG_EXIT_THRESHOLD: i32 = 12_000;
const ANALOG_INITIAL_REPEAT_DELAY: Duration = Duration::from_millis(300);
const ANALOG_REPEAT_INTERVAL: Duration = Duration::from_millis(115);
pub(super) const DIGITAL_INITIAL_REPEAT_DELAY: Duration = Duration::from_millis(300);
pub(super) const DIGITAL_REPEAT_INTERVAL: Duration = Duration::from_millis(115);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControllerStatus {
    pub connected_gamepads: usize,
    pub battery: Option<ControllerBattery>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerBattery {
    pub percent: u8,
    pub charging: bool,
    // SDL can know the percentage but return PowerLevel::Unknown. In that
    // case a corroborating UPower gaming-input report may supply charging.
    pub charging_known: bool,
}

#[derive(Debug, Error)]
pub enum GamepadInputError {
    #[error("SDL3 initialization failed: {0}")]
    Initialization(String),
    #[error("SDL3 gamepad subsystem initialization failed: {0}")]
    GamepadSubsystem(String),
    #[error("SDL3 event pump initialization failed: {0}")]
    EventPump(String),
}

/// Owns SDL's gamepad subsystem and polls it from Slint's main-thread timer.
///
/// SDL requires event pumping on the main thread. Using a Slint timer keeps
/// that requirement explicit and avoids a cross-thread workaround.
pub struct SdlGamepadInput {
    _timer: Timer,
    _state: Rc<RefCell<SdlState>>,
}

impl SdlGamepadInput {
    pub fn start(
        action_sink: Rc<dyn Fn(UiActionEvent)>,
        status_sink: Rc<dyn Fn(ControllerStatus)>,
    ) -> Result<Self, GamepadInputError> {
        let sdl =
            sdl3::init().map_err(|error| GamepadInputError::Initialization(error.to_string()))?;
        let gamepad_subsystem = sdl
            .gamepad()
            .map_err(|error| GamepadInputError::GamepadSubsystem(error.to_string()))?;
        let event_pump = sdl
            .event_pump()
            .map_err(|error| GamepadInputError::EventPump(error.to_string()))?;

        let mut state = SdlState {
            _sdl: sdl,
            gamepad_subsystem,
            event_pump,
            open_gamepads: Vec::new(),
            last_power_poll: Instant::now(),
            enabled: true,
            analog_navigation: AnalogNavigation::default(),
            digital_navigation: DigitalNavigation::default(),
        };
        state.open_initial_gamepads();
        status_sink(state.status());

        let state = Rc::new(RefCell::new(state));
        let timer = Timer::default();
        let timer_state = Rc::clone(&state);
        let timer_action_sink = Rc::clone(&action_sink);
        let timer_status_sink = Rc::clone(&status_sink);

        timer.start(TimerMode::Repeated, POLL_INTERVAL, move || {
            poll_gamepad_events(&timer_state, &timer_action_sink, &timer_status_sink);
        });

        info!(
            poll_interval_ms = POLL_INTERVAL.as_millis(),
            "SDL3 gamepad input started"
        );

        Ok(Self {
            _timer: timer,
            _state: state,
        })
    }

    pub fn set_enabled(&self, enabled: bool) {
        self._state.borrow_mut().set_enabled(enabled);
    }
}

struct SdlState {
    // Keep the SDL context alive for the lifetime of all subsystem handles.
    _sdl: Sdl,
    gamepad_subsystem: GamepadSubsystem,
    event_pump: EventPump,
    open_gamepads: Vec<Gamepad>,
    last_power_poll: Instant,
    enabled: bool,
    analog_navigation: AnalogNavigation,
    digital_navigation: DigitalNavigation,
}

#[derive(Debug, Default)]
struct DigitalNavigation {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    active_direction: Option<UiAction>,
    next_repeat: Option<Instant>,
}

impl DigitalNavigation {
    fn press(&mut self, direction: UiAction, now: Instant) -> Option<UiAction> {
        debug_assert!(direction.repeatable());
        self.set_pressed(direction, true);

        // SDL can deliver another down event while a button is already held.
        // Horizon owns repeat timing, so duplicate downs must not double-step.
        if self.active_direction == Some(direction) {
            return None;
        }

        self.active_direction = Some(direction);
        self.next_repeat = Some(now + DIGITAL_INITIAL_REPEAT_DELAY);
        Some(direction)
    }

    fn release(&mut self, direction: UiAction, now: Instant) {
        debug_assert!(direction.repeatable());
        self.set_pressed(direction, false);

        if self.active_direction != Some(direction) {
            return;
        }

        self.active_direction = self.fallback_direction();
        self.next_repeat = self.active_direction.map(|_| now + DIGITAL_REPEAT_INTERVAL);
    }

    fn repeat_due(&mut self, now: Instant) -> Option<UiAction> {
        let action = self.active_direction?;
        let deadline = self.next_repeat?;
        if now < deadline {
            return None;
        }

        self.next_repeat = Some(now + DIGITAL_REPEAT_INTERVAL);
        Some(action)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn set_pressed(&mut self, direction: UiAction, pressed: bool) {
        match direction {
            UiAction::Up => self.up = pressed,
            UiAction::Down => self.down = pressed,
            UiAction::Left => self.left = pressed,
            UiAction::Right => self.right = pressed,
            _ => {}
        }
    }

    fn fallback_direction(&self) -> Option<UiAction> {
        if self.left {
            Some(UiAction::Left)
        } else if self.right {
            Some(UiAction::Right)
        } else if self.up {
            Some(UiAction::Up)
        } else if self.down {
            Some(UiAction::Down)
        } else {
            None
        }
    }
}

#[derive(Debug, Default)]
struct AnalogNavigation {
    left_x: i16,
    left_y: i16,
    active_direction: Option<UiAction>,
    next_repeat: Option<Instant>,
}

impl AnalogNavigation {
    fn axis_motion(&mut self, axis: Axis, value: i16, now: Instant) -> Option<UiAction> {
        match axis {
            Axis::LeftX => self.left_x = value,
            Axis::LeftY => self.left_y = value,
            _ => return None,
        }

        let direction = self.resolved_direction();
        if direction == self.active_direction {
            return None;
        }

        self.active_direction = direction;
        self.next_repeat = direction.map(|_| now + ANALOG_INITIAL_REPEAT_DELAY);
        direction
    }

    fn repeat_due(&mut self, now: Instant) -> Option<UiAction> {
        let action = self.active_direction?;
        let deadline = self.next_repeat?;
        if now < deadline {
            return None;
        }

        self.next_repeat = Some(now + ANALOG_REPEAT_INTERVAL);
        Some(action)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn resolved_direction(&self) -> Option<UiAction> {
        let x = i32::from(self.left_x);
        let y = i32::from(self.left_y);
        let abs_x = x.abs();
        let abs_y = y.abs();

        // Keep an already-entered direction until it falls below the smaller
        // release threshold. A clearly stronger perpendicular axis may still
        // take over naturally.
        if let Some(active) = self.active_direction {
            match active {
                UiAction::Left | UiAction::Right
                    if abs_x >= ANALOG_EXIT_THRESHOLD && abs_x >= abs_y =>
                {
                    return Some(if x < 0 {
                        UiAction::Left
                    } else {
                        UiAction::Right
                    });
                }
                UiAction::Up | UiAction::Down
                    if abs_y >= ANALOG_EXIT_THRESHOLD && abs_y > abs_x =>
                {
                    return Some(if y < 0 { UiAction::Up } else { UiAction::Down });
                }
                _ => {}
            }
        }

        if abs_x < ANALOG_ENTER_THRESHOLD && abs_y < ANALOG_ENTER_THRESHOLD {
            return None;
        }

        if abs_x >= abs_y && abs_x >= ANALOG_ENTER_THRESHOLD {
            Some(if x < 0 {
                UiAction::Left
            } else {
                UiAction::Right
            })
        } else if abs_y >= ANALOG_ENTER_THRESHOLD {
            Some(if y < 0 { UiAction::Up } else { UiAction::Down })
        } else {
            None
        }
    }
}

impl SdlState {
    fn open_initial_gamepads(&mut self) {
        match self.gamepad_subsystem.gamepads() {
            Ok(ids) => {
                for id in ids {
                    self.open_gamepad(id);
                }
            }
            Err(error) => {
                warn!(%error, "could not enumerate gamepads at startup");
            }
        }
    }

    fn open_gamepad(&mut self, id: sdl3::joystick::JoystickId) -> bool {
        let already_open = self
            .open_gamepads
            .iter()
            .any(|gamepad| gamepad.id().is_ok_and(|open_id| open_id == id));
        if already_open {
            return false;
        }

        match self.gamepad_subsystem.open(id) {
            Ok(gamepad) => {
                let name = gamepad
                    .name()
                    .unwrap_or_else(|| "Unknown gamepad".to_owned());
                info!(controller = %name, "gamepad connected");
                self.open_gamepads.push(gamepad);
                true
            }
            Err(error) => {
                warn!(%error, "failed to open gamepad");
                false
            }
        }
    }

    fn remove_gamepad(&mut self, id: sdl3::joystick::JoystickId) -> bool {
        let before = self.open_gamepads.len();
        self.open_gamepads.retain(|gamepad| {
            let removed_device = gamepad.id().is_ok_and(|open_id| open_id == id);
            !removed_device && gamepad.connected()
        });
        let removed = before.saturating_sub(self.open_gamepads.len());
        if removed > 0 {
            info!(removed, "gamepad disconnected");
        }
        removed > 0
    }

    fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }

        self.enabled = enabled;
        // A controller may stay physically held while Horizon is inactive.
        // Never carry that held/repeat state across an input-ownership change.
        self.reset_navigation_state();
        debug!(enabled, "Horizon gamepad UI input ownership changed");
    }

    /// Device topology or input-ownership changes invalidate held-button/axis
    /// state. SDL may never deliver a matching release/center event while a
    /// controller is disconnected or while Horizon is inactive.
    fn reset_navigation_state(&mut self) {
        self.analog_navigation.reset();
        self.digital_navigation.reset();
    }

    fn status(&self) -> ControllerStatus {
        ControllerStatus {
            connected_gamepads: self.open_gamepads.len(),
            // Prefer player one; otherwise use the first physical controller
            // which actually reports a meaningful battery level.
            battery: self.open_gamepads.iter().find_map(|pad| {
                let reading = pad.power_info();
                battery_from_sdl(reading.state, reading.percentage)
            }),
        }
    }
}

// The SDL power state can be Unknown even when a backend supplies a valid
// percentage (notably some HIDAPI gamepads). Treat the percentage as usable,
// but never infer charging from an unknown state. Do not invent a percentage
// for an unsupported dongle (SDL reports -1 in that case).
fn battery_from_sdl(state: PowerLevel, percentage: i32) -> Option<ControllerBattery> {
    if !(0..=100).contains(&percentage)
        || matches!(state, PowerLevel::NoBattery | PowerLevel::Error)
    {
        return None;
    }
    Some(ControllerBattery {
        percent: percentage as u8,
        charging: matches!(state, PowerLevel::Charging),
        charging_known: !matches!(state, PowerLevel::Unknown),
    })
}

fn poll_gamepad_events(
    state: &Rc<RefCell<SdlState>>,
    action_sink: &Rc<dyn Fn(UiActionEvent)>,
    status_sink: &Rc<dyn Fn(ControllerStatus)>,
) {
    let mut state = state.borrow_mut();
    let mut status_changed = false;

    while let Some(event) = state.event_pump.poll_event() {
        match event {
            Event::GamepadAdded { which, .. } => {
                if state.open_gamepad(which) {
                    state.reset_navigation_state();
                    status_changed = true;
                }
            }
            Event::GamepadRemoved { which, .. } => {
                let removed = state.remove_gamepad(which);
                // Reset even if SDL's bookkeeping has already dropped the
                // handle: the removal event itself invalidates held state.
                state.reset_navigation_state();
                status_changed |= removed;
            }
            // Keep pumping SDL while Horizon is inactive so device topology
            // remains current, but background controller activity must never
            // become Horizon UI navigation.
            _ if !state.enabled => {}
            Event::GamepadAxisMotion { axis, value, .. } => {
                if let Some(action) =
                    state
                        .analog_navigation
                        .axis_motion(axis, value, Instant::now())
                {
                    debug!(?action, "left-stick UI action");
                    action_sink(UiActionEvent::fresh(action));
                }
            }
            Event::GamepadButtonDown { button, .. } => {
                let now = Instant::now();
                if let Some(direction) = direction_for_button(&button) {
                    if let Some(action) = state.digital_navigation.press(direction, now) {
                        debug!(?action, "d-pad UI action");
                        action_sink(UiActionEvent::fresh(action));
                    }
                } else if let Some(action) = action_for_button(button) {
                    debug!(?action, "gamepad UI action");
                    action_sink(UiActionEvent::fresh(action));
                }
            }
            Event::GamepadButtonUp { button, .. } => {
                if let Some(direction) = direction_for_button(&button) {
                    state.digital_navigation.release(direction, Instant::now());
                }
            }
            _ => {}
        }
    }

    if state.enabled {
        let now = Instant::now();
        if let Some(action) = state.analog_navigation.repeat_due(now) {
            debug!(?action, "left-stick repeated UI action");
            action_sink(UiActionEvent::repeated(action));
        }
        if let Some(action) = state.digital_navigation.repeat_due(now) {
            debug!(?action, "d-pad repeated UI action");
            action_sink(UiActionEvent::repeated(action));
        }
    }

    // Power information changes without gamepad hotplug events. Poll slowly,
    // rather than querying controllers on the 8ms input loop.
    if state.last_power_poll.elapsed() >= CONTROLLER_BATTERY_INTERVAL {
        state.last_power_poll = Instant::now();
        status_changed = true;
    }
    if status_changed {
        status_sink(state.status());
    }
}

pub const fn direction_for_button(button: &Button) -> Option<UiAction> {
    match *button {
        Button::DPadUp => Some(UiAction::Up),
        Button::DPadDown => Some(UiAction::Down),
        Button::DPadLeft => Some(UiAction::Left),
        Button::DPadRight => Some(UiAction::Right),
        _ => None,
    }
}

pub const fn action_for_button(button: Button) -> Option<UiAction> {
    match button {
        Button::DPadUp => Some(UiAction::Up),
        Button::DPadDown => Some(UiAction::Down),
        Button::DPadLeft => Some(UiAction::Left),
        Button::DPadRight => Some(UiAction::Right),
        Button::South => Some(UiAction::Accept),
        Button::East => Some(UiAction::Back),
        Button::Start => Some(UiAction::Menu),
        Button::Guide => Some(UiAction::Home),
        Button::LeftShoulder => Some(UiAction::LeftBumper),
        Button::RightShoulder => Some(UiAction::RightBumper),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use sdl3::gamepad::{Axis, Button};

    use super::{
        ANALOG_INITIAL_REPEAT_DELAY, AnalogNavigation, DIGITAL_INITIAL_REPEAT_DELAY,
        DigitalNavigation, action_for_button,
    };
    use crate::input::actions::UiAction;

    #[test]
    fn dpad_maps_to_navigation() {
        assert_eq!(action_for_button(Button::DPadLeft), Some(UiAction::Left));
        assert_eq!(action_for_button(Button::DPadRight), Some(UiAction::Right));
        assert_eq!(action_for_button(Button::DPadUp), Some(UiAction::Up));
        assert_eq!(action_for_button(Button::DPadDown), Some(UiAction::Down));
    }

    #[test]
    fn face_buttons_map_to_semantic_actions() {
        assert_eq!(action_for_button(Button::South), Some(UiAction::Accept));
        assert_eq!(action_for_button(Button::East), Some(UiAction::Back));
    }

    #[test]
    fn shell_buttons_map_to_global_actions() {
        assert_eq!(action_for_button(Button::Start), Some(UiAction::Menu));
        assert_eq!(action_for_button(Button::Guide), Some(UiAction::Home));
        assert_eq!(
            action_for_button(Button::LeftShoulder),
            Some(UiAction::LeftBumper)
        );
        assert_eq!(
            action_for_button(Button::RightShoulder),
            Some(UiAction::RightBumper)
        );
    }

    #[test]
    fn left_stick_enters_direction_only_beyond_deadzone() {
        let now = Instant::now();
        let mut navigation = AnalogNavigation::default();

        assert_eq!(navigation.axis_motion(Axis::LeftX, 8_000, now), None);
        assert_eq!(
            navigation.axis_motion(Axis::LeftX, 20_000, now),
            Some(UiAction::Right)
        );
        assert_eq!(
            navigation.axis_motion(Axis::LeftX, 11_000, now + Duration::from_millis(10)),
            None
        );
        assert_eq!(navigation.active_direction, None);
    }

    #[test]
    fn held_left_stick_repeats_after_initial_delay() {
        let now = Instant::now();
        let mut navigation = AnalogNavigation::default();

        assert_eq!(
            navigation.axis_motion(Axis::LeftY, -24_000, now),
            Some(UiAction::Up)
        );
        assert_eq!(
            navigation.repeat_due(now + ANALOG_INITIAL_REPEAT_DELAY - Duration::from_millis(1)),
            None
        );
        assert_eq!(
            navigation.repeat_due(now + ANALOG_INITIAL_REPEAT_DELAY),
            Some(UiAction::Up)
        );
    }

    #[test]
    fn held_dpad_repeats_after_initial_delay() {
        let now = Instant::now();
        let mut navigation = DigitalNavigation::default();

        assert_eq!(
            navigation.press(UiAction::Right, now),
            Some(UiAction::Right)
        );
        assert_eq!(
            navigation.repeat_due(now + DIGITAL_INITIAL_REPEAT_DELAY - Duration::from_millis(1)),
            None
        );
        assert_eq!(
            navigation.repeat_due(now + DIGITAL_INITIAL_REPEAT_DELAY),
            Some(UiAction::Right)
        );

        navigation.release(
            UiAction::Right,
            now + DIGITAL_INITIAL_REPEAT_DELAY + Duration::from_millis(1),
        );
        assert_eq!(
            navigation.repeat_due(now + DIGITAL_INITIAL_REPEAT_DELAY + Duration::from_millis(200)),
            None
        );
    }

    #[test]
    fn topology_change_reset_clears_all_held_navigation_state() {
        let now = Instant::now();
        let mut analog = AnalogNavigation::default();
        let mut digital = DigitalNavigation::default();

        assert_eq!(
            analog.axis_motion(Axis::LeftX, 24_000, now),
            Some(UiAction::Right)
        );
        assert_eq!(digital.press(UiAction::Down, now), Some(UiAction::Down));

        analog.reset();
        digital.reset();

        assert_eq!(analog.active_direction, None);
        assert_eq!(analog.next_repeat, None);
        assert_eq!(analog.left_x, 0);
        assert_eq!(analog.left_y, 0);
        assert_eq!(digital.active_direction, None);
        assert_eq!(digital.next_repeat, None);
        assert!(!digital.up && !digital.down && !digital.left && !digital.right);
    }
}

#[cfg(test)]
mod battery_tests {
    use super::{ControllerBattery, battery_from_sdl};
    use sdl3::joystick::PowerLevel;

    #[test]
    fn valid_unknown_state_does_not_discard_reported_percent() {
        assert_eq!(
            battery_from_sdl(PowerLevel::Unknown, 46),
            Some(ControllerBattery {
                percent: 46,
                charging: false,
                charging_known: false
            })
        );
    }

    #[test]
    fn unsupported_gamepad_does_not_fake_a_battery() {
        assert_eq!(battery_from_sdl(PowerLevel::Unknown, -1), None);
        assert_eq!(battery_from_sdl(PowerLevel::NoBattery, 50), None);
        assert_eq!(battery_from_sdl(PowerLevel::Error, 50), None);
        assert_eq!(battery_from_sdl(PowerLevel::Charging, 102), None);
    }

    #[test]
    fn charging_from_sdl_is_preserved() {
        assert_eq!(
            battery_from_sdl(PowerLevel::Charging, 39),
            Some(ControllerBattery {
                percent: 39,
                charging: true,
                charging_known: true
            })
        );
    }
}
