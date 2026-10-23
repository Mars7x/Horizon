//! Streaming integer audio decimation for accelerated GBA playback.
//! Filtering before dropping samples avoids folding high frequencies into the
//! audible range. Stereo channels and history persist across libretro batches.
use crate::domain::core_settings::PlaybackSpeed;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

// Packed stereo slots remain atomic even when an overrun discards old frames
// while the reader is copying one. No lock, allocation or filtering in PipeWire.
const CAPACITY: usize = 8192;
pub struct GameAudio {
    slots: Box<[AtomicU64]>,
    read: AtomicUsize,
    write: AtomicUsize,
    limit: AtomicUsize,
    buffer_ms: u16,
    epoch: AtomicU64,
    paused: AtomicBool,
    /// Times the output ran dry and had to rebuild headroom (an audible gap).
    gaps: AtomicU64,
    /// Stereo frames evicted unheard because the queue was full.
    dropped: AtomicU64,
}
impl Default for GameAudio {
    fn default() -> Self {
        Self::with_buffer_ms(50)
    }
}
impl GameAudio {
    /// Bounded headroom for synchronous core work; mGBA keeps its 50 ms queue.
    pub fn with_buffer_ms(buffer_ms: u16) -> Self {
        Self {
            slots: (0..CAPACITY).map(|_| AtomicU64::new(0)).collect(),
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
            limit: AtomicUsize::new(Self::frame_limit(32768, buffer_ms)),
            buffer_ms,
            epoch: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            gaps: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        }
    }
    fn frame_limit(rate: u32, buffer_ms: u16) -> usize {
        (rate as usize)
            .saturating_mul(usize::from(buffer_ms))
            .saturating_div(1000)
            .clamp(1, CAPACITY)
    }
    /// Silence the live output without consuming the frozen game's timeline.
    /// Keeping its bounded headroom avoids re-priming on an ordinary resume.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }
    pub fn configure(&self, rate: u32) {
        self.limit
            .store(Self::frame_limit(rate, self.buffer_ms), Ordering::Release);
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
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
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
    /// Audible problems so far: gaps where the output ran dry, and stereo
    /// frames dropped at the limit. Deliberate clears are not counted.
    pub fn problems(&self) -> (u64, u64) {
        (
            self.gaps.load(Ordering::Relaxed),
            self.dropped.load(Ordering::Relaxed),
        )
    }
    /// The most stereo frames the queue holds before evicting the oldest.
    pub fn limit(&self) -> usize {
        self.limit.load(Ordering::Acquire)
    }
    /// Stereo frames pushed since the queue was created (wrapping).
    pub fn written(&self) -> usize {
        self.write.load(Ordering::Acquire)
    }
    #[cfg(test)]
    pub fn drain(&self) -> Vec<f32> {
        std::iter::from_fn(|| self.pop()).flatten().collect()
    }
}

/// The reader's target fill, as a share of the queue limit.
const SETPOINT: f64 = 0.62;

/// Paces the core by the audio it produces rather than by frame count.
///
/// GBA frames are all one length, but a Game Boy frame is shorter or longer
/// whenever the game switches its screen off or on (games do this while
/// loading), and mGBA hands Game Boy audio over in 512-frame blocks. Pacing by
/// frames then makes the game run up to ~9% fast in audio terms, overfilling
/// the queue. Each frame instead waits as long as the audio it pushed lasts,
/// averaged over about 32 frames so the 512-frame blocks do not jitter the
/// picture. Pushed audio is already at the output rate after speed
/// decimation, so this holds at every playback speed.
pub struct FramePacer {
    rate: f64,
    per_frame: f64,
    /// Selected emulation pace, independent of audio batch boundaries.
    frame_interval: std::time::Duration,
    /// The queue's fill as a share of its limit, averaged over about 16
    /// frames. Above the reader's setpoint frames slow a little (at most 5%),
    /// below it they quicken, so a change of pace (a game starting to toggle
    /// its screen) cannot overfill the queue while the average catches up.
    fill: f64,
}
impl FramePacer {
    pub fn new(rate: u32, fps: f64, speed: u8) -> Self {
        let mut pacer = Self {
            rate: f64::from(rate.max(1)),
            per_frame: 0.0,
            frame_interval: std::time::Duration::ZERO,
            fill: SETPOINT,
        };
        pacer.reset(fps, speed);
        pacer
    }
    /// A new speed or a discontinuity: start again from the nominal frame.
    pub fn reset(&mut self, fps: f64, speed: u8) {
        let seconds = 1.0 / fps.max(1.0) / f64::from(speed.max(1));
        self.frame_interval = std::time::Duration::from_secs_f64(seconds);
        self.per_frame = self.rate * seconds;
        self.fill = SETPOINT;
    }
    /// DS fast-forward keeps the selected emulation target when audio
    /// arrives in larger batches. Otherwise those batches can feed back into
    /// pacing and turn a 2× request back into 1×.
    pub fn fast_forward_interval(
        &mut self,
        pushed: usize,
        queued: usize,
        limit: usize,
    ) -> std::time::Duration {
        self.interval(pushed, queued, limit)
            .min(self.frame_interval)
    }

