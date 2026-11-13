//! What the window edits: every settings file, held in memory, with the way to save each one.
//! Edits apply by saving the same files the other apps watch.

use crate::saver::Saver;
use crate::sections::{WidgetsView, AppsView, DisplayView, FontsView, IconsView, MouseView, PowerView, DateTimeView, StorageView, DefaultAppsView, Section, SoundView, StartupView, UpdatesView};
use shellman_core::settings::bar::BarSettings;
use shellman_core::settings::desktop::DesktopSettings;
use shellman_core::settings::dock::{self, DockSettings};
use shellman_core::settings::launcher::LauncherSettings;
use shellman_core::settings::shell::ShellSettings;
use shellman_core::system::app_name;
use shellman_core::thumbs::Thumbs;
use shellman_ui::appearance::Appearance;

pub struct State {
    pub look: Appearance,
    pub desktop: DesktopSettings,
    /// The Desktop page's folder picker for dropped files, while it is open.
    pub desktop_pick: Option<crate::pick::Pick>,
    /// The Desktop page's widget rows: what is typed, and the connection test.
    pub widgets_view: WidgetsView,
    pub dock: DockSettings,
    /// What each pin is called, in the pins' order. Looking a name up touches the shell, so it is
    /// done when the pins change, not on every frame.
    pub dock_names: Vec<String>,
    pub bar: BarSettings,
    pub launcher: LauncherSettings,
    pub shell: ShellSettings,
    pub saver: Saver,
    pub thumbs: Thumbs,
    /// The Sound page's live state; started when the page is shown.
    pub sound: SoundView,
    /// The Display page's live state; started when the page is shown.
    pub display: DisplayView,
    /// The Mouse & keyboard page's reading; taken when the page is shown.
    pub mouse: MouseView,
    /// The Default apps page's state; started or read when the page is shown.
    pub default_apps: DefaultAppsView,
    /// The Storage page's state; started or read when the page is shown.
    pub storage: StorageView,
    /// The Date, time & region page's state; started or read when the page is shown.
    pub datetime: DateTimeView,
    /// The Power page's state; started or read when the page is shown.
    pub power: PowerView,
    /// The Windows Update page's state; its worker runs while the page is shown.
    pub updates: UpdatesView,
    /// The Apps page's live state; started when the page is shown.
    pub apps: AppsView,
    /// The Startup apps page's live state; started when the page is shown.
    pub startup: StartupView,
    /// The Fonts page's live state; started when the page is shown.
    pub fonts: FontsView,
    /// A page another page asked to go to (the Fonts page's link to Appearance).
    pub goto: Option<Section>,
    /// The icon pack group's live state (Appearance).
    pub icons: IconsView,
    /// Whether the window has real acrylic; without it the glass slider has nothing to do.
    pub glass: bool,
}

impl State {
    pub fn new(ctx: &egui::Context, look: Appearance, glass: bool) -> Self {
        let mut state = Self {
            look,
            desktop: DesktopSettings::load(),
            desktop_pick: None,
            widgets_view: WidgetsView::default(),
            dock: DockSettings::load(),
            dock_names: Vec::new(),
            bar: BarSettings::load(),
            launcher: LauncherSettings::load(),
            shell: ShellSettings::load(),
            saver: Saver::new(),
            thumbs: Thumbs::new(ctx.clone()),
            sound: SoundView::default(),
            display: DisplayView::default(),
            mouse: MouseView::default(),
            default_apps: DefaultAppsView::default(),
            storage: StorageView::default(),
            datetime: DateTimeView::default(),
            power: PowerView::default(),
            updates: UpdatesView::default(),
            apps: AppsView::default(),
            startup: StartupView::default(),
            fonts: FontsView::default(),
            goto: None,
            icons: IconsView::default(),
            glass,
        };
        state.rebuild_names();
        state
    }

    pub fn rebuild_names(&mut self) {
        self.dock_names = self
            .dock
            .pins
            .iter()
            .map(|p| app_name::display_name(&p.app_id, p.exe.as_deref()))
            .collect();
    }

    /// The icon to show for a pin, if it has a source.
    pub fn pin_icon(&self, index: usize) -> Option<std::path::PathBuf> {
        let pin = self.dock.pins.get(index)?;
        dock::icon_source(&pin.app_id, pin.exe.as_deref())
    }

    /// Make this window wear the look.
    pub fn apply_look(&self, ctx: &egui::Context) {
        self.look.apply(ctx);
    }

    pub fn save_look(&self) {
        let look = self.look;
        self.saver.run(move || look.save());
    }

    pub fn save_desktop(&self) {
        let desktop = self.desktop.clone();
        self.saver.run(move || desktop.save());
    }

    pub fn save_dock(&self) {
        let dock = self.dock.clone();
        self.saver.run(move || dock.save());
    }

    pub fn save_bar(&self) {
        let bar = self.bar;
        self.saver.run(move || bar.save());
    }

    pub fn save_launcher(&self) {
        let launcher = self.launcher.clone();
        self.saver.run(move || launcher.save());
    }

    pub fn save_shell(&self) {
        let shell = self.shell;
        self.saver.run(move || shell.save());
    }

    /// Read every file again, after something else changed one (the dock pinning an app, say).
    /// True when anything differs.
    pub fn reload(&mut self, ctx: &egui::Context) -> bool {
        let mut changed = false;
        let look: Appearance = shellman_core::config::load("appearance");
        if look != self.look {
            self.look = look;
            self.apply_look(ctx);
            changed = true;
        }
        let desktop = DesktopSettings::load();
        if desktop != self.desktop {
            self.desktop = desktop;
            changed = true;
        }
        let dock = DockSettings::load();
        if dock != self.dock {
            self.dock = dock;
            self.rebuild_names();
            changed = true;
        }
        let bar = BarSettings::load();
        if bar != self.bar {
            self.bar = bar;
            changed = true;
        }
        let launcher = LauncherSettings::load();
        if launcher != self.launcher {
            self.launcher = launcher;
            changed = true;
        }
        let shell = ShellSettings::load();
        if shell != self.shell {
            self.shell = shell;
            changed = true;
        }
        changed
    }
}
