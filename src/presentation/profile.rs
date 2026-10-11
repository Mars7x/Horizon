//! The profile picture: shown in Home's top bar, chosen in Settings → Profile.
//!
//! Three sources (`services::profile::ProfileSource`): a picture the user
//! chose, the Steam account's avatar, or a built-in avatar. Everything slow
//! (the FileChooser portal, decoding, Steam) runs on worker threads; the UI
//! thread only shows results. Each source keeps its own copy on disk, so
//! switching between them is instant and Steam's shows offline. A Steam
//! animated avatar and an animated avatar frame are kept as Steam's original
//! files and decoded whole, small, so Slint can play them (`AnimatedPicture`).

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::mpsc::Receiver,
};

use image::RgbaImage;
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use tracing::warn;

use crate::{
    AppWindow, ProfileFrameData,
    platform::{data_paths, file_chooser},
    presentation::wake::drain_notify,
    services::{
        profile::{self, Animation, ProfileSource},
        steam_account::{ProfileItem, SteamAccountService, SteamWebApiClient},
        worker::{WorkerSender, spawn, worker_channel},
    },
};

enum ProfileEvent {
    /// The stored copies, read at startup.
    Loaded {
        chosen: Option<RgbaImage>,
        chosen_animated: Option<Animation>,
        steam: Option<RgbaImage>,
        frame: Option<Animation>,
        animated: Option<Animation>,
    },
    /// A pick finished: a new picture (and its frames, when it moves),
    /// cancelled (`None`), or why it failed.
    Chosen(Result<Option<(RgbaImage, Option<Animation>)>, String>),
    /// A Steam fetch finished. `quiet`: a background refresh, which says
    /// nothing unless it changed something.
    Steam {
        result: Result<SteamProfile, String>,
        quiet: bool,
    },
}

struct SteamProfile {
    picture: RgbaImage,
    frame: Update<Animation>,
    animated: Update<Animation>,
}

/// What a Steam fetch learned about an equipped item.
enum Update<T> {
    /// Equipped: this is it.
    Set(T),
    /// None equipped.
    Clear,
    /// It couldn't be fetched: keep the last one.
    Keep,
}

/// A picture that may move, ready for Slint.
struct Moving {
    still: Image,
    frames: ModelRc<ProfileFrameData>,
}

impl From<Animation> for Moving {
    fn from(animation: Animation) -> Self {
        let frames: Vec<_> = animation
            .frames
            .into_iter()
            .map(|(frame, delay)| ProfileFrameData {
                image: slint_image(frame),
                delay_ms: delay.as_millis().min(i32::MAX as u128) as i32,
            })
            .collect();
        Self {
            still: slint_image(animation.still),
            frames: ModelRc::new(VecModel::from(frames)),
        }
    }
}

struct Paths {
    chosen: PathBuf,
    chosen_animated: PathBuf,
    steam: PathBuf,
    frame: PathBuf,
    animated: PathBuf,
    settings: PathBuf,
}

pub struct ProfileController {
    ui: slint::Weak<AppWindow>,
    paths: Option<Paths>,
    steam_account: Rc<RefCell<SteamAccountService>>,
    sender: WorkerSender<ProfileEvent>,
    receiver: RefCell<Receiver<ProfileEvent>>,
    source: RefCell<ProfileSource>,
    chosen: RefCell<Option<Image>>,
    /// The chosen picture's frames, when it is animated.
    chosen_animated: RefCell<Option<Moving>>,
    steam: RefCell<Option<Image>>,
    /// The Steam avatar frame, shown around a square Steam picture.
    frame: RefCell<Option<Moving>>,
    /// The Steam animated avatar, shown in place of the Steam picture.
    animated: RefCell<Option<Moving>>,
    /// The built-in avatar last shown, decoded once.
    avatar: RefCell<Option<(String, Image)>>,
    /// A pick or a Steam fetch is under way (one at a time).
    busy: Cell<bool>,
    feedback: RefCell<String>,
}