    /// How long to wait after a frame that pushed `pushed` stereo frames,
    /// with the queue now holding `queued` of at most `limit`.
    pub fn interval(&mut self, pushed: usize, queued: usize, limit: usize) -> std::time::Duration {
        self.per_frame += (pushed as f64 - self.per_frame) / 32.0;
        self.fill += (queued as f64 / limit.max(1) as f64 - self.fill) / 16.0;
        let correction = ((self.fill - SETPOINT) * 0.5).clamp(-0.05, 0.05);
        std::time::Duration::from_secs_f64(self.per_frame.max(0.0) / self.rate * (1.0 + correction))
    }
}

pub struct AudioReader {
    audio: Arc<GameAudio>,
    epoch: u64,
    target: usize,
    priming: bool,
    fade: usize,
    last: [f32; 2],
    /// The queue's smoothed fill level, for following the device's clock.
    level: f32,
    /// Callbacks since the last clock correction.
    since_correction: u32,
}

/// Callbacks between clock corrections at least: up to 16 a second at 256
/// frames, which follows about 490 ppm of drift. Measured drift is about
/// 15 ppm (one correction every two seconds); crystals rarely exceed 200.
const CORRECTION_SPACING: u32 = 8;

impl AudioReader {
    pub fn new(audio: Arc<GameAudio>, rate: u32) -> Self {
        let epoch = audio.epoch.load(Ordering::Acquire);
        let target = (GameAudio::frame_limit(rate, audio.buffer_ms) * 62 / 100).max(1);
        Self {
            audio,
            epoch,
            // Start at the steady band's level (see `render`), so the next
            // frame's burst does not push the queue to its limit.
            target,
            priming: true,
            fade: 0,
            last: [0.0; 2],
            level: 0.0,
            since_correction: 0,
        }
    }
    pub fn render(&mut self, output: &mut [f32]) {
        output.fill(0.0);
        if self.audio.paused.load(Ordering::Acquire) {
            self.fade = 0;
            self.last = [0.0; 2];
            return;
        }
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
            // Nothing queued has been heard yet: trim what built up before
            // the first callback down to the start level, so playback starts
            // in the steady band instead of at the limit.
            while self.audio.len() > self.target {
                if self.audio.pop().is_none() {
                    break;
                }
            }
            self.level = self.audio.len() as f32;
        }
        // The emulator is paced by the system clock and the sound card by
        // its own, so the queue slowly fills or empties. Keep its average in
        // a steady band by skipping or repeating one sample now and then
        // (30 us): never a refill gap, never samples dropped at the limit.
        // The band sits at 62% of the limit because the queue swings around
        // its average: a frame's burst (a third of the limit) lands on top,
        // and callbacks take 256 frames at a time.
        let fill = self.audio.len();
        self.level += (fill as f32 - self.level) / 64.0;
        self.since_correction = self.since_correction.saturating_add(1);
        let limit = self.audio.limit.load(Ordering::Acquire) as f32;
        let (setpoint, band) = (limit * SETPOINT as f32, limit * 0.05);
        let mut repeat = false;
        if self.since_correction >= CORRECTION_SPACING {
            if self.level > setpoint + band && self.audio.pop().is_some() {
                self.level -= 1.0;
                self.since_correction = 0;
            } else if self.level < setpoint - band && fill > 0 {
                repeat = true;
                self.since_correction = 0;
            }
        }
        for stereo in output.as_chunks_mut::<2>().0 {
            if std::mem::take(&mut repeat) {
                stereo.copy_from_slice(&self.last);
                continue;
            }
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
                // The rest of this callback comes here too: count one gap.
                if !std::mem::replace(&mut self.priming, true) {
                    self.audio.gaps.fetch_add(1, Ordering::Relaxed);
                }
                self.fade = 0;
                self.last = self.last.map(|sample| sample * 0.875);
                stereo.copy_from_slice(&self.last);
            }
        }
    }
}

