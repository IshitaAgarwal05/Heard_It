use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

struct Resampler {
    in_rate: u32,
    out_rate: u32,
    step: f64,
    pos: f64,
    buffer: Vec<f32>,
}

impl Resampler {
    fn new(in_rate: u32, out_rate: u32) -> Self {
        let step = in_rate as f64 / out_rate as f64;
        Resampler { in_rate, out_rate, step, pos: 0.0, buffer: Vec::new() }
    }

    // Push input samples and return resampled i16 vector
    fn push_and_resample(&mut self, input: &[i16]) -> Vec<i16> {
        // append input (as f32)
        for &s in input {
            self.buffer.push(s as f32);
        }

        let mut out: Vec<i16> = Vec::new();

        // Produce resampled output while we have at least two samples available
        // at the current fractional position (pos) and pos+1.
        loop {
            // we need access to floor(pos) and floor(pos)+1
            let pos_floor = self.pos.floor() as usize;
            if pos_floor + 1 >= self.buffer.len() {
                break;
            }

            let frac = (self.pos - (pos_floor as f64)) as f32;
            let s0 = self.buffer[pos_floor];
            let s1 = self.buffer[pos_floor + 1];
            let sample_f = s0 * (1.0 - frac) + s1 * frac;

            // clamp to i16
            let sample_i16 = if sample_f.is_nan() {
                0i16
            } else {
                let v = sample_f.round() as i64;
                if v > i16::MAX as i64 { i16::MAX } else if v < i16::MIN as i64 { i16::MIN } else { v as i16 }
            };
            out.push(sample_i16);

            self.pos += self.step;
        }

        // Drop consumed input samples to keep buffer small. Remove floor(pos) samples
        // from the front and subtract that count from pos.
        let remove = self.pos.floor() as usize;
        if remove > 0 {
            if remove >= self.buffer.len() {
                // If we've consumed everything, clear buffer and reset pos
                self.buffer.clear();
                self.pos = 0.0;
            } else {
                self.buffer.drain(0..remove);
                self.pos -= remove as f64;
            }
        }

        out
    }
}

pub async fn stream_to_deepgram(
    mut rx: UnboundedReceiver<Vec<i16>>,
    app: AppHandle,
    sample_rate: u32,
) {
    let api_key = std::env::var("DEEPGRAM_API_KEY")
        .expect("DEEPGRAM_API_KEY not set");

    // If we will resample to 16000, tell Deepgram we'll be sending 16000 samples/sec.
    let send_sample_rate = if sample_rate != 16000 { 16000 } else { sample_rate };
    let url = format!(
        "wss://api.deepgram.com/v1/listen?encoding=linear16&sample_rate={}&punctuate=true",
        send_sample_rate
    );

    let mut request = url.into_client_request().unwrap();

    request.headers_mut().insert(
        "Authorization",
        format!("Token {}", api_key).parse().unwrap(),
    );

    println!("🌐 Connecting to Deepgram…");
    let (mut ws, _) = connect_async(request).await.expect("WS failed");
    println!("✅ Connected to Deepgram");

    // Prepare resampler (only used if we need to convert device rate -> send_sample_rate)
    let mut maybe_resampler = if sample_rate != send_sample_rate {
        Some(Resampler::new(sample_rate, send_sample_rate))
    } else {
        None
    };

    loop {
        tokio::select! {
            Some(chunk) = rx.recv() => {
                // Resample if needed and accumulate into a send buffer. We batch
                // small frames into larger chunks (~250ms) before sending to Deepgram.
                let out_vec: Vec<i16> = if let Some(res) = maybe_resampler.as_mut() {
                    let v = res.push_and_resample(&chunk);
                    println!("🔁 Resampled {} -> {} samples", chunk.len(), v.len());
                    v
                } else {
                    println!("🔁 Forwarding {} samples (no resample)", chunk.len());
                    chunk
                };

                // threshold: ~250ms worth of samples at send_sample_rate
                let threshold_ms = 250f32;
                let threshold_samples = ((send_sample_rate as f32) * (threshold_ms / 1000.0)).max(800.0) as usize;

                // send buffer stored in outer scope local variable (create when first used)
                static mut SEND_BUF_PTR: *mut Vec<i16> = std::ptr::null_mut();
                // Safety: we mutate only within this single async task; use lazy init
                let send_buf = unsafe {
                    if SEND_BUF_PTR.is_null() {
                        let b: Box<Vec<i16>> = Box::new(Vec::new());
                        SEND_BUF_PTR = Box::into_raw(b);
                    }
                    &mut *SEND_BUF_PTR
                };

                send_buf.extend_from_slice(&out_vec);

                // While we have enough samples, send in threshold-sized chunks
                while send_buf.len() >= threshold_samples {
                    let mut to_send: Vec<i16> = send_buf.drain(0..threshold_samples).collect();
                    let bytes = unsafe {
                        std::slice::from_raw_parts(
                            to_send.as_ptr() as *const u8,
                            to_send.len() * 2,
                        )
                    };
                    println!("📤 Sending {} bytes to Deepgram (sample_rate={})", bytes.len(), send_sample_rate);
                    let _ = ws.send(Message::Binary(bytes.to_vec())).await;
                }
            }

            msg = ws.next() => {
                // Handle websocket messages robustly to avoid macro-level panics
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        println!("📨 Deepgram JSON: {}", text);
                        if let Ok(json) = serde_json::from_str::<Value>(&text) {
                            if let Some(transcript) = json["results"]["channels"][0]["alternatives"][0]["transcript"].as_str() {
                                if !transcript.trim().is_empty() {
                                    println!("📝 TRANSCRIPT: {}", transcript);
                                    let _ = app.emit("transcript", transcript.to_string()).ok();
                                }
                            }
                        }
                    }
                    Some(Ok(_other)) => {
                        // ignore non-text frames
                    }
                    Some(Err(e)) => {
                        eprintln!("❌ Deepgram WS error: {}", e);
                        break;
                    }
                    None => {
                        println!("🔌 Deepgram websocket closed");
                        break;
                    }
                }
            }
        }
    }
}
