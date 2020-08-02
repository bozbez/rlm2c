use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Sample, SampleFormat, Stream, StreamConfig};

use serde::{Deserialize, Serialize};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    volume: f32,
    frequency: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            volume: 0.5,
            frequency: 440.0,
        }
    }
}

pub struct ToneGenerator {
    /*
    device: Device,
    stream: Stream,
    */

    active: Arc<AtomicBool>,
}

impl ToneGenerator {
    pub fn new(config: Config) -> Result<Self, anyhow::Error> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .expect("no default audio device");

        let stream_config = device.default_output_config()?;

        match device.name() {
            Ok(name) => info!("using: {}", name),
            Err(error) => error!("unable to get audio device name: {}", error)
        }

        debug!("{:?}", stream_config);

        let active = Arc::new(AtomicBool::new(false));
        let active_clone = active.clone();

        let sample_rate = stream_config.sample_rate().0 as f32;
        let mut sample_clock = 0f32;

        let Config { volume, frequency } = config;
        info!("volume: {}, frequency: {}", volume, frequency);

        let next_sample = move || {
            sample_clock = (sample_clock + 1.0) % sample_rate;

            if !active_clone.load(Ordering::Relaxed) {
                return 0.0;
            }

            volume * (sample_clock * frequency * 2.0 * std::f32::consts::PI / sample_rate).sin()
        };

        let stream = match stream_config.sample_format() {
            SampleFormat::F32 => {
                Self::init_stream::<f32, _>(&device, &stream_config.into(), next_sample)?
            }

            SampleFormat::I16 => {
                Self::init_stream::<i16, _>(&device, &stream_config.into(), next_sample)?
            }

            SampleFormat::U16 => {
                Self::init_stream::<u16, _>(&device, &stream_config.into(), next_sample)?
            }
        };

        stream.play()?;

        Ok(ToneGenerator {
            /*
            device: device,
            stream: stream,
            */

            active: active,
        })
    }

    fn init_stream<T, F>(
        device: &Device,
        config: &StreamConfig,
        mut next_sample: F,
    ) -> Result<Stream, anyhow::Error>
    where
        T: Sample,
        F: FnMut() -> f32 + Send + 'static,
    {
        let channels = config.channels as usize;

        let err_fn = |err| error!("error in audio stream: {}", err);
        let write_data = move |output: &mut [T], _: &cpal::OutputCallbackInfo| {
            for frame in output.chunks_mut(channels) {
                let value: T = cpal::Sample::from::<f32>(&next_sample());

                for sample in frame.iter_mut() {
                    *sample = value;
                }
            }
        };

        Ok(device.build_output_stream(config, write_data, err_fn)?)
    }

    pub fn enable(&mut self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
    }
}
