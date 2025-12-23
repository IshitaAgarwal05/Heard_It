use cpal::{traits::{DeviceTrait, HostTrait, StreamTrait}, SampleFormat, StreamConfig};
use std::io::{self, Write};
use std::sync::mpsc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut device_name: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--device" => {
                if i + 1 < args.len() {
                    device_name = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let host = cpal::default_host();
    let device = if let Some(name) = device_name {
        host.input_devices()
            .ok()
            .and_then(|mut iter| iter.find(|d| d.name().map(|n| n == name).unwrap_or(false)))
            .or_else(|| host.default_input_device())
    } else {
        host.default_input_device()
    };

    let device = match device {
        Some(d) => d,
        None => {
            eprintln!("No input device available");
            std::process::exit(1);
        }
    };

    let config = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to get default input config: {}", e);
            std::process::exit(1);
        }
    };

    let stream_config: StreamConfig = config.clone().into();
    let sample_rate = stream_config.sample_rate.0;
    let channels = stream_config.channels as usize;

    // channel between audio callback and writer
    let (tx, rx) = mpsc::channel::<Vec<i16>>();

    // build stream according to sample format
    let stream = match config.sample_format() {
        SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| {
                // data may be interleaved if channels > 1; convert to mono by
                // taking the first channel sample from each frame.
                if channels == 1 {
                    let v = data.iter().copied().collect::<Vec<i16>>();
                    let _ = tx.send(v);
                } else {
                    let mut v = Vec::with_capacity(data.len() / channels);
                    for frame_idx in 0..(data.len() / channels) {
                        let sample = data[frame_idx * channels];
                        v.push(sample);
                    }
                    let _ = tx.send(v);
                }
            },
            |e| eprintln!("Audio worker stream error: {}", e),
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _| {
                if channels == 1 {
                    let v = data.iter().map(|s| (*s as i32 - 32768) as i16).collect::<Vec<i16>>();
                    let _ = tx.send(v);
                } else {
                    let mut v = Vec::with_capacity(data.len() / channels);
                    for frame_idx in 0..(data.len() / channels) {
                        let sample_u = data[frame_idx * channels];
                        let sample = (sample_u as i32 - 32768) as i16;
                        v.push(sample);
                    }
                    let _ = tx.send(v);
                }
            },
            |e| eprintln!("Audio worker stream error: {}", e),
            None,
        ),
        SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| {
                if channels == 1 {
                    let v = data.iter().map(|s| (s * (i16::MAX as f32)) as i16).collect::<Vec<i16>>();
                    let _ = tx.send(v);
                } else {
                    let mut v = Vec::with_capacity(data.len() / channels);
                    for frame_idx in 0..(data.len() / channels) {
                        let sample_f = data[frame_idx * channels];
                        let sample = (sample_f * (i16::MAX as f32)) as i16;
                        v.push(sample);
                    }
                    let _ = tx.send(v);
                }
            },
            |e| eprintln!("Audio worker stream error: {}", e),
            None,
        ),
        // `SampleFormat` is non-exhaustive; accept any future/unknown formats by
        // attempting to interpret them as f32 (safe fallback) to keep the worker
        // functional on newer cpal versions.
        _ => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| {
                if channels == 1 {
                    let v = data.iter().map(|s| (s * (i16::MAX as f32)) as i16).collect::<Vec<i16>>();
                    let _ = tx.send(v);
                } else {
                    let mut v = Vec::with_capacity(data.len() / channels);
                    for frame_idx in 0..(data.len() / channels) {
                        let sample_f = data[frame_idx * channels];
                        let sample = (sample_f * (i16::MAX as f32)) as i16;
                        v.push(sample);
                    }
                    let _ = tx.send(v);
                }
            },
            |e| eprintln!("Audio worker stream error: {}", e),
            None,
        ),
    };

    let stream = match stream {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to build input stream: {}", e);
            std::process::exit(2);
        }
    };

    if let Err(e) = stream.play() {
        eprintln!("Failed to start stream: {}", e);
        std::process::exit(3);
    }

    // Writer: write header with magic + sample_rate, then length-prefixed frames
    let mut out = io::stdout();
    // magic
    let _ = out.write_all(b"SRAT");
    let _ = out.write_all(&sample_rate.to_le_bytes());
    let _ = out.flush();

    for frame in rx {
        // write length (number of samples) as u32 LE
        let len = frame.len() as u32;
        let _ = out.write_all(&len.to_le_bytes());
        // write samples as i16 little-endian
        let mut buf: Vec<u8> = Vec::with_capacity((len as usize) * 2);
        for s in frame {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        let _ = out.write_all(&buf);
        let _ = out.flush();
    }

    // Keep process alive while stream is active
    loop {
        std::thread::park();
    }
}
