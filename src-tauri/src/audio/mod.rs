use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, SampleFormat, StreamConfig, BuildStreamError,
};
use tauri::AppHandle;
use tauri::Emitter;
use std::sync::{mpsc, OnceLock, Arc};

enum AudioCommand {
    Start {
        device_name: Option<String>,
        on_data: Arc<dyn Fn(Vec<i16>) + Send + Sync + 'static>,
        app: Option<AppHandle>,
        resp: Option<std::sync::mpsc::Sender<u32>>,
    },
    Stop,
}

static AUDIO_CMD_SENDER: OnceLock<mpsc::Sender<AudioCommand>> = OnceLock::new();

/// 🎙️ List all input devices
pub fn list_input_devices() -> Vec<String> {
    // Prefer PulseAudio host when available; it often avoids ALSA timestamp/device problems.
    let mut preferred_host = None;
    for id in cpal::available_hosts() {
        let name = format!("{:?}", id).to_lowercase();
        if name.contains("pulse") || name.contains("pulseaudio") {
            preferred_host = Some(id);
            break;
        }
    }

    let host = if let Some(id) = preferred_host {
        match cpal::host_from_id(id) {
            Ok(h) => { println!("🌐 Using host: {:?}", id); h }
            Err(_) => { println!("🌐 Fallback to default host"); cpal::default_host() }
        }
    } else {
        println!("🌐 Using default host");
        cpal::default_host()
    };
    host.input_devices()
        .map(|devices| {
            devices
                .filter_map(|d| d.name().ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// 🎙️ Start mic stream (safe fallback)
pub fn start_mic_stream_with_device<F>(
    device_name: String,
    app: AppHandle,
    on_data: F,
) -> Option<u32>
where
    F: Fn(Vec<i16>) + Send + Sync + 'static,
{
    // Ensure audio thread is running and get sender
    let sender = AUDIO_CMD_SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<AudioCommand>();

        std::thread::spawn(move || audio_thread_loop(rx));

        tx
    });

    let boxed: Arc<dyn Fn(Vec<i16>) + Send + Sync + 'static> = Arc::new(on_data);

    let (resp_tx, resp_rx) = std::sync::mpsc::channel::<u32>();

    let _ = sender.send(AudioCommand::Start {
        device_name: if device_name.trim().is_empty() {
            None
        } else {
            Some(device_name)
        },
        on_data: boxed,
        app: Some(app),
        resp: Some(resp_tx),
    });

    // wait briefly for the audio thread to report the selected sample rate
    use std::time::Duration;
    match resp_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(rate) => Some(rate),
        Err(_) => None,
    }
}

/// 🛑 Stop mic stream
pub fn stop_mic_stream() {
    if let Some(sender) = AUDIO_CMD_SENDER.get() {
        let _ = sender.send(AudioCommand::Stop);
    }
    println!("🛑 Mic stream stop requested");
}

fn build_stream_i16(
    device: &Device,
    config: &StreamConfig,
    on_data: Arc<dyn Fn(Vec<i16>) + Send + Sync + 'static>,
) -> Result<cpal::Stream, BuildStreamError> {
    let cb = on_data.clone();
    device.build_input_stream(
        config,
        move |data: &[i16], _| {
            let samples: Vec<i16> = data.iter().copied().collect();
            (cb)(samples);
        },
        |err| eprintln!("❌ Mic stream error: {}", err),
        None,
    )
}

fn build_stream_u16(
    device: &Device,
    config: &StreamConfig,
    on_data: Arc<dyn Fn(Vec<i16>) + Send + Sync + 'static>,
) -> Result<cpal::Stream, BuildStreamError> {
    let cb = on_data.clone();
    device.build_input_stream(
        config,
        move |data: &[u16], _| {
            let samples: Vec<i16> = data.iter().map(|s| (*s as i32 - 32768) as i16).collect();
            (cb)(samples);
        },
        |err| eprintln!("❌ Mic stream error: {}", err),
        None,
    )
}

fn build_stream_f32(
    device: &Device,
    config: &StreamConfig,
    on_data: Arc<dyn Fn(Vec<i16>) + Send + Sync + 'static>,
) -> Result<cpal::Stream, BuildStreamError> {
    let cb = on_data.clone();
    device.build_input_stream(
        config,
        move |data: &[f32], _| {
            let samples: Vec<i16> = data.iter().map(|s| (s * (i16::MAX as f32)) as i16).collect();
            (cb)(samples);
        },
        |err| eprintln!("❌ Mic stream error: {}", err),
        None,
    )
}

fn audio_thread_loop(rx: mpsc::Receiver<AudioCommand>) {
    let host = cpal::default_host();
    let mut _current_stream: Option<cpal::Stream> = None;

    for cmd in rx {
        match cmd {
            AudioCommand::Start { device_name, on_data, app, resp } => {
                let device = if let Some(name) = device_name {
                    host.input_devices()
                        .ok()
                        .and_then(|mut d| d.find(|dev| dev.name().map(|n| n == name).unwrap_or(false)))
                        .or_else(|| {
                            println!("⚠️ Selected mic not found, using default");
                            host.default_input_device()
                        })
                } else {
                    host.default_input_device()
                };

                if let Some(device) = device {
                    println!("🎤 Using input device: {}", device.name().unwrap_or("Unknown".into()));
                    // Use the device default input config (safer across ALSA devices).
                    let config = match device.default_input_config() {
                        Ok(c) => c,
                        Err(e) => { eprintln!("❌ Failed to get default input config: {}", e); continue; }
                    };

                    let mut stream_config: StreamConfig = config.clone().into();
                    // Force mono to avoid dmix/dsnoop channel mapping issues on some ALSA setups
                    stream_config.channels = 1;

                    // Before building/playing the stream, report the chosen sample rate back to caller (if requested)
                    if let Some(tx) = resp {
                        let _ = tx.send(stream_config.sample_rate.0);
                    }

                    // Debug: list a few supported configs for this device
                    if let Ok(mut sup) = device.supported_input_configs() {
                        println!("🔍 Supported configs (first few):");
                        for (i, c) in sup.by_ref().take(3).enumerate() {
                            println!("  {}: fmt={:?} min={:?} max={:?}", i, c.sample_format(), c.min_sample_rate(), c.max_sample_rate());
                        }
                    }

                    // Wrap the provided `on_data` so we can also emit audio level events
                    let maybe_app = app.clone();
                    let orig_cb = on_data.clone();
                    let wrapper = move |samples: Vec<i16>| {
                        // compute RMS level
                        if let Some(ref a) = maybe_app {
                            if !samples.is_empty() {
                                let sum_sq: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
                                let mean = sum_sq / (samples.len() as f64);
                                let rms = mean.sqrt();
                                let mut normalized = (rms / (i16::MAX as f64)) as f32;
                                if normalized.is_nan() { normalized = 0.0 }
                                if normalized < 0.0 { normalized = 0.0 }
                                if normalized > 1.0 { normalized = 1.0 }
                                let _ = a.emit("audio_level", normalized);
                            } else {
                                let _ = a.emit("audio_level", 0.0f32);
                            }
                        }

                        (orig_cb)(samples);
                    };

                    let wrapper_arc: Arc<dyn Fn(Vec<i16>) + Send + Sync + 'static> = Arc::new(wrapper);

                    // Use the default config's sample format
                    let sample_format = config.sample_format();

                    // Debug: print chosen stream config and sample format
                    println!("🔧 StreamConfig: channels={} sample_rate={} sample_format={:?}", stream_config.channels, stream_config.sample_rate.0, sample_format);

                    // Try to build stream for the selected device
                    let build_result = match sample_format {
                        SampleFormat::I16 => build_stream_i16(&device, &stream_config, wrapper_arc.clone()),
                        SampleFormat::U16 => build_stream_u16(&device, &stream_config, wrapper_arc.clone()),
                        SampleFormat::F32 => build_stream_f32(&device, &stream_config, wrapper_arc.clone()),
                        _ => { eprintln!("Unsupported sample format"); continue; }
                    };

                    let mut stream_opt: Option<cpal::Stream> = None;

                    match build_result {
                        Ok(s) => stream_opt = Some(s),
                        Err(e) => {
                            eprintln!("⚠️ Failed to build stream on selected device: {}", e);
                            // Attempt fallback: iterate through all input devices and try to build
                            if let Ok(devices) = host.input_devices() {
                                for d in devices {
                                    if d.name().ok() == device.name().ok() { continue; }
                                    println!("🔁 Trying device: {}", d.name().unwrap_or("unknown".into()));
                                    if let Ok(def_cfg) = d.default_input_config() {
                                        let mut def_stream_config: StreamConfig = def_cfg.clone().into();
                                        def_stream_config.channels = 1; // try mono
                                        let def_sample_format = def_cfg.sample_format();
                                        let def_build = match def_sample_format {
                                            SampleFormat::I16 => build_stream_i16(&d, &def_stream_config, wrapper_arc.clone()),
                                            SampleFormat::U16 => build_stream_u16(&d, &def_stream_config, wrapper_arc.clone()),
                                            SampleFormat::F32 => build_stream_f32(&d, &def_stream_config, wrapper_arc.clone()),
                                            _ => Err(BuildStreamError::StreamConfigNotSupported),
                                        };
                                        match def_build {
                                            Ok(s2) => { stream_opt = Some(s2); break; },
                                            Err(e2) => eprintln!("  ❌ build failed: {}", e2),
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if let Some(s) = stream_opt {
                        if let Err(e) = s.play() {
                            eprintln!("❌ Failed to start mic stream: {}", e);
                        } else {
                            _current_stream = Some(s);
                        }
                    } else {
                        eprintln!("❌ Could not build a working input stream on selected or fallback devices");
                    }
                } else {
                    eprintln!("❌ No input device available on system");
                }
            }
            AudioCommand::Stop => {
                _current_stream = None;
            }
        }
    }
}
