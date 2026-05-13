// GUI module for Klartext-Rust
// Implements the eframe-based desktop UI with drag-and-drop, state management,
// transcription progress display, and export functionality.

use std::path::PathBuf;
use std::sync::mpsc;

use eframe::egui;
use tracing::{error, info};

use crate::core::exporter::{export, ExportFormat};
use crate::models::TranscriptionResult;
use crate::queue::FileQueue;
use crate::summarizer::{spawn_summarizer, SummaryCommand, SummaryMode, SummaryStatus};
use crate::worker::{spawn_worker, Command, WorkerStatus};

/// Application state representing the current phase of the app lifecycle.
#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    /// Model is being loaded in the background.
    LoadingModel,
    /// Model is loaded and ready for transcription.
    Ready,
    /// Audio is being converted/decoded before transcription.
    Converting,
    /// A transcription is in progress.
    Transcribing { progress: f32 },
    /// Model loading failed with the given error message.
    ModelLoadFailed(String),
}

/// Main application struct for the eframe GUI.
pub struct KlartextApp {
    /// Current application state.
    state: AppState,
    /// Channel to send commands to the worker thread.
    command_tx: mpsc::Sender<Command>,
    /// Channel to receive status updates from the worker thread.
    status_rx: mpsc::Receiver<WorkerStatus>,
    /// Current transcription result (if any).
    result: Option<TranscriptionResult>,
    /// Error message to display (if any).
    error: Option<String>,
    /// File queue for managing transcription jobs.
    queue: FileQueue,
    /// Whether files are currently being hovered over the drop zone.
    hovering_files: bool,
    /// Feedback message for export operations.
    export_feedback: Option<String>,
    /// Summarizer command channel.
    summary_tx: mpsc::Sender<SummaryCommand>,
    /// Summarizer status channel.
    summary_rx: mpsc::Receiver<SummaryStatus>,
    /// Current summary result.
    summary: Option<String>,
    /// Whether summarization is in progress.
    summarizing: bool,
    /// Selected summary mode index (0=Podcast, 1=Meeting, 2=Custom).
    summary_mode_idx: usize,
    /// Custom prompt text.
    custom_prompt: String,
    /// Ollama model name.
    ollama_model: String,
    /// When transcription started (for elapsed time display).
    transcription_start: Option<std::time::Instant>,
    /// Total audio duration in seconds (for progress estimation).
    audio_duration_secs: f32,
}

impl KlartextApp {
    /// Create a new KlartextApp instance.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (status_tx, status_rx) = mpsc::channel();

        let (_handle, _abort_flag) = spawn_worker(command_rx, status_tx);

        // Summarizer channels
        let (summary_tx, summary_cmd_rx) = mpsc::channel();
        let (summary_status_tx, summary_rx) = mpsc::channel();
        let _summary_handle = spawn_summarizer(summary_cmd_rx, summary_status_tx);

        let model_path = std::env::var("KLARTEXT_MODEL_PATH")
            .unwrap_or_else(|_| "./models/tdt".to_string());

        let ollama_model = std::env::var("OLLAMA_MODEL")
            .unwrap_or_else(|_| "gemma4:e4b".to_string());

        info!("Sending LoadModel command for: {}", model_path);
        let _ = command_tx.send(Command::LoadModel(PathBuf::from(model_path)));

