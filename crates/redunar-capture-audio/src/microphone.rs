//! Recording-only gain and stereo-linked peak protection. No device volume is changed.

/// Microphone recording gain limits, in decibels.
pub const MIN_MICROPHONE_GAIN_DB: i16 = -12;
pub const MAX_MICROPHONE_GAIN_DB: i16 = 24;
const CEILING: f64 = 29_200.0; // About -1 dBFS, leaving codec headroom.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MicrophoneLevels {
    pub peak: f32,
    pub rms: f32,
    pub limiting: bool,
}

pub struct MicrophoneProcessor {
    gain: f64,
    attenuation: f64,
    levels: MicrophoneLevels,
}

impl MicrophoneProcessor {
    /// Construct a processor. Callers validate persisted/user values separately.
    #[must_use]
    pub fn new(gain_db: i16) -> Self {
        Self {
            gain: 10_f64.powf(
                f64::from(gain_db.clamp(MIN_MICROPHONE_GAIN_DB, MAX_MICROPHONE_GAIN_DB)) / 20.0,
            ),
            attenuation: 1.0,
            levels: MicrophoneLevels::default(),
        }
    }

    /// Process one existing 20 ms frame without adding buffering or latency.
    /// A shared envelope preserves stereo balance. Attack catches the frame's
    /// peak before encoding; recovery takes about 200 ms to avoid pumping.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub fn process(&mut self, samples: &mut [i16]) {
        let peak = samples
            .iter()
            .map(|value| f64::from(*value).abs())
            .fold(0.0, f64::max)
            * self.gain;
        let required = if peak > CEILING { CEILING / peak } else { 1.0 };
        self.attenuation = required.min(self.attenuation + (1.0 - self.attenuation) * 0.1);
        let gain = self.gain * self.attenuation;
        let mut peak = 0.0_f64;
        let mut energy = 0.0;
        for sample in samples.iter_mut() {
            let value = (f64::from(*sample) * gain).round().clamp(-CEILING, CEILING);
            *sample = value as i16;
            peak = peak.max(value.abs());
            energy += value * value;
        }
        self.levels = MicrophoneLevels {
            peak: (peak / 32_768.0) as f32,
            rms: ((energy / samples.len().max(1) as f64).sqrt() / 32_768.0) as f32,
            limiting: self.attenuation < 0.999,
        };
    }

    #[must_use]
    pub const fn levels(&self) -> MicrophoneLevels {
        self.levels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unity_preserves_quiet_pcm_and_silence_has_no_level() {
        let mut processor = MicrophoneProcessor::new(0);
        let mut samples = [-1200, 600, 0, 2000];
        processor.process(&mut samples);
        assert_eq!(samples, [-1200, 600, 0, 2000]);
        assert!(!processor.levels().limiting);
        processor.process(&mut [0; 8]);
        assert_eq!(processor.levels(), MicrophoneLevels::default());
    }

    #[test]
    fn gain_raises_quiet_voice_and_linked_limiter_prevents_overflow() {
        let mut processor = MicrophoneProcessor::new(12);
        let mut quiet = [1000, -500];
        processor.process(&mut quiet);
        assert!((3980..3983).contains(&quiet[0]));
        let mut loud = [25_000, -12_500, i16::MIN, i16::MAX];
        processor.process(&mut loud);
        assert!(loud.iter().all(|sample| i32::from(*sample).abs() <= 29_200));
        assert!((i32::from(loud[0]) + 2 * i32::from(loud[1])).abs() <= 1);
        assert!(processor.levels().limiting);
        assert!(processor.levels().peak <= 0.892);
        let mut recovery = [1000; 8];
        processor.process(&mut recovery);
        assert!(recovery[0] < quiet[0]);
    }
}
