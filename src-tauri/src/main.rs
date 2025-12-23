#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod deepgram;

use tauri::AppHandle;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;
use std::sync::Mutex;
use tauri_plugin_dialog::DialogExt;
use std::fs;
use serde_json;
use std::time::{SystemTime, UNIX_EPOCH};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::io::Read;
use std::thread;

static AUDIO_TX: Mutex<Option<UnboundedSender<Vec<i16>>>> = Mutex::new(None);

/// 🎙️ List available mic devices (CPAL)
#[tauri::command]
fn list_mic_devices() -> Vec<String> {
    audio::list_input_devices()
}

/// 🎙️ Start recording from selected mic
#[tauri::command]
fn start_recording(app: AppHandle, device: String) {
    println!("🎙️ Recording started using device: {}", device);

    let (tx, rx) = mpsc::unbounded_channel::<Vec<i16>>();

    {
        let mut guard = AUDIO_TX.lock().unwrap();
        *guard = Some(tx.clone());
    }

    // Try to spawn the helper audio worker process which writes framed i16 PCM to stdout.
    // If that fails, fall back to the in-process CPAL stream.

    // Helper to locate worker binary next to the current exe.
    fn worker_path_name() -> std::path::PathBuf {
        let worker_name = if cfg!(windows) { "audio_worker.exe" } else { "audio_worker" };
        if let Ok(p) = std::env::current_exe() {
            if let Some(dir) = p.parent() {
                let cand = dir.join(worker_name);
                if cand.exists() {
                    return cand;
                }
            }
        }
        // fallback to just the name (assume in PATH)
        std::path::PathBuf::from(worker_name)
    }

    let worker_path = worker_path_name();

    // Attempt to spawn worker with --device <name>
    let spawn_result = Command::new(&worker_path)
        .arg("--device")
        .arg(&device)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn();

    if let Ok(mut child) = spawn_result {
        // read header (magic + sample_rate)
        if let Some(mut out) = child.stdout.take() {
            // blocking read for header
            let mut header = [0u8; 8];
            match out.read_exact(&mut header) {
                Ok(_) => {
                    if &header[0..4] != b"SRAT" {
                        eprintln!("audio_worker sent invalid header");
                    }
                    let sr = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
                    let sample_rate = if sr == 0 { 16000 } else { sr };

                    println!("🔌 Spawned audio_worker (pid={}) sample_rate={}", child.id(), sample_rate);

                    // Spawn Deepgram streaming task with the received sample_rate
                    tauri::async_runtime::spawn(async move {
                        println!("🧵 Deepgram async task started (worker mode)");
                        deepgram::stream_to_deepgram(rx, app, sample_rate).await;
                        println!("🧵 Deepgram async task ended (worker mode)");
                    });

                    // Move tx clone into a blocking thread that reads frames and forwards
                    let forwarding_sender = tx.clone();
                    thread::spawn(move || {
                        let mut reader = out;
                        loop {
                            // read frame length (u32 LE)
                            let mut lenb = [0u8; 4];
                            if let Err(e) = reader.read_exact(&mut lenb) {
                                eprintln!("audio_worker read error (len): {}", e);
                                break;
                            }
                            let len = u32::from_le_bytes(lenb) as usize;
                            let mut buf = vec![0u8; len * 2];
                            if let Err(e) = reader.read_exact(&mut buf) {
                                eprintln!("audio_worker read error (payload): {}", e);
                                break;
                            }
                            // convert to i16 samples
                            let mut samples = Vec::with_capacity(len);
                            for i in 0..len {
                                let lo = buf[i * 2];
                                let hi = buf[i * 2 + 1];
                                samples.push(i16::from_le_bytes([lo, hi]));
                            }

                            // send to channel
                            if forwarding_sender.send(samples).is_err() {
                                eprintln!("Failed to forward audio frame; receiver closed");
                                break;
                            }
                        }

                        // if we exit loop, ensure child is killed
                        let _ = child.kill();
                    });
                    return;
                }
                Err(e) => {
                    eprintln!("Failed to read header from audio_worker: {}", e);
                    let _ = child.kill();
                }
            }
        } else {
            eprintln!("audio_worker spawned without stdout");
            let _ = child.kill();
        }
    } else if let Err(e) = spawn_result {
        eprintln!("Failed to spawn audio_worker {:?}: {}", worker_path, e);
    }

    // Fallback: if worker spawn failed or header read failed, use in-process CPAL stream
    println!("↩️ Falling back to in-process mic stream");
    let sample_rate = audio::start_mic_stream_with_device(device, app.clone(), move |frame| {
        let guard = AUDIO_TX.lock().unwrap();
        if let Some(sender) = guard.as_ref() {
            let _ = sender.send(frame);
        }
    })
    .unwrap_or(16000);

    // Spawn Deepgram streaming task (fallback)
    println!("🚀 Spawning Deepgram task (fallback)");
    tauri::async_runtime::spawn(async move {
        println!("🧵 Deepgram async task started (fallback)");
        deepgram::stream_to_deepgram(rx, app, sample_rate).await;
        println!("🧵 Deepgram async task ended (fallback)");
    });
}

/// 🛑 Stop recording
#[tauri::command]
fn stop_recording() {
    println!("🛑 Recording stopped");

    {
        let mut guard = AUDIO_TX.lock().unwrap();
        *guard = None;
    }

    audio::stop_mic_stream();
}

