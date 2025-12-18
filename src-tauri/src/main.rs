#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod deepgram;

use tauri::{command, AppHandle};
use tokio::sync::mpsc;

static mut TX: Option<mpsc::UnboundedSender<Vec<i16>>> = None;

#[command]
fn start_recording(app: AppHandle) {
    let (tx, rx) = mpsc::unbounded_channel::<Vec<i16>>();

    unsafe {
        TX = Some(tx);
    }

    audio::start_mic_stream(move |data| {
        unsafe {
            if let Some(tx) = &TX {
                let _ = tx.send(data.to_vec());
            }
        }
    });

    tauri::async_runtime::spawn(
        deepgram::stream_to_deepgram(rx, app)
    );

    println!("🎙️ Recording started");
}

#[command]
fn stop_recording() {
    unsafe {
        TX = None;
    }
    println!("🛑 Recording stopped");
}

fn main() {
    dotenvy::dotenv().ok(); 
    
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
