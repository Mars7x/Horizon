//! The profile: the avatar in Home's top bar, and the Profile page (opened
//! from it), which shows the avatar and name with two actions: "Change
//! picture" opens a dialog of the picture's options, "Edit name" the name
//! editor.
//!
//! Three sources (`services::profile::ProfileSource`): a picture the user
//! chose, the Steam account's avatar, or a built-in avatar. Everything slow
//! (the FileChooser portal, decoding, Steam) runs on worker threads; the UI
//! thread only shows results. Each source keeps its own copy on disk, so
//! switching between them is instant and Steam's shows offline. A Steam
//! animated avatar, and an animated picture the user chose, are kept as their
//! original files and decoded whole, small, so Slint can play them
//! (`AnimatedPicture`).

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
    input::{UiAction, UiActionEvent},
    navigation::step_with_edge_wrap,
    platform::{data_paths, file_chooser},
    presentation::wake::drain_notify,
    services::{
        profile::{self, Animation, ProfilePreferences, ProfileSource},
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
    animated: PathBuf,
    settings: PathBuf,
}

/// What a Profile page input did, for the navigation layer's sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileFeedback {
    /// Not the page's (Back to leave the page, Home, Menu…).
    Ignored,
    /// Handled quietly (a move is heard through the focus snapshot).
    Handled,
    Ok,
    Back,
}

/// The Profile page's actions, left to right.
const CHANGE_PICTURE: i32 = 0;
const EDIT_NAME: i32 = 1;

/// The picture dialog's options, top to bottom, then its Done button.
const PICTURE_CHOOSE: i32 = 0;
const PICTURE_STEAM: i32 = 1;
const PICTURE_REMOVE: i32 = 2;
const PICTURE_DONE: i32 = 3;

/// The name editor's controls: the text, Cancel, Save.
const EDITOR_TEXT: i32 = 0;
const EDITOR_CANCEL: i32 = 1;
const EDITOR_SAVE: i32 = 2;

#[derive(Default)]
struct PageState {
    selection: i32,
    picture_open: bool,
    picture_selection: i32,
    editing_name: bool,
    editor_target: i32,
}

