pub mod achievements;
pub mod activity;
pub mod album;
pub mod appearance;
pub mod clock;
pub mod home;
pub mod library;
pub mod navigation;

pub mod settings;
pub mod status;

/// An optional hook (callback or trait object) installed after a controller is built.
pub(crate) type CallbackSlot<F> = std::cell::RefCell<Option<std::rc::Rc<F>>>;
