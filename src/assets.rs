use gtk::gio::prelude::*;
use std::path::{Path, PathBuf};

pub struct Assets {
    pub dir: PathBuf,
    pub ui_uri: String,
    pub home_uri: String,
    pub icon_path: PathBuf,
    pub icon_uri: String,
    pub icon_name: String,
    pub default_favicon_uri: String,
}

impl Assets {
    pub fn new() -> Self {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let ui_path = dir.join("ui.html");
        let home_path = dir.join("home.html");
        let icon_path = dir.join("icon-128x128.ico");
        let default_favicon_path = dir.join("world_wide_web-128x128.ico");
        let icon_uri = file_uri(&icon_path);
        let default_favicon_uri = if default_favicon_path.exists() {
            file_uri(&default_favicon_path)
        } else {
            icon_uri.clone()
        };
        let icon_name = icon_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("owl-browser")
            .to_string();

        Self {
            dir,
            ui_uri: file_uri(&ui_path),
            home_uri: file_uri(&home_path),
            icon_path,
            icon_uri,
            icon_name,
            default_favicon_uri,
        }
    }

    pub fn register_icon(&self, display: &gtk::gdk::Display) -> Option<String> {
        if !self.icon_path.exists() {
            return None;
        }

        let icon_theme = gtk::IconTheme::for_display(display);
        icon_theme.add_search_path(&self.dir);
        gtk::Window::set_default_icon_name(&self.icon_name);
        Some(self.icon_name.clone())
    }
}

fn file_uri(path: &Path) -> String {
    gtk::gio::File::for_path(path).uri().to_string()
}
