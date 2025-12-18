# 🎧 Heard It — Push-to-Talk Transcription App

Heard It is a desktop push-to-talk application built using **Tauri + React + Rust**, enabling real-time speech-to-text transcription using **Deepgram**, designed for real-time transcription with a clean, distraction-free interface.


---

## ✨ Features
- 🎙️ Push-to-Talk using **SPACE key**
- 🔴 Live microphone capture (Rust + CPAL)
- 🌐 Real-time transcription via Deepgram WebSocket
- ✨ Smooth partial transcript rendering
- 📝 Live transcript display
- 🗂 Transcript history panel (USP)
- 💻 Lightweight cross-platform desktop app (Tauri)


---

## 💡 Why Heard It?
- Unlike typical recorders, Heard It focuses on:
- Instant feedback
- Minimal UI
- Zero friction transcription

---

## 🧠 Tech Stack
- **Frontend:** React + Vite + TypeScript
- **Backend:** Rust (Tauri)
- **Audio:** CPAL
- **Streaming:** WebSocket (Deepgram)
- **UI:** Custom CSS (no bloated UI libs)

---

## ▶️ How It Works
1. Press **Spacebar** or click mic
2. Audio stream starts
3. Audio chunks sent to Deepgram
4. Live text streamed back
5. Final sentences stored in history

---



## 🧠 Architecture
```
React UI
↕ Tauri IPC
Rust Backend
├── Mic capture (CPAL)
├── WebSocket streaming
└── Deepgram STT
```

---


## 🚀 How to Run
- In first terminal,
```bash
export DEEPGRAM_API_KEY=your_key_here
npm install
npm run dev
```

- In another terminal,
```bash
npm run tauri dev
```

---

## 🏆 Why Tauri?
- Native performance
- Secure IPC
- Tiny binary size vs Electron

