import { useEffect, useState, useRef } from "react";
import { Mic, MicOff, Upload, Download } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

export default function App() {
  // 🎙 Mic state
  const [isRecording, setIsRecording] = useState(false);
  const [micDevices, setMicDevices] = useState<string[]>([]);
  const [selectedMic, setSelectedMic] = useState<string>("");

  // 📝 Transcript
  const [liveText, setLiveText] = useState("");
  const [finalText, setFinalText] = useState("");

  // 🗂 History
  const [history, setHistory] = useState<string[]>([]);

  // ⏳ Progress
  const [isProcessing, setIsProcessing] = useState(false);

  const [levels, setLevels] = useState<number[]>([]);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [devicesOpen, setDevicesOpen] = useState(false);

  /* ===========================
     🎤 LOAD MIC DEVICES
  ============================ */
  useEffect(() => {
    // Ask for mic permission (will prompt OS/webview)
    if (navigator && (navigator as any).mediaDevices) {
      (navigator as any).mediaDevices
        .getUserMedia({ audio: true })
        .then((stream: MediaStream) => {
          // stop tracks immediately — permission granted
          stream.getTracks().forEach((t) => t.stop());
        })
        .catch((e: any) => console.warn("Mic permission denied or unavailable", e));
    }

    // Load saved selection
    const saved = localStorage.getItem("selectedMic");

    // Load saved history
    const savedHistory = localStorage.getItem("transcriptHistory");
    if (savedHistory) {
      try {
        setHistory(JSON.parse(savedHistory));
      } catch { /* ignore */ }
    }

    invoke<string[]>("list_mic_devices")
      .then((devices) => {
        console.log("🎤 Available mics:", devices);
        setMicDevices(devices);
        if (saved && devices.includes(saved)) {
          setSelectedMic(saved);
        } else if (devices.length > 0) {
          setSelectedMic(devices[0]);
        }
      })
      .catch(console.error);
  }, []);

  /* ===========================
     🎙 START / STOP MIC
  ============================ */
  const toggleRecording = async () => {
    if (!selectedMic) return;

    try {
      if (!isRecording) {
        setFinalText("");
        setLiveText("");
        await invoke("start_recording", { device: selectedMic });
        setIsRecording(true);
      } else {
        await invoke("stop_recording");
        setIsRecording(false);
      }
    } catch (e) {
      console.error(e);
    }
  };

  /* ===========================
     🎧 FILE UPLOAD
  ============================ */
  const uploadFile = async () => {
    setIsProcessing(true);
    setFinalText("");
    setLiveText("");

    await invoke("pick_and_transcribe_file");
  };

  /* ===========================
     🧠 LISTEN FOR TRANSCRIPTS
  ============================ */
  useEffect(() => {
    const unlisten = listen<string>("transcript", (e) => {
      console.log("📝 Transcript received:", e.payload);
      setFinalText((prev) =>
        prev ? prev + " " + e.payload : e.payload
      );
      setIsProcessing(false);
    });

    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // Listen for audio level events from backend
  useEffect(() => {
    const un = listen<number>("audio_level", (e) => {
      setLevels((prev) => {
        const next = [...prev, e.payload];
        if (next.length > 120) next.shift();
        return next;
      });
    });

    return () => { un.then((u) => u()); };
  }, []);

  /* ===========================
     ⌨ SPACEBAR SUPPORT
  ============================ */
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.code === "Space" && selectedMic) {
        e.preventDefault();
        toggleRecording();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selectedMic, isRecording]);

  /* ===========================
     🗂 SAVE HISTORY
  ============================ */
  useEffect(() => {
    if (!isRecording && finalText) {
      setHistory((h) => [finalText, ...h]);
    }
  }, [isRecording]);

  // persist history to localStorage
  useEffect(() => {
    localStorage.setItem("transcriptHistory", JSON.stringify(history));
  }, [history]);

  // auto save history to app data folder (no dialog)
  useEffect(() => {
    if (history.length === 0) return;
    invoke<string>("save_history_auto", { history })
      .then((path) => console.log("Auto-saved history to:", path))
      .catch((e) => console.warn("Failed to auto-save history:", e));
  }, [history]);

  // draw waveform/levels
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const ratio = window.devicePixelRatio || 1;
    const w = canvas.width = canvas.clientWidth * ratio;
    const h = canvas.height = canvas.clientHeight * ratio;
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = "rgba(255,255,255,0.02)";
    ctx.fillRect(0, 0, w, h);

    if (levels.length === 0) return;
    ctx.beginPath();
    ctx.moveTo(0, h / 2);
    const step = w / Math.max(1, levels.length - 1);
    for (let i = 0; i < levels.length; i++) {
      const x = i * step;
      const y = h / 2 - levels[i] * (h / 2);
      ctx.lineTo(x, y);
    }
    ctx.strokeStyle = "#60a5fa";
    ctx.lineWidth = 2 * ratio;
    ctx.stroke();
  }, [levels]);

  // persist selected mic
  useEffect(() => {
    if (selectedMic) localStorage.setItem("selectedMic", selectedMic);
  }, [selectedMic]);

  /* ===========================
     ⬇ EXPORT
  ============================ */
  const exportTxt = () =>
    invoke("export_txt", { transcript: finalText });

  const exportMd = () =>
    invoke("export_md", { transcript: finalText });

  const exportSrt = () =>
    invoke("export_srt", { transcript: finalText });

  const exportVtt = () =>
    invoke("export_vtt", { transcript: finalText });

  /* ===========================
     🖼 UI
  ============================ */
  return (
    <div className="app">
      <h1 className="title">🎙 Heard It</h1>
      <p className="subtitle">Mic or File → Instant Transcript</p>

      {/* 🎤 Mic Selector (custom) */}
      <div className="mic-dropdown">
        <button className="mic-select" onClick={() => setDevicesOpen(!devicesOpen)}>
          <Mic />
          <span className="mic-selected">{selectedMic || "Select microphone"}</span>
        </button>
        {devicesOpen && (
          <div className="mic-list">
            {micDevices.map((d) => (
              <div
                key={d}
                className="mic-item"
                onClick={() => { setSelectedMic(d); setDevicesOpen(false); }}
              >
                <div className="mic-item-left">
                  <Mic />
                </div>
                <div className="mic-item-right">
                  <div className="mic-name">{d}</div>
                  <div className="mic-vendor">{(d.match(/\(([^)]+)\)/) || ["", ""]).slice(1)[0]}</div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* 🎙 Mic Button */}
      <button
        className={`mic-btn ${isRecording ? "recording" : ""}`}
        disabled={!selectedMic}
        onClick={toggleRecording}
      >
        {isRecording ? <Mic /> : <MicOff />}
      </button>

      {/* VU Meter */}
      <div className="vu-wrap">
        <div className="vu-bar" style={{ width: `${(levels[levels.length-1]||0)*100}%` }} />
        <canvas ref={canvasRef} className="wave-canvas" />
      </div>

      {/* 📂 Upload */}
      <button className="upload-btn" onClick={uploadFile}>
        <Upload /> Upload Audio
      </button>

      {/* ⏳ Progress */}
      {isProcessing && (
        <div className="processing">
          <div className="spin">⏳</div>
          <p className="loading">Processing audio…</p>
        </div>
      )}

      {/* 📝 Transcript */}
      <div className="transcript-box">
        <p>{finalText || liveText || "Your transcript will appear here…"}</p>
      </div>

      {/* ⬇ Export */}
      {finalText && (
        <div className="export">
          <button onClick={exportTxt}>
            <Download /> TXT
          </button>
          <button onClick={exportMd}>
            <Download /> MD
          </button>
          <button onClick={exportSrt}>
            <Download /> SRT
          </button>
          <button onClick={exportVtt}>
            <Download /> VTT
          </button>
        </div>
      )}

      {/* 🗂 History */}
      {history.length > 0 && (
        <div className="history">
          <h3>History</h3>
          {history.map((h, i) => (
            <div key={i} className="history-item">
              {h}
            </div>
          ))}
          <div style={{ marginTop: 12 }}>
            <button
              className="upload-btn"
              // onClick={() => invoke("export_txt", { transcript: history.join("\n\n") })}
              onClick={() => invoke("save_history", { history })}
            >
              <Download /> Save History
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
