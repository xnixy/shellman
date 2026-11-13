//! What the Power settings page offers, from what the machine has. No Win32 here, so every rule
//! is tested: which rows apply, the timeouts to choose from, the actions a button can take, and
//! the battery's sentence.

/// What this machine has.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Caps {
    pub battery: bool,
    pub lid: bool,
    /// Some sleep state (S1 to S3, or connected standby) is there.
    pub sleep: bool,
    pub hibernate: bool,
    pub power_button: bool,
}

/// One setting of the active plan the page edits.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Key {
    ScreenOff,
    Sleep,
    LidClose,
    PowerButton,
}

impl Key {
    pub const ALL: [Key; 4] = [Key::ScreenOff, Key::Sleep, Key::LidClose, Key::PowerButton];

    pub fn label(self) -> &'static str {
        match self {
            Key::ScreenOff => "Turn the screen off after",
            Key::Sleep => "Put the computer to sleep after",
            Key::LidClose => "When I close the lid",
            Key::PowerButton => "When I press the power button",
        }
    }

    /// A timeout (seconds) or an action (one of [`action_choices`]).
    pub fn is_timeout(self) -> bool {
        matches!(self, Key::ScreenOff | Key::Sleep)
    }
}

/// Whether the page shows `key` at all on this machine.
pub fn applies(key: Key, caps: &Caps) -> bool {
    match key {
        Key::ScreenOff => true,
        Key::Sleep => caps.sleep,
        Key::LidClose => caps.lid,
        Key::PowerButton => caps.power_button,
    }
}

/// Whether the "on battery" column is shown: only a machine with a battery has one.
pub fn has_battery_column(caps: &Caps) -> bool {
    caps.battery
}

/// The timeouts offered, in seconds, 0 being "never".
const PRESETS: [u32; 15] = [60, 120, 180, 300, 600, 900, 1200, 1500, 1800, 2700, 3600, 7200, 10800, 14400, 18000];

/// The timeouts to list when the plan has `current`: the presets, with the current value
/// slotted in when it is none of them (set by another tool), "never" last.
pub fn timeout_choices(current: Option<u32>) -> Vec<u32> {
    let mut v: Vec<u32> = PRESETS.to_vec();
    if let Some(c) = current.filter(|c| *c != 0 && !v.contains(c)) {
        v.push(c);
        v.sort_unstable();
    }
    v.push(0);
    v
}

/// "5 minutes", "1 hour", "Never".
pub fn timeout_label(secs: u32) -> String {
    if secs == 0 {
        return "Never".to_owned();
    }
    let unit = |n: u32, one: &str| if n == 1 { format!("{n} {one}") } else { format!("{n} {one}s") };
    if secs < 60 {
        return unit(secs, "second");
    }
    let minutes = secs / 60;
    if minutes < 60 {
        return unit(minutes, "minute");
    }
    let (h, m) = (minutes / 60, minutes % 60);
    if m == 0 {
        unit(h, "hour")
    } else {
        format!("{} {}", unit(h, "hour"), unit(m, "minute"))
    }
}

/// The actions a lid or button can take: Windows' own numbers, 0 nothing, 1 sleep, 2 hibernate,
/// 3 shut down. An action the machine cannot do is not offered, except `current` (so the page
/// can show what is set).
pub fn action_choices(caps: &Caps, current: Option<u32>) -> Vec<(u32, &'static str)> {
    let mut v = vec![(0, "Do nothing")];
    if caps.sleep {
        v.push((1, "Sleep"));
    }
    if caps.hibernate {
        v.push((2, "Hibernate"));
    }
    v.push((3, "Shut down"));
    if let Some(c) = current {
        if !v.iter().any(|(n, _)| *n == c) {
            v.push((c, action_name(c)));
        }
    }
    v
}

pub fn action_name(n: u32) -> &'static str {
    match n {
        0 => "Do nothing",
        1 => "Sleep",
        2 => "Hibernate",
        3 => "Shut down",
        4 => "Turn off the display",
        _ => "Another action",
    }
}

