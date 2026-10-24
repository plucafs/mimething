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
    },
    SingleSet(Result<(), String>),
}

pub struct App {
    all_mimes: Vec<String>,
    apps: Vec<DesktopApp>,
    categories: Vec<ExpandedCategory>,
    default_cache: HashMap<String, Option<String>>,
    search: String,
    selection: Selection,
    selected_candidate: Option<String>,
    candidates: Vec<Candidate>,
    current_defaults: Vec<(String, Option<String>)>,
    status: String,
    job: Option<Arc<Mutex<BulkProgress>>>,
    cancel_flag: Option<Arc<AtomicBool>>,
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
        let all_mimes = mime::load_mime_types();
        let apps = desktop::load_apps();
        let mime_set: HashSet<&str> = all_mimes.iter().map(|s| s.as_str()).collect();

        let categories: Vec<ExpandedCategory> = CATEGORIES
            .iter()
            .map(|c| {
                let mimes = match &c.kind {
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
                ExpandedCategory {
                    name: c.name,
                    mimes,
                }
            })
            .collect();

        Self {
            all_mimes,
            apps,
            categories,
            default_cache: HashMap::new(),
            search: String::new(),
            selection: Selection::None,
            selected_candidate: None,
            candidates: Vec::new(),
            current_defaults: Vec::new(),
            status: String::new(),
            job: None,
            cancel_flag: None,
            msg_rx: Some(msg_rx),
            msg_tx,
            settings,
        }
    }

    fn filtered_mimes(&self) -> Vec<usize> {
        let q = self.search.to_lowercase();
        self.all_mimes
            .iter()
            .enumerate()
            .filter(|(_, m)| q.is_empty() || m.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
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

    fn set_single(&mut self, desktop_id: &str, mime: &str) {
        let tx = self.msg_tx.clone();
        let desktop_id = desktop_id.to_string();
        let mime = mime.to_string();
        thread::spawn(move || {
            let r = mime::set_default(&desktop_id, &mime);
            tx.send(Msg::SingleSet(r)).ok();
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

        let tx = self.msg_tx.clone();
        let desktop_id = desktop_id.to_string();
        let mimes: Vec<String> = mimes.to_vec();
        thread::spawn(move || {
            let mut failed = 0usize;
            let total = mimes.len();
            for (i, m) in mimes.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    progress.lock().unwrap().running = false;
                    tx.send(Msg::BulkFinished {
                        failed,
                        total,
                        cancelled: true,
                    })
                    .ok();
                    return;
                }
                if mime::set_default(&desktop_id, m).is_err() {
                    failed += 1;
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
            })
            .ok();
        });
    }

    fn app_name(&self, desktop_id: &str) -> &str {
        self.apps
            .iter()
            .find(|a| a.id == desktop_id)
            .map(|a| a.name.as_str())
            .unwrap_or("(unknown)")
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
                } => {
                    self.job = None;
                    self.cancel_flag = None;
                    self.default_cache.clear();
                    self.status = if cancelled {
                        format!("Cancelled after {total} types ({failed} failed)")
                    } else if failed == 0 {
                        format!("Done: {total}/{total} types updated")
                    } else {
                        format!("Done: {} updated, {failed} failed", total - failed)
                    };
                    let mimes = self.selection_mimes();
                    if !mimes.is_empty() {
                        self.on_select_mimes(mimes);
                    }
                }
                Msg::SingleSet(r) => match r {
                    Ok(()) => {
                        let m = self.selected_mime().unwrap_or_default();
                        let name = self
                            .selected_candidate
                            .clone()
                            .map(|id| self.app_name(&id).to_string())
                            .unwrap_or_default();
                        self.default_cache.remove(&m);
                        self.status = format!("Set {m} → {name}");
                        let mimes = self.selection_mimes();
                        if !mimes.is_empty() {
                            self.on_select_mimes(mimes);
                        }
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
        ui.heading("Categories");
        ui.add_space(4.0);
        let mut cat_clicked: Option<usize> = None;
        for (i, cat) in self.categories.iter().enumerate() {
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
        }
        if let Some(i) = cat_clicked {
            self.selection = Selection::Category(i);
            let mimes = self.categories[i].mimes.clone();
            self.on_select_mimes(mimes);
        }

        ui.separator();
        ui.heading("All MIME types");
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Search, e.g. pdf or image/")
                .desired_width(f32::INFINITY),
        );
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
                    if ui.selectable_label(selected, m).clicked() {
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

        if self.candidates.is_empty() {
            ui.label("No installed application declares this type.");
            return;
        }

        ui.label(format!("Applications ({}):", self.candidates.len()));
        ui.add_space(4.0);
        let cat_total = match self.selection {
            Selection::Category(i) => self.categories[i].mimes.len(),
            _ => 0,
        };
        let mut cand_clicked: Option<usize> = None;
        egui::Grid::new("candidates")
            .num_columns(3)
            .spacing([8.0, 2.0])
            .min_col_width(0.0)
            .show(ui, |ui| {
                for (idx, c) in self.candidates.iter().enumerate() {
                    if let Some(uri) = &c.icon {
                        ui.add(egui::Image::from_uri(uri).max_size(egui::vec2(128.0, 128.0)));
                    } else {
                        ui.label("  ");
                    }
                    let selected =
                        self.selected_candidate.as_deref() == Some(c.desktop_id.as_str());
                    let extra = if cat_total > 0 {
                        format!("{} / {}", c.coverage, cat_total)
                    } else {
                        c.desktop_id.clone()
                    };
                    let resp1 = ui.selectable_label(selected, &c.name);
                    let resp2 = ui.selectable_label(selected, egui::RichText::new(&extra).weak());
                    if resp1.clicked() || resp2.clicked() {
                        cand_clicked = Some(idx);
                    }
                    ui.end_row();
                }
            });
        if let Some(idx) = cand_clicked {
            self.selected_candidate = Some(self.candidates[idx].desktop_id.clone());
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_messages(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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

            ui.heading(&title);
            // ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !can_apply {
                        ui.disable();
                    }
                    if ui
                        .add_sized([200.0, 28.0], egui::Button::new(&button_text))
                        .clicked()
                    {
                        let desktop_id = self.selected_candidate.clone().unwrap();
                        let mimes = self.selection_mimes();
                        if mimes.len() == 1 {
                            self.set_single(&desktop_id, &mimes[0]);
                        } else {
                            self.set_category(&desktop_id, &mimes);
                        }
                    }
                });
            });
            ui.add_space(8.0);
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("right")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.ui_right_content(ui);
                });
        });
    }
}
