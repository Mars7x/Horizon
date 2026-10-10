//! Read-only desktop status via NetworkManager and UPower system D-Bus APIs.
//! Blocking D-Bus work must stay off the Slint/input thread.

use std::{
    cell::RefCell,
    collections::HashMap,
    sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    thread,
    time::Duration,
};

use zbus::{
    blocking::{Connection, MessageIterator, Proxy},
    message::Type as MessageType,
    MatchRule,
    zvariant::{OwnedObjectPath, OwnedValue},
};

// Periodic safety poll. Actual network changes trigger a D-Bus signal wake-up.
const NETWORK_INTERVAL: Duration = Duration::from_secs(5);
const NETWORK_DISCONNECT_CONFIRM: Duration = Duration::from_millis(150);
const BATTERY_INTERVAL: Duration = Duration::from_secs(2);
const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_IFACE: &str = "org.freedesktop.NetworkManager";
const ACTIVE_IFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const WIRELESS_IFACE: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const ACCESS_POINT_IFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";
const UPOWER: &str = "org.freedesktop.UPower";
const UPOWER_DEVICE: &str = "org.freedesktop.UPower.Device";
const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const UPOWER_ROOT: &str = "/org/freedesktop/UPower";
const UPOWER_IFACE: &str = "org.freedesktop.UPower";
const UPOWER_GAME_CONTROLLER_TYPE: u32 = 12; // Gaming Input, not host Battery.
const BLUEZ: &str = "org.bluez";
const BLUEZ_OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const BLUEZ_DEVICE: &str = "org.bluez.Device1";
const BLUEZ_BATTERY: &str = "org.bluez.Battery1";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NetworkKind {
    #[default]
    Offline,
    Ethernet,
    Wifi,
    Other,
}

