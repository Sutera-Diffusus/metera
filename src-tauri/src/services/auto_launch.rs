use auto_launch::AutoLaunchBuilder;

fn manager() -> Result<auto_launch::AutoLaunch, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    AutoLaunchBuilder::new().set_app_name("Metera")
        .set_app_path(&executable.to_string_lossy()).build().map_err(|error| error.to_string())
}

pub fn get() -> Result<bool, String> { manager()?.is_enabled().map_err(|error| error.to_string()) }

pub fn set(enabled: bool) -> Result<(), String> {
    let manager = manager()?;
    if enabled { manager.enable().map_err(|error| error.to_string()) }
    else if !manager.is_enabled().unwrap_or(false) { Ok(()) }
    else { manager.disable().map_err(|error| error.to_string()) }
}
