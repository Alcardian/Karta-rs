#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use chrono::Local;
use eframe::egui;
use eframe::egui::{Color32, RichText, ScrollArea};
use std::{
  fs::{self, File, OpenOptions},
  io::{self, Write},
  path::{Path, PathBuf},
  sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{self, Receiver, Sender},
    Arc,
  },
  thread,
  time::Duration,
};

/// CSV format constants
const FILE_PREFIX: &str = "data_";
const FILE_EXTENSION: &str = ".csv";
const FILE_HEADER_ROW: &str = "Pantheon,X,Z,Y\n";

const DEFAULT_INTERVAL_MS: u64 = 1000;
const LOG_MAX_LINES: usize = 500;

#[derive(Debug)]
enum AppEvent {
  Info(String),
  Error(String),
}

fn main() -> eframe::Result<()> {
  let native_options = eframe::NativeOptions {
    // Keep defaults; window controls (minimize/maximize/close) are standard.
    ..Default::default()
  };

  let app = KartaApp::new();
  eframe::run_native("Karta", native_options, Box::new(|_| Box::new(app)))
}

struct KartaApp {
  // UI state
  log_lines: Vec<String>,
  running_display: bool,
  refresh_input_ms: u64,

  // Shared control state for worker thread
  running_flag: Arc<AtomicBool>,
  interval_ms: Arc<AtomicU64>,

  // Thread comms
  rx: Receiver<AppEvent>,
  stop_tx: Sender<()>,

  // Worker join handle
  worker: Option<thread::JoinHandle<()>>,

  // Data file path
  data_file: PathBuf,
}

impl KartaApp {
  fn new() -> Self {
    // Prepare output CSV file
    let data_file = generate_unique_file_path();
    if let Err(e) = ensure_csv_with_header(&data_file) {
      eprintln!("Failed to prepare CSV file: {e}");
    }

    // Shared state
    let running_flag = Arc::new(AtomicBool::new(false)); // Start paused
    let interval_ms = Arc::new(AtomicU64::new(DEFAULT_INTERVAL_MS));

    // Channel for worker -> UI events
    let (tx, rx) = mpsc::channel::<AppEvent>();

    // Channel for UI -> worker stop signal
    let (stop_tx, stop_rx) = mpsc::channel::<()>();

    // Spawn worker thread
    let worker = {
      let running_flag = Arc::clone(&running_flag);
      let interval_ms = Arc::clone(&interval_ms);
      let data_file = data_file.clone();

      Some(thread::spawn(move || {
          worker_loop(running_flag, interval_ms, tx, stop_rx, data_file);
      }))
    };

    Self {
      log_lines: vec![format!(
        "Ready. Output file: {}",
        data_file.file_name().unwrap_or_default().to_string_lossy()
      )],
      running_display: false,
      refresh_input_ms: DEFAULT_INTERVAL_MS,
      running_flag,
      interval_ms,
      rx,
      stop_tx,
      worker,
      data_file,
    }
  }

  fn push_log(&mut self, line: impl Into<String>) {
    self.log_lines.push(line.into());
    if self.log_lines.len() > LOG_MAX_LINES {
      let excess = self.log_lines.len() - LOG_MAX_LINES;
      self.log_lines.drain(0..excess);
    }
  }
}

impl eframe::App for KartaApp {
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    // Light theme + parchment background
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    // Soft parchment-like background for panels
    style.visuals.panel_fill = Color32::from_rgb(242, 236, 222);
    ctx.set_style(style);

    // Drain worker events
    while let Ok(ev) = self.rx.try_recv() {
      match ev {
        AppEvent::Info(msg) => self.push_log(msg),
        AppEvent::Error(msg) => self.push_log(format!("[Error] {msg}")),
      }
    }

    // UI
    egui::CentralPanel::default().show(ctx, |ui| {
      ui.vertical(|ui| {
        ui.heading(RichText::new("Karta").strong());
        ui.add_space(4.0);
        ui.horizontal(|ui| {
          let is_running = self.running_flag.load(Ordering::Relaxed);

          // Toggle button
          if !is_running {
            if ui.button("▶ Start").clicked() {
              self.running_flag.store(true, Ordering::Relaxed);
              self.running_display = true;
              self.push_log("Monitoring started.");
            }
          } else if ui.button("⏸ Pause").clicked() {
            self.running_flag.store(false, Ordering::Relaxed);
            self.running_display = false;
            self.push_log("Monitoring paused.");
          }

          // Status text
          let status = if is_running { "Running" } else { "Paused" };
          ui.label(RichText::new(format!("Status: {status}")).strong()
            .color(if is_running {
              Color32::from_rgb(0, 128, 0)
            } else {
              Color32::from_rgb(128, 0, 0)
            }),
          );
        });

        ui.add_space(8.0);

        // Refresh rate control
        ui.horizontal(|ui| {
          ui.label("Polling interval (ms):");
          ui.add(egui::DragValue::new(&mut self.refresh_input_ms).speed(1.0).clamp_range(50..=60_000),);
          if ui.button("Update").clicked() {
            self.interval_ms.store(self.refresh_input_ms, Ordering::Relaxed);
            self.push_log(format!("Polling interval updated to {} ms.",self.refresh_input_ms));
          }
        });

        ui.add_space(8.0);

        // Show current CSV filename
        ui.label(format!("Writing to: {}",self.data_file.file_name().unwrap_or_default().to_string_lossy()));

        ui.add_space(12.0);

        ui.separator();
        ui.add_space(4.0);
        ui.label(RichText::new("Log").strong());
        ui.add_space(4.0);

        ScrollArea::vertical().auto_shrink([false; 2]).stick_to_bottom(true).show(ui, |ui| {
          for line in &self.log_lines {
            ui.monospace(line);
          }
        });
      });
    });

