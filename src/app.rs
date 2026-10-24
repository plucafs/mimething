use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use eframe::egui;

use crate::categories::{CATEGORIES, CategoryKind};
use crate::desktop::{self, DesktopApp};
use crate::icons;
use crate::mime;
use crate::settings;

struct ExpandedCategory {
    name: &'static str,
    icon: Option<String>,
    mimes: Vec<String>,
}

struct Candidate {
    desktop_id: String,
    name: String,
    icon: Option<String>,
    coverage: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Selection {
    None,
    Category(usize),
    Mime(usize),
}

#[derive(Clone, Copy, PartialEq)]
enum LeftTab {
    Types,
    Apps,
}

#[derive(Clone)]
struct UndoEntry {
    label: String,
    restores: Vec<(String, String)>,
}

struct PendingConfirm {
    desktop_id: String,
    app_name: String,
    mimes: Vec<String>,
}

struct BulkProgress {
    done: usize,
    failed: usize,
    total: usize,
    running: bool,
}

enum Msg {
    DefaultsLoaded(Vec<(String, Option<String>)>),
    BulkFinished {
        failed: usize,
        total: usize,
        cancelled: bool,
        restores: Vec<(String, String)>,
    },
    SingleSet(Result<Option<String>, String>),
    UndoDone {
        restored: usize,
        total: usize,
    },
    OverrideRemoved(Result<usize, String>),
}

enum CandMenu {
    SetDefault(String),
    OpenDesktop(String),
    RemoveOverride,
}

struct CandRowActions {
    menu: Option<CandMenu>,
    clicked: Option<String>,
}

/// Horizontal separator spanning the full panel width, ignoring the frame inner margin (8px).
fn full_width_separator(ui: &mut egui::Ui) {
    ui.add(egui::Separator::default().horizontal().grow(8.0));
}

pub struct App {
    all_mimes: Vec<String>,
    mime_exts: HashMap<String, Vec<String>>,
    apps: Vec<DesktopApp>,
    categories: Vec<ExpandedCategory>,
    default_cache: HashMap<String, Option<String>>,
    search: String,
    app_search: String,
    selection: Selection,
    left_tab: LeftTab,
    selected_app: Option<String>,
    selected_candidate: Option<String>,
    candidates: Vec<Candidate>,
    current_defaults: Vec<(String, Option<String>)>,
    status: String,
    job: Option<Arc<Mutex<BulkProgress>>>,
    cancel_flag: Option<Arc<AtomicBool>>,
    undo_stack: Option<UndoEntry>,
    pending_confirm: Option<PendingConfirm>,
    focus_search: bool,
    show_all_apps: bool,
    msg_rx: Option<mpsc::Receiver<Msg>>,
    msg_tx: mpsc::Sender<Msg>,
    settings: settings::Settings,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut fonts = fastframe_fonts::FontSetup::default().definitions();
        let rendering = fastframe_text::detect();
        rendering.apply_to(&mut fonts);
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx
            .all_styles_mut(|style| rendering.apply_to_visuals(&mut style.visuals));
        cc.egui_ctx
            .all_styles_mut(|style| style.interaction.selectable_labels = false);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let settings = settings::load();
        if let Some(theme) = settings.theme {
            cc.egui_ctx.set_theme(theme);
        }
        if let Some(scale) = settings.scale {
            cc.egui_ctx.set_pixels_per_point(scale);
        }

        let (msg_tx, msg_rx) = mpsc::channel();
        let db = mime::load_mime_db();
        let all_mimes = db.mimes;
        let mime_exts = db.exts;
        let apps = desktop::load_apps();
        let mime_set: HashSet<&str> = all_mimes.iter().map(|s| s.as_str()).collect();

        let categories: Vec<ExpandedCategory> = CATEGORIES
            .iter()
            .map(|c| {
                let mut mimes: Vec<String> = match &c.kind {
                    CategoryKind::Prefix(p) => all_mimes
                        .iter()
                        .filter(|m| m.starts_with(p))
                        .cloned()
                        .collect(),
                    CategoryKind::Explicit(list) => list
                        .iter()
                        .filter(|m| mime_set.contains(*m))
                        .map(|s| s.to_string())
                        .collect(),
                };
                for e in c.extras {
                    if mime_set.contains(e) && !mimes.iter().any(|m| m == e) {
                        mimes.push(e.to_string());
                    }
                }
                ExpandedCategory {
                    name: c.name,
                    icon: icons::icon_uri(c.icon),
                    mimes,
                }
            })
            .collect();

