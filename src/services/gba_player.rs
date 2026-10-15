//! First GBA session: validated ROM snapshots, core lifecycle and battery saves.
use crate::{
    domain::{GameId, SessionTrackingMethod, SourceId},
    persistence::SqliteLibraryRepository,
    platform::{
        gba_core::{AudioBuffer, Core, LatestFrame},
        pipewire::{AudioStream, Role},
    },
    services::activity::ActivityRepository,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU16, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
static CORE_RUNNING: AtomicBool = AtomicBool::new(false);
pub struct RomLocation {
    pub game_id: GameId,
    pub title: String,
    pub path: PathBuf,
}
#[derive(Clone)]
pub struct PlayerPaths {
    pub database: PathBuf,
    pub core: PathBuf,
    pub saves: PathBuf,
    pub settings: PathBuf,
    pub album: PathBuf,
}
pub enum Command {
    Pause(bool),
    Stop,
    Shutdown,
    FastForward(bool),
    Screenshot,
    Restart,
}
pub enum Event {
    Ready {
        title: String,
        freeze_when_unfocused: bool,
    },
    Started(String),
    Stopped,
    Failed(String),
    SaveFailed(String),
    SpeedChanged(bool),
    ScreenshotSaved,
    Restarted,
    ActionFailed(String),
}
pub struct RunningGame {
    pub commands: Sender<Command>,
    pub events: Receiver<Event>,
    pub frame: LatestFrame,
    pub buttons: Arc<AtomicU16>,
    worker: Option<JoinHandle<Result<(), String>>>,
}
impl RunningGame {
    pub fn start(paths: PlayerPaths, sha256: String) -> Result<Self, String> {
        if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid ROM identity".into());
        }
        if CORE_RUNNING.swap(true, Ordering::AcqRel) {
            return Err("A game is already running".into());
        }
        let (commands, receiver) = mpsc::channel();
        let (sender, events) = mpsc::channel();
        let frame = Arc::new(Mutex::new(None));
        let buttons = Arc::new(AtomicU16::new(0));
        let (worker_frame, worker_buttons) = (Arc::clone(&frame), Arc::clone(&buttons));
        let worker = thread::Builder::new()
            .name("horizon-gba".into())
            .spawn(move || {
                struct CoreLease;
                impl Drop for CoreLease {
                    fn drop(&mut self) {
                        CORE_RUNNING.store(false, Ordering::Release);
                    }
                }
                let _lease = CoreLease;
                let result = play(
                    paths,
                    &sha256,
                    receiver,
                    &sender,
                    worker_frame,
                    worker_buttons,
                );
                if let Err(error) = &result {
                    let _ = sender.send(Event::Failed(error.clone()));
                }
                result
            });
        match worker {
            Ok(worker) => Ok(Self {
                commands,
                events,
                frame,
                buttons,
                worker: Some(worker),
            }),
            Err(_) => {
                CORE_RUNNING.store(false, Ordering::Release);
                Err("Could not start the GBA worker".into())
            }
        }
    }
    pub fn shutdown(&mut self) -> Result<(), String> {
        let _ = self.commands.send(Command::Shutdown);
        match self.worker.take() {
            Some(worker) => worker
                .join()
                .map_err(|_| "The GBA worker stopped unexpectedly".to_owned())?,
            None => Ok(()),
        }
    }
    pub fn finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }
}
impl Drop for RunningGame {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

pub fn read_rom_snapshot(path: &Path, sha256: &str) -> Result<Vec<u8>, String> {
    let mut file = File::open(path)
        .map_err(|_| "Game folder access is unavailable. Choose its folder again.".to_owned())?;
    let metadata = file
        .metadata()
        .map_err(|_| "Could not read the game file".to_owned())?;
    if !metadata.is_file() || !(192..=32 * 1024 * 1024).contains(&metadata.len()) {
        return Err("This game file is not a supported raw GBA ROM".into());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    (&mut file)
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read the game file".to_owned())?;
    let actual = ring::digest::digest(&ring::digest::SHA256, &bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if actual != sha256 || bytes.len() as u64 != metadata.len() {
        return Err("The game file has changed. Add its folder again before playing.".into());
    }
    Ok(bytes)
}

fn play(
    paths: PlayerPaths,
    sha256: &str,
    commands: Receiver<Command>,
    events: &Sender<Event>,
    frame: LatestFrame,
    buttons: Arc<AtomicU16>,
) -> Result<(), String> {
    fs::create_dir_all(&paths.saves)
        .map_err(|_| "Could not prepare the game save directory".to_owned())?;
    let mut repository =
        SqliteLibraryRepository::open(&paths.database).map_err(|e| e.to_string())?;
    let locations = repository
        .gba_rom_locations(sha256)
        .map_err(|e| e.to_string())?;
    let mut selected = None;
    let mut failure =
        "This game has no available imported ROM. Choose its folder again.".to_owned();
    for location in locations {
        match read_rom_snapshot(&location.path, sha256) {
            Ok(bytes) => {
                selected = Some((location, bytes));
                break;
            }
            Err(error) => failure = error,
        }
    }
    let Some((location, rom)) = selected else {
        return Err(failure);
    };
    let audio: AudioBuffer = Arc::new(Mutex::new(std::collections::VecDeque::with_capacity(32768)));
    let preferences = crate::persistence::settings::SettingsStore::new(paths.settings)
        .load_value::<crate::domain::core_settings::CorePreferences>()
        .map_err(|_| "Could not read core settings. Check cores.json before playing.".to_owned())?;
    let mut core = Core::load(
        &paths.core,
        &rom,
        preferences.mgba,
        frame,
        Arc::clone(&audio),
        Arc::clone(&buttons),
    )?;
    let save_path = crate::platform::data_paths::gba_save_file(&paths.saves, sha256);
    let mut last_save = read_save(&save_path)?.unwrap_or_default();
    if !last_save.is_empty() {
        core.restore_save(&last_save)?;
    }
    let playback_audio = Arc::clone(&audio);
    let stream = AudioStream::open(
        "Horizon GBA",
        Role::Game,
        core.av.sample_rate.round() as u32,
        2,
        256,
        Box::new(move |output, _| {
            output.fill(0.0);
            if let Ok(mut audio) = playback_audio.try_lock() {
                for sample in output {
                    if let Some(next) = audio.pop_front() {
                        *sample = next;
                    } else {
                        break;
                    }
                }
            }
        }),
    )
    .map_err(|_| "Could not open game audio. Check the audio output and try again.".to_owned())?;
    let source = SourceId::new("gba").map_err(|e| e.to_string())?;
    let interval = Duration::from_secs_f64(1.0 / core.av.fps);
    let mut deadline = Instant::now();
    let mut checkpoint = Instant::now();
    let mut paused = false;
    let mut fast_forward = false;
    let mut pending = None;
    let mut session = None;
    let mut frame_attempts = 0u32;
    let _ = events.send(Event::Ready {
        title: location.title.clone(),
        freeze_when_unfocused: preferences.mgba.freeze_when_unfocused,
    });
    let result = (|| -> Result<(), String> {
        loop {
            while let Some(command) = pending.take().or_else(|| commands.try_recv().ok()) {
                match command {
                    Command::Pause(value) => {
                        paused = value;
                        core.set_fast_forward(fast_forward);
                        buttons.store(0, Ordering::Release);
                        stream.set_active(!value);
                        if let Ok(mut audio) = audio.lock() {
                            audio.clear();
                        }
                        deadline = Instant::now();
                    }
                    Command::FastForward(enabled) => {
                        fast_forward = enabled;
                        core.set_fast_forward(enabled);
                        stream.set_active(!paused);
                        if let Ok(mut audio) = audio.lock() {
                            audio.clear();
                        }
                        deadline = Instant::now();
                        let _ = events.send(Event::SpeedChanged(enabled));
                    }
                    Command::Screenshot => {
                        let result = core
                            .capture_frame()
                            .and_then(|frame| write_screenshot(&paths.album, sha256, &frame));
                        let _ = events.send(match result {
                            Ok(()) => Event::ScreenshotSaved,
                            Err(error) => Event::ActionFailed(error),
                        });
                    }
                    Command::Restart => {
                        if let Err(error) = save_current(&core, &save_path, &mut last_save) {
                            paused = true;
                            stream.set_active(false);
                            let _ = events.send(Event::SaveFailed(error));
                            continue;
                        }
                        core.restart();
                        core.set_fast_forward(fast_forward);
                        if !last_save.is_empty()
                            && let Err(error) = core.restore_save(&last_save)
                        {
                            paused = true;
                            stream.set_active(false);
                            let _ = events.send(Event::ActionFailed(error));
                            continue;
                        }
                        buttons.store(0, Ordering::Release);
                        if let Ok(mut audio) = audio.lock() {
                            audio.clear();
                        }
                        paused = false;
                        stream.set_active(true);
                        deadline = Instant::now();
                        let _ = events.send(Event::Restarted);
                    }
                    Command::Stop | Command::Shutdown => {
                        let stopping = matches!(command, Command::Stop);
                        if let Err(error) = if session.is_some() {
                            save_current(&core, &save_path, &mut last_save)
                        } else {
                            Ok(())
                        } {
                            if !stopping {
                                return Err(error);
                            }
                            paused = true;
                            stream.set_active(false);
                            let _ = events.send(Event::SaveFailed(error));
                            continue;
                        }
                        return Ok(());
                    }
                }
            }
            if !paused {
                core.run_frame();
                frame_attempts = frame_attempts.saturating_add(1);
                if frame_attempts >= 120 && core.frames.load(Ordering::Acquire) == 0 {
                    return Err("mGBA could not produce a game frame".into());
                }
                if session.is_none() && core.frames.load(Ordering::Acquire) > 0 {
                    session = Some(
                        repository
                            .begin_play_session(
                                location.game_id,
                                &source,
                                chrono::Utc::now().timestamp(),
                                SessionTrackingMethod::SourceRuntime,
                            )
                            .map_err(|e| e.to_string())?,
                    );
                    let _ = events.send(Event::Started(location.title.clone()));
                }
                deadline += if fast_forward { interval / 2 } else { interval };
                if Instant::now().saturating_duration_since(deadline) > Duration::from_millis(100) {
                    deadline = Instant::now();
                }
            }
            if checkpoint.elapsed() >= Duration::from_secs(5) {
                if let Err(error) = if session.is_some() {
                    save_current(&core, &save_path, &mut last_save)
                } else {
                    Ok(())
                } {
                    paused = true;
                    stream.set_active(false);
                    buttons.store(0, Ordering::Release);
                    let _ = events.send(Event::SaveFailed(error));
                }
                repository
                    .checkpoint_open_play_sessions(chrono::Utc::now().timestamp())
                    .map_err(|e| e.to_string())?;
                checkpoint = Instant::now();
            }
            let delay = if paused {
                Duration::from_millis(100)
            } else {
                deadline.saturating_duration_since(Instant::now())
            };
            match commands.recv_timeout(delay) {
                Ok(command) => pending = Some(command),
                Err(mpsc::RecvTimeoutError::Disconnected) => pending = Some(Command::Shutdown),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    })();
    if let Some(session) = session {
        let end = if result.is_ok() {
            repository.complete_play_session(session, chrono::Utc::now().timestamp())
        } else {
            repository.interrupt_play_session(session)
        };
        end.map_err(|e| e.to_string())?;
    }
    if result.is_ok() {
        let _ = events.send(Event::Stopped);
    }
    result
}

fn write_screenshot(
    album: &Path,
    sha256: &str,
    frame: &crate::platform::gba_core::Frame,
) -> Result<(), String> {
    use crate::{
        domain::{ExternalGameId, SourceGameRef},
        services::album::catalog::{HorizonAlbum, part_path},
    };
    use image::ImageEncoder;
    let game = SourceGameRef::new(
        SourceId::new("gba").map_err(|e| e.to_string())?,
        ExternalGameId::new(format!("sha256:{sha256}")).map_err(|e| e.to_string())?,
    );
    let path = HorizonAlbum::new(album.to_owned())
        .new_capture_path(Some(&game), "png", chrono::Utc::now())
        .map_err(|_| "Could not prepare the screenshot in Album".to_owned())?;
    let partial = part_path(&path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&partial)
        .map_err(|_| "Could not create the screenshot".to_owned())?;
    let result = (|| -> Result<(), String> {
        image::codecs::png::PngEncoder::new(&mut file)
            .write_image(
                &frame.rgba,
                frame.width,
                frame.height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|_| "Could not encode the screenshot".to_owned())?;
        file.sync_all()
            .map_err(|_| "Could not save the screenshot".to_owned())?;
        fs::hard_link(&partial, &path)
            .map_err(|_| "Could not publish the screenshot in Album".to_owned())?;
        Ok(())
    })();
    if let Err(error) = fs::remove_file(&partial) {
        tracing::warn!(%error, "could not remove screenshot partial file");
    }
    result
}

fn read_save(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match File::open(path) {
        Ok(file) => {
            let size = file
                .metadata()
                .map_err(|_| "Could not inspect the existing save".to_owned())?
                .len();
            if size == 0 || size > 128 * 1024 {
                return Err("The existing save is invalid. It has been preserved.".into());
            }
            let mut bytes = Vec::with_capacity(size as usize);
            file.take(128 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "Could not read the existing save".to_owned())?;
            if bytes.len() as u64 != size {
                return Err("The save changed while reading. It has been preserved.".into());
            }
            Ok(Some(bytes))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Could not read the existing save. It has been preserved.".into()),
    }
}
fn save_current(core: &Core, path: &Path, last: &mut Vec<u8>) -> Result<(), String> {
    let bytes = core.save_ram()?;
    if bytes.is_empty() || bytes == *last {
        return Ok(());
    }
    write_save(path, &bytes)?;
    *last = bytes;
    Ok(())
}
pub fn write_save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > 128 * 1024 {
        return Err("Invalid game save size".into());
    }
    fn replace(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let temporary = path.with_extension("tmp");
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        if let Some(parent) = path.parent() {
            File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
    if let Some(previous) = read_save(path)? {
        replace(&path.with_extension("sav.bak"), &previous)
            .map_err(|_| "Could not preserve the previous save".to_owned())?;
    }
    replace(path,bytes).map_err(|_| "Could not write the game save. Free storage or restore folder access, then try returning to Horizon again.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn directory(name: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("horizon-gba-{name}-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }
    #[test]
    fn launch_rejects_changed_content_even_when_filename_is_unchanged() {
        let dir = directory("hash");
        let path = dir.join("same.gba");
        let bytes = vec![0u8; 192];
        fs::write(&path, &bytes).unwrap();
        let hash = ring::digest::digest(&ring::digest::SHA256, &bytes)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(read_rom_snapshot(&path, &hash).unwrap(), bytes);
        fs::write(&path, vec![1u8; 192]).unwrap();
        assert!(read_rom_snapshot(&path, &hash).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn battery_save_replacement_preserves_prior_data_and_rejects_invalid_existing_data() {
        let dir = directory("save");
        let path = dir.join("game.sav");
        write_save(&path, &[1; 512]).unwrap();
        write_save(&path, &[2; 512]).unwrap();
        assert_eq!(read_save(&path).unwrap().unwrap(), vec![2; 512]);
        assert_eq!(
            fs::read(path.with_extension("sav.bak")).unwrap(),
            vec![1; 512]
        );
        fs::write(&path, vec![0u8; 128 * 1024 + 1]).unwrap();
        assert!(write_save(&path, &[3; 512]).is_err());
        assert_eq!(fs::metadata(&path).unwrap().len(), 128 * 1024 + 1);
        assert_eq!(
            fs::read(path.with_extension("sav.bak")).unwrap(),
            vec![1; 512]
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn screenshots_publish_native_pixels_and_preserve_unavailable_album_data() {
        let dir = directory("screenshots");
        let frame = crate::platform::gba_core::Frame {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
            smooth: false,
        };
        write_screenshot(&dir, &"a".repeat(64), &frame).unwrap();
        let shots = crate::services::album::catalog::HorizonAlbum::new(dir.clone()).scan();
        assert_eq!(shots.len(), 1);
        assert_eq!(
            image::open(shots[0].path()).unwrap().to_rgba8().as_raw(),
            &frame.rgba
        );
        let blocked = dir.join("unavailable");
        fs::write(&blocked, b"preserved").unwrap();
        assert!(write_screenshot(&blocked, &"a".repeat(64), &frame).is_err());
        assert_eq!(fs::read(blocked).unwrap(), b"preserved");
        fs::remove_dir_all(dir).unwrap();
    }
}