    // Keep the UI snappy even when idle
    ctx.request_repaint_after(Duration::from_millis(100));
  }
}

impl Drop for KartaApp {
  fn drop(&mut self) {
    // Ask worker to stop
    let _ = self.stop_tx.send(());

    // Join the worker if possible
    if let Some(handle) = self.worker.take() {
      let _ = handle.join();
    }
  }
}

/// The worker thread: polls clipboard when running, appends valid entries to CSV, and reports to UI.
fn worker_loop(
  running_flag: Arc<AtomicBool>,
  interval_ms: Arc<AtomicU64>,
  tx: Sender<AppEvent>,
  stop_rx: Receiver<()>,
  data_file: PathBuf,
) {
  let mut last_clipboard = String::new();

  // Prepare a local Clipboard handle; if it fails, keep retrying with delays.
  // (arboard uses platform backends; creation can fail transiently)
  let mut clipboard = loop {
      match arboard::Clipboard::new() {
          Ok(cb) => break cb,
          Err(e) => {
              let _ = tx.send(AppEvent::Error(format!("Clipboard init failed: {e}. Retrying in 1s…")));
              if stop_received_now(&stop_rx) {
                  return;
              }
              thread::sleep(Duration::from_millis(1000));
          }
      }
  };

  // Main loop
  loop {
    // Stop?
    if stop_received_now(&stop_rx) {
      let _ = tx.send(AppEvent::Info("Stopping…".to_string()));
      return;
    }

    let is_running = running_flag.load(Ordering::Relaxed);
    let sleep_ms = interval_ms.load(Ordering::Relaxed);

    if is_running {
      match clipboard.get_text() {
        Ok(current) => {
          if current != last_clipboard {
            // Update last clipboard regardless of validity
            last_clipboard = current.clone();

            if current.contains("/jumploc") {
              let formatted = convert_spaces_to_commas(&current);

              if let Err(e) = append_line(&data_file, &formatted) {
                let _ = tx.send(AppEvent::Error(format!("Failed writing to CSV: {e}")));
              } else {
                let _ = tx.send(AppEvent::Info(format!("Appended: {}",preview_line(&formatted))));
              }
            }
          }
        }
        Err(e) => {
          let _ = tx.send(AppEvent::Error(format!(
            "Clipboard read error: {e}"
          )));
          // Try to re-create clipboard once if it looks broken
          if let Ok(cb) = arboard::Clipboard::new() {
            clipboard = cb;
          }
        }
      }
    }

    // Sleep respecting current interval, but also wake early on stop signal
    // (simple approach: split sleep into small chunks)
    let mut slept = 0u64;
    while slept < sleep_ms {
      if stop_received_now(&stop_rx) {
        let _ = tx.send(AppEvent::Info("Stopping…".to_string()));
        return;
      }
      let chunk = (sleep_ms - slept).min(50);
      thread::sleep(Duration::from_millis(chunk));
      slept += chunk;
    }
  }
}

fn stop_received_now(stop_rx: &Receiver<()>) -> bool {
  match stop_rx.try_recv() {
    Ok(_) | Err(mpsc::TryRecvError::Disconnected) => true,
    Err(mpsc::TryRecvError::Empty) => false,
  }
}

fn generate_unique_file_path() -> PathBuf {
  let ts = Local::now().format("%y%m%d_%H%M").to_string();
  let filename = format!("{FILE_PREFIX}{ts}{FILE_EXTENSION}");
  std::env::current_dir()
    .unwrap_or_else(|_| ".".into())
    .join(filename)
}

fn ensure_csv_with_header(path: &Path) -> io::Result<()> {
  if !path.exists() {
    if let Some(parent) = path.parent() {
      if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent)?;
      }
    }
    let mut f = File::create(path)?;
    f.write_all(FILE_HEADER_ROW.as_bytes())?;
    f.flush()?;
  }
  Ok(())
}

fn append_line(path: &Path, line: &str) -> io::Result<()> {
  let mut f = OpenOptions::new()
    .create(true)
    .append(true)
    .open(path)?;
  f.write_all(line.as_bytes())?;
  f.write_all(b"\n")?;
  f.flush()?;
  Ok(())
}

fn convert_spaces_to_commas(input: &str) -> String {
  // Match Java behavior: only replace ASCII spaces with commas
  input.replace(' ', ",")
}

/// Truncate long lines for the on-screen log
fn preview_line(s: &str) -> String {
  const MAX: usize = 120;
  if s.len() <= MAX {
    s.to_string()
  } else {
    format!("{}…", &s[..MAX])
  }
}
