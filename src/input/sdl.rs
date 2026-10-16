use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use sdl3::{
    EventPump, GamepadSubsystem, Sdl,
    event::Event,
    gamepad::{Axis, Button, Gamepad, GamepadType},
    joystick::{JoystickId, PowerLevel},
};
use slint::{Timer, TimerMode};
use thiserror::Error;
use tracing::{debug, info, warn};

use super::actions::{ControllerFamily, UiAction, UiActionEvent};

/// Valve's USB vendor, and the Steam Deck's built-in controller.
const VALVE_VENDOR: u16 = 0x28de;
const STEAM_DECK_PRODUCT: u16 = 0x1205;
/// Steam Input's virtual gamepad, as SDL reports it only when it can't read
/// which real controller is behind it (Steam's `SteamVirtualGamepadInfo`
/// file, under Steam's config). When it can, SDL reports the real one's IDs,
/// name and type instead.
const STEAM_VIRTUAL_GAMEPAD: u16 = 0x11ff;
/// The Odin 3's own controls as its kernel driver reports them (the device
/// tree's `gamepad-vid`/`gamepad-pid`), and the Xbox Elite 2 pad InputPlumber
/// presents them as on SteamOS-style systems (ArmadaOS).
const ODIN3_GAMEPAD: (u16, u16) = (0x2020, 0x3001);
const XBOX_ELITE_2: (u16, u16) = (0x045e, 0x0b00);

/// A handheld whose built-in controls get their own glyphs and drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Handheld {
    #[default]
    None,
    SteamDeck,
    Odin3,
}

/// A controller action, with the family of the controller that made it.
pub type GamepadActionSink = Rc<dyn Fn(UiActionEvent, ControllerFamily)>;

// SDL is pumped from a UI-thread timer whose rate follows what input can
// do right now: every 8ms while a controller can navigate Horizon, slower
// while none is connected (only hotplug matters), and slowest while a game
// has the foreground (topology and battery only), so Horizon stays out of
// the game's way.
const POLL_INTERVAL: Duration = Duration::from_millis(8);
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(100);
const BACKGROUND_POLL_INTERVAL: Duration = Duration::from_millis(250);
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
    /// The controller used last (else the first connected), for the
    /// footer's controller icon.
    pub shape: Option<ControllerShape>,
}

/// Which controller drawing represents a connected pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerShape {
    /// A pad SDL can't name: a generic gamepad.
    Generic,
    Xbox360,
    /// Xbox One and Series pads (SDL doesn't tell them apart).
    XboxSeries,
    PlayStation3,
    PlayStation4,
    PlayStation5,
    SwitchPro,
    JoyConLeft,
    JoyConRight,
    JoyConPair,
    SteamDeck,
    Odin3,
}

/// An open controller: what it is, read when it connects, and what Horizon
/// makes of it, decided again whenever the set of controllers changes.
#[derive(Debug, Clone, Copy)]
struct PadKind {
    id: JoystickId,
    facts: PadFacts,
    family: ControllerFamily,
    shape: ControllerShape,
    /// A second view of a controller another pad already gives (the Odin 3's
    /// raw pad beside InputPlumber's): its input is dropped.
    ignored: bool,
}

/// What SDL and sysfs say about a controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PadFacts {
    pub kind: GamepadType,
    /// As SDL reports them: behind Steam Input, the real controller's.
    pub ids: Option<(u16, u16)>,
    /// Made in software (uinput), as InputPlumber's and Steam's pads are.
    pub virtual_device: bool,
    /// Steam Input's slot, for one of Steam's virtual pads.
    pub steam_slot: Option<u16>,
}

/// Which Xbox Elite 2 pads exist on the system, Steam's view aside: real
/// ones and InputPlumber's (virtual). Steam hides them from SDL, not sysfs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EliteDevices {
    pub real: bool,
    pub inputplumber: bool,
}

/// How Horizon treats one controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PadRole {
    pub family: ControllerFamily,
    pub shape: ControllerShape,
    pub ignored: bool,
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
    timer: Rc<Timer>,
    state: Rc<RefCell<SdlState>>,
}

impl SdlGamepadInput {
    pub fn start(
        action_sink: GamepadActionSink,
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
            pads: Vec::new(),
            last_family: ControllerFamily::default(),
            last_used: None,
            handheld: detect_handheld(),
            last_power_poll: Instant::now(),
            enabled: true,
            enabled_since_ns: 0,
            analog_navigation: AnalogNavigation::default(),
            digital_navigation: DigitalNavigation::default(),
        };
        state.open_initial_gamepads();
        status_sink(state.status());

        let interval = state.poll_interval();
        let state = Rc::new(RefCell::new(state));
        let timer = Rc::new(Timer::default());
        let timer_state = Rc::clone(&state);
        let timer_action_sink = Rc::clone(&action_sink);
        let timer_status_sink = Rc::clone(&status_sink);
        let own_timer = Rc::downgrade(&timer);