        Self {
            all_mimes,
            mime_exts,
            apps,
            categories,
            default_cache: HashMap::new(),
            search: String::new(),
            app_search: String::new(),
            selection: Selection::None,
            left_tab: LeftTab::Types,
            selected_app: None,
            selected_candidate: None,
            candidates: Vec::new(),
            current_defaults: Vec::new(),
            status: String::new(),
            job: None,
            cancel_flag: None,
            undo_stack: None,
            pending_confirm: None,
            focus_search: false,
            show_all_apps: false,
            msg_rx: Some(msg_rx),
            msg_tx,
            settings,
        }
    }

    fn filtered_mimes(&self) -> Vec<usize> {
        let q = self.search.trim().to_lowercase();
        self.all_mimes
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                if q.is_empty() || m.to_lowercase().contains(&q) {
                    return true;
                }
                self.mime_exts
                    .get(*m)
                    .is_some_and(|exts| exts.iter().any(|e| e.contains(&q)))
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn filtered_apps(&self) -> Vec<usize> {
        let q = self.app_search.trim().to_lowercase();
        self.apps
            .iter()
            .enumerate()
            .filter(|(_, a)| {
                q.is_empty()
                    || a.name.to_lowercase().contains(&q)
                    || a.id.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn mime_label(&self, m: &str) -> String {
        match self.mime_exts.get(m) {
            Some(exts) if !exts.is_empty() => {
                let shown: Vec<String> = exts.iter().take(3).map(|e| format!("*.{e}")).collect();
                let suffix = if exts.len() > 3 {
                    format!("{}, …", shown.join(", "))
                } else {
                    shown.join(", ")
                };
                format!("{m}  ({suffix})")
            }
            _ => m.to_string(),
        }
    }

    fn selection_mimes(&self) -> Vec<String> {
        match self.selection {
            Selection::None => Vec::new(),
            Selection::Category(i) => self.categories[i].mimes.clone(),
            Selection::Mime(i) => vec![self.all_mimes[i].clone()],
        }
    }

    fn on_select_mimes(&mut self, mimes: Vec<String>) {
        self.selected_candidate = None;
        self.current_defaults.clear();
        self.show_all_apps = false;

        self.candidates = self
            .apps
            .iter()
            .filter_map(|app| {
                let coverage = mimes
                    .iter()
                    .filter(|m| app.mimes.iter().any(|am| am == *m))
                    .count();
                if coverage == 0 {
                    None
                } else {
                    Some(Candidate {
                        desktop_id: app.id.clone(),
                        name: app.name.clone(),
                        icon: icons::icon_uri(&app.icon),
                        coverage,
                    })
                }
            })
            .collect();
        self.candidates.sort_by(|a, b| {
            b.coverage
                .cmp(&a.coverage)
                .then_with(|| a.name.cmp(&b.name))
        });

        if mimes.len() == 1 {
            let mime = mimes[0].clone();
            match self.default_cache.get(&mime).cloned() {
                Some(d) => self.current_defaults.push((mime, d)),
                None => {
                    let tx = self.msg_tx.clone();
                    thread::spawn(move || {
                        let d = mime::query_default(&mime);
                        tx.send(Msg::DefaultsLoaded(vec![(mime, d)])).ok();
                    });
                }
            }
        } else {
            let tx = self.msg_tx.clone();
            thread::spawn(move || {
                let mut results = Vec::with_capacity(mimes.len());
                for m in &mimes {
                    let d = mime::query_default(m);
                    results.push((m.clone(), d));
                }
                tx.send(Msg::DefaultsLoaded(results)).ok();
            });
        }
    }

    fn refresh_selection(&mut self) {
        self.default_cache.clear();
        let mimes = self.selection_mimes();
        if !mimes.is_empty() {
            self.on_select_mimes(mimes);
        }
    }

    fn set_single(&mut self, desktop_id: &str, mime: &str) {
        let tx = self.msg_tx.clone();
        let desktop_id = desktop_id.to_string();
        let mime = mime.to_string();
        thread::spawn(move || {
            let prev = mime::query_default(&mime);
            match mime::set_default(&desktop_id, &mime) {
                Ok(()) => tx.send(Msg::SingleSet(Ok(prev))).ok(),
                Err(e) => tx.send(Msg::SingleSet(Err(e))).ok(),
            }
        });
    }

    fn set_category(&mut self, desktop_id: &str, mimes: &[String]) {
        let progress = Arc::new(Mutex::new(BulkProgress {
            done: 0,
            failed: 0,
            total: mimes.len(),
            running: true,
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        self.job = Some(progress.clone());
        self.cancel_flag = Some(cancel.clone());

        let prev_map: HashMap<String, Option<String>> = mimes
            .iter()
            .map(|m| (m.clone(), self.default_cache.get(m).cloned().flatten()))
            .collect();

        let tx = self.msg_tx.clone();
        let desktop_id = desktop_id.to_string();
        let mimes: Vec<String> = mimes.to_vec();
        thread::spawn(move || {
            let mut failed = 0usize;
            let total = mimes.len();
            let mut restores: Vec<(String, String)> = Vec::new();
            for (i, m) in mimes.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    progress.lock().unwrap().running = false;
                    tx.send(Msg::BulkFinished {
                        failed,
                        total,
                        cancelled: true,
                        restores,
                    })
                    .ok();
                    return;
                }
                match mime::set_default(&desktop_id, m) {
                    Ok(()) => {
                        if let Some(Some(prev)) = prev_map.get(m) {
                            restores.push((m.clone(), prev.clone()));
                        }
                    }
                    Err(_) => failed += 1,
                }
                let mut p = progress.lock().unwrap();
                p.done = i + 1;
                p.failed = failed;
            }
            progress.lock().unwrap().running = false;
            tx.send(Msg::BulkFinished {
                failed,
                total,
                cancelled: false,
                restores,
            })
            .ok();
        });
    }

    fn undo_last(&mut self) {
        let Some(entry) = self.undo_stack.clone() else {
            return;
        };
        let tx = self.msg_tx.clone();
        thread::spawn(move || {
            let total = entry.restores.len();
            let mut restored = 0usize;
            for (m, d) in &entry.restores {
                if mime::set_default(d, m).is_ok() {
                    restored += 1;
                }
            }
            tx.send(Msg::UndoDone { restored, total }).ok();
        });
    }

    fn remove_override(&mut self, mimes: &[String]) {
        let tx = self.msg_tx.clone();
        let mimes: Vec<String> = mimes.to_vec();
        thread::spawn(move || {
            let r = mime::remove_defaults(&mimes);
            tx.send(Msg::OverrideRemoved(r)).ok();
        });
    }

    fn app_name(&self, desktop_id: &str) -> &str {
        self.apps
            .iter()
            .find(|a| a.id == desktop_id)
            .map(|a| a.name.as_str())
            .unwrap_or("(unknown)")
    }

    fn apply_selection(&mut self, desktop_id: &str) {
        let mimes = self.selection_mimes();
        if mimes.len() == 1 {
            self.set_single(desktop_id, &mimes[0]);
        } else {
            let app_name = self.app_name(desktop_id).to_string();
            self.pending_confirm = Some(PendingConfirm {
                desktop_id: desktop_id.to_string(),
                app_name,
                mimes,
            });
        }
    }

    fn poll_messages(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.msg_rx.take() else {
            return;
        };
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Msg::DefaultsLoaded(results) => {
                    let selected: HashSet<String> = self.selection_mimes().into_iter().collect();
                    for (m, d) in results {
                        self.default_cache.insert(m.clone(), d.clone());
                        if selected.contains(&m) {
                            self.current_defaults.push((m, d));
                        }
                    }
                }
                Msg::BulkFinished {
                    failed,
                    total,
                    cancelled,
                    restores,
                } => {
                    self.job = None;
                    self.cancel_flag = None;
                    let status = if cancelled {
                        format!("Cancelled after {total} types ({failed} failed)")
                    } else if failed == 0 {
                        format!("Done: {total}/{total} types updated")
                    } else {
                        format!("Done: {} updated, {failed} failed", total - failed)
                    };
                    self.status = status;
                    if !restores.is_empty() {
                        self.undo_stack = Some(UndoEntry {
                            label: format!("{total} types"),
                            restores,
                        });
                    }
                    self.refresh_selection();
                }
                Msg::SingleSet(r) => match r {
                    Ok(prev) => {
                        let m = self.selected_mime().unwrap_or_default();
                        let name = self
                            .selected_candidate
                            .clone()
                            .map(|id| self.app_name(&id).to_string())
                            .unwrap_or_default();
                        self.status = format!("Set {m} → {name}");
                        if let Some(p) = prev {
                            self.undo_stack = Some(UndoEntry {
                                label: m.clone(),
                                restores: vec![(m, p)],
                            });
                        }
                        self.refresh_selection();
                    }
                    Err(e) => self.status = format!("Error: {e}"),
                },
                Msg::UndoDone { restored, total } => {
                    self.status = format!("Restored {restored}/{total} previous defaults");
                    self.undo_stack = None;
                    self.refresh_selection();
                }
                Msg::OverrideRemoved(r) => match r {
                    Ok(0) => self.status = "No overrides to remove".to_string(),
                    Ok(n) => {
                        self.status = format!("Removed {n} overrides (system default applies)");
                        self.refresh_selection();
                    }
                    Err(e) => self.status = format!("Error: {e}"),
                },
            }
        }
        self.msg_rx = Some(rx);
        if self.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
        }
    }

    fn selected_mime(&self) -> Option<String> {
        match self.selection {
            Selection::Mime(i) => Some(self.all_mimes[i].clone()),
            _ => None,
        }
    }

    fn ui_left(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let t = ui.selectable_label(self.left_tab == LeftTab::Types, "Types");
            let a = ui.selectable_label(self.left_tab == LeftTab::Apps, "Apps");
            if t.clicked() {
                self.left_tab = LeftTab::Types;
            }
            if a.clicked() {
                self.left_tab = LeftTab::Apps;
            }
        });
        ui.add_space(4.0);
        match self.left_tab {
            LeftTab::Types => self.ui_left_types(ui),
            LeftTab::Apps => self.ui_left_apps(ui),
        }
    }

