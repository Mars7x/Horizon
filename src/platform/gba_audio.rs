//! Streaming 2:1 audio decimation for accelerated GBA playback.
//! Filtering before dropping samples avoids folding high frequencies into the
//! audible range. Stereo channels and history persist across libretro batches.
const TAPS: usize = 63;

pub(super) struct SpeedAudio {
    fast: bool,
    phase: bool,
    position: usize,
    history: [[f32; 2]; TAPS],
    coefficients: [f32; TAPS],
}

impl Default for SpeedAudio {
    fn default() -> Self {
        let mut coefficients = [0.0; TAPS];
        // Leave a transition band below the decimated Nyquist frequency (0.25).
        let cutoff = 0.22;
        for (index, coefficient) in coefficients.iter_mut().enumerate() {
            let offset = index as f64 - (TAPS / 2) as f64;
            let sinc = if offset == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * std::f64::consts::PI * cutoff * offset).sin()
                    / (std::f64::consts::PI * offset)
            };
            let window =
                0.54 - 0.46 * (2.0 * std::f64::consts::PI * index as f64 / (TAPS - 1) as f64).cos();
            *coefficient = (sinc * window) as f32;
        }
        let gain: f32 = coefficients.iter().sum();
        for coefficient in &mut coefficients {
            *coefficient /= gain;
        }
        Self {
            fast: false,
            phase: false,
            position: 0,
            history: [[0.0; 2]; TAPS],
            coefficients,
        }
    }
}

impl SpeedAudio {
    pub fn set_fast_forward(&mut self, enabled: bool) {
        self.fast = enabled;
        self.phase = false;
        self.position = 0;
        self.history.fill([0.0; 2]);
    }

    pub fn process(&mut self, stereo: [f32; 2]) -> Option<[f32; 2]> {
        if !self.fast {
            return Some(stereo);
        }
        self.history[self.position] = stereo;
        self.position = (self.position + 1) % TAPS;
        self.phase = !self.phase;
        if self.phase {
            return None;
        }
        let mut output = [0.0; 2];
        let mut cursor = self.position;
        for coefficient in self.coefficients {
            cursor = if cursor == 0 { TAPS - 1 } else { cursor - 1 };
            for (channel, sample) in output.iter_mut().enumerate() {
                *sample += self.history[cursor][channel] * coefficient;
            }
        }
        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_speed_is_exact_and_speed_changes_discard_filter_history() {
        let mut audio = SpeedAudio::default();
        assert_eq!(audio.process([0.25, -0.5]), Some([0.25, -0.5]));
        audio.set_fast_forward(true);
        for _ in 0..128 {
            audio.process([1.0, -1.0]);
        }
        audio.set_fast_forward(false);
        assert_eq!(audio.process([0.0, 0.0]), Some([0.0, 0.0]));
        audio.set_fast_forward(true);
        assert_eq!(audio.process([0.0, 0.0]), None);
        assert_eq!(audio.process([0.0, 0.0]), Some([0.0, 0.0]));
    }

    #[test]
    fn accelerated_audio_halves_sample_count_preserves_stereo_and_rejects_aliasing() {
        let mut audio = SpeedAudio::default();
        audio.set_fast_forward(true);
        let mut output = Vec::new();
        // Independent low and high tones in the two channels.
        for index in 0..4096 {
            let time = index as f32 * 2.0 * std::f32::consts::PI;
            if let Some(sample) = audio.process([(time * 0.04).sin(), (time * 0.35).sin()]) {
                output.push(sample);
            }
        }
        assert_eq!(output.len(), 2048);
        let stable = &output[128..];
        let rms = |channel: usize| {
            (stable
                .iter()
                .map(|sample| sample[channel].powi(2))
                .sum::<f32>()
                / stable.len() as f32)
                .sqrt()
        };
        assert!((0.69..0.72).contains(&rms(0)));
        assert!(rms(1) < 0.002);
        // Low input tone advances twice as far per output sample: higher pitch.
        let crossings = stable
            .windows(2)
            .filter(|pair| pair[0][0] < 0.0 && pair[1][0] >= 0.0)
            .count();
        assert!((crossings as f32 - stable.len() as f32 * 0.08).abs() < 2.0);
    }
}
