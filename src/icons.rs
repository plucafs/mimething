use std::path::Path;

const SIZES: [&str; 5] = ["128x128", "96x96", "64x64", "48x48", "32x32"];

pub fn icon_uri(icon_name: &str) -> Option<String> {
    let name = icon_name.trim();
    if name.is_empty() {
        return None;
    }
    if name.starts_with('/') && Path::new(name).is_file() {
        return Some(format!("file://{name}"));
    }
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(format!("{home}/.local/share/icons"));
    }
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(format!("{home}/.icons"));
    }
    dirs.push("/usr/share/icons".to_string());

    for dir in &dirs {
        for size in SIZES {
            let p = format!("{dir}/hicolor/{size}/apps/{name}.png");
            if Path::new(&p).is_file() {
                return Some(format!("file://{p}"));
            }
        }
    }
    let p = format!("/usr/share/pixmaps/{name}.png");
    if Path::new(&p).is_file() {
        return Some(format!("file://{p}"));
    }
    None
}