const TAPS: usize = 63;

/// Decimates at the selected speed, independent of CPU/GPU work estimates.
/// Retuning from individual frame costs changes pitch and feeds extra audio
/// back into pacing even when the emulator is already reaching its target.
pub(super) struct SpeedAudio {
    factor: usize,
    ratio: f64,
    phase: f64,
    position: usize,
    history: [[f32; 2]; TAPS],
    coefficients: [f32; TAPS],
}

impl Default for SpeedAudio {
    fn default() -> Self {
        Self {
            factor: 1,
            ratio: 1.0,
            phase: 0.0,
            position: 0,
            history: [[0.0; 2]; TAPS],
            coefficients: [0.0; TAPS],
        }
    }
}
impl SpeedAudio {
    pub fn set_speed(&mut self, speed: PlaybackSpeed) {
        self.factor = usize::from(speed.factor());
        self.phase = 0.0;
        self.position = 0;
        self.history.fill([0.0; 2]);
        self.design(self.factor as f64);
    }

    fn design(&mut self, ratio: f64) {
        if ratio == self.ratio && ratio != 1.0 {
            return;
        }
        self.ratio = ratio;
        let cutoff = 0.44 / ratio;
        for (index, coefficient) in self.coefficients.iter_mut().enumerate() {
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
        let gain: f32 = self.coefficients.iter().sum();
        for coefficient in &mut self.coefficients {
            *coefficient /= gain;
        }
    }

    pub fn process(&mut self, stereo: [f32; 2]) -> Option<[f32; 2]> {
        if self.factor == 1 {
            return Some(stereo);
        }
        self.history[self.position] = stereo;
        self.position = (self.position + 1) % TAPS;
        self.phase += 1.0;
        if self.phase < self.ratio {
            return None;
        }
        self.phase -= self.ratio;
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
        audio.set_speed(PlaybackSpeed::Double);
        for _ in 0..128 {
            audio.process([1.0, -1.0]);
        }
        audio.set_speed(PlaybackSpeed::Normal);
        assert_eq!(audio.process([0.0, 0.0]), Some([0.0, 0.0]));
        audio.set_speed(PlaybackSpeed::Double);
        assert_eq!(audio.process([0.0, 0.0]), None);
        assert_eq!(audio.process([0.0, 0.0]), Some([0.0, 0.0]));
    }

    #[test]
    fn double_speed_keeps_exact_output_rate_across_uneven_batches() {
        let mut audio = SpeedAudio::default();
        audio.set_speed(PlaybackSpeed::Double);
        let (mut input, mut output) = (0usize, 0usize);
        // Odd-sized and bursty batches keep the same 2:1 ratio. A long
        // frame or snapshot must not retune the filter or its output rate.
        for batch in [547, 548, 1095, 1, 513, 2048].into_iter().cycle().take(300) {
            for _ in 0..batch {
                output += usize::from(audio.process([0.5, -0.5]).is_some());
            }
            input += batch;
            assert_eq!(output, input / 2);
        }
        audio.set_speed(PlaybackSpeed::Normal);
        assert_eq!(audio.process([0.25, -0.5]), Some([0.25, -0.5]));
    }

    #[test]
    fn all_accelerated_speeds_preserve_stereo_pitch_and_reject_aliasing() {
        for speed in PlaybackSpeed::ALL.into_iter().skip(1) {
            let mut audio = SpeedAudio::default();
            audio.set_speed(speed);
            let mut output = Vec::new();
            for i in 0..12288 {
                let t = i as f32 * 2.0 * std::f32::consts::PI;
                if let Some(frame) = audio.process([(t * 0.02).sin(), (t * 0.35).sin()]) {
                    output.push(frame);
                }
            }
            assert_eq!(output.len(), 12288 / usize::from(speed.factor()));
            let stable = &output[128..];
            let rms = |channel: usize| {
                (stable.iter().map(|f| f[channel].powi(2)).sum::<f32>() / stable.len() as f32)
                    .sqrt()
            };
            assert!((0.69..0.72).contains(&rms(0)), "{speed:?}: {}", rms(0));
            assert!(rms(1) < 0.003, "{speed:?}: alias {}", rms(1));
            let crossings = stable
                .windows(2)
                .filter(|pair| pair[0][0] < 0.0 && pair[1][0] >= 0.0)
                .count();
            assert!(
                (crossings as f32 - stable.len() as f32 * 0.02 * f32::from(speed.factor())).abs()
                    < 2.0
            );
            audio.set_speed(PlaybackSpeed::Normal);
            assert_eq!(audio.process([0.25, -0.5]), Some([0.25, -0.5]));
        }
    }
}

#[cfg(test)]
mod problem_tests {
    use super::*;

