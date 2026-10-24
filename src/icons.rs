use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const SIZES: [u32; 6] = [128, 96, 64, 48, 32, 24];

/// Themes in search order; oxygen ships big PNGs for generic mimetypes.
const THEMES: [&str; 4] = ["hicolor", "oxygen", "Adwaita", "breeze"];

fn theme_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        roots.push(PathBuf::from(&home).join(".local/share/icons"));
        roots.push(PathBuf::from(home).join(".icons"));
    }
    roots.push(PathBuf::from("/usr/share/icons"));
    roots
}

fn candidates(root: &Path, theme: &str, size: u32, name: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let px = format!("{size}x{size}");
    for sub in ["apps", "mimetypes"] {
        v.push(root.join(theme).join(&px).join(sub).join(format!("{name}.png")));
        v.push(root.join(theme).join("base").join(&px).join(sub).join(format!("{name}.png")));
        v.push(root.join(theme).join(sub).join(size.to_string()).join(format!("{name}.png")));
    }
    v
}

fn resolve(icon_name: &str) -> Option<String> {
    let name = icon_name.trim();
    if name.is_empty() {
        return None;
    }
    if name.starts_with('/') && Path::new(name).is_file() {
        return Some(format!("file://{name}"));
    }
    for root in theme_roots() {
        if !root.is_dir() {
            continue;
        }
        for theme in THEMES {
            for size in SIZES {
                for path in candidates(&root, theme, size, name) {
                    if path.is_file() {
                        return Some(format!("file://{}", path.display()));
                    }
                }
            }
        }
    }
    let p = format!("/usr/share/pixmaps/{name}.png");
    Path::new(&p).is_file().then(|| format!("file://{p}"))
}

pub fn icon_uri(icon_name: &str) -> Option<String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(hit) = cache.lock().unwrap().get(icon_name) {
        return hit.clone();
    }
    let uri = resolve(icon_name);
    cache
        .lock()
        .unwrap()
        .insert(icon_name.to_string(), uri.clone());
    uri
}
