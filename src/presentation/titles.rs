//! Display formatting only: preserve original catalog names and ROM identities.
pub(crate) fn game_title(title: &str) -> String {
    crate::domain::rom_titles::display_title(title)
}
