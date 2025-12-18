use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub fn start_mic_stream<F>(mut callback: F)
where
    F: FnMut(&[i16]) + Send + 'static,
{
    let host = cpal::default_host();

    let device = match host.default_input_device() {
        Some(d) => d,
        None => {
            eprintln!("❌ No input audio device found");
            return;
        }
    };

    let config = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("❌ Failed to get input config: {:?}", e);
            return;
        }
    };

    let _sample_rate = config.sample_rate().0;
    let _channels = config.channels() as usize;

    let stream = match config.sample_format() {
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _| {
                callback(data);
            },
            |err| eprintln!("Stream error: {:?}", err),
            None,
        ),

        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _| {
                // Convert f32 [-1.0, 1.0] → i16
                let converted: Vec<i16> = data
                    .iter()
                    .map(|s| (s * i16::MAX as f32) as i16)
                    .collect();
                callback(&converted);
            },
            |err| eprintln!("Stream error: {:?}", err),
            None,
        ),

        _ => {
            eprintln!("❌ Unsupported sample format");
            return;
        }
    };


    if let Err(e) = stream {
        eprintln!("❌ Failed to start input stream: {:?}", e);
        return;
    }

    let stream = stream.unwrap();
    stream.play().ok();
}
