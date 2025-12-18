import { useEffect, useState } from "react";
import { Mic, MicOff } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import "./App.css";

export default function App() {
  // 🎯 SINGLE SOURCE OF TRUTH
  const [isRecording, setIsRecording] = useState(false);

  // 📝 Live + final transcript
  const [liveText, setLiveText] = useState("");
  const [finalText, setFinalText] = useState("");

  // 🗂 Transcript history
  const [history, setHistory] = useState<string[]>([]);

  // 🎙️ Start / Stop recording
  const toggleRecording = async () => {
    try {
      if (!isRecording) {
        await invoke("start_recording");
      } else {
        await invoke("stop_recording");
      }
      setIsRecording((prev) => !prev);
    } catch (err) {
      console.error("Toggle failed:", err);
    }
  };

  // ⌨️ SPACEBAR ↔ MIC SYNC
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.code === "Space") {
        e.preventDefault();
        toggleRecording();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isRecording]);

  // 🧠 LISTEN TO BACKEND TRANSCRIPTS
  useEffect(() => {
    const unlistenPromise = listen<string>("transcript", (event) => {
      const text = event.payload.trim();
      if (!text) return;

      // ✨ Simple smoothing heuristic
      if (text.endsWith(".") || text.endsWith("?") || text.endsWith("!")) {
        setFinalText((prev) => (prev ? prev + " " + text : text));
        setLiveText("");
      } else {
        setLiveText(text);
      }
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  // 🗂 SAVE TRANSCRIPT WHEN RECORDING STOPS
  useEffect(() => {
    if (!isRecording && finalText.trim()) {
      setHistory((prev) => [finalText, ...prev]);
      setFinalText("");
      setLiveText("");
    }
  }, [isRecording]);

  return (
    <div className="app">
      {/* Header */}
      <h1 className="title">🎙️ Heard It</h1>
      <p className="subtitle">Press spacebar or click the mic</p>

      {/* Mic Button */}
      <div
        className={`mic ${isRecording ? "active" : ""}`}
        onClick={toggleRecording}
      >
        {isRecording ? <Mic size={44} /> : <MicOff size={44} />}
      </div>

      {/* Transcript Display */}
      <div className="transcript">
        <span className="final">{finalText}</span>
        <span className="live">{liveText}</span>
      </div>

      {/* History Panel */}
      {history.length > 0 && (
        <div className="history">
          <h3>Transcript History</h3>
          {history.map((item, idx) => (
            <div key={idx} className="history-item">
              {item}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
