fn main() -> Result<(), horizon::error::AppError> {
    // Steam names the real controllers behind its virtual pads in a file it
    // passes to what it launches; SDL reads it when it starts. If Horizon
    // wasn't given it (started some other way, or the variable didn't reach
    // the sandbox), point SDL at Steam's own.
    if std::env::var_os("SteamVirtualGamepadInfo").is_none()
        && let Some(file) = horizon::sources::steam::virtual_gamepad_info_file()
    {
        // SAFETY: nothing else runs yet; no other thread can read the
        // environment while it changes.
        unsafe { std::env::set_var("SteamVirtualGamepadInfo", file) };
    }
    horizon::app::run()
}
