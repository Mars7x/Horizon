use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use slint::{ComponentHandle, Model};
use tracing::debug;

use crate::{
    AppRouteView, AppWindow,
    audio::UiSoundCue,
    input::{UiAction, UiActionEvent},
    navigation::{
        AppRoute, Navigator, PageLocation, RouteFocusMemory, SettingsPage, ShellFocus,
        ShellFocusRegion, TopUtility, UtilityPage, nearest_game_for_x, nearest_utility_for_x,
        utility_center_x,
    },
    presentation::{
        keyboard::{KeyboardController, KeyboardFeedback},
        profile::{ProfileController, ProfileFeedback},
    },
};

use super::{
    CallbackSlot,
    activity::ActivityDetailsActions,
    album::{AlbumController, AlbumFeedback},
    home::HomeController,
    library::LibraryController,
    settings::SettingsController,
};

/// Bridges pure Rust navigation/focus state to Slint presentation state.
///
/// Device adapters remain screen-agnostic. They emit `UiActionEvent`; this
/// controller decides whether an action is global navigation, shell focus, or
/// page-local behavior.
pub struct NavigationController {
    navigator: RefCell<Navigator>,
    restoring_history: Cell<bool>,
    focus: RefCell<ShellFocus>,
    focus_memory: RefCell<RouteFocusMemory>,
    home: Rc<HomeController>,
    library: Rc<LibraryController>,
    settings: Rc<SettingsController>,
    action_sound: CallbackSlot<dyn Fn(UiSoundCue)>,
    activity_on_enter: CallbackSlot<dyn Fn(&AppWindow)>,
    activity_details: CallbackSlot<dyn ActivityDetailsActions>,
    profile: RefCell<Option<Rc<ProfileController>>>,
    album: RefCell<Option<Rc<AlbumController>>>,
    keyboard: RefCell<Option<Rc<KeyboardController>>>,
}

impl NavigationController {
    pub fn new(
        ui: &AppWindow,
        home: Rc<HomeController>,
        library: Rc<LibraryController>,
        settings: Rc<SettingsController>,
    ) -> Rc<Self> {
        let controller = Rc::new(Self {
            navigator: RefCell::new(Navigator::default()),
            restoring_history: Cell::new(false),
            focus: RefCell::new(ShellFocus::default()),
            focus_memory: RefCell::new(RouteFocusMemory::default()),
            home,
            library,
            settings,
            action_sound: RefCell::new(None),
            activity_on_enter: RefCell::new(None),
            activity_details: RefCell::new(None),
            profile: RefCell::new(None),
            album: RefCell::new(None),
            keyboard: RefCell::new(None),
        });
        controller.publish_route(ui);
        controller.publish_focus(ui);
        controller.bind_ui_callbacks(ui);
        let weak = Rc::downgrade(&controller);
        controller
            .settings
            .set_location_changed(Rc::new(move |page| {
                if let Some(controller) = weak.upgrade() {
                    controller.record_page(AppRoute::Utility(UtilityPage::Settings), page);
                }
            }));
        let weak = Rc::downgrade(&controller);
        let weak_ui = ui.as_weak();
        ui.on_settings_back(move || {
            if let (Some(controller), Some(ui)) = (weak.upgrade(), weak_ui.upgrade())
                && controller.current_route() == AppRoute::Utility(UtilityPage::Settings)
            {
                controller.handle_action(&ui, UiActionEvent::fresh(UiAction::Back));
            }
        });
        controller
    }

    /// Also given to the pages whose clicks play their own sounds; call it
    /// after they are installed.
    pub fn set_action_sound(&self, callback: Rc<dyn Fn(UiSoundCue)>) {
        self.settings.set_action_sound(Rc::clone(&callback));
        self.home.set_sound(Rc::clone(&callback));
        if let Some(profile) = self.profile.borrow().as_ref() {
            profile.set_sound(Rc::clone(&callback));
        }
        *self.action_sound.borrow_mut() = Some(callback);
    }

    /// Refresh Activity's visible data before publishing the new page route.
    pub fn set_activity_on_enter(&self, callback: Rc<dyn Fn(&AppWindow)>) {
        *self.activity_on_enter.borrow_mut() = Some(callback);
    }

    /// The on-screen keyboard: while open, it takes input before any page.
    pub fn set_keyboard(&self, keyboard: Rc<KeyboardController>) {
        *self.keyboard.borrow_mut() = Some(keyboard);
    }

    /// The Profile page, opened by clicking Home's avatar (or A on it).
    pub fn set_profile(self: &Rc<Self>, controller: Rc<ProfileController>, ui: &AppWindow) {
        *self.profile.borrow_mut() = Some(controller);
        let weak = ui.as_weak();
        let navigation = Rc::clone(self);
        ui.on_profile_open(move || {
            if let Some(ui) = weak.upgrade() {
                navigation.open_profile(&ui);
            }
        });
    }

    fn open_profile(&self, ui: &AppWindow) {
        if self.current_route() == AppRoute::Profile {
            return;
        }
        self.cue(UiSoundCue::Ok);
        self.navigate_to(ui, AppRoute::Profile);
    }

