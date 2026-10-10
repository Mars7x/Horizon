//! Combines independent host/SDL observations for the shell status area.
//! Presentation never reads D-Bus or hardware directly.

use crate::{
    AppWindow,
    input::ControllerStatus,
    platform::system_status::{BatteryReading, SystemStatus},
};

pub struct StatusController;

impl StatusController {
    pub fn publish(ui: &AppWindow, host: SystemStatus, controller: ControllerStatus) {
        use crate::platform::system_status::NetworkKind;
        ui.set_connected_controller_count(controller.connected_gamepads as i32);
        ui.set_status_network_kind(host.network.ui_code());
        ui.set_status_wifi_strength(if host.network == NetworkKind::Wifi {
            host.wifi_strength as i32
        } else {
            0
        });
        ui.set_status_network_limited(host.limited);
        let battery = preferred_battery(
            host.host_battery,
            controller.battery,
            if controller.connected_gamepads == 1 {
                host.controller_battery_fallback
            } else {
                // With multiple pads, don't assign a system-bus percentage
                // to the wrong physical controller.
                None
            },
        );
        ui.set_status_battery_visible(battery.is_some());
        ui.set_status_battery_percent(battery.map_or(0, |b| b.percent as i32));
        ui.set_status_battery_charging(battery.is_some_and(|b| b.charging));
    }
}

fn preferred_battery(
    host: Option<BatteryReading>,
    controller: Option<crate::input::sdl::ControllerBattery>,
    fallback: Option<BatteryReading>,
) -> Option<BatteryReading> {
    if let Some(host) = host {
        return Some(host);
    }
    if let Some(sdl) = controller {
        // Keep SDL's measured percentage and explicit charging state. When
        // SDL only knows the percentage, accept UPower's positive charging
        // signal if it plausibly describes the same battery (and only after
        // the single-controller/single-peripheral restriction in publish()).
        // BlueZ supplies no charging signal and must never be treated as one.
        let fallback_charging = !sdl.charging_known
            && fallback
                .is_some_and(|other| other.charging && sdl.percent.abs_diff(other.percent) <= 10);
        return Some(BatteryReading {
            percent: sdl.percent,
            charging: sdl.charging || fallback_charging,
        });
    }
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::sdl::ControllerBattery;
    #[test]
    fn host_always_wins() {
        let host = BatteryReading {
            percent: 12,
            charging: true,
        };
        let pad = ControllerBattery {
            percent: 99,
            charging: false,
            charging_known: true,
        };
        assert_eq!(
            preferred_battery(
                Some(host),
                Some(pad),
                Some(BatteryReading {
                    percent: 80,
                    charging: false
                })
            ),
            Some(host)
        );
    }
    #[test]
    fn charging_from_upower_augments_unknown_sdl_state_only_for_matching_percentage() {
        let sdl_unknown = ControllerBattery {
            percent: 19,
            charging: false,
            charging_known: false,
        };
        let upower = BatteryReading {
            percent: 20,
            charging: true,
        };
        assert_eq!(
            preferred_battery(None, Some(sdl_unknown), Some(upower)),
            Some(BatteryReading {
                percent: 19,
                charging: true
            })
        );
        // A peripheral reporting a very different percentage is not safely
        // identifiable as the same SDL gamepad: never borrow its power state.
        assert!(
            !preferred_battery(
                None,
                Some(sdl_unknown),
                Some(BatteryReading {
                    percent: 70,
                    charging: true
                })
            )
            .unwrap()
            .charging
        );
        // A definitive SDL discharging state is not overridden by a bus value.
        let sdl_known = ControllerBattery {
            charging_known: true,
            ..sdl_unknown
        };
        assert!(
            !preferred_battery(None, Some(sdl_known), Some(upower))
                .unwrap()
                .charging
        );
        // A known SDL charging state remains authoritative.
        let sdl_charging = ControllerBattery {
            percent: 19,
            charging: true,
            charging_known: true,
        };
        assert!(
            preferred_battery(None, Some(sdl_charging), None)
                .unwrap()
                .charging
        );
    }

    #[test]
    fn controller_fallback_and_desktop_hide() {
        let pad = ControllerBattery {
            percent: 44,
            charging: false,
            charging_known: true,
        };
        assert_eq!(
            preferred_battery(
                None,
                Some(pad),
                Some(BatteryReading {
                    percent: 83,
                    charging: false
                })
            )
            .map(|b| b.percent),
            Some(44)
        );
        assert_eq!(preferred_battery(None, None, None), None);
        let bluetooth = BatteryReading {
            percent: 67,
            charging: false,
        };
        assert_eq!(
            preferred_battery(None, None, Some(bluetooth)),
            Some(bluetooth)
        );
    }
}
