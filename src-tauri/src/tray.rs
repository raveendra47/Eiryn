use tauri::{
    AppHandle, Wry,
    menu::{CheckMenuItem, CheckMenuItemBuilder, Menu, MenuItem, MenuItemBuilder, PredefinedMenuItem},
    tray::{TrayIcon, TrayIconBuilder},
};

pub struct TrayMenu {
    pub tray: TrayIcon<Wry>,
    pub play_pause: MenuItem<Wry>,
    pub next_item: MenuItem<Wry>,
    pub prev_item: MenuItem<Wry>,
    pub repeat: CheckMenuItem<Wry>,
    pub shuffle: CheckMenuItem<Wry>,
    pub favorite: CheckMenuItem<Wry>,
    track_label: MenuItem<Wry>,
    next_label: MenuItem<Wry>,
}

impl TrayMenu {
    pub fn new(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let play_pause = MenuItemBuilder::with_id("play_pause", "Play").build(app)?;
        let next_item = MenuItemBuilder::with_id("next", "Next Track").build(app)?;
        let prev_item = MenuItemBuilder::with_id("prev", "Previous Track").build(app)?;
        let repeat = CheckMenuItemBuilder::with_id("repeat", "Repeat")
            .enabled(true)
            .checked(false)
            .build(app)?;
        let shuffle = CheckMenuItemBuilder::with_id("shuffle", "Shuffle")
            .enabled(true)
            .checked(false)
            .build(app)?;
        let favorite = CheckMenuItemBuilder::with_id("favorite", "Favorite")
            .enabled(true)
            .checked(false)
            .build(app)?;
        let track_label = MenuItemBuilder::with_id("track", "No track playing")
            .enabled(false)
            .build(app)?;
        let next_label = MenuItemBuilder::with_id("next_track", "Next: None")
            .enabled(false)
            .build(app)?;
        let show = MenuItemBuilder::with_id("show", "Show")
            .accelerator("CmdOrCtrl+Shift+M")
            .build(app)?;
        let quit = MenuItemBuilder::with_id("quit", "Quit")
            .accelerator("CmdOrCtrl+Q")
            .build(app)?;
        let s1 = PredefinedMenuItem::separator(app)?;
        let s2 = PredefinedMenuItem::separator(app)?;
        let s3 = PredefinedMenuItem::separator(app)?;
        let s4 = PredefinedMenuItem::separator(app)?;

        let menu = Menu::with_items(app, &[
            &track_label, &next_label, &s1,
            &play_pause, &next_item, &prev_item, &s2,
            &repeat, &shuffle, &s3,
            &favorite, &s4,
            &show, &quit,
        ])?;

        let icon = app.default_window_icon()
            .cloned()
            .ok_or("No default window icon bundled — tray cannot be created")?;

        let tray = TrayIconBuilder::new()
            .icon(icon)
            .menu(&menu)
            .tooltip("Eiryn")
            .build(app)?;

        Ok(Self {
            tray,
            play_pause, next_item, prev_item,
            repeat, shuffle, favorite,
            track_label, next_label,
        })
    }

    pub fn set_playing(&self, playing: bool) {
        let _ = self.play_pause.set_text(if playing { "Pause" } else { "Play" });
    }

    pub fn set_track_info(&self, title: &str, artist: &str) {
        let label = if title.is_empty() {
            "No track playing".into()
        } else if artist.is_empty() {
            title.to_string()
        } else {
            format!("{title} — {artist}")
        };
        let _ = self.track_label.set_text(&label);
    }

    pub fn set_next_info(&self, title: &str) {
        let label = if title.is_empty() {
            "Next: None".into()
        } else {
            format!("Next: {title}")
        };
        let _ = self.next_label.set_text(&label);
    }

    pub fn set_repeat(&self, mode: &str) {
        let (label, checked) = match mode {
            "all" => ("Repeat (All)", true),
            "one" => ("Repeat (One)", true),
            _ => ("Repeat", false),
        };
        let _ = self.repeat.set_text(label);
        let _ = self.repeat.set_checked(checked);
    }

    pub fn set_shuffle(&self, enabled: bool) {
        let _ = self.shuffle.set_checked(enabled);
    }

    pub fn set_favorite(&self, enabled: bool) {
        let _ = self.favorite.set_checked(enabled);
    }

    pub fn set_next_enabled(&self, enabled: bool) {
        let _ = self.next_item.set_enabled(enabled);
    }

    pub fn set_prev_enabled(&self, enabled: bool) {
        let _ = self.prev_item.set_enabled(enabled);
    }
}
