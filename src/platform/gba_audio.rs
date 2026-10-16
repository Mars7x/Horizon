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
    epoch: AtomicU64,
    paused: AtomicBool,
}
impl Default for GameAudio {
    fn default() -> Self {
        Self {
            slots: (0..CAPACITY).map(|_| AtomicU64::new(0)).collect(),
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
            limit: AtomicUsize::new(32768 / 20),
            epoch: AtomicU64::new(0),
            paused: AtomicBool::new(false),
        }
    }
}
impl GameAudio {
    /// Silence the live output without consuming the frozen game's timeline.
    /// Keeping its bounded headroom avoids re-priming on an ordinary resume.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }
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
    factor: usize,
    phase: usize,
    position: usize,
    history: [[f32; 2]; TAPS],
    coefficients: [f32; TAPS],
}

impl Default for SpeedAudio {
    fn default() -> Self {
        Self {
            factor: 1,
            phase: 0,
            position: 0,
            history: [[0.0; 2]; TAPS],
            coefficients: [0.0; TAPS],
        }
    }
}
impl SpeedAudio {
    pub fn set_speed(&mut self, speed: PlaybackSpeed) {
        self.factor = usize::from(speed.factor());
        self.phase = 0;
        self.position = 0;
        self.history.fill([0.0; 2]);
        let cutoff = 0.44 / self.factor as f64;
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
        self.phase = (self.phase + 1) % self.factor;
        if self.phase != 0 {
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
mod queue_tests {
    use super::*;
    #[test]
    #[ignore = "10-minute real mGBA/PipeWire audio soak; requires HORIZON_GBA_CORE"]
    fn real_core_pipewire_audio_soak() {
        use crate::platform::{
            gba_core,
            pipewire::{AudioStream, Role},
        };
        use std::{
            sync::Mutex,
            time::{Duration, Instant},
        };
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
            Arc::new(Mutex::new(None)),
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
                core.set_speed(PlaybackSpeed::ALL[phase as usize % 4]);
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
            deadline += interval / u32::from(PlaybackSpeed::ALL[phase as usize % 4].factor());
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
            for _ in 0..1400 * usize::from(selected.factor()) {
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
        for speed in [1, 2, 3, 4] {
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
