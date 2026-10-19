//! Explicit, bounded microphone checks. No files or OS volume changes.
use crate::MicrophoneMode;
use redunar_capture_audio::{
    AUDIO_CHANNELS, AUDIO_FRAME_SAMPLES_PER_CHANNEL, AUDIO_SAMPLE_RATE, MicrophoneAudioCapture,
    MicrophoneLevels, OpusDecoder,
};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const MAX_FRAMES: usize = 300; // Six seconds at the capture library's 20 ms cadence.
const PCM_BYTES: usize = MAX_FRAMES * AUDIO_FRAME_SAMPLES_PER_CHANNEL * AUDIO_CHANNELS as usize * 2;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct MicrophoneTestStatus {
    pub id: u64,
    pub phase: &'static str,
    pub seconds_recorded: u16,
    pub levels: Option<MicrophoneLevels>,
    pub message: Option<&'static str>,
}

struct ResultState {
    phase: &'static str,
    frames: usize,
    levels: Option<(Instant, MicrophoneLevels)>,
    wav: Option<Vec<u8>>,
    message: Option<&'static str>,
    finished: Option<Instant>,
}

pub(crate) struct MicrophoneTest {
    id: u64,
    stop: Arc<AtomicBool>,
    result: Arc<Mutex<ResultState>>,
    worker: Option<JoinHandle<()>>,
}

impl MicrophoneTest {
    #[cfg(test)]
    pub(crate) fn fixture(id: u64) -> Self {
        Self {
            id,
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
            result: Arc::new(Mutex::new(ResultState {
                phase: "ready",
                frames: MAX_FRAMES,
                levels: None,
                wav: Some(wav(vec![0; 16])),
                message: None,
                finished: Some(Instant::now()),
            })),
        }
    }

    pub(crate) fn start(mode: MicrophoneMode, gain_db: i16) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(ResultState {
            phase: "starting",
            frames: 0,
            levels: None,
            wav: None,
            message: None,
            finished: None,
        }));
        let worker_stop = stop.clone();
        let worker_result = result.clone();
        let worker = thread::Builder::new()
            .name("redunar-microphone-test".into())
            .spawn(move || {
                let outcome = record(&mode, gain_db, &worker_stop, &worker_result);
                let mut result = lock(&worker_result);
                result.finished = Some(Instant::now());
                result.levels = None;
                if worker_stop.load(Ordering::Acquire) {
                    result.phase = "cancelled";
                    return;
                }
                match outcome {
                    Ok(wav) => {
                        result.wav = Some(wav);
                        result.phase = "ready";
                    }
                    Err(message) => {
                        result.message = Some(message);
                        result.phase = "failed";
                    }
                }
            })
            .map_err(|_| "Could not start the microphone test".to_owned())?;
        Ok(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            stop,
            result,
            worker: Some(worker),
        })
    }

    pub(crate) fn status(&self) -> MicrophoneTestStatus {
        let mut result = lock(&self.result);
        expire(&mut result);
        MicrophoneTestStatus {
            id: self.id,
            phase: result.phase,
            seconds_recorded: u16::try_from(result.frames / 50).unwrap_or(6),
            levels: result
                .levels
                .filter(|(time, _)| time.elapsed() < Duration::from_millis(500))
                .map(|(_, levels)| levels),
            message: result.message,
        }
    }

    pub(crate) const fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn take_audio(&self) -> Result<Vec<u8>, String> {
        let mut result = lock(&self.result);
        expire(&mut result);
        result
            .wav
            .take()
            .ok_or_else(|| "No microphone test recording is available".to_owned())
    }
}

fn expire(result: &mut ResultState) {
    if result
        .finished
        .is_some_and(|time| time.elapsed() > Duration::from_mins(1))
    {
        result.wav = None;
        result.phase = "expired";
    }
}

