use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("XDG_DATA_HOME") {
        if !home.is_empty() {
            dirs.push(PathBuf::from(home));
        }
    } else if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share"));
    }
    let xdg_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for d in xdg_dirs.split(':').filter(|s| !s.is_empty()) {
        dirs.push(PathBuf::from(d));
    }
    dirs
}

pub fn load_mime_types() -> Vec<String> {
    let mut set = BTreeSet::new();
    for dir in data_dirs() {
        let path = dir.join("mime/globs2");
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in content.lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(3, ':');
            let _priority = parts.next();
            let Some(mime) = parts.next() else { continue };
            if mime.contains('/') {
                set.insert(mime.to_string());
            }
        }
    }
    set.into_iter().collect()
}

pub fn query_default(mime: &str) -> Option<String> {
    let output = Command::new("xdg-mime")
        .args(["query", "default", mime])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() || s == "no default" {
        None
    } else {
        Some(s)
    }
}

pub fn set_default(desktop_id: &str, mime: &str) -> Result<(), String> {
    let output = Command::new("xdg-mime")
        .args(["default", desktop_id, mime])
        .output()
        .map_err(|e| format!("failed to run xdg-mime: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}
