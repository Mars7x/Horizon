pub mod activity;
pub mod album;
pub mod appearance;
pub mod catalog;
pub mod clock;
pub mod friends;
pub mod home;
pub mod keyboard;
pub mod library;
pub mod library_reload;
pub(crate) mod mounted;
pub mod navigation;
pub mod profile;

pub mod settings;
pub mod status;
pub(crate) mod titles;
pub mod wake;

/// An optional hook (callback or trait object) installed after a controller is built.
pub(crate) type CallbackSlot<F> = std::cell::RefCell<Option<std::rc::Rc<F>>>;

pub mod rom_setup;

pub mod gba_player;
pub mod onboarding;