    #[test]
    fn ds_headroom_absorbs_a_snapshot_stall_without_changing_mgba_latency() {
        for (buffer_ms, expected_gaps) in [(50, 1), (100, 0)] {
            let audio = Arc::new(GameAudio::with_buffer_ms(buffer_ms));
            audio.configure(32768);
            let mut reader = AudioReader::new(Arc::clone(&audio), 32768);
            assert_eq!(audio.limit(), 32768 * usize::from(buffer_ms) / 1000);
            for _ in 0..reader.target {
                audio.push([0.25, -0.25]);
            }
            // Six 256-frame callbacks consume about 47 ms while the worker
            // serializes a state, reads the GPU and reduces its preview.
            let mut output = [0.0; 512];
            for _ in 0..6 {
                reader.render(&mut output);
            }
            assert_eq!(audio.problems(), (expected_gaps, 0));
            assert!(audio.len() <= audio.limit());
        }
        assert_eq!(GameAudio::default().limit(), 32768 / 20);
    }

    #[test]
    fn gaps_and_drops_are_counted_but_deliberate_clears_are_not() {
        let audio = Arc::new(GameAudio::default());
        audio.configure(32768);
        let limit = audio.limit();
        for _ in 0..limit + 10 {
            audio.push([0.1, 0.1]);
        }
        assert_eq!(audio.problems(), (0, 10));
        audio.clear();
        assert_eq!(audio.problems(), (0, 10));
        let mut reader = AudioReader::new(Arc::clone(&audio), 32768);
        for _ in 0..limit {
            audio.push([0.1, 0.1]);
        }
        let mut output = vec![0.0; 512];
        // Priming after a clear is deliberate; running dry afterwards is not.
        reader.render(&mut output);
        assert_eq!(audio.problems().0, 0);
        for _ in 0..20 {
            reader.render(&mut output);
        }
        assert_eq!(audio.problems().0, 1);
    }
}

#[cfg(test)]
mod pacer_tests {
    use super::FramePacer;

    #[test]
    fn ds_fast_forward_caps_bursty_audio_to_the_selected_deadline() {
        let fps = 59.8261;
        let mut paced = FramePacer::new(32768, fps, 2);
        let target = 1.0 / fps / 2.0;
        // A batch may deliver two frames of decimated audio at once.
        // Even a full queue must not turn the frame target into normal speed.
        for _ in 0..300 {
            let interval = paced.fast_forward_interval(548, 1638, 1638);
            assert!(interval.as_secs_f64() <= target + 1e-9);
        }
        // Switching back to normal restores ordinary audio-clocked pacing.
        paced.reset(fps, 1);
        for _ in 0..300 {
            paced.interval(548, 1015, 1638);
        }
        let normal = paced.interval(548, 1015, 1638).as_secs_f64();
        assert!((normal / (1.0 / fps) - 1.0).abs() < 0.01);
    }