    /// Pointer callbacks act only while Album is the current route, so a
    /// page that is zooming away cannot be operated.
    pub fn set_album(self: &Rc<Self>, controller: Rc<AlbumController>, ui: &AppWindow) {
        let weak = Rc::downgrade(self);
        controller.set_location_changed(Rc::new(move |page| {
            if let Some(nav) = weak.upgrade() {
                nav.record_page(AppRoute::Utility(UtilityPage::Album), page);
            }
        }));
        *self.album.borrow_mut() = Some(Rc::clone(&controller));
        let on_album = |navigation: &Rc<Self>| {
            navigation.current_route() == AppRoute::Utility(UtilityPage::Album)
        };
        let (weak, album, navigation) = (ui.as_weak(), Rc::clone(&controller), Rc::clone(self));
        ui.on_album_choose(move |index| {
            if let Some(ui) = weak.upgrade()
                && on_album(&navigation)
                && album.choose(&ui, index)
            {
                navigation.cue(UiSoundCue::Ok);
            }
        });
        let (weak, album, navigation) = (ui.as_weak(), Rc::clone(&controller), Rc::clone(self));
        ui.on_album_cycle_game(move || {
            if let Some(ui) = weak.upgrade()
                && on_album(&navigation)
            {
                navigation.cue(UiSoundCue::Ok);
                album.cycle_game(&ui);
            }
        });

        let (weak, album, navigation) = (ui.as_weak(), Rc::clone(&controller), Rc::clone(self));
        ui.on_album_scroll_rows(move |rows| {
            if let Some(ui) = weak.upgrade()
                && on_album(&navigation)
            {
                album.scroll_rows(&ui, rows);
            }
        });
        let (weak, album, navigation) = (ui.as_weak(), Rc::clone(&controller), Rc::clone(self));
        ui.on_album_step(move |direction| {
            if let Some(ui) = weak.upgrade()
                && on_album(&navigation)
            {
                let before = ui.get_album_slide_key();
                album.step(&ui, direction);
                if ui.get_album_slide_key() != before {
                    navigation.cue(UiSoundCue::Navigation);
                }
            }
        });

        let (weak, album, navigation) = (ui.as_weak(), Rc::clone(&controller), Rc::clone(self));
        ui.on_album_close_viewer(move || {
            if let Some(ui) = weak.upgrade()
                && on_album(&navigation)
                && album.close_viewer(&ui)
            {
                navigation.cue(UiSoundCue::Back);
            }
        });
        let (weak, album, navigation) = (ui.as_weak(), Rc::clone(&controller), Rc::clone(self));
        ui.on_album_dialog_choose(move |choice| {
            if let Some(ui) = weak.upgrade()
                && on_album(&navigation)
            {
                navigation.album_feedback(album.choose_in_dialog(&ui, choice));
            }
        });
        let (weak, album) = (ui.as_weak(), controller);
        ui.on_album_layout_changed(move || {
            if let Some(ui) = weak.upgrade() {
                album.on_layout_changed(&ui);
            }
        });
    }

    /// Route-local lifecycle for pages that hold resources (Album images).
    fn leave_route(&self, ui: &AppWindow, from: AppRoute) {
        if from == AppRoute::Utility(UtilityPage::Settings) {
            self.settings.on_leave(ui);
        }
        if from == AppRoute::Utility(UtilityPage::Album)
            && let Some(album) = self.album.borrow().as_ref()
        {
            album.on_leave(ui);
        }
    }

    pub fn set_activity_details(self: &Rc<Self>, actions: Rc<dyn ActivityDetailsActions>) {
        let weak = Rc::downgrade(self);
        actions.set_location_changed(Rc::new(move |page| {
            if let Some(nav) = weak.upgrade() {
                nav.record_page(AppRoute::Utility(UtilityPage::Activity), page);
            }
        }));
        *self.activity_details.borrow_mut() = Some(actions);
    }

    /// Entering Activity through either a direct route or the Back stack is a
    /// new visit. Nested detail Back is not: that path preserves the cover.
    fn prepare_activity_visit(&self, ui: &AppWindow) {
        // Leaving Activity through Home/Menu can retain its nested details UI.
        // A new visit must show the overview, not that previous game's history.
        if ui.get_activity_details_visible()
            && let Some(details) = self.activity_details.borrow().as_ref()
        {
            details.close(ui);
        }
        ui.set_activity_pressed_index(-1);
        ui.set_activity_selected_index(0);
        if let Some(refresh) = self.activity_on_enter.borrow().as_ref() {
            refresh(ui);
        }
        // A reordered most-played list may restore the former first GameId
        // during refresh. Force the NEW first cover for every new visit.
        ui.set_activity_selected_index(0);
    }

    fn album_feedback(&self, feedback: AlbumFeedback) {
        match feedback {
            AlbumFeedback::Ok => self.cue(UiSoundCue::Ok),
            AlbumFeedback::Back => self.cue(UiSoundCue::Back),
            AlbumFeedback::None => {}
        }
    }

    fn cue(&self, cue: UiSoundCue) {
        if let Some(callback) = self.action_sound.borrow().as_ref() {
            callback(cue);
        }
    }

    pub fn current_route(&self) -> AppRoute {
        self.navigator.borrow().current()
    }

    /// Entry point for shell/header callbacks. Route policy, history, and
    /// route-local shell-focus restoration remain Rust-owned.
    pub fn navigate_to(&self, ui: &AppWindow, route: AppRoute) {
        let from = self.current_route();
        if route == from {
            return;
        }

        self.remember_focus_for_route(from);
        if self.navigator.borrow_mut().navigate_to(route) {
            debug!(?from, to = ?route, "route changed");
            self.open_route(ui, from, route);
        }
    }