pub struct ProfileController {
    ui: slint::Weak<AppWindow>,
    paths: Option<Paths>,
    steam_account: Rc<RefCell<SteamAccountService>>,
    sender: WorkerSender<ProfileEvent>,
    receiver: RefCell<Receiver<ProfileEvent>>,
    source: RefCell<ProfileSource>,
    name: RefCell<String>,
    page: RefCell<PageState>,
    chosen: RefCell<Option<Image>>,
    /// The chosen picture's frames, when it is animated.
    chosen_animated: RefCell<Option<Moving>>,
    steam: RefCell<Option<Image>>,
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
                animated: data_paths::steam_animated_avatar_path()?,
                settings: data_paths::profile_settings_path()?,
            })
        })()
        .inspect_err(|error| warn!(%error, "the profile picture has no place to live"))
        .ok();
        let preferences = paths
            .as_ref()
            .map(|paths| profile::load_preferences(&paths.settings))
            .unwrap_or_default();
        let (notify, drain) = drain_notify(|controller: &ProfileController| controller.drain());
        let (sender, receiver) = worker_channel(notify);
        let controller = Rc::new(Self {
            ui: ui.as_weak(),
            paths,
            steam_account,
            sender,
            receiver: RefCell::new(receiver),
            source: RefCell::new(preferences.source),
            name: RefCell::new(preferences.name),
            page: RefCell::new(PageState::default()),
            chosen: RefCell::new(None),
            chosen_animated: RefCell::new(None),
            steam: RefCell::new(None),
            animated: RefCell::new(None),
            avatar: RefCell::new(None),
            busy: Cell::new(false),
            feedback: RefCell::new(String::new()),
        });
        drain.bind(&controller);
        if let Some(paths) = &controller.paths {
            let chosen_animated = paths.chosen_animated.clone();
            let (chosen, steam, animated) = (
                paths.chosen.clone(),
                paths.steam.clone(),
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
                    animated: profile::load_animation(&animated, profile::ANIMATED_SIDE),
                });
            });
        }
        // Steam pictures change: refresh the copy quietly in the background.
        if controller.uses_steam() {
            controller.fetch_steam(true);
        }
        controller.publish(ui);
        controller.bind_callbacks(ui);
        controller
    }

    fn bind_callbacks(self: &Rc<Self>, ui: &AppWindow) {
        let (weak, controller) = (ui.as_weak(), Rc::clone(self));
        ui.on_profile_activate(move |action| {
            if let Some(ui) = weak.upgrade() {
                controller.page.borrow_mut().selection = action;
                controller.activate(&ui, action);
            }
        });
        let (weak, controller) = (ui.as_weak(), Rc::clone(self));
        ui.on_profile_activate_picture(move |option| {
            if let Some(ui) = weak.upgrade() {
                controller.page.borrow_mut().picture_selection = option;
                controller.activate_picture(&ui, option);
            }
        });
        let (weak, controller) = (ui.as_weak(), Rc::clone(self));
        ui.on_profile_save_name(move |name| {
            if let Some(ui) = weak.upgrade() {
                controller.save_name(&ui, &name);
            }
        });
        let (weak, controller) = (ui.as_weak(), Rc::clone(self));
        ui.on_profile_cancel_name(move || {
            if let Some(ui) = weak.upgrade() {
                controller.close_editor(&ui);
            }
        });
    }

    /// A fresh visit to the Profile page: "Change picture", nothing open.
    pub fn on_enter(&self, ui: &AppWindow) {
        *self.page.borrow_mut() = PageState::default();
        self.feedback.borrow_mut().clear();
        self.publish(ui);
    }

    /// Input while the Profile page is shown. An open dialog takes everything
    /// but global actions; Back closes it.
    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) -> ProfileFeedback {
        let (editing, picture_open) = {
            let page = self.page.borrow();
            (page.editing_name, page.picture_open)
        };
        if editing {
            return self.editor_action(ui, event);
        }
        if picture_open {
            return self.picture_action(ui, event);
        }
        match event.action {
            UiAction::Left | UiAction::Right => {
                let delta = if event.action == UiAction::Left {
                    -1
                } else {
                    1
                };
                let mut page = self.page.borrow_mut();
                page.selection = step_with_edge_wrap(
                    page.selection,
                    CHANGE_PICTURE,
                    EDIT_NAME,
                    delta,
                    event.repeated,
                );
                drop(page);
                self.publish(ui);
                ProfileFeedback::Handled
            }
            // Nothing above or below the actions.
            UiAction::Up | UiAction::Down => ProfileFeedback::Handled,
            UiAction::Accept if !event.repeated => {
                let action = self.page.borrow().selection;
                self.activate(ui, action);
                ProfileFeedback::Ok
            }
            _ => ProfileFeedback::Ignored,
        }
    }

    /// Open what a page action opens.
    fn activate(&self, ui: &AppWindow, action: i32) {
        match action {
            CHANGE_PICTURE => {
                self.feedback.borrow_mut().clear();
                // Start on the first option that can be used.
                let first = self.picture_targets()[0];
                let mut page = self.page.borrow_mut();
                page.picture_open = true;
                page.picture_selection = first;
            }
            EDIT_NAME => {
                ui.set_profile_name_draft(self.name.borrow().clone().into());
                let mut page = self.page.borrow_mut();
                page.editing_name = true;
                page.editor_target = EDITOR_TEXT;
            }
            _ => {}
        }
        self.publish(ui);
    }

    /// The picture dialog's options that can be used now, in order; Done
    /// always can.
    fn picture_targets(&self) -> Vec<i32> {
        let busy = self.busy.get();
        let steam = self.steam_account.borrow().connected() || self.uses_steam();
        [
            (PICTURE_CHOOSE, !busy),
            (PICTURE_STEAM, steam && !busy),
            (PICTURE_REMOVE, self.has_chosen_picture() && !busy),
            (PICTURE_DONE, true),
        ]
        .into_iter()
        .filter_map(|(option, usable)| usable.then_some(option))
        .collect()
    }

    fn picture_action(&self, ui: &AppWindow, event: UiActionEvent) -> ProfileFeedback {
        match event.action {
            UiAction::Up | UiAction::Down => {
                let targets = self.picture_targets();
                let current = self.page.borrow().picture_selection;
                // Where the selection is now among the usable options (or
                // the nearest after it, if it just became unusable).
                let at = targets
                    .iter()
                    .position(|&option| option >= current)
                    .unwrap_or(targets.len() - 1) as i32;
                let delta = if event.action == UiAction::Up { -1 } else { 1 };
                let last = targets.len() as i32 - 1;
                let next = step_with_edge_wrap(at, 0, last, delta, event.repeated);
                self.page.borrow_mut().picture_selection = targets[next as usize];
                self.publish(ui);
                ProfileFeedback::Handled
            }
            UiAction::Left | UiAction::Right => ProfileFeedback::Handled,
            UiAction::Back => {
                self.close_picture(ui);
                ProfileFeedback::Back
            }
            UiAction::Accept if !event.repeated => {
                let option = self.page.borrow().picture_selection;
                if option == PICTURE_DONE {
                    self.close_picture(ui);
                    ProfileFeedback::Ok
                } else if self.activate_picture(ui, option) {
                    ProfileFeedback::Ok
                } else {
                    ProfileFeedback::Handled
                }
            }
            UiAction::Home | UiAction::Menu | UiAction::Forward => {
                self.close_picture(ui);
                ProfileFeedback::Ignored
            }
            _ => ProfileFeedback::Handled,
        }
    }

    /// Do what a picture option does. Returns whether it did something.
    fn activate_picture(&self, ui: &AppWindow, option: i32) -> bool {
        let done = match option {
            PICTURE_CHOOSE => self.choose(ui),
            PICTURE_STEAM => self.set_use_steam(ui, !self.uses_steam()),
            PICTURE_REMOVE => self.remove(ui),
            PICTURE_DONE => {
                self.close_picture(ui);
                true
            }
            _ => false,
        };
        // An option that just became unusable (a removed picture, Steam
        // turned off without an account) hands the selection on; one
        // that's only waiting on the work it started keeps it.
        if !self.busy.get() {
            let targets = self.picture_targets();
            let mut page = self.page.borrow_mut();
            if page.picture_open && !targets.contains(&page.picture_selection) {
                page.picture_selection = targets[0];
            }
        }
        self.publish(ui);
        done
    }

    fn close_picture(&self, ui: &AppWindow) {
        let mut page = self.page.borrow_mut();
        page.picture_open = false;
        page.selection = CHANGE_PICTURE;
        drop(page);
        self.feedback.borrow_mut().clear();
        self.publish(ui);
    }

    fn editor_action(&self, ui: &AppWindow, event: UiActionEvent) -> ProfileFeedback {
        let target = self.page.borrow().editor_target;
        let next = match (target, event.action) {
            (EDITOR_TEXT, UiAction::Down) => EDITOR_SAVE,
            (EDITOR_CANCEL | EDITOR_SAVE, UiAction::Up) => EDITOR_TEXT,
            (EDITOR_SAVE, UiAction::Left) => EDITOR_CANCEL,
            (EDITOR_CANCEL, UiAction::Right) => EDITOR_SAVE,
            (_, UiAction::Back) => {
                self.close_editor(ui);
                return ProfileFeedback::Back;
            }
            (EDITOR_CANCEL, UiAction::Accept) if !event.repeated => {
                self.close_editor(ui);
                return ProfileFeedback::Back;
            }
            (EDITOR_SAVE, UiAction::Accept) if !event.repeated => {
                let draft = ui.get_profile_name_draft();
                self.save_name(ui, &draft);
                return ProfileFeedback::Ok;
            }
            (_, UiAction::Home | UiAction::Menu | UiAction::Forward) => {
                self.close_editor(ui);
                return ProfileFeedback::Ignored;
            }
            _ => target,
        };
        if next != target {
            self.page.borrow_mut().editor_target = next;
            self.publish(ui);
        }
        ProfileFeedback::Handled
    }

    fn save_name(&self, ui: &AppWindow, draft: &str) {
        let name = profile::clean_name(draft);
        let changed = *self.name.borrow() != name;
        *self.name.borrow_mut() = name;
        if changed {
            self.save_preferences();
            *self.feedback.borrow_mut() = if self.name.borrow().is_empty() {
                "Name removed.".into()
            } else {
                "Name saved.".into()
            };
        }
        self.close_editor(ui);
    }

    fn close_editor(&self, ui: &AppWindow) {
        let mut page = self.page.borrow_mut();
        page.editing_name = false;
        page.editor_target = EDITOR_TEXT;
        drop(page);
        self.publish(ui);
    }

    /// The user's own chosen picture exists (so it can be removed).
    pub fn has_chosen_picture(&self) -> bool {
        self.chosen.borrow().is_some()
    }

    pub fn uses_steam(&self) -> bool {
        matches!(*self.source.borrow(), ProfileSource::Steam)
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
            self.say(
                ui,
                "Set up your Steam Account in Settings → Third-Party first.",
            );
            return false;
        }
        self.set_source(if on {
            ProfileSource::Steam
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
        let Some((path, animated_path)) = self
            .paths
            .as_ref()
            .map(|paths| (paths.steam.clone(), paths.animated.clone()))
        else {
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
                    // An animated avatar is a nicety: failing to get it
                    // keeps the last one.
                    let animated = match client.animated_avatar() {
                        Ok(item) => keep_item(&item, &animated_path, profile::ANIMATED_SIDE),
                        Err(error) => {
                            warn!(%error, "the Steam animated avatar was not fetched");
                            Update::Keep
                        }
                    };
                    Ok(SteamProfile { picture, animated })
                });
            let _ = sender.send(ProfileEvent::Steam { result, quiet });
        });
        if !started {
            self.busy.set(false);
        }
    }

    fn set_source(&self, source: ProfileSource) {
        *self.source.borrow_mut() = source;
        self.save_preferences();
    }

    fn save_preferences(&self) {
        let preferences = ProfilePreferences {
            name: self.name.borrow().clone(),
            source: self.source.borrow().clone(),
        };
        if let Some(paths) = &self.paths
            && let Err(error) = profile::save_preferences(&paths.settings, &preferences)
        {
            warn!(%error, "the profile could not be saved");
        }
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
                        Ok(SteamProfile { picture, animated }) => {
                            *self.steam.borrow_mut() = Some(slint_image(picture));
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
            ProfileSource::Steam => self
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

    /// Where the shown picture comes from, under the name.
    fn picture_caption(&self, has_picture: bool) -> &'static str {
        match &*self.source.borrow() {
            ProfileSource::Steam => "Steam profile picture",
            ProfileSource::Chosen if has_picture => "Picture from this device",
            ProfileSource::Avatar { .. } if has_picture => "Horizon avatar",
            _ => "No picture yet",
        }
    }

    fn publish(&self, ui: &AppWindow) {
        let current = self.current();
        ui.set_profile_picture_caption(self.picture_caption(current.is_some()).into());
        ui.set_profile_has_picture(current.is_some());
        ui.set_profile_picture(current.unwrap_or_default());
        ui.set_profile_page_has_chosen(self.has_chosen_picture());
        ui.set_profile_page_uses_steam(self.uses_steam());
        // Whichever picture is shown plays, if it moves.
        let frames = match &*self.source.borrow() {
            ProfileSource::Chosen => self
                .chosen_animated
                .borrow()
                .as_ref()
                .map(|moving| moving.frames.clone()),
            ProfileSource::Steam => self
                .animated
                .borrow()
                .as_ref()
                .map(|moving| moving.frames.clone()),
            ProfileSource::Avatar { .. } => None,
        };
        ui.set_profile_picture_frames(
            frames.unwrap_or_else(|| ModelRc::new(VecModel::<ProfileFrameData>::default())),
        );
        ui.set_profile_page_steam_available(self.steam_account.borrow().connected());
        ui.set_profile_page_busy(self.busy.get());
        ui.set_profile_page_feedback(self.feedback.borrow().clone().into());
        let name = self.name.borrow();
        ui.set_profile_name(name.as_str().into());
        ui.set_profile_monogram(profile::monogram(&name).into());
        let page = self.page.borrow();
        ui.set_profile_page_selection(page.selection);
        ui.set_profile_picture_open(page.picture_open);
        ui.set_profile_picture_selection(page.picture_selection);
        ui.set_profile_editing_name(page.editing_name);
        ui.set_profile_editor_target(page.editor_target);
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
