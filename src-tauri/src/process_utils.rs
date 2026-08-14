//! Windows process helpers for shell and updater launches.

use std::{path::Path, process::Command};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn hide_command_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

pub fn shell_open(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("rundll32.exe");
        cmd.args(["url.dll,FileProtocolHandler", target]);
        hide_command_window(&mut cmd);
        cmd.spawn()
            .map(|_| ())
            .map_err(|error| format!("open failed: {error}"))
    }
    #[cfg(not(windows))]
    {
        open::that_detached(target).map_err(|error| error.to_string())
    }
}

pub fn launch_executable(path: &Path) -> Result<(), String> {
    let mut cmd = Command::new(path);
    hide_command_window(&mut cmd);
    cmd.spawn()
        .map(|_| ())
        .map_err(|error| format!("launch failed: {error}"))
}