/// 📄 Export transcript as TXT
#[tauri::command]
async fn export_txt(app: AppHandle, transcript: String) -> Result<(), String> {
    app.dialog()
        .file()
        .set_title("Export Transcript (.txt)")
        .add_filter("Text File", &["txt"])
        .save_file(move |path| {
            if let Some(p) = path.and_then(|f| f.as_path().map(|p| p.to_path_buf())) {
                let _ = fs::write(p, transcript);
            }
        });

    Ok(())
}

/// 📄 Export transcript as Markdown
#[tauri::command]
async fn export_md(app: AppHandle, transcript: String) -> Result<(), String> {
    let content = format!("# Transcript\n\n{}", transcript);

    app.dialog()
        .file()
        .set_title("Export Transcript (.md)")
        .add_filter("Markdown", &["md"])
        .save_file(move |path| {
            if let Some(p) = path.and_then(|f| f.as_path().map(|p| p.to_path_buf())) {
                let _ = fs::write(p, content);
            }
        });

    Ok(())
}

/// 📄 Export transcript as SRT
#[tauri::command]
async fn export_srt(app: AppHandle, transcript: String) -> Result<(), String> {
    // naive sentence split
    let parts: Vec<&str> = transcript.split(". ").collect();
    let mut srt = String::new();
    let mut time: u64 = 0;
    for (i, p) in parts.iter().enumerate() {
        let start = time;
        let end = time + 5; // 5s per chunk
        let idx = i + 1;
        let start_ts = format!("{:02}:{:02}:{:02},000", start / 3600, (start % 3600) / 60, start % 60);
        let end_ts = format!("{:02}:{:02}:{:02},000", end / 3600, (end % 3600) / 60, end % 60);
        srt.push_str(&format!("{}\n{} --> {}\n{}\n\n", idx, start_ts, end_ts, p.trim()));
        time = end;
    }

    app.dialog()
        .file()
        .set_title("Export Transcript (.srt)")
        .add_filter("SRT", &["srt"])
        .save_file(move |path| {
            if let Some(p) = path.and_then(|f| f.as_path().map(|p| p.to_path_buf())) {
                let _ = fs::write(p, srt.clone());
            }
        });

    Ok(())
}

/// 📄 Export transcript as VTT
#[tauri::command]
async fn export_vtt(app: AppHandle, transcript: String) -> Result<(), String> {
    let parts: Vec<&str> = transcript.split(". ").collect();
    let mut vtt = String::from("WEBVTT\n\n");
    let mut time: u64 = 0;
    for p in parts.iter() {
        let start = time;
        let end = time + 5;
        let start_ts = format!("{:02}:{:02}:{:02}.000", start / 3600, (start % 3600) / 60, start % 60);
        let end_ts = format!("{:02}:{:02}:{:02}.000", end / 3600, (end % 3600) / 60, end % 60);
        vtt.push_str(&format!("{} --> {}\n{}\n\n", start_ts, end_ts, p.trim()));
        time = end;
    }

    app.dialog()
        .file()
        .set_title("Export Transcript (.vtt)")
        .add_filter("VTT", &["vtt"])
        .save_file(move |path| {
            if let Some(p) = path.and_then(|f| f.as_path().map(|p| p.to_path_buf())) {
                let _ = fs::write(p, vtt.clone());
            }
        });

    Ok(())
}

/// 💾 Save history silently to the app data directory (no dialog)
#[tauri::command]
fn save_history_auto(_app: AppHandle, history: Vec<String>) -> Result<String, String> {
    let content = serde_json::to_string_pretty(&history).map_err(|e| e.to_string())?;

    // fallback to $HOME/.local/share/heard_it if app dir isn't available
    let dir: PathBuf = match std::env::var("HOME") {
        Ok(h) => PathBuf::from(h).join(".local/share/heard_it"),
        Err(_) => return Err("Could not resolve HOME directory".into()),
    };

    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Err(format!("Failed to create app dir: {}", e));
    }

    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    let filename = format!("transcript_history_{}.json", now);
    let path = dir.join(filename);

    std::fs::write(&path, content).map_err(|e| e.to_string())?;

    Ok(path.to_string_lossy().to_string())
}

/// 💾 Save transcript history to disk (JSON)
#[tauri::command]
async fn save_history(app: AppHandle, history: Vec<String>) -> Result<(), String> {
    let content = match serde_json::to_string_pretty(&history) {
        Ok(s) => s,
        Err(e) => return Err(format!("Failed to serialize history: {}", e)),
    };

    app.dialog()
        .file()
        .set_title("Save Transcript History (.json)")
        .add_filter("JSON", &["json"])
        .save_file(move |path| {
            if let Some(p) = path.and_then(|f| f.as_path().map(|p| p.to_path_buf())) {
                let _ = fs::write(p, content.clone());
            }
        });

    Ok(())
}

/// 🚀 App entry
fn main() {
    dotenvy::dotenv().ok();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            list_mic_devices,
            start_recording,
            stop_recording,
            export_txt,
            export_md,
            export_srt,
            export_vtt,
            save_history,
            save_history_auto
        ])
        .run(tauri::generate_context!())
        .expect("❌ error while running tauri application");
}
