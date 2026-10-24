use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;

fn log_cmd(cmd: &str) {
    eprintln!("[defeditor] $ {cmd}");
}

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

pub struct MimeDb {
    pub mimes: Vec<String>,
    /// mime type -> lowercase file extensions (from globs2, e.g. "pdf" for `*.pdf`)
    pub exts: HashMap<String, Vec<String>>,
}

fn glob_to_ext(glob: &str) -> Option<String> {
    let g = glob.trim();
    let ext = g.strip_prefix("*.")?;
    if ext.is_empty() || ext.contains('*') {
        return None;
    }
    Some(ext.to_lowercase())
}

pub fn load_mime_db() -> MimeDb {
    let mut set = BTreeSet::new();
    let mut exts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
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
            if !mime.contains('/') {
                continue;
            }
            set.insert(mime.to_string());
            if let Some(glob) = parts.next()
                && let Some(ext) = glob_to_ext(glob)
            {
                exts.entry(mime.to_string()).or_default().insert(ext);
            }
        }
    }
    let exts = exts
        .into_iter()
        .map(|(k, v)| (k, v.into_iter().collect()))
        .collect();
    MimeDb {
        mimes: set.into_iter().collect(),
        exts,
    }
}

pub fn query_default(mime: &str) -> Option<String> {
    log_cmd(&format!("xdg-mime query default {mime}"));
    let output = Command::new("xdg-mime")
        .args(["query", "default", mime])
        .output()
        .ok()?;
    if !output.status.success() {
        log_cmd(&format!(
            "  ↳ exit {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let s = if s.is_empty() || s == "no default" { None } else { Some(s) };
    eprintln!("[defeditor]  ↳ {}", s.as_deref().unwrap_or("(no default)"));
    s
}

pub fn set_default(desktop_id: &str, mime: &str) -> Result<(), String> {
    log_cmd(&format!("xdg-mime default {desktop_id} {mime}"));
    let output = Command::new("xdg-mime")
        .args(["default", desktop_id, mime])
        .output()
        .map_err(|e| format!("failed to run xdg-mime: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        log_cmd(&format!("  ↳ exit {:?}: {err}", output.status.code()));
        Err(err)
    }
}

fn mimeapps_path() -> Option<PathBuf> {
    let dir = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))?;
    Some(dir.join("mimeapps.list"))
}

/// Drop the entries for `mimes` from the `[Default Applications]` section of the
/// user's `mimeapps.list`, so the system/distro default applies again.
/// Returns how many entries were removed.
pub fn remove_defaults(mimes: &[String]) -> Result<usize, String> {
    let Some(path) = mimeapps_path() else {
        return Err("cannot resolve config dir".to_string());
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Ok(0),
    };
    let target: HashSet<&str> = mimes.iter().map(|s| s.as_str()).collect();
    let mut out = String::with_capacity(content.len());
    let mut section = String::new();
    let mut removed = 0usize;
    for line in content.lines() {
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if section == "Default Applications"
            && let Some((key, _)) = line.split_once('=')
            && target.contains(key.trim())
        {
            removed += 1;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if removed > 0 {
        log_cmd(&format!(
            "remove {removed} Default Applications entries for {} mimes from {}",
            mimes.len(),
            path.display()
        ));
        let tmp = path.with_extension("list.tmp");
        std::fs::write(&tmp, &out).map_err(|e| format!("write: {e}"))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("rename: {e}"))?;
    }
    Ok(removed)
}