impl Drop for MicrophoneTest {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn record(
    mode: &MicrophoneMode,
    gain_db: i16,
    stop: &AtomicBool,
    result: &Mutex<ResultState>,
) -> Result<Vec<u8>, &'static str> {
    let selected = match mode {
        MicrophoneMode::Off => return Err("Enable microphone recording before testing"),
        MicrophoneMode::DefaultInput => None,
        MicrophoneMode::SelectedInput(name) => Some(name.as_str()),
    };
    let sources = redunar_capture_audio::discover_microphone_audio_sources_for(selected);
    if stop.load(Ordering::Acquire) {
        return Err("Microphone test cancelled");
    }
    let started = Instant::now();
    let timestamp =
        redunar_capture_audio::monotonic_now_ns().ok_or("Microphone clock is unavailable")?;
    let mut capture = sources
        .iter()
        .find_map(|source| {
            MicrophoneAudioCapture::start_with_gain(source, timestamp, started, gain_db).ok()
        })
        .ok_or("The selected microphone is unavailable")?;
    let mut decoder = OpusDecoder::open().map_err(|_| "Microphone test playback is unavailable")?;
    let mut pcm = Vec::with_capacity(PCM_BYTES);
    for index in 0..MAX_FRAMES {
        let packet = loop {
            if stop.load(Ordering::Acquire) {
                return Err("Microphone test cancelled");
            }
            if started.elapsed() > Duration::from_secs(8) {
                return Err("The microphone stopped delivering audio");
            }
            if let Some(packet) = capture
                .next_packet(Duration::from_millis(100))
                .map_err(|_| "Microphone capture failed. Check the input and try again.")?
            {
                break packet;
            }
        };
        let samples = decoder
            .decode_frame(&packet)
            .map_err(|_| "Microphone test playback could not be decoded")?;
        if samples.len() != AUDIO_FRAME_SAMPLES_PER_CHANNEL * AUDIO_CHANNELS as usize
            || pcm.len() + samples.len() * 2 > PCM_BYTES
        {
            return Err("Microphone test exceeded its audio bound");
        }
        for sample in samples {
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
        let mut result = lock(result);
        result.phase = "recording";
        result.frames = index + 1;
        result.levels = Some((Instant::now(), capture.levels()));
    }
    // Drop kills and reaps the helper before the result can be played.
    drop(capture);
    Ok(wav(pcm))
}

fn wav(pcm: Vec<u8>) -> Vec<u8> {
    let length = u32::try_from(pcm.len()).expect("six-second PCM bound");
    let mut bytes = Vec::with_capacity(pcm.len() + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(length + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&u16::from(AUDIO_CHANNELS).to_le_bytes());
    bytes.extend_from_slice(&AUDIO_SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(AUDIO_SAMPLE_RATE * u32::from(AUDIO_CHANNELS) * 2).to_le_bytes());
    bytes.extend_from_slice(&(u16::from(AUDIO_CHANNELS) * 2).to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend(pcm);
    bytes
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn playback_is_bounded_wav_and_can_only_be_taken_once() {
        let audio = wav(vec![0; 3840]);
        assert_eq!(&audio[..4], b"RIFF");
        assert_eq!(&audio[36..40], b"data");
        assert_eq!(audio.len(), 3884);
        let test = MicrophoneTest {
            id: 1,
            stop: Arc::new(AtomicBool::new(false)),
            result: Arc::new(Mutex::new(ResultState {
                phase: "ready",
                frames: 1,
                levels: None,
                wav: Some(audio),
                message: None,
                finished: Some(Instant::now()),
            })),
            worker: None,
        };
        assert_eq!(test.take_audio().unwrap().len(), 3884);
        assert!(test.take_audio().is_err());
    }
    #[test]
    fn expired_audio_is_not_returned() {
        let mut result = ResultState {
            phase: "ready",
            frames: 300,
            levels: None,
            wav: Some(vec![1]),
            message: None,
            finished: Some(Instant::now().checked_sub(Duration::from_secs(61)).unwrap()),
        };
        expire(&mut result);
        assert_eq!(result.phase, "expired");
        assert!(result.wav.is_none());
    }
    #[test]
    fn drop_cancels_and_joins_owned_worker() {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let reaped = Arc::new(AtomicBool::new(false));
        let worker_reaped = reaped.clone();
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                thread::yield_now();
            }
            worker_reaped.store(true, Ordering::Release);
        });
        let test = MicrophoneTest {
            id: 1,
            stop,
            worker: Some(worker),
            result: Arc::new(Mutex::new(ResultState {
                phase: "starting",
                frames: 0,
                levels: None,
                wav: None,
                message: None,
                finished: None,
            })),
        };
        drop(test);
        assert!(reaped.load(Ordering::Acquire));
    }
}
