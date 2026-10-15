//! Streaming 2:1 audio decimation for accelerated GBA playback.
//! Filtering before dropping samples avoids folding high frequencies into the
//! audible range. Stereo channels and history persist across libretro batches.
use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

// Packed stereo slots remain atomic even when an overrun discards old frames
// while the reader is copying one. No lock, allocation or filtering in PipeWire.
const CAPACITY: usize = 8192;
pub struct GameAudio {
    slots: Box<[AtomicU64]>,
    read: AtomicUsize,
    write: AtomicUsize,
    limit: AtomicUsize,
    epoch: AtomicU64,
}
impl Default for GameAudio {
    fn default() -> Self {
        Self {
            slots: (0..CAPACITY).map(|_| AtomicU64::new(0)).collect(),
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
            limit: AtomicUsize::new(32768 / 20),
            epoch: AtomicU64::new(0),
        }
    }
}
impl GameAudio {
    pub fn configure(&self, rate: u32) {
        self.limit
            .store((rate as usize / 20).clamp(1, CAPACITY), Ordering::Release);
    }
    pub fn len(&self) -> usize {
        // Read head before tail: only the worker advances tail; clearing and
        // overrun eviction also run on that same worker.
        let read = self.read.load(Ordering::Acquire);
        self.write
            .load(Ordering::Acquire)
            .wrapping_sub(read)
            .min(CAPACITY)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn push(&self, stereo: [f32; 2]) {
        // Single producer. Eviction uses the same CAS as consumption so a
        // reader can never return a slot overwritten underneath it.
        let write = self.write.load(Ordering::Relaxed);
        let limit = self.limit.load(Ordering::Acquire);
        loop {
            let read = self.read.load(Ordering::Acquire);
            if write.wrapping_sub(read) < limit {
                break;
            }
            let _ = self.read.compare_exchange(
                read,
                read.wrapping_add(1),
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        let packed = u64::from(stereo[0].to_bits()) | (u64::from(stereo[1].to_bits()) << 32);
        self.slots[write % CAPACITY].store(packed, Ordering::Relaxed);
        self.write.store(write.wrapping_add(1), Ordering::Release);
    }
    pub fn pop(&self) -> Option<[f32; 2]> {
        // Bounded retries: an extreme producer overrun cannot spin the RT
        // callback indefinitely. A failed CAS means the old frame was evicted.
        for _ in 0..2 {
            let read = self.read.load(Ordering::Acquire);
            if read == self.write.load(Ordering::Acquire) {
                return None;
            }
            let packed = self.slots[read % CAPACITY].load(Ordering::Relaxed);
            if self
                .read
                .compare_exchange(
                    read,
                    read.wrapping_add(1),
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return Some([
                    f32::from_bits(packed as u32),
                    f32::from_bits((packed >> 32) as u32),
                ]);
            }
        }
        None
    }
    pub fn clear(&self) {
        self.read
            .store(self.write.load(Ordering::Acquire), Ordering::Release);
        self.epoch.fetch_add(1, Ordering::Release);
    }
    #[cfg(test)]
    pub fn drain(&self) -> Vec<f32> {
        std::iter::from_fn(|| self.pop()).flatten().collect()
    }
}

pub struct AudioReader {
    audio: Arc<GameAudio>,
    epoch: u64,
    target: usize,
    priming: bool,
    fade: usize,
    last: [f32; 2],
}
impl AudioReader {
    pub fn new(audio: Arc<GameAudio>, rate: u32) -> Self {
        let epoch = audio.epoch.load(Ordering::Acquire);
        Self {
            audio,
            epoch,
            target: (rate as usize / 25).max(1),
            priming: true,
            fade: 0,
            last: [0.0; 2],
        }
    }
    pub fn render(&mut self, output: &mut [f32]) {
        output.fill(0.0);
        let epoch = self.audio.epoch.load(Ordering::Acquire);
        if self.epoch != epoch {
            self.epoch = epoch;
            self.priming = true;
            self.fade = 0;
            self.last = [0.0; 2];
        }
        if self.priming {
            if self.audio.len() < self.target {
                for stereo in output.as_chunks_mut::<2>().0 {
                    self.last = self.last.map(|sample| sample * 0.875);
                    stereo.copy_from_slice(&self.last);
                }
                return;
            }
            self.priming = false;
        }
        for stereo in output.as_chunks_mut::<2>().0 {
            if let Some(frame) = if self.priming { None } else { self.audio.pop() } {
                self.fade = (self.fade + 1).min(64);
                for channel in 0..2 {
                    stereo[channel] = frame[channel] * self.fade as f32 / 64.0;
                }
                self.last.copy_from_slice(stereo);
            } else if !self.priming && !self.audio.is_empty() {
                // A producer eviction can win both bounded CAS attempts.
                // Bridge that one stereo frame instead of treating a full
                // queue as starvation and unnecessarily rebuilding headroom.
                stereo.copy_from_slice(&self.last);
            } else {
                // Fade to silence on a real starvation, then rebuild headroom
                // rather than repeatedly alternating a frame with silence.
                self.priming = true;
                self.fade = 0;
                self.last = self.last.map(|sample| sample * 0.875);
                stereo.copy_from_slice(&self.last);
            }
        }
    }
}

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

#[cfg(test)]
mod queue_tests {
    use super::*;
    #[test]
    fn queue_is_bounded_stereo_aligned_and_reset_discards_old_sound() {
        let audio = Arc::new(GameAudio::default());
        audio.configure(8000);
        // Exercise index wrap as well as overrun trimming.
        audio.read.store(usize::MAX - 9, Ordering::Relaxed);
        audio.write.store(usize::MAX - 9, Ordering::Relaxed);
        for i in 0..1000 {
            audio.push([i as f32, -(i as f32)]);
        }
        assert_eq!(audio.len(), 400);
        for i in 600..1000 {
            assert_eq!(audio.pop(), Some([i as f32, -(i as f32)]));
        }
        assert!(audio.is_empty());
        audio.configure(96000);
        for i in 0..1000 {
            audio.push([i as f32, -(i as f32)]);
        }
        audio.configure(8000);
        audio.push([1000.0, -1000.0]);
        assert_eq!(audio.len(), 400);
        assert_eq!(audio.pop(), Some([601.0, -601.0]));
        audio.clear();
        let mut reader = AudioReader::new(audio.clone(), 8000);
        for _ in 0..400 {
            audio.push([0.5, -0.25]);
        }
        let mut output = [0.0; 128];
        reader.render(&mut output);
        assert!(
            output
                .as_chunks::<2>()
                .0
                .iter()
                .all(|f| f[0] > 0.0 && f[1] == -f[0] / 2.0)
        );
        audio.clear();
        for _ in 0..100 {
            audio.push([-0.5, 0.25]);
        }
        reader.render(&mut output);
        assert!(output.iter().all(|v| *v == 0.0)); // Rebuild headroom.
        for _ in 0..220 {
            audio.push([-0.5, 0.25]);
        }
        reader.render(&mut output);
        assert!(
            output
                .as_chunks::<2>()
                .0
                .iter()
                .all(|f| f[0] < 0.0 && f[1] == -f[0] / 2.0)
        );
    }
    #[test]
    fn concurrent_overruns_never_tear_stereo_or_reorder_frames() {
        let audio = Arc::new(GameAudio::default());
        let producer_audio = audio.clone();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let producer_done = done.clone();
        let producer = std::thread::spawn(move || {
            for i in 1..100_000 {
                producer_audio.push([i as f32, -(i as f32)]);
            }
            producer_done.store(true, Ordering::Release);
        });
        let mut previous = 0.0;
        while !done.load(Ordering::Acquire) || !audio.is_empty() {
            if let Some([left, right]) = audio.pop() {
                assert_eq!(right, -left);
                assert!(left > previous);
                previous = left;
            }
        }
        producer.join().unwrap();
        assert!(previous > 0.0);
    }
    #[test]
    fn bounded_headroom_absorbs_brief_frame_jitter_at_both_speeds() {
        for speed in [1, 2] {
            let rate = 32768;
            let audio = Arc::new(GameAudio::default());
            audio.configure(rate);
            let mut reader = AudioReader::new(audio.clone(), rate);
            let mut core_due = 0.0;
            let mut output_due = 0.0;
            let interval = 1_000_000.0 / (59.7275 * speed as f64);
            let mut frame = 0;
            let mut heard = false;
            let mut output = [0.0; 512];
            for now in (0..2_000_000).step_by(100) {
                // Deliberately delay one core frame by 12 ms, then catch up.
                if !(1_000_000..1_012_000).contains(&now) {
                    while now as f64 >= core_due {
                        let next = (((frame + 1) as f64 * interval * rate as f64 / 1_000_000.0)
                            .floor()) as usize;
                        let previous = ((frame as f64 * interval * rate as f64 / 1_000_000.0)
                            .floor()) as usize;
                        for _ in previous..next {
                            audio.push([0.5, -0.25]);
                        }
                        core_due += interval;
                        frame += 1;
                    }
                }
                if now as f64 >= output_due {
                    reader.render(&mut output);
                    if heard {
                        assert!(
                            output
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .all(|f| f[0] > 0.0 && f[1] < 0.0),
                            "dropout at {now}, {speed}x"
                        );
                    }
                    heard |= output[0] > 0.0;
                    output_due += 256.0 * 1_000_000.0 / rate as f64;
                }
                assert!(audio.len() <= rate as usize / 20);
            }
            assert!(heard);
        }
    }
}
