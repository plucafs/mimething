use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone)]
pub struct DesktopApp {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub mimes: Vec<String>,
}

fn applications_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }
    let xdg_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for d in xdg_dirs.split(':').filter(|s| !s.is_empty()) {
        dirs.push(PathBuf::from(d).join("applications"));
    }
    dirs
}

fn parse_desktop_file(path: &PathBuf) -> Option<DesktopApp> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let mut typ = String::new();
    let mut name = String::new();
    let mut icon = String::new();
    let mut mimes = Vec::new();
    let mut no_display = false;
    let mut hidden = false;

    for line in content.lines() {
        let line = line.trim_end();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "Type" => typ = value.to_string(),
            "Name" => name = value.to_string(),
            "Icon" => icon = value.to_string(),
            "NoDisplay" => no_display = value == "true",
            "Hidden" => hidden = value == "true",
            "MimeType" => {
                mimes = value
                    .split(';')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
            }
            _ => {}
        }
    }

    if typ != "Application" || name.is_empty() || no_display || hidden {
        return None;
    }
    let id = path.file_name()?.to_string_lossy().to_string();
    Some(DesktopApp {
        id,
        name,
        icon,
        mimes,
    })
}

pub fn load_apps() -> Vec<DesktopApp> {
    let mut by_id: HashMap<String, DesktopApp> = HashMap::new();
    for dir in applications_dirs().into_iter().rev() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            if let Some(app) = parse_desktop_file(&path) {
                by_id.insert(app.id.clone(), app);
            }
        }
    }
    let mut apps: Vec<DesktopApp> = by_id.into_values().collect();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}