    /// Mouse forward button: reopen the page Back last left, as a fresh
    /// visit, exactly as if it had been opened again.
    fn record_page(&self, route: AppRoute, page: PageLocation) {
        if !self.restoring_history.get() && self.current_route() == route {
            self.navigator.borrow_mut().record_page(page);
        }
    }
    fn restore_history(&self, ui: &AppWindow, from: AppRoute) {
        let to = self.current_route();
        let page = self.navigator.borrow().page();
        self.restoring_history.set(true);
        if from != to {
            self.open_route(ui, from, to);
        }
        match page {
            PageLocation::Settings(view) => self.settings.restore_page(ui, view),
            PageLocation::Activity(game) => {
                if let Some(details) = self.activity_details.borrow().as_ref() {
                    details.restore(ui, game);
                }
            }
            PageLocation::Album(path) => {
                if let Some(album) = self.album.borrow().as_ref() {
                    album.restore_viewer(ui, &path);
                }
            }
            PageLocation::Root => match to {
                AppRoute::Utility(UtilityPage::Settings) => {
                    self.settings.restore_page(ui, SettingsPage::Root)
                }
                AppRoute::Utility(UtilityPage::Activity) => {
                    if let Some(details) = self.activity_details.borrow().as_ref() {
                        details.close(ui);
                    }
                }
                AppRoute::Utility(UtilityPage::Album) => {
                    if let Some(album) = self.album.borrow().as_ref() {
                        album.close_viewer(ui);
                    }
                }
                _ => {}
            },
        }
        self.restoring_history.set(false);
    }

    fn handle_forward(&self, ui: &AppWindow) {
        let from = self.current_route();
        self.remember_focus_for_route(from);
        if self.navigator.borrow_mut().go_forward() {
            let to = self.current_route();
            debug!(?from, ?to, "Forward reopened the route Back left");
            self.cue(UiSoundCue::Ok);
            self.restore_history(ui, from);
        } else {
            debug!(route = ?from, "Forward ignored with nothing to reopen");
        }
    }

    /// Leave `from` and enter `route` as a fresh visit (after the navigator
    /// has already moved there).
    fn open_route(&self, ui: &AppWindow, from: AppRoute, route: AppRoute) {
        self.leave_route(ui, from);
        if route == AppRoute::Utility(UtilityPage::Album)
            && let Some(album) = self.album.borrow().as_ref()
        {
            album.start_new_visit(ui);
        }
        if route == AppRoute::Utility(UtilityPage::Settings) {
            self.settings.on_enter(ui);
        }
        if route == AppRoute::Library {
            self.library.on_enter(ui);
        }
        if route == AppRoute::Utility(UtilityPage::Activity) {
            self.prepare_activity_visit(ui);
        }
        if route == AppRoute::Profile
            && let Some(profile) = self.profile.borrow().as_ref()
        {
            profile.on_enter(ui);
        }
        self.publish_route_change(ui, from, route);
        self.restore_focus_for_route(ui, route);
    }

    pub fn handle_action(&self, ui: &AppWindow, event: UiActionEvent) {
        if event.repeated && event.action.is_global() {
            debug!(action = ?event.action, "repeated global action ignored by navigation layer");
            return;
        }
        let keyboard = self
            .keyboard
            .borrow()
            .as_ref()
            .map_or(KeyboardFeedback::Ignored, |keyboard| {
                keyboard.handle_action(ui, event)
            });
        if keyboard == KeyboardFeedback::Handled {
            return;
        }
        if self.current_route() == AppRoute::Profile {
            let feedback = self
                .profile
                .borrow()
                .as_ref()
                .map_or(ProfileFeedback::Ignored, |profile| {
                    profile.handle_action(ui, event)
                });
            match feedback {
                ProfileFeedback::Ignored => {}
                ProfileFeedback::Handled => return,
                ProfileFeedback::Ok => {
                    self.cue(UiSoundCue::Ok);
                    return;
                }
                ProfileFeedback::Back => {
                    self.cue(UiSoundCue::Back);
                    return;
                }
            }
        }
        if self.current_route() == AppRoute::Utility(UtilityPage::Album)
            && event.action == UiAction::Forward
            && self
                .album
                .borrow()
                .as_ref()
                .is_some_and(|album| album.has_transient())
        {
            return;
        }
        if self.current_route() == AppRoute::Utility(UtilityPage::Album)
            && event.action == UiAction::Back
            && self
                .album
                .borrow()
                .as_ref()
                .is_some_and(|album| album.has_transient() && album.back(ui))
        {
            self.cue(UiSoundCue::Back);
            return;
        }
        if self.current_route() == AppRoute::Utility(UtilityPage::Settings)
            && !self.settings.has_transient(ui)
        {
            if event.action == UiAction::Forward {
                self.handle_forward(ui);
                return;
            }
            if event.action == UiAction::Back
                && self.navigator.borrow().page() != PageLocation::Root
            {
                self.handle_back(ui);
                return;
            }
        }
        if self.current_route() == AppRoute::Utility(UtilityPage::Settings)
            && self.settings.handle_action(ui, event)
        {
            return;
        }
        match event.action {
            UiAction::Back => self.handle_back(ui),
            UiAction::Home => self.handle_home(ui),
            UiAction::Forward => self.handle_forward(ui),
            UiAction::Menu => {}
            UiAction::Accept
                if !event.repeated
                    && self.current_route() == AppRoute::Home
                    && self.focus.borrow().region() != ShellFocusRegion::TopUtilities
                    && self.home.library_tile_selected() =>
            {
                self.open_library(ui);
            }
            _ if self.current_route().uses_shell_chrome()
                && self.focus.borrow().region() == ShellFocusRegion::TopUtilities =>
            {
                self.handle_utility_action(ui, event)
            }
            UiAction::Up if self.current_route().uses_shell_chrome() => {
                let changed = if self.current_route() == AppRoute::Home {
                    let selected_index = self.home.selected_index();
                    let anchored_utility = self
                        .focus
                        .borrow_mut()
                        .anchored_utility_for_content(selected_index);

                    let utility = anchored_utility.unwrap_or_else(|| {
                        let game_center_x = ui.get_home_first_game_center_x_px()
                            + selected_index as f32 * ui.get_home_game_stride_px();
                        nearest_utility_for_x(
                            game_center_x,
                            ui.get_logical_viewport_width_px(),
                            ui.get_utility_center_step_px(),
                        )
                    });

                    self.focus
                        .borrow_mut()
                        .enter_utilities_from_content(utility, selected_index)
                } else {
                    self.focus.borrow_mut().enter_utilities()
                };

                if changed {
                    debug!(
                        utility = ?self.focus.borrow().utility(),
                        "focus entered top utilities by reciprocal anchor or shortest center distance"
                    );
                    self.publish_focus(ui);
                }
            }
            _ => self.dispatch_to_active_page(ui, event),
        }
    }

