//! Searching the settings: what the title pill's field looks through. Each page is listed with
//! its groups and a few words people would look for; a query matches the page's name first, then
//! a group's title, then those words.

use crate::sections::Section;

/// One place a query can lead: a page, and the group on it (or the page itself when empty).
struct Entry {
    section: Section,
    group: &'static str,
    /// Words that find it, besides its own name.
    words: &'static str,
}

const fn e(section: Section, group: &'static str, words: &'static str) -> Entry {
    Entry { section, group, words }
}

const ENTRIES: &[Entry] = &[
    e(Section::Appearance, "", "look theme dark light colour color style"),
    e(Section::Appearance, "Accent color", "colour swatch windows custom highlight"),
    e(Section::Appearance, "Glass", "transparency blur acrylic solid clear opacity"),
    e(Section::Desktop, "", "wallpaper background icons right-click"),
    e(Section::Desktop, "Desktop style", "clean widgets shelf"),
    e(Section::Desktop, "Shelf", "stacks downloads recent drop edge"),
    e(Section::Sound, "", "audio speakers microphone volume"),
    e(Section::Sound, "Output", "speakers headphones volume mute default device spatial atmos format balance test"),
    e(Section::Sound, "Input", "microphone mic level meter record"),
    e(Section::Sound, "Volume mixer", "apps per app sessions"),
    e(Section::Sound, "More sound settings", "classic control panel"),
    e(Section::Display, "", "monitor screen"),
    e(Section::Display, "Resolution", "refresh rate hertz size"),
    e(Section::Display, "Brightness", "dim backlight"),
    e(Section::Display, "Arrangement", "monitors layout position scale orientation rotate primary"),
    e(Section::Mouse, "", "keyboard pointer cursor touchpad"),
    e(Section::Mouse, "Mouse", "pointer speed sensitivity acceleration precision enhance primary button left right swap scroll wheel lines double click"),
    e(Section::Mouse, "Keyboard", "repeat delay rate keys typing key repeat"),
    e(Section::Power, "", "battery energy sleep screen lid plan"),
    e(Section::Power, "Power plan", "mode balanced performance efficiency saver high performance profile"),
    e(Section::Power, "Screen and sleep", "timeout turn off display idle standby suspend battery plugged in"),
    e(Section::Power, "Buttons and lid", "close lid power button hibernate shut down"),
    e(Section::Power, "Battery", "charge level percent charging remaining"),
    e(Section::DateTime, "", "clock region locale language calendar timezone"),
    e(Section::DateTime, "Time zone", "timezone zone utc daylight saving dst automatic pacific eastern clock"),
    e(Section::DateTime, "Format", "12 hour 24 hour clock am pm short date long date first day of week monday sunday calendar region"),
    e(Section::Storage, "", "disk drive space free used capacity"),
    e(Section::Storage, "Drives", "disk c: free space full capacity partition volume"),
    e(Section::Storage, "On the system drive", "downloads documents desktop pictures videos appdata recycle bin large folders size usage cleanup"),
    e(Section::DefaultApps, "", "default programs open with file types associations browser email handler"),
    e(Section::DefaultApps, "File types and links", "txt pdf png jpg mp3 mp4 zip html http https mailto web browser pdf viewer music video picture photo"),
    e(Section::Apps, "", "programs installed software"),
    e(Section::Apps, "Uninstall", "remove modify change programs"),
    e(Section::Startup, "", "autostart login sign in boot"),
    e(Section::Startup, "Starts when you sign in", "autorun run at startup apps"),
    e(Section::Fonts, "", "system font typeface segoe ui windows text fontman"),
    e(Section::Fonts, "Windows uses", "status restart restore segoe ui current font"),
    e(Section::Fonts, "Fonts", "installed families add font file typeface"),
    e(Section::Fonts, "Preview", "explorer message box sample"),
    e(Section::Fonts, "Weights", "bold semibold light italic face"),
    e(Section::Fonts, "Tune", "glyph size line height slant italics window text title bar menu tooltip"),
    e(Section::Fonts, "Coverage", "window text apps file explorer windows 11 shell old dialogs ms shell dlg"),
    e(Section::Fonts, "Backups", "restore segoe ui use again restore file reg sign in check"),
    e(Section::Dock, "", "taskbar"),
    e(Section::Dock, "Icon size", "big small zoom"),
    e(Section::Dock, "Show the dock on", "monitors screens"),
    e(Section::Dock, "Pinned apps", "favourites favorites pin unpin"),
    e(Section::TopBar, "", "panel menu bar"),
    e(Section::TopBar, "Clock", "time hours 12 24 date"),
    e(Section::TopBar, "Show tray icons on", "notification area monitors"),
    e(Section::Launcher, "", "search start menu"),
    e(Section::Launcher, "Most used", "history clear recent"),
    e(Section::About, "", "version info"),
    e(Section::About, "Windows apps", "classic control panel"),
    e(Section::About, "Settings folder", "files config json"),
];