    fn ui_left_types(&mut self, ui: &mut egui::Ui) {
        ui.heading("Categories");
        ui.add_space(4.0);
        let mut cat_clicked: Option<usize> = None;
        egui::Grid::new("categories")
            .num_columns(2)
            .spacing([8.0, 2.0])
            .min_col_width(0.0)
            .show(ui, |ui| {
                for (i, cat) in self.categories.iter().enumerate() {
                    if let Some(uri) = &cat.icon {
                        ui.add(egui::Image::from_uri(uri).max_size(egui::vec2(24.0, 24.0)));
                    } else {
                        ui.label("  ");
                    }
                    let selected = self.selection == Selection::Category(i);
                    if ui
                        .selectable_label(
                            selected,
                            format!("{}  ({} types)", cat.name, cat.mimes.len()),
                        )
                        .clicked()
                    {
                        cat_clicked = Some(i);
                    }
                    ui.end_row();
                }
            });
        if let Some(i) = cat_clicked {
            self.selection = Selection::Category(i);
            let mimes = self.categories[i].mimes.clone();
            self.on_select_mimes(mimes);
        }

        full_width_separator(ui);
        ui.heading("All MIME types");
        ui.add_space(4.0);
        let r = ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Search, e.g. pdf or image/")
                .desired_width(f32::INFINITY),
        );
        if self.focus_search {
            ui.memory_mut(|m| m.request_focus(r.id));
            self.focus_search = false;
        }
        ui.add_space(4.0);