        Self {
            state: AppState::LoadingModel,
            command_tx,
            status_rx,
            result: None,
            error: None,
            queue: FileQueue::new(),
            hovering_files: false,
            export_feedback: None,
            summary_tx,
            summary_rx,
            summary: None,
            summarizing: false,
            summary_mode_idx: 0,
            custom_prompt: String::new(),
            ollama_model,
            transcription_start: None,
            audio_duration_secs: 0.0,
        }
    }

    fn poll_worker_status(&mut self) {
        while let Ok(status) = self.status_rx.try_recv() {
            match status {
                WorkerStatus::ModelLoading => {
                    self.state = AppState::LoadingModel;
                }
                WorkerStatus::ModelLoaded => {
                    info!("Model loaded successfully");
                    self.state = AppState::Ready;
                }
                WorkerStatus::ModelLoadError(msg) => {
                    error!("Model load failed: {}", msg);
                    self.state = AppState::ModelLoadFailed(msg);
                }
                WorkerStatus::Converting => {
                    self.state = AppState::Converting;
                }
                WorkerStatus::TranscriptionProgress(progress) => {
                    self.state = AppState::Transcribing { progress };
                }
                WorkerStatus::TranscriptionComplete(result) => {
                    info!("Transcription complete for: {:?}", result.source_file);
                    self.queue.update_status(
                        &result.source_file,
                        crate::models::QueueStatus::Completed,
                    );
                    self.result = Some(result);
                    self.start_next_queued_file();
                }
                WorkerStatus::TranscriptionCancelled => {
                    info!("Transcription cancelled");
                    self.result = None;
                    self.export_feedback = Some("Transcription was cancelled.".to_string());
                    self.state = AppState::Ready;
                }
                WorkerStatus::TranscriptionError(msg) => {
                    error!("Transcription error: {}", msg);
                    if let Some(in_progress_file) = self
                        .queue
                        .files()
                        .iter()
                        .find(|f| f.status == crate::models::QueueStatus::InProgress)
                        .map(|f| f.path.clone())
                    {
                        self.queue.update_status(
                            &in_progress_file,
                            crate::models::QueueStatus::Failed(msg.clone()),
                        );
                    }
                    self.error = Some(msg);
                    self.start_next_queued_file();
                }
            }
        }
    }

    fn start_next_queued_file(&mut self) {
        if let Some(next_file) = self.queue.next_pending() {
            let path = next_file.path.clone();
            self.queue
                .update_status(&path, crate::models::QueueStatus::InProgress);
            self.state = AppState::Transcribing { progress: 0.0 };
            self.transcription_start = Some(std::time::Instant::now());
            let _ = self.command_tx.send(Command::Transcribe(path));
        } else {
            self.state = AppState::Ready;
        }
    }

    fn handle_dropped_files(&mut self, files: Vec<egui::DroppedFile>) {
        let paths: Vec<PathBuf> = files.into_iter().filter_map(|f| f.path).collect();
        if paths.is_empty() {
            return;
        }

        let queue_result = self.queue.submit_files(&paths);
        if !queue_result.rejected.is_empty() {
            let rejected_msgs: Vec<String> = queue_result
                .rejected
                .iter()
                .map(|(path, reason)| format!("{}: {}", path.display(), reason))
                .collect();
            self.error = Some(format!(
                "Some files were rejected:\n{}",
                rejected_msgs.join("\n")
            ));
        }

        if self.state == AppState::Ready {
            self.start_next_queued_file();
        }
    }

    fn handle_file_dialog(&mut self) {
        let files = rfd::FileDialog::new()
            .add_filter("Audio", &["wav", "mp3"])
            .pick_files();

        if let Some(paths) = files {
            let queue_result = self.queue.submit_files(&paths);
            if !queue_result.rejected.is_empty() {
                let rejected_msgs: Vec<String> = queue_result
                    .rejected
                    .iter()
                    .map(|(path, reason)| format!("{}: {}", path.display(), reason))
                    .collect();
                self.error = Some(format!(
                    "Some files were rejected:\n{}",
                    rejected_msgs.join("\n")
                ));
            }
            if self.state == AppState::Ready {
                self.start_next_queued_file();
            }
        }
    }

    fn render_loading_model(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.spinner();
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Modell wird geladen...")
                    .size(16.0)
                    .color(egui::Color32::GRAY),
            );
        });
    }

    fn render_converting(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);

            // Show which file
            if let Some(file) = self.queue.files().iter().find(|f| {
                f.status == crate::models::QueueStatus::InProgress
            }) {
                let filename = file
                    .path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                ui.label(
                    egui::RichText::new(format!("Konvertiere: {}", filename))
                        .size(14.0)
                        .strong(),
                );
            }

            ui.add_space(10.0);
            ui.spinner();
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Audio wird dekodiert und konvertiert...")
                    .size(12.0)
                    .color(egui::Color32::GRAY),
            );

            if let Some(start) = self.transcription_start {
                let elapsed_secs = start.elapsed().as_secs();
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{}:{:02} vergangen",
                        elapsed_secs / 60, elapsed_secs % 60
                    ))
                    .size(12.0)
                    .color(egui::Color32::GRAY),
                );
            }

            ui.add_space(15.0);
            if ui.button("Abbrechen").clicked() {
                let _ = self.command_tx.send(Command::Cancel);
            }
        });
    }

    fn render_drop_zone(&mut self, ui: &mut egui::Ui) {
        // Smaller drop zone when we have results
        let height = if self.result.is_some() { 80.0 } else { 150.0 };
        let drop_zone_size = egui::vec2(ui.available_width() - 40.0, height);
        let (rect, _response) = ui.allocate_exact_size(drop_zone_size, egui::Sense::hover());

        let border_color = if self.hovering_files {
            egui::Color32::from_rgb(80, 160, 240)
        } else {
            egui::Color32::from_rgb(180, 180, 180)
        };

        let fill_color = if self.hovering_files {
            egui::Color32::from_rgba_premultiplied(80, 160, 240, 15)
        } else {
            egui::Color32::from_rgba_premultiplied(245, 245, 245, 255)
        };

        ui.painter().rect(
            rect,
            6.0,
            fill_color,
            egui::Stroke::new(1.5, border_color),
            egui::StrokeKind::Inside,
        );

        let text = if self.result.is_some() {
            "Audiodatei hierher ziehen oder Button klicken"
        } else {
            "Audiodatei hierher ziehen\n(.wav, .mp3)"
        };

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(14.0),
            egui::Color32::from_rgb(120, 120, 120),
        );
    }

    fn render_ready(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(15.0);
            self.render_drop_zone(ui);
            ui.add_space(10.0);

            if ui
                .button(egui::RichText::new("\u{1F4C2} Audiodatei ausw\u{00E4}hlen").size(13.0))
                .clicked()
            {
                self.handle_file_dialog();
            }
        });

        // Queue display (compact)
        if !self.queue.is_empty() {
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            for file in self.queue.files() {
                let status_icon = match &file.status {
                    crate::models::QueueStatus::Pending => "\u{23F3}",
                    crate::models::QueueStatus::InProgress => "\u{1F504}",
                    crate::models::QueueStatus::Completed => "\u{2705}",
                    crate::models::QueueStatus::Failed(_) => "\u{274C}",
                };
                let filename = file
                    .path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| file.path.display().to_string());
                ui.label(format!("  {} {}", status_icon, filename));
            }
        }
    }

    fn render_transcribing(&self, ui: &mut egui::Ui, progress: f32) {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);

            // Show which file is being processed
            if let Some(file) = self.queue.files().iter().find(|f| {
                f.status == crate::models::QueueStatus::InProgress
            }) {
                let filename = file
                    .path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                ui.label(
                    egui::RichText::new(format!("Transkribiere: {}", filename))
                        .size(14.0)
                        .strong(),
                );
            } else {
                ui.label(
                    egui::RichText::new("Transkription l\u{00E4}uft...")
                        .size(14.0)
                        .strong(),
                );
            }

            ui.add_space(12.0);

            // Progress bar
            if progress > 0.0 {
                let pct = (progress * 100.0) as u32;
                let progress_bar = egui::ProgressBar::new(progress)
                    .text(format!("{}%", pct));
                ui.add_sized([400.0, 20.0], progress_bar);
            } else {
                // Single chunk or first chunk — show animated indeterminate bar
                ui.spinner();
            }

            ui.add_space(8.0);

            // Time info
            if let Some(start) = self.transcription_start {
                let elapsed = start.elapsed();
                let elapsed_secs = elapsed.as_secs();

                if progress > 0.05 {
                    let total_estimated = elapsed.as_secs_f32() / progress;
                    let remaining = (total_estimated - elapsed.as_secs_f32()).max(0.0) as u64;
                    ui.label(
                        egui::RichText::new(format!(
                            "{}:{:02} vergangen \u{2022} ~{}:{:02} verbleibend",
                            elapsed_secs / 60, elapsed_secs % 60,
                            remaining / 60, remaining % 60
                        ))
                        .size(12.0)
                        .color(egui::Color32::GRAY),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(format!(
                            "{}:{:02} vergangen",
                            elapsed_secs / 60, elapsed_secs % 60
                        ))
                        .size(12.0)
                        .color(egui::Color32::GRAY),
                    );
                }
            }

            ui.add_space(15.0);

            if ui.button("Abbrechen").clicked() {
                let _ = self.command_tx.send(Command::Cancel);
            }
        });

        // Queue display during transcription
        if self.queue.len() > 1 {
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            for file in self.queue.files() {
                let status_icon = match &file.status {
                    crate::models::QueueStatus::Pending => "\u{23F3}",
                    crate::models::QueueStatus::InProgress => "\u{1F504}",
                    crate::models::QueueStatus::Completed => "\u{2705}",
                    crate::models::QueueStatus::Failed(_) => "\u{274C}",
                };
                let filename = file
                    .path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| file.path.display().to_string());
                ui.label(format!("  {} {}", status_icon, filename));
            }
        }
    }

    fn render_model_load_failed(&self, ui: &mut egui::Ui, error_msg: &str) {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(
                egui::RichText::new("\u{26A0}\u{FE0F} Modell konnte nicht geladen werden")
                    .size(18.0)
                    .color(egui::Color32::from_rgb(220, 80, 80)),
            );
            ui.add_space(10.0);
            ui.label(error_msg);
            ui.add_space(10.0);
            ui.label("Bitte pr\u{00FC}fen:");
            ui.label("  \u{2022} KLARTEXT_MODEL_PATH zeigt auf den Modell-Ordner");
            ui.label("  \u{2022} Der Ordner enth\u{00E4}lt encoder-model.onnx, decoder_joint-model.onnx, vocab.txt");
            ui.add_space(15.0);

            if ui
                .button(egui::RichText::new("\u{1F504} Erneut versuchen").size(13.0))
                .clicked()
            {
                let model_path = std::env::var("KLARTEXT_MODEL_PATH")
                    .unwrap_or_else(|_| "./models/tdt".to_string());
                let _ = self
                    .command_tx
                    .send(Command::LoadModel(PathBuf::from(model_path)));
            }
        });
    }

    fn render_result_view(&mut self, ui: &mut egui::Ui) {
        // Clone data we need to avoid borrow conflicts
        let result_data = self.result.clone();
        
        if let Some(result) = &result_data {
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);

            let source_name = result
                .source_file
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Transkription")
                        .size(14.0)
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(&source_name)
                            .size(11.0)
                            .color(egui::Color32::GRAY),
                    );
                });
            });

            ui.add_space(4.0);

            // Scrollable text area — selectable and copyable
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    let mut text = result.text.clone();
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_width(f32::INFINITY)
                            .desired_rows(10)
                            .interactive(true)
                            .font(egui::FontId::proportional(15.0)),
                    );
                });

            ui.add_space(6.0);

            // Action buttons
            ui.horizontal(|ui| {
                if ui.button("\u{1F4CB} Kopieren").clicked() {
                    ui.ctx().copy_text(result.text.clone());
                    self.export_feedback = Some("\u{2705} In Zwischenablage kopiert".to_string());
                }
                ui.separator();
                if ui.button("\u{1F4BE} Als TXT exportieren").clicked() {
                    self.export_result(ExportFormat::Txt);
                }
                if ui.button("\u{1F4BE} Als Markdown exportieren").clicked() {
                    self.export_result(ExportFormat::Markdown);
                }
            });
        }

        // Feedback message
        if let Some(feedback) = &self.export_feedback {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(feedback.as_str())
                    .size(12.0)
                    .color(egui::Color32::from_rgb(80, 160, 80)),
            );
        }
    }

    fn export_result(&mut self, format: ExportFormat) {
        let result = match &self.result {
            Some(r) => r.clone(),
            None => return,
        };

        let (filter_name, extensions, default_ext) = match format {
            ExportFormat::Txt => ("Text", vec!["txt"], "txt"),
            ExportFormat::Markdown => ("Markdown", vec!["md"], "md"),
        };

        let default_filename = result
            .source_file
            .file_stem()
            .map(|s| format!("{}.{}", s.to_string_lossy(), default_ext))
            .unwrap_or_else(|| format!("transcription.{}", default_ext));

        let save_path = rfd::FileDialog::new()
            .add_filter(filter_name, &extensions)
            .set_file_name(&default_filename)
            .save_file();

        if let Some(path) = save_path {
            match export(&result, &path, format) {
                Ok(()) => {
                    info!("Exported to: {:?}", path);
                    self.export_feedback =
                        Some(format!("\u{2705} Exportiert: {}", path.display()));
                }
                Err(e) => {
                    error!("Export failed: {}", e);
                    self.export_feedback =
                        Some(format!("\u{274C} Export fehlgeschlagen: {}", e));
                }
            }
        }
    }

    fn poll_summary_status(&mut self) {
        while let Ok(status) = self.summary_rx.try_recv() {
            match status {
                SummaryStatus::InProgress => {
                    self.summarizing = true;
                }
                SummaryStatus::Complete(text) => {
                    self.summarizing = false;
                    self.summary = Some(text);
                }
                SummaryStatus::Error(msg) => {
                    self.summarizing = false;
                    self.error = Some(format!("Zusammenfassung fehlgeschlagen: {}", msg));
                }
            }
        }
    }

    fn render_summary_section(&mut self, ui: &mut egui::Ui) {
        let has_result = self.result.is_some();
        if !has_result {
            return;
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        ui.label(
            egui::RichText::new("\u{1F4DD} Zusammenfassung")
                .size(14.0)
                .strong(),
        );
        ui.add_space(4.0);

        // Mode selection
        ui.horizontal(|ui| {
            ui.label("Modus:");
            egui::ComboBox::from_id_salt("summary_mode")
                .selected_text(match self.summary_mode_idx {
                    0 => "Zusammenfassung",
                    1 => "Protokoll mit Todos",
                    _ => "Eigener Prompt",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.summary_mode_idx, 0, "Zusammenfassung");
                    ui.selectable_value(&mut self.summary_mode_idx, 1, "Protokoll mit Todos");
                    ui.selectable_value(&mut self.summary_mode_idx, 2, "Eigener Prompt");
                });

            ui.label("Modell:");
            ui.text_edit_singleline(&mut self.ollama_model);
        });

        // Custom prompt input
        if self.summary_mode_idx == 2 {
            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::multiline(&mut self.custom_prompt)
                    .desired_width(f32::INFINITY)
                    .desired_rows(2)
                    .hint_text("Eigenen Prompt eingeben..."),
            );
        }

        ui.add_space(4.0);

        // Summarize button
        ui.horizontal(|ui| {
            let can_summarize = !self.summarizing && self.result.is_some();
            if ui
                .add_enabled(
                    can_summarize,
                    egui::Button::new("\u{2728} Zusammenfassen"),
                )
                .clicked()
            {
                if let Some(result) = &self.result {
                    let mode = match self.summary_mode_idx {
                        0 => SummaryMode::PodcastSummary,
                        1 => SummaryMode::MeetingProtocol,
                        _ => SummaryMode::Custom(self.custom_prompt.clone()),
                    };
                    let _ = self.summary_tx.send(SummaryCommand::Summarize {
                        text: result.text.clone(),
                        mode,
                        model: self.ollama_model.clone(),
                    });
                }
            }

            if self.summarizing {
                ui.spinner();
                ui.label(
                    egui::RichText::new("Wird zusammengefasst...")
                        .size(12.0)
                        .color(egui::Color32::GRAY),
                );
            }
        });

        // Display summary result
        if let Some(summary) = &self.summary.clone() {
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .id_salt("summary_scroll")
                .max_height(200.0)
                .show(ui, |ui| {
                    let mut text = summary.clone();
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_width(f32::INFINITY)
                            .desired_rows(6)
                            .interactive(true)
                            .font(egui::TextStyle::Body),
                    );
                });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("\u{1F4CB} Zusammenfassung kopieren").clicked() {
                    ui.ctx().copy_text(summary.clone());
                    self.export_feedback =
                        Some("\u{2705} Zusammenfassung kopiert".to_string());
                }
            });
        }
    }

    fn render_error_dialog(&mut self, ctx: &egui::Context) {
        if let Some(error_msg) = self.error.clone() {
            let mut should_close = false;
            egui::Window::new("Fehler")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new("\u{26A0}\u{FE0F} Ein Fehler ist aufgetreten")
                            .size(14.0)
                            .color(egui::Color32::from_rgb(220, 80, 80)),
                    );
                    ui.add_space(8.0);
                    ui.label(&error_msg);
                    ui.add_space(8.0);
                    if ui.button("OK").clicked() {
                        should_close = true;
                    }
                });

            if should_close {
                self.error = None;
                if matches!(self.state, AppState::Transcribing { .. } | AppState::Converting) {
                    self.state = AppState::Ready;
                }
            }
        }
    }
}

impl eframe::App for KlartextApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker_status();
        self.poll_summary_status();

        let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped_files.is_empty() {
            self.handle_dropped_files(dropped_files);
        }

        self.hovering_files = ctx.input(|i| !i.raw.hovered_files.is_empty());
        self.render_error_dialog(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.state.clone() {
                AppState::LoadingModel => {
                    self.render_loading_model(ui);
                }
                AppState::Ready => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.render_ready(ui);
                        self.render_result_view(ui);
                        self.render_summary_section(ui);
                    });
                }
                AppState::Converting => {
                    self.render_converting(ui);
                }
                AppState::Transcribing { progress } => {
                    self.render_transcribing(ui, progress);
                }
                AppState::ModelLoadFailed(msg) => {
                    self.render_model_load_failed(ui, &msg);
                }
            }
        });

        ctx.request_repaint();
    }
}

impl Drop for KlartextApp {
    fn drop(&mut self) {
        info!("KlartextApp shutting down");
        let _ = self.command_tx.send(Command::Cancel);
        let _ = self.command_tx.send(Command::Shutdown);
        let _ = self.summary_tx.send(SummaryCommand::Shutdown);
    }
}