impl ProfileController {
    pub fn new(ui: &AppWindow, steam_account: Rc<RefCell<SteamAccountService>>) -> Rc<Self> {
        let paths = (|| {
            Ok::<_, data_paths::DataPathError>(Paths {
                chosen: data_paths::profile_picture_path()?,
                chosen_animated: data_paths::profile_animated_path()?,
                steam: data_paths::steam_profile_picture_path()?,
                frame: data_paths::steam_profile_frame_path()?,
                animated: data_paths::steam_animated_avatar_path()?,
                settings: data_paths::profile_settings_path()?,
            })
        })()
        .inspect_err(|error| warn!(%error, "the profile picture has no place to live"))
        .ok();
        let source = paths
            .as_ref()
            .map(|paths| profile::load_source(&paths.settings))
            .unwrap_or_default();
        let (notify, drain) = drain_notify(|controller: &ProfileController| controller.drain());
        let (sender, receiver) = worker_channel(notify);
        let controller = Rc::new(Self {
            ui: ui.as_weak(),
            paths,
            steam_account,
            sender,
            receiver: RefCell::new(receiver),
            source: RefCell::new(source),
            chosen: RefCell::new(None),
            chosen_animated: RefCell::new(None),
            steam: RefCell::new(None),
            frame: RefCell::new(None),
            animated: RefCell::new(None),
            avatar: RefCell::new(None),
            busy: Cell::new(false),
            feedback: RefCell::new(String::new()),
        });
        drain.bind(&controller);
        if let Some(paths) = &controller.paths {
            let chosen_animated = paths.chosen_animated.clone();
            let (chosen, steam, frame, animated) = (
                paths.chosen.clone(),
                paths.steam.clone(),
                paths.frame.clone(),
                paths.animated.clone(),
            );
            let sender = controller.sender.clone();
            spawn("profile-load", move || {
                let _ = sender.send(ProfileEvent::Loaded {
                    chosen: profile::load(&chosen),
                    chosen_animated: std::fs::read(&chosen_animated).ok().and_then(|bytes| {
                        profile::decode_chosen_animation(&bytes, profile::ANIMATED_SIDE)
                    }),
                    steam: profile::load(&steam),
                    frame: profile::load_animation(&frame, profile::FRAME_SIDE),
                    animated: profile::load_animation(&animated, profile::ANIMATED_SIDE),
                });
            });
        }
        // Steam pictures change: refresh the copy quietly in the background.
        if controller.uses_steam() {
            controller.fetch_steam(true);
        }
        controller.publish(ui);
        controller
    }

    /// The user's own chosen picture exists (so it can be removed).
    pub fn has_chosen_picture(&self) -> bool {
        self.chosen.borrow().is_some()
    }

    pub fn uses_steam(&self) -> bool {
        matches!(*self.source.borrow(), ProfileSource::Steam { .. })
    }

    /// The Steam picture is shown square, with its frame.
    pub fn steam_square(&self) -> bool {
        matches!(*self.source.borrow(), ProfileSource::Steam { square: true })
    }

    /// Show the Steam picture square with its avatar frame, or round.
    /// Returns whether anything changed.
    pub fn set_steam_square(&self, ui: &AppWindow, square: bool) -> bool {
        if !self.uses_steam() || square == self.steam_square() {
            return false;
        }
        self.set_source(ProfileSource::Steam { square });
        self.feedback.borrow_mut().clear();
        self.publish(ui);
        true
    }

