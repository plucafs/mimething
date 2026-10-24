use eframe::egui;
use std::path::PathBuf;

#[derive(Default)]
pub struct Settings {
    pub theme: Option<egui::Theme>,
    pub scale: Option<f32>,
}

fn config_path() -> Option<PathBuf> {
    let dir = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))?;
    let dir = dir.join("defeditor");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("settings.conf"))
}

pub fn load() -> Settings {
    let mut s = Settings::default();
    let Some(path) = config_path() else {
        return s;
    };
    let Ok(content) = std::fs::read_to_string(path) else {
        return s;
    };
    for line in content.lines() {
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim();
            match key.trim() {
                "theme" => match value {
                    "dark" => s.theme = Some(egui::Theme::Dark),
                    "light" => s.theme = Some(egui::Theme::Light),
                    _ => {}
                },
                "scale" => {
                    if let Ok(f) = value.parse::<f32>()
                        && f > 0.05
                        && f < 10.0
                    {
                        s.scale = Some(f);
                    }
                }
                _ => {}
            }
        }
    }
    s
}

pub fn save(s: &Settings) {
    let Some(path) = config_path() else {
        return;
    };
    let mut out = String::new();
    match s.theme {
        Some(egui::Theme::Dark) => out.push_str("theme = dark\n"),
        Some(egui::Theme::Light) => out.push_str("theme = light\n"),
        None => {}
    }
    if let Some(scale) = s.scale {
        out.push_str(&format!("scale = {scale}\n"));
    }
    let _ = std::fs::write(path, out);
}