        timer.start(TimerMode::Repeated, interval, move || {
            poll_gamepad_events(&timer_state, &timer_action_sink, &timer_status_sink);
            // A controller arriving or leaving changes the rate.
            if let Some(timer) = own_timer.upgrade() {
                pace(&timer, &timer_state.borrow());
            }
        });

        debug!(
            poll_interval_ms = POLL_INTERVAL.as_millis(),
            "gamepad input started"
        );

        Ok(Self { timer, state })
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.state.borrow_mut().set_enabled(enabled);
        pace(&self.timer, &self.state.borrow());
    }

    /// The family of the first connected controller, if any.
    pub fn first_family(&self) -> Option<ControllerFamily> {
        self.state
            .borrow()
            .pads
            .iter()
            .find(|pad| !pad.ignored)
            .map(|pad| pad.family)
    }

    /// Device-family held state for the embedded game; InputManager applies the saved A/B swap.
    pub fn gameplay_buttons(&self) -> u16 {
        self.gameplay_snapshot().map_or(0, |(buttons, _)| buttons)
    }
    pub fn gameplay_snapshot(&self) -> Option<(u16, u64)> {
        let state = self.state.borrow();
        if !state.enabled {
            return None;
        }
        let selected = state
            .last_used
            .filter(|id| state.pads.iter().any(|pad| pad.id == *id && !pad.ignored))
            .or_else(|| state.pads.iter().find(|pad| !pad.ignored).map(|pad| pad.id));
        let pad = state
            .open_gamepads
            .iter()
            .find(|pad| pad.id().ok() == selected)?;
        let family = state
            .pads
            .iter()
            .find(|pad| Some(pad.id) == selected)
            .map(|pad| pad.family)
            .unwrap_or_default();
        let nintendo = matches!(family, ControllerFamily::Nintendo | ControllerFamily::Odin);
        let mut buttons = 0;
        for (button, bit) in [
            (Button::South, if nintendo { 0 } else { 8 }),
            (Button::East, if nintendo { 8 } else { 0 }),
            (Button::West, if nintendo { 1 } else { 9 }),
            (Button::North, if nintendo { 9 } else { 1 }),
            (Button::Back, 2),
            (Button::Start, 3),
            (Button::DPadUp, 4),
            (Button::DPadDown, 5),
            (Button::DPadLeft, 6),
            (Button::DPadRight, 7),
            (Button::LeftShoulder, 10),
            (Button::RightShoulder, 11),
        ] {
            if pad.button(button) {
                buttons |= 1 << bit;
            }
        }
        if pad.axis(Axis::TriggerLeft) > 18000 {
            buttons |= 1 << 12;
        }
        if pad.axis(Axis::TriggerRight) > 18000 {
            buttons |= 1 << 13;
        }
        let x = pad.axis(Axis::LeftX);
        let y = pad.axis(Axis::LeftY);
        if x < -18000 {
            buttons |= 1 << 6;
        } else if x > 18000 {
            buttons |= 1 << 7;
        }
        if y < -18000 {
            buttons |= 1 << 4;
        } else if y > 18000 {
            buttons |= 1 << 5;
        }
        Some((buttons, u64::from(selected?.raw())))
    }
}

fn pace(timer: &Timer, state: &SdlState) {
    let interval = state.poll_interval();
    if timer.interval() != interval {
        timer.set_interval(interval);
    }
}