    /// Open the file picker. Returns whether a pick started.
    pub fn choose(&self, ui: &AppWindow) -> bool {
        let Some((path, animated_path)) = self
            .paths
            .as_ref()
            .map(|paths| (paths.chosen.clone(), paths.chosen_animated.clone()))
        else {
            self.say(ui, "Profile pictures can't be saved here.");
            return false;
        };
        if self.busy.replace(true) {
            return false;
        }
        self.feedback.borrow_mut().clear();
        self.publish(ui);
        let sender = self.sender.clone();
        let started = spawn("profile-choose", move || {
            let result = match file_chooser::choose_image("Choose a profile picture") {
                Ok(None) => Ok(None),
                Ok(Some(source)) => profile::prepare(&source)
                    .and_then(|picture| profile::save(&path, &picture).map(|()| picture))
                    .map(|picture| {
                        // An animated picture keeps its original, to play.
                        let animation = std::fs::read(&source).ok().and_then(|bytes| {
                            let animation =
                                profile::decode_chosen_animation(&bytes, profile::ANIMATED_SIDE)?;
                            profile::save_bytes(&animated_path, &bytes).ok()?;
                            Some(animation)
                        });
                        if animation.is_none() {
                            let _ = profile::remove(&animated_path);
                        }
                        Some((picture, animation))
                    })
                    .map_err(|error| error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let _ = sender.send(ProfileEvent::Chosen(result));
        });
        if !started {
            self.busy.set(false);
            self.publish(ui);
        }
        started
    }

    /// Delete the chosen picture. Returns whether there was one.
    pub fn remove(&self, ui: &AppWindow) -> bool {
        let Some(paths) = &self.paths else {
            return false;
        };
        if !self.has_chosen_picture() || self.busy.get() {
            return false;
        }
        match profile::remove(&paths.chosen).and(profile::remove(&paths.chosen_animated)) {
            Ok(()) => {
                *self.chosen.borrow_mut() = None;
                *self.chosen_animated.borrow_mut() = None;
                self.say(ui, "Picture removed.");
                true
            }
            Err(error) => {
                warn!(%error, "the profile picture could not be removed");
                self.say(ui, "Couldn't remove the picture.");
                false
            }
        }
    }

    /// Turn the Steam picture on (fetching it) or off (back to the chosen
    /// picture). Returns whether anything changed.
    pub fn set_use_steam(&self, ui: &AppWindow, on: bool) -> bool {
        if on == self.uses_steam() || self.busy.get() {
            return false;
        }
        if on && !self.steam_account.borrow().connected() {
            self.say(ui, "Set up your Steam Account in Third-Party first.");
            return false;
        }
        self.set_source(if on {
            ProfileSource::Steam { square: false }
        } else {
            ProfileSource::Chosen
        });
        if on {
            self.busy.set(true);
            self.feedback.borrow_mut().clear();
            self.fetch_steam(false);
        } else {
            self.feedback.borrow_mut().clear();
        }
        self.publish(ui);
        true
    }

    /// The Steam account was saved or removed: a Steam picture follows it.
    pub fn steam_account_changed(&self, ui: &AppWindow) {
        if self.uses_steam() && self.steam_account.borrow().connected() && !self.busy.get() {
            self.busy.set(true);
            self.fetch_steam(false);
            self.publish(ui);
        }
    }

    fn fetch_steam(&self, quiet: bool) {
        let Some((path, frame_path, animated_path)) = self.paths.as_ref().map(|paths| {
            (
                paths.steam.clone(),
                paths.frame.clone(),
                paths.animated.clone(),
            )
        }) else {
            self.busy.set(false);
            return;
        };
        let Some(credentials) = self.steam_account.borrow().credentials() else {
            self.busy.set(false);
            return;
        };
        let sender = self.sender.clone();
        let started = spawn("profile-steam", move || {
            let result = SteamWebApiClient::new(credentials)
                .map_err(|error| error.without_url().to_string())
                .and_then(|client| {
                    let picture = client
                        .profile_avatar()
                        .map_err(|error| error.to_string())
                        .and_then(|bytes| {
                            profile::prepare_bytes(&bytes).map_err(|error| error.to_string())
                        })?;
                    profile::save(&path, &picture).map_err(|error| error.to_string())?;
                    // Equipped items are niceties: failing to get them keeps
                    // the last ones.
                    let (frame, animated) = match client.profile_items() {
                        Ok(items) => (
                            keep_item(&items.frame, &frame_path, profile::FRAME_SIDE),
                            keep_item(
                                &items.animated_avatar,
                                &animated_path,
                                profile::ANIMATED_SIDE,
                            ),
                        ),
                        Err(error) => {
                            warn!(%error, "Steam profile items were not fetched");
                            (Update::Keep, Update::Keep)
                        }
                    };
                    Ok(SteamProfile {
                        picture,
                        frame,
                        animated,
                    })
                });
            let _ = sender.send(ProfileEvent::Steam { result, quiet });
        });
        if !started {
            self.busy.set(false);
        }
    }

    fn set_source(&self, source: ProfileSource) {
        if let Some(paths) = &self.paths
            && let Err(error) = profile::save_source(&paths.settings, &source)
        {
            warn!(%error, "the profile picture choice could not be saved");
        }
        *self.source.borrow_mut() = source;
    }

    fn say(&self, ui: &AppWindow, message: &str) {
        *self.feedback.borrow_mut() = message.to_owned();
        self.publish(ui);
    }

    fn drain(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let events: Vec<_> = self.receiver.borrow().try_iter().collect();
        for event in events {
            match event {
                ProfileEvent::Loaded {
                    chosen,
                    chosen_animated,
                    steam,
                    frame,
                    animated,
                } => {
                    // A pick or fetch that finished first wins over the
                    // startup read.
                    if self.chosen.borrow().is_none() {
                        *self.chosen.borrow_mut() = chosen.map(slint_image);
                        *self.chosen_animated.borrow_mut() = chosen_animated.map(Moving::from);
                    }
                    if self.steam.borrow().is_none() {
                        *self.steam.borrow_mut() = steam.map(slint_image);
                    }
                    if self.frame.borrow().is_none() {
                        *self.frame.borrow_mut() = frame.map(Moving::from);
                    }
                    if self.animated.borrow().is_none() {
                        *self.animated.borrow_mut() = animated.map(Moving::from);
                    }
                }
                ProfileEvent::Chosen(result) => {
                    self.busy.set(false);
                    match result {
                        Ok(Some((picture, animation))) => {
                            *self.chosen.borrow_mut() = Some(slint_image(picture));
                            *self.chosen_animated.borrow_mut() = animation.map(Moving::from);
                            // Choosing a picture is choosing to show it.
                            self.set_source(ProfileSource::Chosen);
                            *self.feedback.borrow_mut() = "Picture updated.".into();
                        }
                        Ok(None) => {}
                        Err(error) => {
                            warn!(%error, "the profile picture was not set");
                            *self.feedback.borrow_mut() = "Couldn't use that image.".into();
                        }
                    }
                }
                ProfileEvent::Steam { result, quiet } => {
                    self.busy.set(false);
                    match result {
                        Ok(SteamProfile {
                            picture,
                            frame,
                            animated,
                        }) => {
                            *self.steam.borrow_mut() = Some(slint_image(picture));
                            apply(&self.frame, frame);
                            apply(&self.animated, animated);
                            if !quiet {
                                *self.feedback.borrow_mut() = "Using your Steam picture.".into();
                            }
                        }
                        Err(error) => {
                            warn!(%error, "the Steam profile picture was not fetched");
                            if !quiet {
                                *self.feedback.borrow_mut() = if self.steam.borrow().is_some() {
                                    "Couldn't reach Steam; showing the last Steam picture."
                                } else {
                                    "Couldn't get your Steam picture."
                                }
                                .into();
                            }
                        }
                    }
                }
            }
        }
        self.publish(&ui);
    }

    /// The picture for the current source, if it has one.
    fn current(&self) -> Option<Image> {
        match &*self.source.borrow() {
            ProfileSource::Chosen => self.chosen.borrow().clone(),
            ProfileSource::Steam { .. } => self
                .animated
                .borrow()
                .as_ref()
                .map(|animated| animated.still.clone())
                .or_else(|| self.steam.borrow().clone()),
            ProfileSource::Avatar { id } => {
                if let Some((shown, image)) = &*self.avatar.borrow()
                    && shown == id
                {
                    return Some(image.clone());
                }
                let image = profile::avatar(id)
                    .and_then(|avatar| profile::avatar_picture(avatar).ok())
                    .map(slint_image)?;
                *self.avatar.borrow_mut() = Some((id.clone(), image.clone()));
                Some(image)
            }
        }
    }

    fn publish(&self, ui: &AppWindow) {
        let current = self.current();
        ui.set_profile_has_picture(current.is_some());
        ui.set_profile_picture(current.unwrap_or_default());
        ui.set_settings_profile_has_chosen(self.has_chosen_picture());
        ui.set_settings_profile_uses_steam(self.uses_steam());
        let square = self.steam_square();
        ui.set_settings_profile_steam_square(square);
        ui.set_profile_square(square);
        let none = || ModelRc::new(VecModel::<ProfileFrameData>::default());
        let frame = self.frame.borrow();
        let frame = frame.as_ref().filter(|_| square);
        ui.set_profile_has_frame(frame.is_some());
        ui.set_profile_frame(frame.map(|frame| frame.still.clone()).unwrap_or_default());
        ui.set_profile_frame_frames(frame.map_or_else(none, |frame| frame.frames.clone()));
        // Whichever picture is shown plays, if it moves.
        let frames = match &*self.source.borrow() {
            ProfileSource::Chosen => self
                .chosen_animated
                .borrow()
                .as_ref()
                .map(|moving| moving.frames.clone()),
            ProfileSource::Steam { .. } => self
                .animated
                .borrow()
                .as_ref()
                .map(|moving| moving.frames.clone()),
            ProfileSource::Avatar { .. } => None,
        };
        ui.set_profile_picture_frames(frames.unwrap_or_else(none));
        ui.set_settings_profile_steam_available(self.steam_account.borrow().connected());
        ui.set_settings_profile_choosing(self.busy.get());
        ui.set_settings_profile_feedback(self.feedback.borrow().clone().into());
    }
}

/// Keep an equipped item's original file and decode it, or forget it when
/// none is equipped; `Keep` when it couldn't be fetched, decoded or kept.
fn keep_item(item: &ProfileItem, path: &std::path::Path, side: u32) -> Update<Animation> {
    match item {
        ProfileItem::Downloaded(bytes) => match profile::decode_animation(bytes, side) {
            Ok(animation) if profile::save_bytes(path, bytes).is_ok() => Update::Set(animation),
            _ => Update::Keep,
        },
        ProfileItem::NotEquipped => {
            let _ = profile::remove(path);
            Update::Clear
        }
        ProfileItem::Failed => Update::Keep,
    }
}

fn apply(slot: &RefCell<Option<Moving>>, update: Update<Animation>) {
    match update {
        Update::Set(animation) => *slot.borrow_mut() = Some(animation.into()),
        Update::Clear => *slot.borrow_mut() = None,
        Update::Keep => {}
    }
}

fn slint_image(picture: RgbaImage) -> Image {
    let (width, height) = picture.dimensions();
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        picture.as_raw(),
        width,
        height,
    ))
}