/// `GetSystemPowerStatus`, as far as the sentence needs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Status {
    /// 0 on battery, 1 plugged in, 255 unknown.
    pub ac_line: u8,
    pub charging: bool,
    /// 0..=100, or 255 when unknown.
    pub percent: u8,
    /// Seconds left on battery, or `u32::MAX` when unknown.
    pub seconds_left: u32,
    pub saver_on: bool,
}

/// "78%, charging" / "42%, 2 h 10 min left" / "100%, plugged in".
pub fn battery_text(s: &Status) -> String {
    let level = if s.percent <= 100 { format!("{}%", s.percent) } else { "Level unknown".to_owned() };
    let state = if s.charging {
        "charging".to_owned()
    } else if s.ac_line == 1 {
        "plugged in".to_owned()
    } else if s.seconds_left != u32::MAX {
        let m = s.seconds_left / 60;
        if m >= 60 {
            format!("{} h {} min left", m / 60, m % 60)
        } else {
            format!("{m} min left")
        }
    } else {
        "on battery".to_owned()
    };
    let saver = if s.saver_on { ", battery saver on" } else { "" };
    format!("{level}, {state}{saver}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desktop() -> Caps {
        Caps { battery: false, lid: false, sleep: true, hibernate: true, power_button: true }
    }

    #[test]
    fn a_desktop_has_no_lid_row_and_no_battery_column() {
        assert!(!applies(Key::LidClose, &desktop()));
        assert!(applies(Key::ScreenOff, &desktop()));
        assert!(applies(Key::PowerButton, &desktop()));
        assert!(!has_battery_column(&desktop()));
    }

    #[test]
    fn no_sleep_state_hides_the_sleep_row() {
        let c = Caps { sleep: false, ..desktop() };
        assert!(!applies(Key::Sleep, &c));
        assert_eq!(action_choices(&c, None).iter().map(|a| a.0).collect::<Vec<_>>(), [0, 2, 3]);
    }

    #[test]
    fn timeouts_are_sorted_with_never_last_and_keep_an_odd_value() {
        let t = timeout_choices(Some(420));
        assert_eq!(t.last(), Some(&0));
        assert!(t.windows(2).take(t.len() - 2).all(|w| w[0] < w[1]));
        assert!(t.contains(&420));
        assert_eq!(timeout_choices(Some(300)).len(), PRESETS.len() + 1);
        assert_eq!(timeout_choices(Some(0)).len(), PRESETS.len() + 1);
    }

    #[test]
    fn timeout_words() {
        assert_eq!(timeout_label(0), "Never");
        assert_eq!(timeout_label(60), "1 minute");
        assert_eq!(timeout_label(300), "5 minutes");
        assert_eq!(timeout_label(3600), "1 hour");
        assert_eq!(timeout_label(5400), "1 hour 30 minutes");
        assert_eq!(timeout_label(45), "45 seconds");
    }

    #[test]
    fn an_action_set_elsewhere_is_still_shown() {
        let a = action_choices(&desktop(), Some(4));
        assert_eq!(a.last(), Some(&(4, "Turn off the display")));
    }

    #[test]
    fn battery_sentences() {
        let on = Status { ac_line: 0, charging: false, percent: 42, seconds_left: 7800, saver_on: false };
        assert_eq!(battery_text(&on), "42%, 2 h 10 min left");
        let plug = Status { ac_line: 1, percent: 100, seconds_left: u32::MAX, ..on };
        assert_eq!(battery_text(&plug), "100%, plugged in");
        let ch = Status { charging: true, ac_line: 1, percent: 78, seconds_left: u32::MAX, saver_on: true };
        assert_eq!(battery_text(&ch), "78%, charging, battery saver on");
        let unknown = Status { ac_line: 255, percent: 255, seconds_left: u32::MAX, ..Status::default() };
        assert_eq!(battery_text(&unknown), "Level unknown, on battery");
    }
}
