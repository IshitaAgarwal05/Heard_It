use std::{fs, path::PathBuf};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

pub async fn transcribe_file(path: PathBuf, app: AppHandle) {
    println!("🚀 Starting file transcription");

    let api_key = std::env::var("DEEPGRAM_API_KEY")
        .expect("DEEPGRAM_API_KEY not set");

    let audio_bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("❌ Failed to read file: {}", e);
            return;
        }
    };

    let client = reqwest::Client::new();

    let response = client
        .post("https://api.deepgram.com/v1/listen?punctuate=true")
        .header("Authorization", format!("Token {}", api_key))
        .header("Content-Type", "audio/*")
        .body(audio_bytes)
        .send()
        .await;

    let response = match response {
        Ok(r) => r,
        Err(e) => {
            eprintln!("❌ HTTP error: {}", e);
            return;
        }
    };

    let json: Value = match response.json().await {
        Ok(j) => j,
        Err(e) => {
            eprintln!("❌ JSON parse error: {}", e);
            return;
        }
    };

    println!("📨 Deepgram JSON: {}", json);

    let transcript = json["results"]["channels"][0]["alternatives"][0]["transcript"]
        .as_str()
        .unwrap_or("")
        .to_string();

    if transcript.is_empty() {
        println!("⚠️ Empty transcript");
    } else {
        println!("📝 TRANSCRIPT: {}", transcript);
        let _ = app.emit("transcript", transcript);
    }
}
