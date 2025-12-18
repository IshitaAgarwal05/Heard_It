use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

pub async fn stream_to_deepgram(
    mut rx: UnboundedReceiver<Vec<i16>>,
    app: AppHandle,
) {
    let api_key = std::env::var("DEEPGRAM_API_KEY")
        .expect("DEEPGRAM_API_KEY not set");

    let mut request =
        "wss://api.deepgram.com/v1/listen?encoding=linear16&sample_rate=16000"
            .into_client_request()
            .unwrap();

    request.headers_mut().insert(
        "Authorization",
        format!("Token {}", api_key).parse().unwrap(),
    );

    let (ws, _) = connect_async(request)
        .await
        .expect("Deepgram WS failed");

    let mut ws = ws;

    while let Some(chunk) = rx.recv().await {
        let bytes = unsafe {
            std::slice::from_raw_parts(
                chunk.as_ptr() as *const u8,
                chunk.len() * 2,
            )
        };

        ws.send(Message::Binary(bytes.to_vec())).await.ok();

        if let Some(Ok(Message::Text(text))) = ws.next().await {
            if let Ok(json) = serde_json::from_str::<Value>(&text) {
                if let Some(transcript) =
                    json["channel"]["alternatives"][0]["transcript"]
                        .as_str()
                {
                    if !transcript.is_empty() {
                        let _ = app.emit("transcript", transcript.to_string());
                    }
                }
            }
        }
    }
}