struct SdlState {
    // Keep the SDL context alive for the lifetime of all subsystem handles.
    _sdl: Sdl,
    gamepad_subsystem: GamepadSubsystem,
    event_pump: EventPump,
    open_gamepads: Vec<Gamepad>,
    /// Each open controller's family, read once when it connects.
    pads: Vec<PadKind>,
    /// The controller used last, for held-button repeats.
    last_family: ControllerFamily,
    /// Which controller that was, for the footer's icon.
    last_used: Option<JoystickId>,
    handheld: Handheld,
    last_power_poll: Instant,
    enabled: bool,
    /// SDL's clock (ns) when Horizon last got input back; earlier controller
    /// events happened while something else had it.
    enabled_since_ns: u64,
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
                let path = gamepad.path();
                let node = path.as_deref().and_then(input_node);
                let facts = PadFacts {
                    kind: self.gamepad_subsystem.type_for_id(id),
                    ids: gamepad.vendor_id().zip(gamepad.product_id()),
                    virtual_device: node.as_ref().is_some_and(|node| node.virtual_device),
                    steam_slot: node.as_ref().and_then(|node| node.steam_slot),
                };
                info!(controller = %name, ?path, ?facts, "gamepad connected");
                self.pads.push(PadKind {
                    id,
                    facts,
                    family: ControllerFamily::default(),
                    shape: ControllerShape::Generic,
                    ignored: false,
                });
                self.open_gamepads.push(gamepad);
                self.assign_roles();
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
        let open = self
            .open_gamepads
            .iter()
            .filter_map(|gamepad| gamepad.id().ok())
            .collect::<Vec<_>>();
        self.pads.retain(|pad| open.contains(&pad.id));
        let removed = before.saturating_sub(self.open_gamepads.len());
        if removed > 0 {
            info!(removed, "gamepad disconnected");
            self.assign_roles();
        }
        removed > 0
    }

    /// Decide every open controller's family, drawing and whether it's a
    /// duplicate, from all of them together: which pad is a handheld's own
    /// can depend on the others.
    fn assign_roles(&mut self) {
        let facts: Vec<PadFacts> = self.pads.iter().map(|pad| pad.facts).collect();
        let elites = if self.handheld == Handheld::Odin3 {
            elite_devices()
        } else {
            EliteDevices::default()
        };
        for (pad, role) in self
            .pads
            .iter_mut()
            .zip(pad_roles(&facts, self.handheld, elites))
        {
            if (pad.family, pad.shape, pad.ignored) != (role.family, role.shape, role.ignored) {
                info!(
                    family = ?role.family,
                    shape = ?role.shape,
                    ignored = role.ignored,
                    "gamepad role"
                );
            }
            pad.family = role.family;
            pad.shape = role.shape;
            pad.ignored = role.ignored;
        }
    }

    fn ignores(&self, id: JoystickId) -> bool {
        self.pads.iter().any(|pad| pad.id == id && pad.ignored)
    }

    fn poll_interval(&self) -> Duration {
        if !self.enabled {
            BACKGROUND_POLL_INTERVAL
        } else if self.open_gamepads.is_empty() {
            IDLE_POLL_INTERVAL
        } else {
            POLL_INTERVAL
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }

        self.enabled = enabled;
        if enabled {
            // SDL stamps events in nanoseconds; its millisecond clock is
            // close enough to tell a queued press from a new one.
            self.enabled_since_ns = sdl3::timer::ticks().saturating_mul(1_000_000);
        }
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
            connected_gamepads: self.pads.iter().filter(|pad| !pad.ignored).count(),
            // Prefer player one; otherwise use the first physical controller
            // which actually reports a meaningful battery level.
            battery: self.open_gamepads.iter().find_map(|pad| {
                let reading = pad.power_info();
                battery_from_sdl(reading.state, reading.percentage)
            }),
            shape: self
                .pads
                .iter()
                .find(|pad| Some(pad.id) == self.last_used)
                .or(self.pads.iter().find(|pad| !pad.ignored))
                .map(|pad| pad.shape),
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

impl SdlState {
    /// The family of the controller behind an event, remembered for repeats.
    fn family_of(&mut self, id: JoystickId) -> ControllerFamily {
        let family = self
            .pads
            .iter()
            .find(|pad| pad.id == id)
            .map_or(ControllerFamily::default(), |pad| pad.family);
        self.last_family = family;
        self.last_used = Some(id);
        family
    }
}

fn poll_gamepad_events(
    state: &Rc<RefCell<SdlState>>,
    action_sink: &GamepadActionSink,
    status_sink: &Rc<dyn Fn(ControllerStatus)>,
) {
    let mut state = state.borrow_mut();
    let mut status_changed = false;
    let used_before = state.last_used;

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
            // SDL turns SIGTERM and SIGINT into this; Horizon has no SDL
            // windows, so nothing else sends it. Stop the way closing the
            // window would, also while a game is in front.
            Event::Quit { .. } => {
                info!("asked to quit");
                if let Err(error) = slint::quit_event_loop() {
                    warn!(%error, "couldn't stop the event loop");
                }
            }
            // Keep pumping SDL while Horizon is inactive so device topology
            // remains current, but background controller activity must never
            // become Horizon UI navigation.
            _ if !state.enabled => {}
            // Pressed before Horizon got input back but read only now, such
            // as the button that quit a game: it was the game's press.
            Event::GamepadAxisMotion { timestamp, .. }
            | Event::GamepadButtonDown { timestamp, .. }
            | Event::GamepadButtonUp { timestamp, .. }
                if timestamp < state.enabled_since_ns => {}
            // The other view of the same controller gives this input.
            Event::GamepadAxisMotion { which, .. }
            | Event::GamepadButtonDown { which, .. }
            | Event::GamepadButtonUp { which, .. }
                if state.ignores(which) => {}
            Event::GamepadAxisMotion {
                which, axis, value, ..
            } => {
                if matches!(axis, Axis::TriggerLeft | Axis::TriggerRight) && value > 18000 {
                    state.family_of(which);
                }
                if let Some(action) =
                    state
                        .analog_navigation
                        .axis_motion(axis, value, Instant::now())
                {
                    debug!(?action, "left-stick UI action");
                    let family = state.family_of(which);
                    action_sink(UiActionEvent::fresh(action), family);
                }
            }
            Event::GamepadButtonDown { which, button, .. } => {
                let now = Instant::now();
                let family = state.family_of(which);
                if let Some(direction) = direction_for_button(&button) {
                    if let Some(action) = state.digital_navigation.press(direction, now) {
                        debug!(?action, "d-pad UI action");
                        action_sink(UiActionEvent::fresh(action), family);
                    }
                } else if let Some(action) = action_for_button(button, family) {
                    debug!(?action, "gamepad UI action");
                    action_sink(UiActionEvent::fresh(action), family);
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

    // Switching controllers changes the footer's icon.
    status_changed |= state.last_used != used_before;

    if state.enabled {
        let now = Instant::now();
        if let Some(action) = state.analog_navigation.repeat_due(now) {
            debug!(?action, "left-stick repeated UI action");
            action_sink(UiActionEvent::repeated(action), state.last_family);
        }
        if let Some(action) = state.digital_navigation.repeat_due(now) {
            debug!(?action, "d-pad repeated UI action");
            action_sink(UiActionEvent::repeated(action), state.last_family);
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

/// SDL names face buttons by position. Most controllers confirm with the
/// bottom one; Nintendo's confirm with the right one (labelled A), go back
/// with the bottom (B) and use the top (X) for the secondary action, so the
/// labels on the pad match the hints.
pub const fn action_for_button(button: Button, family: ControllerFamily) -> Option<UiAction> {
    // The Odin 3 wears Nintendo's labels too.
    let nintendo = matches!(family, ControllerFamily::Nintendo | ControllerFamily::Odin);
    match button {
        Button::DPadUp => Some(UiAction::Up),
        Button::DPadDown => Some(UiAction::Down),
        Button::DPadLeft => Some(UiAction::Left),
        Button::DPadRight => Some(UiAction::Right),
        Button::South if nintendo => Some(UiAction::Back),
        Button::East if nintendo => Some(UiAction::Accept),
        Button::North if nintendo => Some(UiAction::Secondary),
        Button::West if nintendo => None,
        Button::South => Some(UiAction::Accept),
        Button::East => Some(UiAction::Back),
        Button::West => Some(UiAction::Secondary),
        Button::Start => Some(UiAction::Menu),
        Button::LeftStick | Button::RightStick => Some(UiAction::StickPress),
        Button::Guide => Some(UiAction::Home),
        Button::LeftShoulder => Some(UiAction::LeftBumper),
        Button::RightShoulder => Some(UiAction::RightBumper),
        _ => None,
    }
}

/// Which glyphs a controller's buttons wear. `virtual_device`: the pad was
/// made in software (uinput), as InputPlumber's are.
pub fn controller_family(
    kind: GamepadType,
    vendor: Option<u16>,
    product: Option<u16>,
    virtual_device: bool,
    handheld: Handheld,
) -> ControllerFamily {
    let ids = vendor.zip(product);
    // A handheld's own controls: directly, through InputPlumber, or behind
    // Steam Input (Game Mode). Behind Steam, SDL reports the real controller
    // when Steam says which it is; a virtual pad it can't resolve is taken for
    // the handheld's own, the likeliest on a handheld.
    let unresolved_steam = ids == Some((VALVE_VENDOR, STEAM_VIRTUAL_GAMEPAD));
    if ids == Some((VALVE_VENDOR, STEAM_DECK_PRODUCT))
        || (handheld == Handheld::SteamDeck && unresolved_steam)
    {
        return ControllerFamily::SteamDeck;
    }
    // InputPlumber's Elite 2 has a real one's name and IDs; only a real one
    // is a USB device. (Over Bluetooth an Elite 2 has other IDs.)
    let inputplumber = ids == Some(XBOX_ELITE_2) && virtual_device;
    if handheld == Handheld::Odin3
        && (ids == Some(ODIN3_GAMEPAD) || inputplumber || unresolved_steam)
    {
        return ControllerFamily::Odin;
    }
    match kind {
        GamepadType::PS3 | GamepadType::PS4 | GamepadType::PS5 => ControllerFamily::PlayStation,
        GamepadType::NintendoSwitchPro
        | GamepadType::NintendoSwitchJoyconLeft
        | GamepadType::NintendoSwitchJoyconRight
        | GamepadType::NintendoSwitchJoyconPair => ControllerFamily::Nintendo,
        _ => ControllerFamily::Xbox,
    }
}

/// The drawing for a pad: the handheld's for a handheld's own controls, else
/// the model SDL reports, else a generic gamepad.
pub fn controller_shape(kind: GamepadType, family: ControllerFamily) -> ControllerShape {
    match family {
        ControllerFamily::SteamDeck => return ControllerShape::SteamDeck,
        ControllerFamily::Odin => return ControllerShape::Odin3,
        _ => {}
    }
    match kind {
        GamepadType::Xbox360 => ControllerShape::Xbox360,
        GamepadType::XboxOne => ControllerShape::XboxSeries,
        GamepadType::PS3 => ControllerShape::PlayStation3,
        GamepadType::PS4 => ControllerShape::PlayStation4,
        GamepadType::PS5 => ControllerShape::PlayStation5,
        GamepadType::NintendoSwitchPro => ControllerShape::SwitchPro,
        GamepadType::NintendoSwitchJoyconLeft => ControllerShape::JoyConLeft,
        GamepadType::NintendoSwitchJoyconRight => ControllerShape::JoyConRight,
        GamepadType::NintendoSwitchJoyconPair => ControllerShape::JoyConPair,
        _ => ControllerShape::Generic,
    }
}

/// Which handheld this is, if one with its own controls. Readable inside
/// Flatpak: the Deck from its firmware's board name (LCD "Jupiter", OLED
/// "Galileo"); the Odin 3 from its device tree's model, which sysfs shows as
/// the SoC's machine (Flatpak hides /sys/firmware; the device tree itself is
/// read too, for a build outside the sandbox).
fn detect_handheld() -> Handheld {
    let read = |path: &str| {
        std::fs::read_to_string(path)
            .map(|value| value.trim().to_owned())
            .unwrap_or_default()
    };
    if read("/sys/class/dmi/id/board_vendor") == "Valve"
        && matches!(
            read("/sys/class/dmi/id/board_name").as_str(),
            "Jupiter" | "Galileo"
        )
    {
        return Handheld::SteamDeck;
    }
    if is_odin3(
        &read("/sys/devices/soc0/machine"),
        &read("/sys/firmware/devicetree/base/compatible"),
    ) {
        return Handheld::Odin3;
    }
    Handheld::None
}

/// Each controller's role, from all of them together. Mostly
/// `controller_family` per pad; on an Odin 3 two things need the others:
///
/// - Behind Steam Input, SDL reports each pad as the real controller behind
///   it, so the Odin's own controls (InputPlumber's Elite 2) and a real
///   Elite 2 look the same: both virtual with Elite 2 IDs. Sysfs, which Steam
///   doesn't hide, says which exist. With no real Elite 2 it's the Odin's;
///   with no InputPlumber pad it's the real one; with both, the Odin's has
///   Steam's first slot, as its controls are there from boot.
/// - The raw pad and InputPlumber's (or Steam's) can both be open when
///   InputPlumber shares raw input (ArmadaOS); the raw one is then ignored so
///   a press counts once.
pub fn pad_roles(pads: &[PadFacts], handheld: Handheld, elites: EliteDevices) -> Vec<PadRole> {
    let first_steam_elite = pads
        .iter()
        .filter(|pad| pad.ids == Some(XBOX_ELITE_2))
        .filter_map(|pad| pad.steam_slot)
        .min();
    let families: Vec<ControllerFamily> = pads
        .iter()
        .map(|pad| {
            let family = controller_family(
                pad.kind,
                pad.ids.map(|ids| ids.0),
                pad.ids.map(|ids| ids.1),
                pad.virtual_device,
                handheld,
            );
            let steam_elite = pad.ids == Some(XBOX_ELITE_2) && pad.steam_slot.is_some();
            if handheld != Handheld::Odin3 || !steam_elite {
                return family;
            }
            let odin = !elites.real || (elites.inputplumber && pad.steam_slot == first_steam_elite);
            if odin {
                ControllerFamily::Odin
            } else {
                ControllerFamily::Xbox
            }
        })
        .collect();
    let odin_elsewhere = |index: usize| {
        families.iter().enumerate().any(|(other, family)| {
            other != index
                && *family == ControllerFamily::Odin
                && pads[other].ids != Some(ODIN3_GAMEPAD)
        })
    };
    pads.iter()
        .zip(&families)
        .enumerate()
        .map(|(index, (pad, &family))| PadRole {
            family,
            shape: controller_shape(pad.kind, family),
            ignored: handheld == Handheld::Odin3
                && pad.ids == Some(ODIN3_GAMEPAD)
                && odin_elsewhere(index),
        })
        .collect()
}

/// What sysfs says about an SDL device path's evdev node.
#[derive(Debug, Clone, PartialEq, Eq)]
struct InputNode {
    virtual_device: bool,
    steam_slot: Option<u16>,
}

/// Readable inside Flatpak. `None` for a path that isn't an evdev node (a
/// HIDAPI device).
fn input_node(path: &str) -> Option<InputNode> {
    let node = path.strip_prefix("/dev/input/")?;
    let class = std::path::PathBuf::from(format!("/sys/class/input/{node}"));
    let device = std::fs::canonicalize(&class).ok()?;
    let ids = sysfs_ids(&class);
    let name = std::fs::read_to_string(class.join("device/name")).unwrap_or_default();
    Some(InputNode {
        virtual_device: is_uinput_sysfs(&device.to_string_lossy()),
        steam_slot: steam_slot(ids, name.trim()),
    })
}

fn is_uinput_sysfs(device: &str) -> bool {
    device.starts_with("/sys/devices/virtual/input/")
}

/// A Steam virtual pad's slot, from its evdev name ("… pad 2"), as SDL reads
/// it. Steam's pads keep Valve's IDs in sysfs even when SDL reports the real
/// controller's.
fn steam_slot(ids: Option<(u16, u16)>, name: &str) -> Option<u16> {
    if ids != Some((VALVE_VENDOR, STEAM_VIRTUAL_GAMEPAD)) {
        return None;
    }
    let digits = &name[name.rfind("pad ")? + 4..];
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse().ok()
}

/// An evdev node's vendor and product, from `device/id/` (hex).
fn sysfs_ids(class: &std::path::Path) -> Option<(u16, u16)> {
    let read = |name: &str| {
        let text = std::fs::read_to_string(class.join("device/id").join(name)).ok()?;
        u16::from_str_radix(text.trim(), 16).ok()
    };
    read("vendor").zip(read("product"))
}

/// Which Elite 2 pads exist, from every evdev node in sysfs.
fn elite_devices() -> EliteDevices {
    let mut elites = EliteDevices::default();
    let Ok(entries) = std::fs::read_dir("/sys/class/input") else {
        return elites;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("event") {
            continue;
        }
        let class = entry.path();
        if sysfs_ids(&class) != Some(XBOX_ELITE_2) {
            continue;
        }
        let virtual_device = std::fs::canonicalize(&class)
            .is_ok_and(|device| is_uinput_sysfs(&device.to_string_lossy()));
        if virtual_device {
            elites.inputplumber = true;
        } else {
            elites.real = true;
        }
    }
    elites
}

/// The Odin 3 by its device tree: model "AYN Odin 3", compatible "ayn,odin3"
/// (a NUL-separated list).
fn is_odin3(machine: &str, compatible: &str) -> bool {
    machine.eq_ignore_ascii_case("AYN Odin 3")
        || compatible
            .split('\0')
            .any(|entry| entry.trim() == "ayn,odin3")
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use sdl3::gamepad::{Axis, Button};

    use super::ControllerShape;
    use super::{
        ANALOG_INITIAL_REPEAT_DELAY, AnalogNavigation, DIGITAL_INITIAL_REPEAT_DELAY,
        DigitalNavigation, EliteDevices, Handheld, PadFacts, action_for_button, controller_family,
        controller_shape, input_node, is_odin3, is_uinput_sysfs, pad_roles, steam_slot,
    };
    use crate::input::actions::{ControllerFamily, UiAction};

    #[test]
    fn dpad_maps_to_navigation() {
        assert_eq!(
            action_for_button(Button::DPadLeft, ControllerFamily::Xbox),
            Some(UiAction::Left)
        );
        assert_eq!(
            action_for_button(Button::DPadRight, ControllerFamily::Xbox),
            Some(UiAction::Right)
        );
        assert_eq!(
            action_for_button(Button::DPadUp, ControllerFamily::Xbox),
            Some(UiAction::Up)
        );
        assert_eq!(
            action_for_button(Button::DPadDown, ControllerFamily::Xbox),
            Some(UiAction::Down)
        );
    }

    #[test]
    fn face_buttons_map_to_semantic_actions() {
        let xbox = ControllerFamily::Xbox;
        assert_eq!(
            action_for_button(Button::South, xbox),
            Some(UiAction::Accept)
        );
        assert_eq!(action_for_button(Button::East, xbox), Some(UiAction::Back));
        assert_eq!(
            action_for_button(Button::West, xbox),
            Some(UiAction::Secondary)
        );
    }

    #[test]
    fn nintendo_pads_confirm_with_a_on_the_right() {
        let nintendo = ControllerFamily::Nintendo;
        assert_eq!(
            action_for_button(Button::East, nintendo),
            Some(UiAction::Accept)
        );
        assert_eq!(
            action_for_button(Button::South, nintendo),
            Some(UiAction::Back)
        );
        assert_eq!(
            action_for_button(Button::North, nintendo),
            Some(UiAction::Secondary)
        );
        assert_eq!(action_for_button(Button::West, nintendo), None);
        assert_eq!(
            action_for_button(Button::LeftShoulder, nintendo),
            Some(UiAction::LeftBumper)
        );
    }

    #[test]
    fn controllers_are_told_apart_by_type_and_valve_ids() {
        use sdl3::gamepad::GamepadType;
        assert_eq!(
            controller_family(GamepadType::PS5, None, None, false, Handheld::None),
            ControllerFamily::PlayStation
        );
        assert_eq!(
            controller_family(
                GamepadType::NintendoSwitchJoyconPair,
                None,
                None,
                false,
                Handheld::None
            ),
            ControllerFamily::Nintendo
        );
        assert_eq!(
            controller_family(
                GamepadType::Standard,
                Some(0x28de),
                Some(0x1205),
                false,
                Handheld::None
            ),
            ControllerFamily::SteamDeck
        );
        // Steam's virtual gamepad is a Deck only on a Deck.
        assert_eq!(
            controller_family(
                GamepadType::Xbox360,
                Some(0x28de),
                Some(0x11ff),
                false,
                Handheld::SteamDeck
            ),
            ControllerFamily::SteamDeck
        );
        assert_eq!(
            controller_family(
                GamepadType::Xbox360,
                Some(0x28de),
                Some(0x11ff),
                false,
                Handheld::None
            ),
            ControllerFamily::Xbox
        );
        assert_eq!(
            controller_family(
                GamepadType::Unknown,
                Some(0x045e),
                None,
                false,
                Handheld::None
            ),
            ControllerFamily::Xbox
        );
    }

    #[test]
    fn shell_buttons_map_to_global_actions() {
        assert_eq!(
            action_for_button(Button::Start, ControllerFamily::Xbox),
            Some(UiAction::Menu)
        );
        assert_eq!(
            action_for_button(Button::Guide, ControllerFamily::Xbox),
            Some(UiAction::Home)
        );
        assert_eq!(
            action_for_button(Button::LeftShoulder, ControllerFamily::Xbox),
            Some(UiAction::LeftBumper)
        );
        assert_eq!(
            action_for_button(Button::RightShoulder, ControllerFamily::Xbox),
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

    #[test]
    fn each_pad_gets_its_own_drawing() {
        use sdl3::gamepad::GamepadType;
        let shape = |kind| {
            controller_shape(
                kind,
                controller_family(kind, None, None, false, Handheld::None),
            )
        };
        assert_eq!(shape(GamepadType::PS5), ControllerShape::PlayStation5);
        assert_eq!(shape(GamepadType::PS4), ControllerShape::PlayStation4);
        assert_eq!(shape(GamepadType::Xbox360), ControllerShape::Xbox360);
        assert_eq!(shape(GamepadType::XboxOne), ControllerShape::XboxSeries);
        assert_eq!(
            shape(GamepadType::NintendoSwitchPro),
            ControllerShape::SwitchPro
        );
        assert_eq!(
            shape(GamepadType::NintendoSwitchJoyconLeft),
            ControllerShape::JoyConLeft
        );
        assert_eq!(
            shape(GamepadType::NintendoSwitchJoyconPair),
            ControllerShape::JoyConPair
        );
        assert_eq!(shape(GamepadType::Unknown), ControllerShape::Generic);
        assert_eq!(
            controller_shape(GamepadType::XboxOne, ControllerFamily::SteamDeck),
            ControllerShape::SteamDeck
        );
    }

    #[test]
    fn the_odin_3s_own_controls_are_recognised_on_an_odin_3() {
        use sdl3::gamepad::GamepadType;
        let odin = |vendor, product, virtual_device| {
            controller_family(
                GamepadType::XboxOne,
                Some(vendor),
                Some(product),
                virtual_device,
                Handheld::Odin3,
            )
        };
        // Raw, through InputPlumber's Xbox Elite 2, and through Steam Input.
        assert_eq!(odin(0x2020, 0x3001, false), ControllerFamily::Odin);
        assert_eq!(odin(0x045e, 0x0b00, true), ControllerFamily::Odin);
        assert_eq!(odin(0x28de, 0x11ff, true), ControllerFamily::Odin);
        // Behind Steam Input, resolved by SDL: the Odin's own controls are
        // InputPlumber's Elite 2 (still virtual); other pads are themselves.
        assert_eq!(
            controller_family(
                GamepadType::PS5,
                Some(0x054c),
                Some(0x0ce6),
                true,
                Handheld::Odin3
            ),
            ControllerFamily::PlayStation
        );
        // A real Steam Controller isn't the Odin's.
        assert_eq!(odin(0x28de, 0x1102, true), ControllerFamily::Xbox);
        // A real Elite 2 plugged into an Odin is an Xbox pad.
        assert_eq!(odin(0x045e, 0x0b00, false), ControllerFamily::Xbox);
        // Another pad on an Odin keeps its own family.
        assert_eq!(odin(0x045e, 0x0b12, false), ControllerFamily::Xbox);
        // An Elite 2 elsewhere is an Xbox pad.
        assert_eq!(
            controller_family(
                GamepadType::XboxOne,
                Some(0x045e),
                Some(0x0b00),
                false,
                Handheld::None
            ),
            ControllerFamily::Xbox
        );
        assert_eq!(
            controller_shape(GamepadType::XboxOne, ControllerFamily::Odin),
            ControllerShape::Odin3
        );
    }

    #[test]
    fn odin_3_confirms_with_a_on_the_right() {
        assert_eq!(
            action_for_button(Button::East, ControllerFamily::Odin),
            Some(UiAction::Accept)
        );
        assert_eq!(
            action_for_button(Button::South, ControllerFamily::Odin),
            Some(UiAction::Back)
        );
    }

    #[test]
    fn uinput_pads_are_told_from_bus_pads() {
        assert!(is_uinput_sysfs(
            "/sys/devices/virtual/input/input42/event21"
        ));
        assert!(!is_uinput_sysfs(
            "/sys/devices/platform/soc@0/a600000.usb/usb1/1-1/1-1:1.0/input/input7/event5"
        ));
        assert!(!is_uinput_sysfs(
            "/sys/devices/virtual/misc/uhid/0005:045E:0B22.0003/input/input9/event6"
        ));
        assert_eq!(input_node("/dev/hidraw3"), None);
    }

    #[test]
    fn steam_pads_are_told_by_their_ids_and_numbered_by_name() {
        let valve = Some((0x28de, 0x11ff));
        assert_eq!(steam_slot(valve, "Microsoft X-Box 360 pad 0"), Some(0));
        assert_eq!(steam_slot(valve, "Steam Virtual Gamepad pad 12"), Some(12));
        assert_eq!(
            steam_slot(Some((0x045e, 0x0b00)), "Microsoft X-Box One Elite 2 pad"),
            None
        );
        assert_eq!(steam_slot(valve, "Steam Deck"), None);
    }

    #[test]
    fn behind_steam_the_odins_elite_2_is_told_from_a_real_one() {
        use sdl3::gamepad::GamepadType;
        let steam_elite = |slot| PadFacts {
            kind: GamepadType::XboxOne,
            ids: Some((0x045e, 0x0b00)),
            virtual_device: true,
            steam_slot: Some(slot),
        };
        let families = |pads: &[PadFacts], elites| {
            pad_roles(pads, Handheld::Odin3, elites)
                .into_iter()
                .map(|role| role.family)
                .collect::<Vec<_>>()
        };
        let only_inputplumber = EliteDevices {
            real: false,
            inputplumber: true,
        };
        let only_real = EliteDevices {
            real: true,
            inputplumber: false,
        };
        let both = EliteDevices {
            real: true,
            inputplumber: true,
        };
        assert_eq!(
            families(&[steam_elite(0)], only_inputplumber),
            [ControllerFamily::Odin]
        );
        assert_eq!(
            families(&[steam_elite(0)], only_real),
            [ControllerFamily::Xbox]
        );
        assert_eq!(
            families(&[steam_elite(1), steam_elite(0)], both),
            [ControllerFamily::Xbox, ControllerFamily::Odin]
        );
        // Off an Odin nothing changes.
        assert_eq!(
            pad_roles(&[steam_elite(0)], Handheld::None, both)[0].family,
            ControllerFamily::Xbox
        );
    }

    #[test]
    fn the_odins_raw_pad_is_ignored_beside_inputplumbers() {
        use sdl3::gamepad::GamepadType;
        let raw = PadFacts {
            kind: GamepadType::Unknown,
            ids: Some((0x2020, 0x3001)),
            virtual_device: false,
            steam_slot: None,
        };
        let inputplumber = PadFacts {
            kind: GamepadType::XboxOne,
            ids: Some((0x045e, 0x0b00)),
            virtual_device: true,
            steam_slot: None,
        };
        let roles = pad_roles(
            &[raw, inputplumber],
            Handheld::Odin3,
            EliteDevices::default(),
        );
        assert!(roles[0].ignored && !roles[1].ignored);
        assert_eq!(roles[1].family, ControllerFamily::Odin);
        // Alone, the raw pad is the Odin's controls.
        let roles = pad_roles(&[raw], Handheld::Odin3, EliteDevices::default());
        assert!(!roles[0].ignored);
        assert_eq!(roles[0].family, ControllerFamily::Odin);
    }

    #[test]
    fn the_odin_3_is_found_by_its_device_tree() {
        assert!(is_odin3("AYN Odin 3", ""));
        assert!(is_odin3("", "ayn,odin3\0qcom,cq8725s\0qcom,sm8750\0"));
        assert!(!is_odin3("SM8750", "ayn,odin2\0qcom,qcs8550\0"));
        assert!(!is_odin3("", ""));
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