    #[test]
    fn game_boy_blocks_pace_like_the_audio_without_jitter() {
        let mut pacer = FramePacer::new(32768, 59.7275, 1);
        let nominal = 1.0 / 59.7275;
        // Steady Game Boy: 512-frame blocks, a double block every 14 frames.
        let mut total = 0.0;
        let (mut shortest, mut longest) = (f64::MAX, 0.0f64);
        for frame in 0..1400 {
            let pushed = if frame % 14 == 13 { 1024 } else { 512 };
            let wait = pacer.interval(pushed, 1015, 1638).as_secs_f64();
            if frame >= 200 {
                shortest = shortest.min(wait);
                longest = longest.max(wait);
            }
            total += wait;
        }
        // About real time overall, with each frame within 6% of nominal.
        assert!((total / 1400.0 / nominal - 1.0).abs() < 0.01);
        assert!(
            shortest > nominal * 0.94 && longest < nominal * 1.06,
            "{shortest} {longest}"
        );
        // Screen toggling: 9% more audio per frame means 9% slower frames.
        let mut pacer = FramePacer::new(32768, 59.7275, 1);
        let mut total = 0.0;
        for frame in 0..1200 {
            let wait = pacer
                .interval(if frame % 6 == 5 { 1024 } else { 512 }, 1015, 1638)
                .as_secs_f64();
            if frame >= 600 {
                total += wait;
            }
        }
        let average_audio = (5.0 * 512.0 + 1024.0) / 6.0 / 32768.0;
        assert!((total / 600.0 / average_audio - 1.0).abs() < 0.01);
        // A queue running full slows frames by at most 5%; an empty one
        // quickens them by at most 5%.
        let mut full = FramePacer::new(32768, 59.7275, 1);
        let mut empty = FramePacer::new(32768, 59.7275, 1);
        let (mut slow, mut fast) = (0.0, 0.0);
        for _ in 0..200 {
            slow = full.interval(549, 1638, 1638).as_secs_f64();
            fast = empty.interval(549, 0, 1638).as_secs_f64();
        }
        assert!((slow / nominal - 1.05).abs() < 0.005, "{slow}");
        assert!((fast / nominal - 0.95).abs() < 0.005, "{fast}");
        // Fast-forward pushes decimated audio; reset starts from its pace.
        let mut pacer = FramePacer::new(32768, 59.7275, 2);
        assert!((pacer.interval(274, 1015, 1638).as_secs_f64() * 2.0 / nominal - 1.0).abs() < 0.01);
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;

    /// Simulates a long session in virtual time: the core pushes a frame's
    /// samples every 1/59.7275 s; the sound card takes 256 frames per
    /// callback at its own clock, `drift_ppm` away. Returns (times the
    /// queue reached the limit, refill gaps).
    fn simulate(drift_ppm: f64, minutes: f64) -> (usize, usize) {
        let rate = 32_768u32;
        let audio = Arc::new(GameAudio::default());
        audio.configure(rate);
        let limit = audio.limit.load(Ordering::Acquire);
        let mut reader = AudioReader::new(audio.clone(), rate);
        let frame_time = 1.0 / 59.7275;
        let samples_per_frame = f64::from(rate) * frame_time;
        let callback_time = 256.0 / (f64::from(rate) * (1.0 + drift_ppm * 1e-6));
        let (mut next_frame, mut next_callback) = (0.0, frame_time * 2.0);
        let (mut owed, mut at_limit, mut gaps) = (0.0, 0, 0);
        let mut output = [0.0f32; 512];
        let mut started = false;
        while next_frame < minutes * 60.0 {
            if next_frame <= next_callback {
                owed += samples_per_frame;
                while owed >= 1.0 {
                    audio.push([0.25, -0.25]);
                    owed -= 1.0;
                }
                if started && audio.len() >= limit {
                    at_limit += 1;
                }
                next_frame += frame_time;
            } else {
                let was_priming = reader.priming;
                reader.render(&mut output);
                if !was_priming {
                    started = true;
                }
                if started && reader.priming && !was_priming {
                    gaps += 1;
                }
                next_callback += callback_time;
            }
        }
        (at_limit, gaps)
    }

    #[test]
    fn steady_queue_follows_a_slow_or_fast_sound_card() {
        // 300 ppm is 20x the drift measured on real hardware (~15 ppm).
        for drift in [-300.0, 0.0, 300.0] {
            let (at_limit, gaps) = simulate(drift, 30.0);
            assert_eq!(
                at_limit, 0,
                "{drift} ppm: queue reached its limit {at_limit} times"
            );
            assert_eq!(gaps, 0, "{drift} ppm: queue ran dry");
        }
    }

    /// Measures whether the emulator (paced by the system clock) and the
    /// sound card (its own clock) drift apart over a long 1x session: the
    /// queue's fill level creeping one way, ending in refill gaps (empty) or
    /// dropped samples (pinned at the limit). Writes silence.
    #[test]
    #[ignore = "real mGBA/PipeWire drift measurement; requires HORIZON_GBA_CORE"]
    fn real_core_audio_drift_at_normal_speed() {
        use crate::platform::{
            gba_core,
            pipewire::{AudioStream, Role},
        };
        use std::time::{Duration, Instant};
        let path = std::env::var_os("HORIZON_GBA_CORE").expect("set HORIZON_GBA_CORE");
        let seconds = std::env::var("HORIZON_AUDIO_DRIFT_SECONDS")
            .map(|value| value.parse::<u64>().unwrap())
            .unwrap_or(600)
            .clamp(20, 7200);
        let audio = Arc::new(GameAudio::default());
        // HORIZON_AUDIO_DRIFT_ROM: gba (default), gb, or gb-toggle (screen
        // switched off and on every few frames, as games do while loading).
        let rom = match std::env::var("HORIZON_AUDIO_DRIFT_ROM").as_deref() {
            Ok("gb") => gba_core::test_gb_rom(false, false, false),
            Ok("gb-toggle") => gba_core::test_gb_toggling_rom(),
            _ => gba_core::test_rom(),
        };
        let mut core = gba_core::Core::load(
            std::path::Path::new(&path),
            &rom,
            Default::default(),
            audio.clone(),
            Default::default(),
        )
        .unwrap();
        let rate = core.av.sample_rate.round() as u32;
        let limit = audio.limit.load(Ordering::Acquire);
        let mut dropped = 0u64;
        // Paced like the player (HORIZON_AUDIO_DRIFT_FRAME_PACING=1 for the
        // old fixed frame interval, to compare).
        let fixed = std::env::var_os("HORIZON_AUDIO_DRIFT_FRAME_PACING").is_some();
        let interval = Duration::from_secs_f64(1.0 / core.av.fps);
        let mut pacer = FramePacer::new(rate, core.av.fps, 1);
        let refills = Arc::new(AtomicU64::new(0));
        let consumed = Arc::new(AtomicU64::new(0));
        let mut reader = AudioReader::new(audio.clone(), rate);
        let (counted, frames_out) = (refills.clone(), consumed.clone());
        let mut was_priming = true;
        let stream = AudioStream::open(
            "Horizon audio drift",
            Role::Game,
            rate,
            2,
            256,
            Box::new(move |output, _| {
                reader.render(output);
                if reader.priming && !was_priming {
                    counted.fetch_add(1, Ordering::Relaxed);
                }
                was_priming = reader.priming;
                frames_out.fetch_add(output.len() as u64 / 2, Ordering::Relaxed);
                output.fill(0.0);
            }),
        )
        .unwrap();
        let start = Instant::now();
        let mut deadline = Instant::now();
        let (mut produced, mut pinned_checks, mut checks) = (0u64, 0u64, 0u64);
        let (mut low, mut high) = (usize::MAX, 0usize);
        let mut report_at = Duration::from_secs(10);
        println!("drift: rate={rate} fps={:.4} limit={limit}", core.av.fps);
        while start.elapsed() < Duration::from_secs(seconds) {
            let before = audio.write.load(Ordering::Acquire);
            let fill_before = audio.len();
            core.run_frame();
            let made = audio.write.load(Ordering::Acquire).wrapping_sub(before);
            produced += made as u64;
            // Anything beyond the cap evicts the oldest queued samples.
            dropped += (fill_before + made).saturating_sub(limit) as u64;
            let fill = audio.len();
            checks += 1;
            if fill + 8 >= limit {
                pinned_checks += 1;
            }
            if start.elapsed() > Duration::from_secs(2) {
                low = low.min(fill);
                high = high.max(fill);
            }
            deadline += if fixed {
                interval
            } else {
                pacer.interval(made, audio.len(), limit)
            };
            if Instant::now().saturating_duration_since(deadline) > Duration::from_millis(100) {
                deadline = Instant::now();
            }
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            if start.elapsed() >= report_at {
                println!(
                    "drift {:>4}s: fill={fill:>5} (min {low}, max {high}) refills={} dropped={dropped} at_limit={:.1}% produced={produced} played={}",
                    report_at.as_secs(),
                    refills.load(Ordering::Relaxed),
                    pinned_checks as f64 * 100.0 / checks as f64,
                    consumed.load(Ordering::Relaxed)
                );
                report_at += Duration::from_secs(10);
            }
        }
        drop(stream);
        let wall = start.elapsed().as_secs_f64();
        let played = consumed.load(Ordering::Relaxed) as f64;
        println!(
            "drift complete: {wall:.0}s produced {:.2} frames/s, played {:.2} frames/s, refills={}",
            produced as f64 / wall,
            played / wall,
            refills.load(Ordering::Relaxed)
        );
    }
    #[test]
    #[ignore = "10-minute real mGBA/PipeWire audio soak; requires HORIZON_GBA_CORE"]
    fn real_core_pipewire_audio_soak() {
        use crate::platform::{
            gba_core,
            pipewire::{AudioStream, Role},
        };
        use std::time::{Duration, Instant};
        #[derive(Default)]
        struct Metrics {
            callbacks: AtomicU64,
            starved: AtomicU64,
            silent: AtomicU64,
            invalid: AtomicU64,
            paused_leaks: AtomicU64,
            resumes: AtomicU64,
            bad_resumes: AtomicU64,
            max_gap_us: AtomicU64,
            max_queue: AtomicUsize,
        }
        let path = std::env::var_os("HORIZON_GBA_CORE").expect("set HORIZON_GBA_CORE");
        let seconds = std::env::var("HORIZON_AUDIO_SOAK_SECONDS")
            .map(|value| value.parse::<u64>().unwrap())
            .unwrap_or(600)
            .clamp(10, 3600);
        let audio = Arc::new(GameAudio::default());
        let mut core = gba_core::Core::load(
            std::path::Path::new(&path),
            &gba_core::test_rom(),
            Default::default(),
            audio.clone(),
            Default::default(),
        )
        .unwrap();
        let rate = core.av.sample_rate.round() as u32;
        let interval = Duration::from_secs_f64(1.0 / core.av.fps);
        let metrics = Arc::new(Metrics::default());
        let start = Instant::now();
        let settling = Arc::new(AtomicU64::new(500_000));
        let resume_pending = Arc::new(AtomicBool::new(false));
        let mut reader = AudioReader::new(audio.clone(), rate);
        let mut last_callback = Instant::now();
        let output_audio = audio.clone();
        let measured = metrics.clone();
        let settle = settling.clone();
        let resuming = resume_pending.clone();
        let stream = AudioStream::open(
            "Horizon audio soak",
            Role::Game,
            rate,
            2,
            256,
            Box::new(move |output, _| {
                let now = Instant::now();
                measured.max_gap_us.fetch_max(
                    now.duration_since(last_callback).as_micros() as u64,
                    Ordering::Relaxed,
                );
                last_callback = now;
                measured.callbacks.fetch_add(1, Ordering::Relaxed);
                let paused = output_audio.paused.load(Ordering::Acquire);
                reader.render(output);
                let silent = output.iter().all(|sample| sample.abs() < 1e-8);
                if output
                    .iter()
                    .any(|sample| !sample.is_finite() || sample.abs() > 1.0)
                {
                    measured.invalid.fetch_add(1, Ordering::Relaxed);
                }
                // Check only callbacks entirely within a pause, avoiding a
                // control-thread transition racing this callback's snapshot.
                if paused && output_audio.paused.load(Ordering::Acquire) && !silent {
                    measured.paused_leaks.fetch_add(1, Ordering::Relaxed);
                }
                if !paused
                    && now.duration_since(start).as_micros() as u64
                        >= settle.load(Ordering::Acquire)
                {
                    if reader.priming {
                        measured.starved.fetch_add(1, Ordering::Relaxed);
                    }
                    if silent {
                        measured.silent.fetch_add(1, Ordering::Relaxed);
                    }
                    if resuming.swap(false, Ordering::AcqRel) {
                        measured.resumes.fetch_add(1, Ordering::Relaxed);
                        if reader.priming || silent {
                            measured.bad_resumes.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                // Quiet synthetic tone; metrics above inspect unattenuated audio.
                for sample in output {
                    *sample *= 0.05;
                }
            }),
        )
        .unwrap();
        let mut deadline = Instant::now();
        let mut phase = 0;
        // Offset freezes from minute-boundary speed changes so resume checks
        // never hide inside the speed-change settling interval.
        let mut next_pause = Duration::from_secs(17);
        let mut report_at = Duration::from_secs(60);
        let mut frames = 0u64;
        while start.elapsed() < Duration::from_secs(seconds) {
            let elapsed = start.elapsed();
            let next_phase = elapsed.as_secs() / 60;
            if next_phase != phase {
                phase = next_phase;
                audio.set_paused(true);
                core.set_speed(PlaybackSpeed::ALL[phase as usize % PlaybackSpeed::ALL.len()]);
                audio.clear();
                settling.store(
                    (elapsed + Duration::from_millis(500)).as_micros() as u64,
                    Ordering::Release,
                );
                audio.set_paused(false);
                deadline = Instant::now();
            }
            if elapsed >= next_pause {
                audio.set_paused(true);
                std::thread::sleep(Duration::from_millis(300));
                resume_pending.store(true, Ordering::Release);
                audio.set_paused(false);
                deadline = Instant::now();
                next_pause += Duration::from_secs(15);
            }
            core.run_frame();
            metrics.max_queue.fetch_max(audio.len(), Ordering::Relaxed);
            frames += 1;
            deadline += interval
                / u32::from(PlaybackSpeed::ALL[phase as usize % PlaybackSpeed::ALL.len()].factor());
            if Instant::now().saturating_duration_since(deadline) > Duration::from_millis(100) {
                deadline = Instant::now();
            }
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            if elapsed >= report_at {
                println!(
                    "soak {}s: frames={frames}, callbacks={}, starved={}, silent={}, max_gap_us={}, max_queue={}",
                    elapsed.as_secs(),
                    metrics.callbacks.load(Ordering::Relaxed),
                    metrics.starved.load(Ordering::Relaxed),
                    metrics.silent.load(Ordering::Relaxed),
                    metrics.max_gap_us.load(Ordering::Relaxed),
                    metrics.max_queue.load(Ordering::Relaxed)
                );
                report_at += Duration::from_secs(60);
            }
        }
        drop(stream);
        println!(
            "soak complete: wall={:.2}s, rate={rate}, frames={frames}, callbacks={}, starved={}, silent={}, invalid={}, pause_leaks={}, resumes={}, bad_resumes={}, max_gap_us={}, max_queue={}",
            start.elapsed().as_secs_f64(),
            metrics.callbacks.load(Ordering::Relaxed),
            metrics.starved.load(Ordering::Relaxed),
            metrics.silent.load(Ordering::Relaxed),
            metrics.invalid.load(Ordering::Relaxed),
            metrics.paused_leaks.load(Ordering::Relaxed),
            metrics.resumes.load(Ordering::Relaxed),
            metrics.bad_resumes.load(Ordering::Relaxed),
            metrics.max_gap_us.load(Ordering::Relaxed),
            metrics.max_queue.load(Ordering::Relaxed)
        );
        assert!(
            metrics.callbacks.load(Ordering::Relaxed) > seconds * 20,
            "PipeWire did not keep consuming audio"
        );
        assert_eq!(metrics.invalid.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.paused_leaks.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.bad_resumes.load(Ordering::Relaxed), 0);
        assert_eq!(
            metrics.starved.load(Ordering::Relaxed),
            0,
            "unexpected steady-playback underrun"
        );
        assert_eq!(
            metrics.silent.load(Ordering::Relaxed),
            0,
            "unexpected steady-playback silence"
        );
        assert!(metrics.max_queue.load(Ordering::Relaxed) <= rate as usize / 20);
    }
    #[test]
    fn paused_output_preserves_headroom_and_resumes_in_the_next_callback() {
        for selected in PlaybackSpeed::ALL {
            let audio = Arc::new(GameAudio::default());
            audio.configure(32768);
            let mut reader = AudioReader::new(audio.clone(), 32768);
            let mut speed = SpeedAudio::default();
            speed.set_speed(selected);
            for _ in 0..1100 * usize::from(selected.factor()) {
                if let Some(frame) = speed.process([0.5, -0.25]) {
                    audio.push(frame);
                }
            }
            let mut output = [0.0; 512];
            reader.render(&mut output);
            let queued = audio.len();
            assert!(queued > 256 && queued < reader.target);
            audio.set_paused(true);
            for _ in 0..100 {
                reader.render(&mut output);
                assert!(output.iter().all(|sample| *sample == 0.0));
                assert_eq!(audio.len(), queued);
            }
            audio.set_paused(false);
            reader.render(&mut output);
            assert!(
                output
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|f| f[0] > 0.0 && f[1] < 0.0)
            );
            assert_eq!(audio.len(), queued - 256);
            assert_eq!(reader.fade, 64);
        }
    }
    #[test]
    fn clearing_while_paused_discards_old_audio_and_still_requires_headroom() {
        let audio = Arc::new(GameAudio::default());
        audio.configure(8000);
        let mut reader = AudioReader::new(audio.clone(), 8000);
        for _ in 0..400 {
            audio.push([0.5, -0.25]);
        }
        let mut output = [0.0; 128];
        reader.render(&mut output);
        audio.set_paused(true);
        audio.clear();
        for _ in 0..100 {
            audio.push([-0.5, 0.25]);
        }
        reader.render(&mut output);
        assert!(output.iter().all(|sample| *sample == 0.0));
        audio.set_paused(false);
        reader.render(&mut output);
        assert!(output.iter().all(|sample| *sample == 0.0));
        for _ in 0..220 {
            audio.push([-0.5, 0.25]);
        }
        reader.render(&mut output);
        assert!(
            output
                .as_chunks::<2>()
                .0
                .iter()
                .all(|f| f[0] < 0.0 && f[1] > 0.0)
        );
    }
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