    fn bind_ui_callbacks(self: &Rc<Self>, ui: &AppWindow) {
        let ui_weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_activate_utility(move |index| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let Some(utility) = TopUtility::from_index(index) else {
                return;
            };

            controller.focus.borrow_mut().select_utility(utility);
            controller.publish_focus(&ui);
            controller.activate_utility(&ui, utility);
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_open_library(move || {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Home
            {
                controller.open_library(&ui);
            }
        });

        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_select_game(move |index| {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Home
            {
                controller.focus.borrow_mut().leave_utilities();
                controller.publish_focus(&ui);
                controller.home.activate_from_pointer(&ui, index);
            }
        });

        // Pointer activation is routed through the shell too, so it restores
        // content focus and cannot operate a retained/outgoing Library page.
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_library_select(move |index| {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Library
            {
                controller.focus.borrow_mut().leave_utilities();
                controller.publish_focus(&ui);
                let before = ui.get_library_selected_index();
                controller.library.select_on_page(&ui, index);
                if ui.get_library_selected_index() != before {
                    controller.cue(UiSoundCue::Navigation);
                }
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_library_cycle_filter(move || {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Library
            {
                controller.focus.borrow_mut().leave_utilities();
                controller.publish_focus(&ui);
                controller.cue(UiSoundCue::Ok);
                controller.library.cycle_filter(&ui);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_library_cycle_sort(move || {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Library
            {
                controller.focus.borrow_mut().leave_utilities();
                controller.publish_focus(&ui);
                controller.cue(UiSoundCue::Ok);
                controller.library.cycle_sort(&ui);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_library_scroll_rows(move |direction| {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Library
            {
                controller.library.scroll_by_row(&ui, direction);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_library_back(move || {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Library
            {
                controller.handle_back(&ui);
            }
        });
        // Activity has exactly one controller focus region (the cover strip).
        // Charts and the Milestones placeholder never capture navigation.
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_activity_choose(move |index| {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Utility(UtilityPage::Activity)
            {
                let len = ui.get_activity_covers().row_count() as i32;
                if index >= 0 && index < len && !ui.get_activity_details_visible() {
                    ui.set_activity_selected_index(index);
                    controller.begin_activity_details_open(&ui, index as usize);
                }
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_activity_detail_back(move || {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Utility(UtilityPage::Activity)
                && ui.get_activity_details_visible()
            {
                controller.handle_back(&ui);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_activity_detail_layout_changed(move || {
            if let Some(ui) = weak.upgrade()
                && let Some(details) = controller.activity_details.borrow().as_ref()
            {
                details.refresh_visible_rows(&ui);
            }
        });
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_activity_detail_scroll(move |direction| {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Utility(UtilityPage::Activity)
                && ui.get_activity_details_visible()
                && let Some(details) = controller.activity_details.borrow().as_ref()
            {
                details.scroll_rows(&ui, direction);
            }
        });
        // Each move closure takes ownership of its own Weak/AppWindow and Rc.
        let weak = ui.as_weak();
        let controller = Rc::clone(self);
        ui.on_library_layout_changed(move || {
            if let Some(ui) = weak.upgrade()
                && controller.current_route() == AppRoute::Library
            {
                controller.library.on_viewport_changed(&ui);
            }
        });
    }

    fn move_activity_selection(&self, ui: &AppWindow, delta: i32) {
        let len = ui.get_activity_covers().row_count();
        if len == 0 {
            return;
        }
        let last = len.saturating_sub(1).min(i32::MAX as usize) as i32;
        let next = ui
            .get_activity_selected_index()
            .saturating_add(delta)
            .clamp(0, last);
        if next != ui.get_activity_selected_index() {
            ui.set_activity_pressed_index(-1);
        }
        ui.set_activity_selected_index(next);
    }

    fn open_library(&self, ui: &AppWindow) {
        if self.current_route() == AppRoute::Library {
            return;
        }
        self.cue(UiSoundCue::Ok);
        self.navigate_to(ui, AppRoute::Library);
    }

    fn handle_back(&self, ui: &AppWindow) {
        let from = self.current_route();
        if from == AppRoute::Utility(UtilityPage::Activity)
            && ui.get_activity_details_visible()
            && self.navigator.borrow().page() == PageLocation::Root
        {
            if let Some(details) = self.activity_details.borrow().as_ref() {
                details.close(ui);
                self.cue(UiSoundCue::Back);
            }
            return;
        }
        self.remember_focus_for_route(from);
        let changed = self.navigator.borrow_mut().go_back();
        if changed {
            self.cue(UiSoundCue::Back);
            self.restore_history(ui, from);
        }
    }

    fn handle_home(&self, ui: &AppWindow) {
        let from = self.current_route();
        self.remember_focus_for_route(from);

        let home_snapshot = self
            .focus_memory
            .borrow()
            .recall(AppRoute::Home)
            .as_content();
        self.focus.borrow_mut().restore(home_snapshot);
        self.focus_memory
            .borrow_mut()
            .remember(AppRoute::Home, home_snapshot);
        self.home.reset_for_global_home(ui);
        self.publish_focus(ui);

        if self.navigator.borrow_mut().go_home() {
            debug!(
                ?from,
                to = ?AppRoute::Home,
                "Home reset top-level navigation, content focus, and first-game selection"
            );
            self.leave_route(ui, from);
            self.publish_route_change(ui, from, AppRoute::Home);
        }
    }

    fn handle_utility_action(&self, ui: &AppWindow, event: UiActionEvent) {
        match event.action {
            UiAction::Left | UiAction::Right => {
                let delta = if event.action == UiAction::Left {
                    -1
                } else {
                    1
                };
                if self.focus.borrow_mut().move_utility(delta) {
                    debug!(utility = ?self.focus.borrow().utility(), "top utility focus moved");
                    self.publish_focus(ui);
                }
            }
            UiAction::Down => {
                let changed = if self.current_route() == AppRoute::Home {
                    let (utility, exact_return_index) = {
                        let focus = self.focus.borrow();
                        (focus.utility(), focus.content_anchor_index())
                    };

                    let game_index = exact_return_index.unwrap_or_else(|| {
                        let utility_x = utility_center_x(
                            utility,
                            ui.get_logical_viewport_width_px(),
                            ui.get_utility_center_step_px(),
                        );
                        nearest_game_for_x(
                            utility_x,
                            ui.get_home_first_game_center_x_px(),
                            ui.get_home_game_stride_px(),
                            self.home.game_count(),
                        )
                    });

                    self.home.select_from_shell(ui, game_index);
                    self.focus
                        .borrow_mut()
                        .leave_utilities_to_content(game_index)
                } else {
                    self.focus.borrow_mut().leave_utilities()
                };

                if changed {
                    debug!(
                        "focus returned to page content with reciprocal anchor or spatial transfer"
                    );
                    self.publish_focus(ui);
                }
            }
            UiAction::Accept if !event.repeated => {
                if self.focus.borrow().profile() {
                    self.open_profile(ui);
                } else {
                    let utility = self.focus.borrow().utility();
                    self.activate_utility(ui, utility);
                }
            }
            UiAction::Up
            | UiAction::Accept
            | UiAction::LeftBumper
            | UiAction::RightBumper
            | UiAction::Secondary
            | UiAction::AlternateSecondary
            | UiAction::StickPress
            | UiAction::RightStickPress => {
                debug!(
                    action = ?event.action,
                    repeated = event.repeated,
                    "action has no top-utility behavior"
                );
            }
            UiAction::Back | UiAction::Forward | UiAction::Menu | UiAction::Home => {
                unreachable!("global actions handled first")
            }
        }
    }

    fn activate_utility(&self, ui: &AppWindow, utility: TopUtility) {
        let route = utility.route();
        let from = self.current_route();
        if route == from {
            return;
        }
        // The destination is a known valid utility route. Dispatch OK before
        // synchronous on_enter() and route publishing, not after them.
        self.cue(UiSoundCue::Ok);
        self.navigate_to(ui, route);
        if self.current_route() != from {
            debug!(?from, to = ?route, ?utility, "utility submenu opened");
        }
    }

    /// Share the same Home-like press-in for controller Accept and pointer
    /// activation. Wait just long enough for the selected cover to visibly
    /// depress before the details rise begins.
    fn begin_activity_details_open(&self, ui: &AppWindow, index: usize) {
        if self.current_route() != AppRoute::Utility(UtilityPage::Activity)
            || ui.get_activity_details_visible()
            || ui.get_activity_pressed_index() >= 0
            || index >= ui.get_activity_covers().row_count()
        {
            return;
        }

        // Reduced Motion skips both the press timing and the visual transition.
        if ui.get_activity_reduced_motion() {
            if let Some(details) = self.activity_details.borrow().as_ref()
                && details.open(ui, index)
            {
                self.cue(UiSoundCue::Ok);
            }
            return;
        }

        ui.set_activity_pressed_index(index as i32);
        let weak_ui = ui.as_weak();
        let details = self.activity_details.borrow().as_ref().cloned();
        let cue = self.action_sound.borrow().as_ref().cloned();
        slint::Timer::single_shot(Duration::from_millis(125), move || {
            let Some(ui) = weak_ui.upgrade() else { return };
            if ui.get_activity_pressed_index() != index as i32 {
                return;
            }
            ui.set_activity_pressed_index(-1);
            // An earlier Back/Home/route change cancels the pending press.
            if ui.get_current_route() != AppRouteView::Activity
                || ui.get_activity_details_visible()
                || ui.get_activity_selected_index() != index as i32
            {
                return;
            }
            if let Some(details) = details.as_ref()
                && details.open(&ui, index)
                && let Some(cue) = cue.as_ref()
            {
                cue(UiSoundCue::Ok);
            }
        });
    }

    fn dispatch_to_active_page(&self, ui: &AppWindow, event: UiActionEvent) {
        match self.current_route() {
            AppRoute::Home => self.home.handle_action(ui, event),
            AppRoute::Library => self.library.handle_action(ui, event),
            // No achievement provider is configured yet; Back remains global.
            AppRoute::Utility(UtilityPage::Achievements) => {}
            AppRoute::Utility(UtilityPage::Album) => {
                let feedback = self
                    .album
                    .borrow()
                    .as_ref()
                    .map_or(AlbumFeedback::None, |album| album.handle_action(ui, event));
                self.album_feedback(feedback);
            }
            AppRoute::Utility(UtilityPage::Activity) => {
                if ui.get_activity_details_visible() {
                    if let Some(details) = self.activity_details.borrow().as_ref() {
                        match event.action {
                            UiAction::Up => details.scroll_rows(ui, -1),
                            UiAction::Down => details.scroll_rows(ui, 1),
                            _ => {}
                        }
                    }
                } else {
                    match event.action {
                        UiAction::Left => self.move_activity_selection(ui, -1),
                        UiAction::Right => self.move_activity_selection(ui, 1),
                        UiAction::Accept if !event.repeated => {
                            self.begin_activity_details_open(
                                ui,
                                ui.get_activity_selected_index().max(0) as usize,
                            );
                        }
                        _ => {}
                    }
                }
            }
            route => {
                debug!(
                    ?route,
                    action = ?event.action,
                    repeated = event.repeated,
                    "page-local action deferred until its controller is introduced"
                );
            }
        }
    }

    fn remember_focus_for_route(&self, route: AppRoute) {
        self.focus_memory
            .borrow_mut()
            .remember(route, self.focus.borrow().snapshot());
    }

    fn restore_focus_for_route(&self, ui: &AppWindow, route: AppRoute) {
        let snapshot = self.focus_memory.borrow().recall(route);
        self.focus.borrow_mut().restore(snapshot);
        self.publish_focus(ui);
    }

    fn publish_route(&self, ui: &AppWindow) {
        let route = self.current_route();
        ui.set_transition_from_route(route_view(route));
        ui.set_current_route(route_view(route));
    }

    /// Publish exactly one outgoing route for the visual transition layer.
    /// Older outgoing pages are dropped immediately when rapid navigation
    /// retargets the shell, preventing several half-faded pages from stacking.
    fn publish_route_change(&self, ui: &AppWindow, from: AppRoute, to: AppRoute) {
        if from == AppRoute::Utility(UtilityPage::Activity)
            && from != to
            && let Some(details) = self.activity_details.borrow().as_ref()
        {
            details.close(ui);
        }
        ui.set_transition_from_route(route_view(from));
        ui.set_current_route(route_view(to));
        if to == AppRoute::Home && from != AppRoute::Home {
            self.home.reveal_row_changes(ui);
        }
    }

    fn publish_focus(&self, ui: &AppWindow) {
        let focus = *self.focus.borrow();
        ui.set_top_utilities_focused(focus.region() == ShellFocusRegion::TopUtilities);
        // On the avatar, no utility shows focus.
        ui.set_profile_focused(focus.profile());
        ui.set_focused_utility_index(if focus.profile() {
            -1
        } else {
            focus.utility().index()
        });
    }
}

fn route_view(route: AppRoute) -> AppRouteView {
    match route {
        AppRoute::Home => AppRouteView::Home,
        AppRoute::Library => AppRouteView::Library,
        AppRoute::Utility(UtilityPage::Album) => AppRouteView::Album,
        AppRoute::Utility(UtilityPage::Activity) => AppRouteView::Activity,
        AppRoute::Utility(UtilityPage::Achievements) => AppRouteView::Achievements,
        AppRoute::Utility(UtilityPage::Settings) => AppRouteView::Settings,
        AppRoute::Profile => AppRouteView::Profile,
    }
}

#[cfg(test)]
mod tests {
    use super::route_view;
    use crate::{
        AppRouteView,
        navigation::{AppRoute, UtilityPage},
    };

    #[test]
    fn every_rust_route_has_an_explicit_slint_view_mapping() {
        assert!(matches!(route_view(AppRoute::Home), AppRouteView::Home));
        assert!(matches!(
            route_view(AppRoute::Library),
            AppRouteView::Library
        ));

        assert!(route_view(AppRoute::Utility(UtilityPage::Album)) == AppRouteView::Album);
        assert!(route_view(AppRoute::Utility(UtilityPage::Activity)) == AppRouteView::Activity);
        assert!(
            route_view(AppRoute::Utility(UtilityPage::Achievements)) == AppRouteView::Achievements
        );
        assert!(route_view(AppRoute::Utility(UtilityPage::Settings)) == AppRouteView::Settings);
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;
    use crate::{
        appearance::store::AppearanceStore,
        persistence::{SqliteLibraryRepository, settings::SettingsStore},
        services::{
            activity::{ActivityRepository, ActivityService},
            artwork::ArtworkService,
            launch::{GameLaunchService, LaunchExecutionError, LaunchExecutor},
            library::LibraryRepository,
            rom_import::{self, RomSetup},
            settings::SettingsService,
            steam_account::SteamAccountService,
        },
    };
    struct CountLaunch(Rc<Cell<usize>>);
    impl LaunchExecutor for CountLaunch {
        fn execute(
            &self,
            _: &crate::sources::SourceLaunchTarget,
        ) -> Result<(), LaunchExecutionError> {
            self.0.set(self.0.get() + 1);
            Ok(())
        }
    }
    #[test]
    #[ignore = "requires Wayland and a fresh HORIZON_NAVIGATION_TEST_DIR"]
    fn mouse_history_drag_click_and_crisp_album() {
        let directory =
            std::path::PathBuf::from(std::env::var_os("HORIZON_NAVIGATION_TEST_DIR").unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("homebrew.gba"),
            crate::platform::gba_core::test_rom(),
        )
        .unwrap();
        let mut repository =
            SqliteLibraryRepository::open(directory.join("library.sqlite3")).unwrap();
        let scan = rom_import::scan_folder(&directory, None).unwrap();
        repository
            .save_rom_import(&RomSetup::default(), &scan.candidates)
            .unwrap();
        let games = repository.list_games().unwrap();
        let game = games[0].game().id();
        let now = chrono::Utc::now().timestamp();
        let session = repository
            .begin_play_session(
                game,
                &crate::domain::SourceId::new("gba").unwrap(),
                now - 120,
                crate::domain::SessionTrackingMethod::SourceRuntime,
            )
            .unwrap();
        repository.complete_play_session(session, now - 60).unwrap();
        let ui = AppWindow::new().unwrap();
        let registry = Rc::new(crate::sources::production_source_registry().unwrap());
        let catalog = crate::presentation::catalog::Catalog::shared(games.clone());
        let activity = Rc::new(ActivityService::new(Rc::new(RefCell::new(repository))));
        let launches = Rc::new(Cell::new(0));
        let launcher = Rc::new(GameLaunchService::new(
            Rc::clone(&registry),
            Rc::new(CountLaunch(Rc::clone(&launches))),
        ));
        let home = HomeController::new(
            &ui,
            Rc::clone(&catalog),
            &[game],
            &ArtworkService::new(Rc::clone(&registry), None, None),
            &registry,
            launcher,
            activity.clone(),
        );
        let library = LibraryController::new(
            &ui,
            Rc::clone(&catalog),
            Rc::clone(&registry),
            Rc::clone(&home),
            activity.clone(),
        );
        let appearance = crate::presentation::appearance::AppearanceController::new(
            &ui,
            AppearanceStore::new(directory.join("appearance.json")),
        )
        .unwrap();
        let settings = SettingsController::new(
            &ui,
            SettingsService::load(SettingsStore::new(directory.join("artwork.json"))).unwrap(),
            appearance,
            Rc::new(RefCell::new(
                SteamAccountService::load(directory.join("steam.json")).unwrap(),
            )),
            directory.join("controller.json"),
            crate::platform::data_paths::CoreSettingsPaths {
                playback: directory.join("playback.json"),
                mgba: directory.join("cores/mgba.json"),
                ds: directory.join("cores/melonds-ds.json"),
            },
        );
        let navigation = NavigationController::new(&ui, home, library, settings);
        let showcase = crate::presentation::activity::ActivityShowcaseController::new(
            &ui,
            catalog,
            Rc::clone(&navigation.home),
        );
        let overview = activity.overview(10, 10).unwrap();
        showcase.refresh(&ui, &overview);
        let details =
            crate::presentation::activity::ActivityDetailsController::new(activity, showcase);
        navigation.set_activity_details(details);
        let album_dir = directory.join("album");
        std::fs::create_dir_all(album_dir.join("horizon")).unwrap();
        let pixels = image::RgbaImage::from_fn(32, 16, |x, y| {
            image::Rgba(if (x + y) % 2 == 0 {
                [255, 0, 255, 255]
            } else {
                [0, 255, 0, 255]
            })
        });
        pixels.save(album_dir.join("horizon/one.png")).unwrap();
        pixels.save(album_dir.join("horizon/two.png")).unwrap();
        let album = AlbumController::new(
            &ui,
            &games,
            Some(album_dir),
            Vec::new(),
            Some(directory.join("cache")),
        );
        navigation.set_album(Rc::clone(&album), &ui);
        ui.global::<crate::Motion>().set_reduced_motion(true);
        ui.show().unwrap();
        let stage = Rc::new(Cell::new(0));
        let count = Rc::new(Cell::new(0));
        let progress = Rc::clone(&stage);
        let ticks = Rc::clone(&count);
        let weak = ui.as_weak();
        let output = directory.clone();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(250),
            move || {
                let ui = weak.upgrade().unwrap();
                ticks.set(ticks.get() + 1);
                assert!(
                    ticks.get() < 60,
                    "navigation runtime timed out at {}",
                    progress.get()
                );
                let back = || navigation.handle_action(&ui, UiActionEvent::fresh(UiAction::Back));
                let forward =
                    || navigation.handle_action(&ui, UiActionEvent::fresh(UiAction::Forward));
                match progress.get() {
                    0 => {
                        navigation.navigate_to(&ui, AppRoute::Utility(UtilityPage::Settings));
                        ui.invoke_settings_activate(3);
                        ui.invoke_settings_activate(0);
                        assert_eq!(ui.get_settings_view(), 7);
                        back();
                        assert_eq!(ui.get_settings_view(), 6);
                        back();
                        assert_eq!(ui.get_settings_view(), 0);
                        back();
                        assert_eq!(navigation.current_route(), AppRoute::Home);
                        forward();
                        assert_eq!(ui.get_settings_view(), 0);
                        forward();
                        assert_eq!(ui.get_settings_view(), 6);
                        forward();
                        assert_eq!(ui.get_settings_view(), 7);
                        ui.invoke_settings_back();
                        assert_eq!(ui.get_settings_view(), 6);
                        forward();
                        assert_eq!(ui.get_settings_view(), 7);
                        back();
                        back();
                        ui.invoke_settings_activate(2);
                        assert_eq!(ui.get_settings_view(), 2);
                        forward();
                        assert_eq!(ui.get_settings_view(), 2);
                        back();
                        forward();
                        assert_eq!(ui.get_settings_view(), 2);
                        navigation.handle_home(&ui);
                        navigation.handle_action(&ui, UiActionEvent::fresh(UiAction::Menu));
                        assert_eq!(navigation.current_route(), AppRoute::Home);
                        navigation.navigate_to(&ui, AppRoute::Utility(UtilityPage::Activity));
                        navigation.begin_activity_details_open(&ui, 0);
                        assert!(ui.get_activity_details_visible());
                        back();
                        assert!(!ui.get_activity_details_visible());
                        forward();
                        assert!(ui.get_activity_details_visible());
                        back();
                        back();
                        assert_eq!(navigation.current_route(), AppRoute::Home);
                        forward();
                        forward();
                        assert!(ui.get_activity_details_visible());
                        navigation.navigate_to(&ui, AppRoute::Utility(UtilityPage::Album));
                        progress.set(1);
                    }
                    1 if ui.get_album_total_count() == 2 => {
                        assert!(album.choose(&ui, 0));
                        progress.set(2);
                    }
                    2 if ui.get_album_viewer_has_image() => {
                        let image = ui.window().take_snapshot().unwrap();
                        for y in 200..500 {
                            for x in 320..960 {
                                let p = image.as_slice()[(y * image.width() + x) as usize];
                                assert!(
                                    [p.r, p.g, p.b] == [255, 0, 255]
                                        || [p.r, p.g, p.b] == [0, 255, 0],
                                    "smoothed pixel: {p:?}"
                                );
                            }
                        }
                        image::save_buffer(
                            output.join("album-pixels.png"),
                            image.as_bytes(),
                            image.width(),
                            image.height(),
                            image::ColorType::Rgba8,
                        )
                        .unwrap();
                        album.step(&ui, 1);
                        let page = navigation.navigator.borrow().page();
                        back();
                        assert!(!ui.get_album_viewing());
                        forward();
                        assert!(ui.get_album_viewing());
                        assert_eq!(navigation.navigator.borrow().page(), page);
                        ui.invoke_album_close_viewer();
                        assert!(!ui.get_album_viewing());
                        forward();
                        assert!(ui.get_album_viewing());
                        navigation.handle_home(&ui);
                        progress.set(3);
                    }
                    3 => {
                        use slint::platform::{PointerEventButton, WindowEvent};
                        let rect = ui.invoke_player_capture_origin();
                        let position = slint::LogicalPosition::new(
                            (rect.x + rect.width / 2.0) * 1280.0,
                            (rect.y + rect.height / 2.0) * 720.0,
                        );
                        ui.window().dispatch_event(WindowEvent::PointerPressed {
                            position,
                            button: PointerEventButton::Left,
                        });
                        let moved = slint::LogicalPosition::new(position.x - 100.0, position.y);
                        ui.window()
                            .dispatch_event(WindowEvent::PointerMoved { position: moved });
                        ui.window().dispatch_event(WindowEvent::PointerReleased {
                            position: moved,
                            button: PointerEventButton::Left,
                        });
                        assert_eq!(launches.get(), 0);
                        progress.set(4);
                    }
                    4..=8 => progress.set(progress.get() + 1),
                    9 => {
                        use slint::platform::{PointerEventButton, WindowEvent};
                        let rect = ui.invoke_player_capture_origin();
                        let position = slint::LogicalPosition::new(
                            (rect.x + rect.width / 2.0) * 1280.0,
                            (rect.y + rect.height / 2.0) * 720.0,
                        );
                        ui.window().dispatch_event(WindowEvent::PointerPressed {
                            position,
                            button: PointerEventButton::Left,
                        });
                        ui.window().dispatch_event(WindowEvent::PointerReleased {
                            position,
                            button: PointerEventButton::Left,
                        });
                        assert_eq!(
                            launches.get(),
                            1,
                            "click must dispatch one game launch after drag settles"
                        );
                        progress.set(10);
                        slint::quit_event_loop().unwrap();
                    }
                    _ => {}
                }
            },
        );
        slint::run_event_loop().unwrap();
        assert_eq!(stage.get(), 10);
    }
}
