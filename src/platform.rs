use std::path::Path;
use std::process::{Command, Stdio};

/// Processes that will blue-screen or log out Windows if killed.
const CRITICAL: &[&str] = &[
    "system",
    "registry",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "memory compression",
    "secure system",
    // linux
    "systemd",
    "init",
    "kthreadd",
];

pub fn is_critical(pid: u32, name: &str) -> bool {
    pid <= 4 || CRITICAL.contains(&name.to_lowercase().as_str())
}

/// Opens the file manager with the file selected (or its folder on Linux).
pub fn reveal(path: &Path) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // explorer needs the quotes inside the /select, argument, so bypass Rust's quoting.
        Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg("-R").arg(path).spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = path.parent().unwrap_or(path);
        Command::new("xdg-open")
            .arg(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
    }
    Ok(())
}

pub fn copy(text: &str) -> anyhow::Result<()> {
    match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) {
        Ok(()) => Ok(()),
        Err(e) => {
            // WSL has no X/Wayland clipboard but can reach the Windows one.
            if cfg!(unix) {
                use std::io::Write;
                if let Ok(mut child) = Command::new("clip.exe").stdin(Stdio::piped()).spawn() {
                    if let Some(stdin) = child.stdin.as_mut() {
                        stdin.write_all(text.as_bytes())?;
                    }
                    child.wait()?;
                    return Ok(());
                }
            }
            Err(e.into())
        }
    }
}

#[cfg(windows)]
pub fn is_elevated() -> Option<bool> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut elev = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elev as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        (ok != 0).then_some(elev.TokenIsElevated != 0)
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> Option<bool> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("Uid:"))?;
    let euid = line.split_whitespace().nth(2)?;
    Some(euid == "0")
}
