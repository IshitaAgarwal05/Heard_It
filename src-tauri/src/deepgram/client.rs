use tokio_tungstenite::{connect_async, WebSocketStream};
use tokio_tungstenite::tungstenite::Message;
use tokio::net::TcpStream;
use futures_util::{SinkExt, StreamExt};
use url::Url;

pub type DGSocket = WebSocketStream<TcpStream>;

pub async fn connect(api_key: String) -> DGSocket {
    let url = Url::parse(
        "wss://api.deepgram.com/v1/listen?encoding=linear16&sample_rate=16000"
    ).unwrap();

    let req = http::Request::builder()
        .uri(url.as_str())
        .header("Authorization", format!("Token {}", api_key))
        .body(())
        .unwrap();

    let (ws, _) = connect_async(req).await.expect("Deepgram WS failed");
    ws
}