        let filtered = self.filtered_mimes();
        let row_h = 20.0;
        let mut mime_clicked: Option<usize> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_h, filtered.len(), |ui, range| {
                for &i in &filtered[range] {
                    let m = &self.all_mimes[i];
                    let selected = self.selection == Selection::Mime(i);
                    let label = self.mime_label(m);
                    if ui.selectable_label(selected, label).clicked() {
                        mime_clicked = Some(i);
                    }
                }
            });
        if let Some(i) = mime_clicked {
            self.selection = Selection::Mime(i);
            let mimes = vec![self.all_mimes[i].clone()];
            self.on_select_mimes(mimes);
        }
    }

    fn ui_left_apps(&mut self, ui: &mut egui::Ui) {
        ui.heading("Applications");
        ui.add_space(4.0);
        let r = ui.add(
            egui::TextEdit::singleline(&mut self.app_search)
                .hint_text("Search apps…")
                .desired_width(f32::INFINITY),
        );
        if self.focus_search {
            ui.memory_mut(|m| m.request_focus(r.id));
            self.focus_search = false;
        }
        ui.add_space(4.0);

        let filtered = self.filtered_apps();
        let row_h = 28.0;
        let mut app_clicked: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt("apps")
            .auto_shrink([false, false])
            .show_rows(ui, row_h, filtered.len(), |ui, range| {
                for &i in &filtered[range] {
                    let app = &self.apps[i];
                    let selected = self.selected_app.as_deref() == Some(app.id.as_str());
                    ui.horizontal(|ui| {
                        if let Some(uri) = icons::icon_uri(&app.icon) {
                            ui.add(egui::Image::from_uri(uri).max_size(egui::vec2(24.0, 24.0)));
                        } else {
                            ui.label("  ");
                        }
                        ui.add_space(6.0);
                        if ui.selectable_label(selected, &app.name).clicked() {
                            app_clicked = Some(i);
                        }
                    });
                }
            });
        if let Some(i) = app_clicked {
            self.selected_app = Some(self.apps[i].id.clone());
        }
    }

    fn ui_app_detail(&mut self, ui: &mut egui::Ui) {
        let Some(app) = self
            .apps
            .iter()
            .find(|a| Some(&a.id) == self.selected_app.as_ref())
        else {
            ui.centered_and_justified(|ui| {
                ui.label("Select an application on the left");
            });
            return;
        };
        let name = app.name.clone();
        let id = app.id.clone();
        let icon = icons::icon_uri(&app.icon);
        let mimes: Vec<String> = app.mimes.clone();

        ui.horizontal(|ui| {
            if let Some(uri) = &icon {
                ui.add(egui::Image::from_uri(uri).max_size(egui::vec2(32.0, 32.0)));
            }
            ui.heading(&name);
        });
        ui.label(egui::RichText::new(&id).weak());
        full_width_separator(ui);
        ui.add_space(4.0);
        ui.label(format!("{} MIME types handled:", mimes.len()));
        ui.add_space(4.0);

        let mut jump: Option<String> = None;
        egui::ScrollArea::vertical()
            .id_salt("app_mimes")
            .auto_shrink([false, false])
            .show_rows(ui, 22.0, mimes.len(), |ui, range| {
                for m in &mimes[range] {
                    if ui.selectable_label(false, m).clicked() {
                        jump = Some(m.clone());
                    }
                }
            });
        if let Some(m) = jump
            && let Some(i) = self.all_mimes.iter().position(|x| *x == m)
        {
            self.left_tab = LeftTab::Types;
            self.selection = Selection::Mime(i);
            self.on_select_mimes(vec![m]);
        }
    }

    fn ui_right_content(&mut self, ui: &mut egui::Ui) {
        if matches!(self.selection, Selection::None) {
            return;
        }
        ui.add_space(8.0);

        if !self.current_defaults.is_empty() {
            ui.label("Current defaults:");
            let mut summary: HashMap<&str, usize> = HashMap::new();
            for (_, d) in &self.current_defaults {
                *summary.entry(d.as_deref().unwrap_or("(none)")).or_insert(0) += 1;
            }
            let mut items: Vec<(&str, usize)> = summary.into_iter().collect();
            items.sort_by_key(|(id, count)| (std::cmp::Reverse(*count), *id));
            for (id, count) in items.iter().take(10) {
                let label = if *id == "(none)" {
                    "(none)".to_string()
                } else {
                    format!("{} ({count})", self.app_name(id))
                };
                ui.label(format!("  • {label}"));
            }
            if items.len() > 10 {
                ui.label(format!("  … and {} more", items.len() - 10));
            }
            ui.add_space(8.0);
        } else if self.job.is_none() {
            ui.label("Current default: (none)");
            ui.add_space(8.0);
        }

        if let Some(job) = &self.job {
            let p = job.lock().unwrap();
            let frac = if p.total == 0 {
                1.0
            } else {
                p.done as f32 / p.total as f32
            };
            ui.label(format!(
                "Applying… {} / {} ({} failed)",
                p.done, p.total, p.failed
            ));
            ui.add(egui::ProgressBar::new(frac).show_percentage());
            if ui.button("Cancel").clicked()
                && let Some(flag) = &self.cancel_flag
            {
                flag.store(true, Ordering::Relaxed);
            }
            return;
        }

        let declaring_empty = self.candidates.is_empty();
        let show_all = self.show_all_apps || declaring_empty;
        ui.horizontal(|ui| {
            if declaring_empty {
                ui.label(format!(
                    "Applications (all {} — none declare this type):",
                    self.apps.len()
                ));
            } else if show_all {
                ui.label(format!("Applications (all {}):", self.apps.len()));
                if ui.small_button("Only declaring").clicked() {
                    self.show_all_apps = false;
                }
            } else {
                ui.label(format!("Applications ({}):", self.candidates.len()));
                if ui.small_button("Show all apps").clicked() {
                    self.show_all_apps = true;
                }
            }
        });
        ui.add_space(4.0);
        let cat_total = match self.selection {
            Selection::Category(i) => self.categories[i].mimes.len(),
            _ => 0,
        };
        let mut actions = CandRowActions {
            menu: None,
            clicked: None,
        };
        let declaring_ids: HashSet<&str> = self
            .candidates
            .iter()
            .map(|c| c.desktop_id.as_str())
            .collect();
        egui::Grid::new("candidates")
            .num_columns(3)
            .spacing([8.0, 2.0])
            .min_col_width(0.0)
            .show(ui, |ui| {
                for c in &self.candidates {
                    Self::candidate_row(
                        ui,
                        &c.name,
                        &c.icon,
                        &if cat_total > 0 {
                            format!("{} / {}", c.coverage, cat_total)
                        } else {
                            c.desktop_id.clone()
                        },
                        &c.desktop_id,
                        self.selected_candidate.as_deref() == Some(c.desktop_id.as_str()),
                        &mut actions,
                    );
                }
                if show_all {
                    for app in &self.apps {
                        if declaring_ids.contains(app.id.as_str()) {
                            continue;
                        }
                        let icon = icons::icon_uri(&app.icon);
                        let extra = if cat_total > 0 {
                            "—".to_string()
                        } else {
                            app.id.clone()
                        };
                        Self::candidate_row(
                            ui,
                            &app.name,
                            &icon,
                            &extra,
                            &app.id,
                            self.selected_candidate.as_deref() == Some(app.id.as_str()),
                            &mut actions,
                        );
                    }
                }
            });
        if let Some(id) = actions.clicked {
            self.selected_candidate = Some(id);
        }
        match actions.menu {
            Some(CandMenu::SetDefault(id)) => {
                self.selected_candidate = Some(id.clone());
                self.apply_selection(&id);
            }
            Some(CandMenu::OpenDesktop(id)) => {
                if let Some(path) = desktop::desktop_path(&id) {
                    eprintln!("[defeditor] $ xdg-open {}", path.display());
                    thread::spawn(move || {
                        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
                    });
                    self.status = format!("Opening {id}…");
                } else {
                    self.status = format!("Cannot find {id}");
                }
            }
            Some(CandMenu::RemoveOverride) => {
                let mimes = self.selection_mimes();
                self.remove_override(&mimes);
            }
            None => {}
        }
    }

    fn candidate_row(
        ui: &mut egui::Ui,
        name: &str,
        icon: &Option<String>,
        extra: &str,
        desktop_id: &str,
        selected: bool,
        actions: &mut CandRowActions,
    ) {
        if let Some(uri) = icon {
            ui.add(egui::Image::from_uri(uri).max_size(egui::vec2(128.0, 128.0)));
        } else {
            ui.label("  ");
        }
        let resp1 = ui.selectable_label(selected, name);
        let resp2 = ui.selectable_label(selected, egui::RichText::new(extra).weak());
        resp1.context_menu(|ui| {
            if ui.button("Set as default").clicked() {
                actions.menu = Some(CandMenu::SetDefault(desktop_id.to_string()));
                ui.close();
            }
            if ui.button("Open .desktop file").clicked() {
                actions.menu = Some(CandMenu::OpenDesktop(desktop_id.to_string()));
                ui.close();
            }
            if ui.button("Remove override (system default)").clicked() {
                actions.menu = Some(CandMenu::RemoveOverride);
                ui.close();
            }
        });
        if resp1.clicked() || resp2.clicked() {
            actions.clicked = Some(desktop_id.to_string());
        }
        ui.end_row();
    }

    fn ui_confirm_window(&mut self, ctx: &egui::Context) {
        if self.pending_confirm.is_none() {
            return;
        }
        let mut yes = false;
        let mut no = false;
        egui::Window::new("Confirm")
            .id(egui::Id::new("bulk_confirm"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let req = self.pending_confirm.as_ref().unwrap();
                ui.label(format!(
                    "Set “{}” as default for {} MIME types?",
                    req.app_name,
                    req.mimes.len()
                ));
                ui.add_space(4.0);
                ui.label("This overwrites the current default of every listed type.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        yes = true;
                    }
                    if ui.button("Cancel").clicked() {
                        no = true;
                    }
                });
            });
        if yes {
            if let Some(req) = self.pending_confirm.take() {
                self.set_category(&req.desktop_id, &req.mimes);
            }
        } else if no {
            self.pending_confirm = None;
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_messages(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
            self.focus_search = true;
        }

        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                egui::containers::menu::MenuButton::new("File").ui(ui, |ui| {
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                egui::containers::menu::MenuButton::new("View").ui(ui, |ui| {
                    if ui.button("Toggle theme").clicked() {
                        let ctx = ui.ctx().clone();
                        let new_theme = if ctx.theme() == egui::Theme::Dark {
                            egui::Theme::Light
                        } else {
                            egui::Theme::Dark
                        };
                        ctx.set_theme(new_theme);
                        self.settings.theme = Some(new_theme);
                        settings::save(&self.settings);
                    }
                    let ctx = ui.ctx().clone();
                    let current = ctx.pixels_per_point();
                    let steps = [1.0_f32, 1.25, 1.5, 1.75, 2.0];
                    let next = *steps
                        .iter()
                        .find(|s| **s > current + 0.03)
                        .unwrap_or(&steps[0]);
                    if ui.button("Scale UI").clicked() {
                        ctx.set_pixels_per_point(next);
                        self.settings.scale = Some(next);
                        settings::save(&self.settings);
                    }
                });
            });
        });

        egui::Panel::left("left")
            .resizable(true)
            .min_size(240.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("left")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.ui_left(ui);
                    });
            });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} MIME types · {} applications",
                    self.all_mimes.len(),
                    self.apps.len()
                ));
                if !self.status.is_empty() {
                    ui.separator();
                    ui.label(&self.status);
                }
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            if self.left_tab == LeftTab::Apps {
                self.ui_app_detail(ui);
                return;
            }

            if self.selection == Selection::None {
                ui.centered_and_justified(|ui| {
                    ui.label("Select a category or a MIME type on the left");
                });
                return;
            }

            let title = match self.selection {
                Selection::Category(i) => self.categories[i].name.to_string(),
                Selection::Mime(i) => self.all_mimes[i].clone(),
                Selection::None => unreachable!(),
            };
            let button_text = if matches!(self.selection, Selection::Category(_)) {
                let total = match self.selection {
                    Selection::Category(i) => self.categories[i].mimes.len(),
                    _ => 0,
                };
                format!("Set as default for all {total} types")
            } else {
                "Set as default".to_string()
            };
            let can_apply = self.selected_candidate.is_some() && self.job.is_none();
            let can_undo = self.undo_stack.is_some() && self.job.is_none();

            ui.heading(&title);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let set_clicked = ui
                        .scope(|ui| {
                            if !can_apply {
                                ui.disable();
                            }
                            ui.add_sized(
                                [200.0, 28.0],
                                egui::Button::new(&button_text),
                            )
                            .clicked()
                        })
                        .inner;
                    let undo_clicked = ui
                        .scope(|ui| {
                            if !can_undo {
                                ui.disable();
                            }
                            let r = ui.add_sized([80.0, 28.0], egui::Button::new("Undo"));
                            let r = match &self.undo_stack {
                                Some(entry) => r.on_hover_text(format!("Undo: {}", entry.label)),
                                None => r,
                            };
                            r.clicked()
                        })
                        .inner;
                    if set_clicked {
                        let desktop_id = self.selected_candidate.clone().unwrap();
                        self.apply_selection(&desktop_id);
                    }
                    if undo_clicked {
                        self.undo_last();
                    }
                });
            });
            ui.add_space(8.0);
            full_width_separator(ui);

            egui::ScrollArea::vertical()
                .id_salt("right")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.ui_right_content(ui);
                });
        });

        self.ui_confirm_window(ui.ctx());
    }
}