/// Where a query leads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hit {
    pub section: Section,
    /// The group on the page the query matched, empty for the page itself.
    pub group: &'static str,
}

impl Hit {
    /// The line the result list shows: the page, and the group after it.
    pub fn label(&self) -> String {
        if self.group.is_empty() {
            self.section.label().to_owned()
        } else {
            format!("{}  \u{b7}  {}", self.section.label(), self.group)
        }
    }
}

/// How well `text` matches the lowercase `query`: 3 for the start of the text, 2 for the start
/// of one of its words, 1 for anywhere inside it, 0 for not at all.
fn score(text: &str, query: &str) -> u32 {
    let text = text.to_lowercase();
    if text.starts_with(query) {
        3
    } else if text.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(query)) {
        2
    } else if text.contains(query) {
        1
    } else {
        0
    }
}

/// The places `query` leads to, best first. Every word of the query has to match something in
/// the entry.
pub fn find(query: &str) -> Vec<Hit> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(u32, usize, Hit)> = Vec::new();
    for (order, entry) in ENTRIES.iter().enumerate() {
        let mut total = 0;
        let mut all = true;
        for w in &words {
            // The page's name outranks a group's, which outranks the loose words.
            let best = (score(entry.section.label(), w) * 100)
                .max(score(entry.group, w) * 10)
                .max(score(entry.words, w));
            if best == 0 {
                all = false;
                break;
            }
            total += best;
        }
        if all {
            hits.push((total, order, Hit { section: entry.section, group: entry.group }));
        }
    }
    // Best first; the list's own order settles ties, which keeps a page before its groups.
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.into_iter().map(|(_, _, h)| h).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_name_finds_the_page_first() {
        let hits = find("sound");
        assert_eq!(hits[0], Hit { section: Section::Sound, group: "" });
        assert!(hits.iter().all(|h| h.section == Section::Sound || !h.group.is_empty()));
    }

    #[test]
    fn a_group_title_finds_its_group() {
        assert_eq!(find("glass")[0], Hit { section: Section::Appearance, group: "Glass" });
        assert_eq!(find("pinned")[0].group, "Pinned apps");
    }

    #[test]
    fn loose_words_find_a_group() {
        let hits = find("microphone");
        assert!(hits.contains(&Hit { section: Section::Sound, group: "Input" }));
        assert!(find("refresh").iter().any(|h| h.section == Section::Display));
    }

    #[test]
    fn every_word_has_to_match() {
        assert!(find("sound zebra").is_empty());
        let hits = find("sound mixer");
        assert_eq!(hits[0], Hit { section: Section::Sound, group: "Volume mixer" });
    }

    #[test]
    fn an_empty_query_finds_nothing() {
        assert!(find("").is_empty());
        assert!(find("   ").is_empty());
    }

    #[test]
    fn the_fonts_page_and_its_tabs_are_found() {
        assert_eq!(find("fonts")[0], Hit { section: Section::Fonts, group: "" });
        assert!(find("restore segoe").iter().any(|h| h.section == Section::Fonts));
        assert!(find("line height").iter().any(|h| *h == Hit { section: Section::Fonts, group: "Tune" }));
        assert!(find("old dialogs").iter().any(|h| *h == Hit { section: Section::Fonts, group: "Coverage" }));
    }

    #[test]
    fn every_page_is_reachable() {
        for s in Section::ALL {
            assert!(find(s.label()).iter().any(|h| h.section == s && h.group.is_empty()), "{}", s.label());
        }
    }
}