impl NetworkKind {
    pub fn ui_code(self) -> i32 {
        match self { Self::Offline => 0, Self::Ethernet => 1, Self::Wifi => 2, Self::Other => 3 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatteryReading {
    pub percent: u8,
    pub charging: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SystemStatus {
    pub network: NetworkKind,
    pub wifi_strength: u8,
    pub limited: bool,
    pub host_battery: Option<BatteryReading>,
    /// Peripheral battery from UPower Gaming Input / BlueZ, never a host battery.
    /// Only presented when SDL currently has a connected gamepad.
    pub controller_battery_fallback: Option<BatteryReading>,
}

// A single missing NetworkManager reply must not erase a previously verified
// link. A real disconnection requires two successful disconnected snapshots;
// repeated read failures eventually fall back to offline instead of preserving
// an unverified connection indefinitely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NetworkReading {
    kind: NetworkKind,
    wifi_strength: u8,
    limited: bool,
}

impl NetworkReading {
    fn offline() -> Self {
        Self { kind: NetworkKind::Offline, wifi_strength: 0, limited: false }
    }

    fn physical(kind: NetworkKind, wifi_strength: u8, connectivity: u32) -> Self {
        // NetworkManager's Internet-connectivity result is distinct from the
        // physical link: "none" must not turn an active Ethernet link into an
        // unplugged-cable icon. Mark restricted Internet access separately.
        Self { kind, wifi_strength, limited: matches!(connectivity, 1..=3) }
    }
}

#[derive(Default)]
struct NetworkFilter {
    last: Option<NetworkReading>,
    disconnected_polls: u8,
    failed_polls: u8,
}

impl NetworkFilter {
    fn update(&mut self, observation: Option<NetworkReading>) -> NetworkReading {
        match observation {
            Some(reading) if reading.kind != NetworkKind::Offline => {
                self.last = Some(reading);
                self.disconnected_polls = 0;
                self.failed_polls = 0;
            }
            Some(reading) => {
                self.failed_polls = 0;
                self.disconnected_polls = self.disconnected_polls.saturating_add(1);
                if self.disconnected_polls >= 2 || self.last.is_none() {
                    self.last = Some(reading);
                }
            }
            None => {
                self.disconnected_polls = 0;
                self.failed_polls = self.failed_polls.saturating_add(1);
                if self.failed_polls >= 3 {
                    self.last = Some(NetworkReading::offline());
                }
            }
        }
        self.last.unwrap_or_else(NetworkReading::offline)
    }
}

// Network and battery have independent D-Bus polling paths: a delayed
// NetworkManager reply cannot hold the first Bluetooth battery indication.
// Merge partial observations before publishing; never overwrite a fresh power
// reading with an unrelated network-only snapshot (or vice versa).
#[derive(Clone, Copy, Debug)]
enum StatusUpdate {
    Network(NetworkReading),
    Power {
        host: Option<BatteryReading>,
        controller: Option<BatteryReading>,
    },
}

pub struct StatusMonitor {
    receiver: Receiver<StatusUpdate>,
    last: RefCell<SystemStatus>,
}

impl StatusMonitor {
    pub fn start() -> Self {
        let (sender, receiver) = mpsc::channel();
        // NetworkManager broadcasts property/signal changes for active links,
        // devices, and Wi-Fi access points. Coalesce bursts into one wake-up;
        // no D-Bus call or event stream ever runs on the Slint/input thread.
        let (network_wake, network_events) = mpsc::sync_channel::<()>(1);
        if let Err(error) = thread::Builder::new()
            .name("horizon-network-events".into())
            .spawn(move || watch_network_changes(network_wake)) {
            tracing::warn!(%error, "network event listener unavailable; polling fallback active");
        }
        let network_sender = sender.clone();
        if let Err(error) = thread::Builder::new()
            .name("horizon-network-status".into())
            .spawn(move || {
                let mut bus: Option<Connection> = None;
                let mut filter = NetworkFilter::default();
                loop {
                    if bus.is_none() { bus = Connection::system().ok(); }
                    let observation = bus.as_ref().and_then(read_network);
                    // A confirmed offline state is checked a second time after
                    // 150 ms, not five seconds later. This filters transient
                    // handoffs without introducing a visibly slow disconnect.
                    let confirm = matches!(observation, Some(r) if r.kind == NetworkKind::Offline)
                        && filter.disconnected_polls == 0 && filter.last.is_some();
                    let result = filter.update(observation);
                    if network_sender.send(StatusUpdate::Network(result)).is_err() { break; }
                    let timeout = if confirm { NETWORK_DISCONNECT_CONFIRM } else { NETWORK_INTERVAL };
                    match network_events.recv_timeout(timeout) {
                        Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => thread::sleep(timeout),
                    }
                }
            }) {
            tracing::warn!(%error, "network status observer unavailable");
        }
        if let Err(error) = thread::Builder::new()
            .name("horizon-power-status".into())
            .spawn(move || {
                let mut bus: Option<Connection> = None;
                loop {
                    if bus.is_none() { bus = Connection::system().ok(); }
                    // Read host battery and peripheral sources separately from
                    // NetworkManager; invalid reports remain None, not 100%.
                    let host = bus.as_ref().and_then(read_host_battery);
                    let controller = bus.as_ref().and_then(read_controller_battery);
                    if sender.send(StatusUpdate::Power { host, controller }).is_err() { break; }
                    thread::sleep(BATTERY_INTERVAL);
                }
            }) {
            tracing::warn!(%error, "battery status observer unavailable");
        }
        Self { receiver, last: RefCell::new(SystemStatus::default()) }
    }

    pub fn latest(&self) -> Option<SystemStatus> {
        let mut last = self.last.borrow_mut();
        let before = *last;
        let mut received = false;
        while let Ok(update) = self.receiver.try_recv() {
            received = true;
            match update {
                StatusUpdate::Network(reading) => {
                    last.network = reading.kind;
                    last.wifi_strength = reading.wifi_strength;
                    last.limited = reading.limited;
                }
                StatusUpdate::Power { host, controller } => {
                    last.host_battery = host;
                    last.controller_battery_fallback = controller;
                }
            }
        }
        if received && *last != before { Some(*last) } else { None }
    }
}

// Signal-driven observation supplements (not replaces) the periodic fallback.
// Broadly match signals from NetworkManager itself: root property changes,
// ActiveConnection.State, device carrier/link changes, and access-point
// Strength updates can all originate at different NetworkManager object paths.
// Signals are hints to re-query the authoritative snapshot, not status values.
fn watch_network_changes(wake: SyncSender<()>) {
    loop {
        let attempt = (|| -> zbus::Result<()> {
            let bus = Connection::system()?;
            let rule = MatchRule::builder()
                .msg_type(MessageType::Signal)
                .sender(NM)?
                .build();
            let mut messages = MessageIterator::for_match_rule(rule, &bus, Some(32))?;
            for message in &mut messages {
                if message.is_err() { break; }
                match wake.try_send(()) {
                    Ok(()) | Err(TrySendError::Full(())) => {}
                    Err(TrySendError::Disconnected(())) => return Ok(()),
                }
            }
            Ok(())
        })();
        if let Err(error) = attempt {
            tracing::debug!(%error, "NetworkManager signal stream unavailable; retrying");
        }
        // When a connection drops, reinstall match rules for the new owner.
        thread::sleep(Duration::from_secs(1));
    }
}

fn read_network(bus: &Connection) -> Option<NetworkReading> {
    let root = Proxy::new(bus, NM, NM_PATH, NM_IFACE).ok()?;
    // 1=none, 2=portal, 3=limited, 4=full, 0=unknown.
    let connectivity = root.get_property::<u32>("Connectivity").unwrap_or(0);
    let active_paths: Vec<OwnedObjectPath> = root.get_property("ActiveConnections").ok()?;
    let primary: Option<OwnedObjectPath> = root.get_property("PrimaryConnection").ok();
    let mut choices: Vec<(bool, NetworkKind, u8)> = Vec::new();
    let mut incomplete = false;
    for path in active_paths {
        let Ok(active) = Proxy::new(bus, NM, path.as_str(), ACTIVE_IFACE) else {
            incomplete = true;
            continue;
        };
        let state = match active.get_property::<u32>("State") {
            Ok(state) => state,
            Err(_) => { incomplete = true; continue; }
        };
        if state != 2 { continue; }
        let connection_type = match active.get_property::<String>("Type") {
            Ok(value) => value,
            Err(_) => { incomplete = true; continue; }
        };
        let kind = match connection_type.as_str() {
            "802-3-ethernet" => NetworkKind::Ethernet,
            "802-11-wireless" => NetworkKind::Wifi,
            _ => continue, // VPNs are not the physical network indicator.
        };
        let strength = if kind == NetworkKind::Wifi {
            wifi_strength(bus, &active).unwrap_or(0)
        } else { 0 };
        let is_primary = primary.as_ref().is_some_and(|p| p.as_str() == path.as_str());
        choices.push((is_primary, kind, strength));
    }
    // Prefer an active primary physical link, then wired if the primary is a VPN.
    choices.sort_by_key(|(primary, kind, _)| (!*primary, *kind != NetworkKind::Ethernet));
    if let Some((_, kind, strength)) = choices.first().copied() {
        return Some(NetworkReading::physical(kind, strength, connectivity));
    }
    // A valid empty/unsupported physical-link list is an offline observation;
    // a partial D-Bus failure is UNKNOWN and should not trigger a false offline.
    if incomplete { None } else { Some(NetworkReading::offline()) }
}

fn wifi_strength(bus: &Connection, active: &Proxy<'_>) -> Option<u8> {
    let paths: Vec<OwnedObjectPath> = active.get_property("Devices").ok()?;
    for path in paths {
        let Ok(device) = Proxy::new(bus, NM, path.as_str(), WIRELESS_IFACE) else { continue };
        let Ok(ap_path) = device.get_property::<OwnedObjectPath>("ActiveAccessPoint") else { continue };
        if ap_path.as_str() == "/" { continue; }
        if let Ok(ap) = Proxy::new(bus, NM, ap_path.as_str(), ACCESS_POINT_IFACE)
            && let Ok(strength) = ap.get_property::<u8>("Strength") {
            return Some(strength.min(100));
        }
    }
    None
}

fn read_host_battery(bus: &Connection) -> Option<BatteryReading> {
    let device = Proxy::new(bus, UPOWER, DISPLAY_DEVICE, UPOWER_DEVICE).ok()?;
    // UPower's display device aggregates *system* batteries and excludes
    // wireless peripheral batteries from host charge status.
    let present = device.get_property::<bool>("IsPresent").ok()?;
    let kind = device.get_property::<u32>("Type").ok()?;
    if !present || kind != 2 { return None; } // UPower Battery (not UPS or mouse).
    let percent = device.get_property::<f64>("Percentage").ok()?;
    let state = device.get_property::<u32>("State").unwrap_or(0);
    parse_battery(percent, state)
}

// SDL3 remains the first choice for controller *percentage*. UPower's Gaming
// Input entry also reports charging state, unlike BlueZ Battery1, which only
// supplies a percentage. Prefer UPower when available so its charging flag can
// supplement an SDL report whose power state is Unknown. Neither bus can
// manufacture a charge reading that the receiver/driver never publishes.
fn read_controller_battery(bus: &Connection) -> Option<BatteryReading> {
    read_upower_gamepad_battery(bus).or_else(|| read_bluez_gamepad_battery(bus))
}

fn read_upower_gamepad_battery(bus: &Connection) -> Option<BatteryReading> {
    let root = Proxy::new(bus, UPOWER, UPOWER_ROOT, UPOWER_IFACE).ok()?;
    let paths: Vec<OwnedObjectPath> = root.call("EnumerateDevices", &()).ok()?;
    let mut candidates = Vec::new();
    for path in paths {
        let Ok(device) = Proxy::new(bus, UPOWER, path.as_str(), UPOWER_DEVICE) else { continue };
        if device.get_property::<u32>("Type").ok() != Some(UPOWER_GAME_CONTROLLER_TYPE) {
            continue;
        }
        if device.get_property::<bool>("IsPresent").ok() != Some(true) { continue; }
        let Ok(percent) = device.get_property::<f64>("Percentage") else { continue };
        let state = device.get_property::<u32>("State").unwrap_or(0);
        if let Some(battery) = parse_battery(percent, state) {
            candidates.push(battery);
        }
    }
    // Never choose a random controller when more than one has a battery.
    single_candidate(candidates)
}

fn read_bluez_gamepad_battery(bus: &Connection) -> Option<BatteryReading> {
    let objects = Proxy::new(bus, BLUEZ, "/", BLUEZ_OBJECT_MANAGER).ok()?;
    let managed: HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>> =
        objects.call("GetManagedObjects", &()).ok()?;
    let mut candidates = Vec::new();
    for (path, interfaces) in managed {
        if !interfaces.contains_key(BLUEZ_BATTERY) || !interfaces.contains_key(BLUEZ_DEVICE) {
            continue;
        }
        let Ok(device) = Proxy::new(bus, BLUEZ, path.as_str(), BLUEZ_DEVICE) else { continue };
        if device.get_property::<bool>("Connected").ok() != Some(true) { continue; }
        let icon = device.get_property::<String>("Icon").unwrap_or_default();
        let name = device.get_property::<String>("Name")
            .or_else(|_| device.get_property::<String>("Alias"))
            .unwrap_or_default();
        if !is_gamepad_device(&icon, &name) { continue; }
        let Ok(battery) = Proxy::new(bus, BLUEZ, path.as_str(), BLUEZ_BATTERY) else { continue };
        let Ok(percent) = battery.get_property::<u8>("Percentage") else { continue };
        if percent > 100 { continue; }
        // BlueZ Battery1 has no charging-state property. Do not guess.
        candidates.push(BatteryReading { percent, charging: false });
    }
    single_candidate(candidates)
}

fn single_candidate(values: Vec<BatteryReading>) -> Option<BatteryReading> {
    if values.len() == 1 { values.into_iter().next() } else { None }
}

fn is_gamepad_device(icon: &str, name: &str) -> bool {
    if icon == "input-gaming" { return true; }
    let name = name.to_ascii_lowercase();
    (name.contains("8bitdo") && (name.contains("ultimate") || name.contains("pro") ||
        name.contains("controller") || name.contains("sn30"))) ||
        name.contains("gamepad") ||
        name.contains("game controller") || name.contains("xbox wireless controller") ||
        name.contains("dualsense") || name.contains("dualshock") ||
        name.contains("pro controller")
}

fn parse_battery(percent: f64, state: u32) -> Option<BatteryReading> {
    if !percent.is_finite() || !(0.0..=100.0).contains(&percent) { return None; }
    // UPower DeviceState=1 is actively Charging. State=5 is PendingCharge:
    // plugged in but charge paused (e.g. battery conservation threshold),
    // so it must not be shown as an active lightning-bolt charge state.
    Some(BatteryReading { percent: percent.round() as u8, charging: state == 1 })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_charge_never_fakes_full() {
        assert_eq!(parse_battery(f64::NAN, 1), None);
        assert_eq!(parse_battery(-1.0, 2), None);
        assert_eq!(parse_battery(101.0, 2), None);
    }
    #[test]
    fn charging_and_percentage() {
        assert_eq!(parse_battery(47.7, 1), Some(BatteryReading { percent: 48, charging: true }));
        assert_eq!(parse_battery(0.0, 2), Some(BatteryReading { percent: 0, charging: false }));
        assert_eq!(parse_battery(100.0, 4), Some(BatteryReading { percent: 100, charging: false }));
        assert_eq!(parse_battery(80.0, 5), Some(BatteryReading { percent: 80, charging: false }));
        assert_eq!(parse_battery(80.0, 6), Some(BatteryReading { percent: 80, charging: false }));
    }
    #[test]
    fn connected_ethernet_survives_one_failed_or_disconnected_poll() {
        let mut filter = NetworkFilter::default();
        let ethernet = NetworkReading::physical(NetworkKind::Ethernet, 0, 4);
        assert_eq!(filter.update(Some(ethernet)), ethernet);
        assert_eq!(filter.update(None), ethernet);
        assert_eq!(filter.update(Some(NetworkReading::offline())), ethernet);
        assert_eq!(filter.update(Some(ethernet)), ethernet);
        // Genuine disconnection is published after two consecutive reads.
        assert_eq!(filter.update(Some(NetworkReading::offline())), ethernet);
        assert_eq!(filter.update(Some(NetworkReading::offline())).kind, NetworkKind::Offline);
    }

    #[test]
    fn repeated_poll_failure_expires_cached_link() {
        let mut filter = NetworkFilter::default();
        let ethernet = NetworkReading::physical(NetworkKind::Ethernet, 0, 4);
        filter.update(Some(ethernet));
        assert_eq!(filter.update(None), ethernet);
        assert_eq!(filter.update(None), ethernet);
        assert_eq!(filter.update(None).kind, NetworkKind::Offline);
        // A new valid snapshot restores the wired icon immediately.
        assert_eq!(filter.update(Some(ethernet)), ethernet);
    }

    #[test]
    fn internet_check_failure_keeps_physical_link_visible() {
        let result = NetworkReading::physical(NetworkKind::Ethernet, 0, 1);
        assert_eq!(result.kind, NetworkKind::Ethernet);
        assert!(result.limited);
        assert_eq!(NetworkReading::physical(NetworkKind::Ethernet, 0, 0).kind, NetworkKind::Ethernet);
    }

}

#[cfg(test)]
mod peripheral_tests {
    use super::*;

    #[test]
    fn only_gamepads_can_use_the_bluez_fallback() {
        assert!(is_gamepad_device("", "8BitDo Ultimate 2"));
        assert!(is_gamepad_device("input-gaming", "Wireless Controller"));
        assert!(!is_gamepad_device("audio-headphones", "My Headphones"));
        assert!(!is_gamepad_device("input-mouse", "Bluetooth Mouse"));
    }

    #[test]
    fn ambiguous_peripherals_have_no_selected_battery() {
        let battery = BatteryReading { percent: 65, charging: false };
        assert_eq!(single_candidate(vec![]), None);
        assert_eq!(single_candidate(vec![battery]), Some(battery));
        assert_eq!(single_candidate(vec![battery, battery]), None);
    }
}

#[cfg(test)]
mod update_merging_tests {
    use super::*;
    #[test]
    fn independently_received_updates_retain_both_status_categories() {
        let ethernet = NetworkReading::physical(NetworkKind::Ethernet, 0, 4);
        let battery = BatteryReading { percent: 38, charging: true };
        let mut value = SystemStatus::default();
        let events = [StatusUpdate::Power { host: None, controller: Some(battery) }, StatusUpdate::Network(ethernet)];
        for update in events {
            match update {
                StatusUpdate::Network(n) => { value.network=n.kind; value.limited=n.limited; value.wifi_strength=n.wifi_strength; }
                StatusUpdate::Power {host, controller} => {value.host_battery=host;value.controller_battery_fallback=controller;}
            }
        }
        assert_eq!(value.network, NetworkKind::Ethernet);
        assert_eq!(value.controller_battery_fallback, Some(battery));
    }
}
