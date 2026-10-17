use std::env;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::process::Command;

/// `C:\Tools\craftpulse.exe` -> `"C:\Tools\craftpulse.exe"`.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn quoted_path(path: &str) -> String {
    format!("\"{}\"", path.trim_matches('"'))
}

/// An XDG autostart entry. The path is quoted the way the Desktop Entry spec asks.
#[cfg(any(target_os = "linux", test))]
fn desktop_entry(exe: &str) -> String {
    let quoted: String = exe
        .chars()
        .flat_map(|ch| match ch {
            '"' | '`' | '$' | '\\' => vec!['\\', ch],
            _ => vec![ch],
        })
        .collect();
    format!(
        "[Desktop Entry]\nType=Application\nName=CraftPulse\nComment=Discord Rich Presence for the ArtCraft apps\nExec=\"{quoted}\"\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    )
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let exe = env::current_exe().map_err(|e| e.to_string())?;
        let mut command = Command::new("reg.exe");
        // No console window flashing up while reg.exe runs.
        command.creation_flags(0x0800_0000);
        command.args([
            if enabled { "add" } else { "delete" },
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "CraftPulse",
        ]);
        if enabled {
            // The path must be quoted, or a folder name with a space (for example
            // "John Smith") stops Windows from starting it at login.
            command
                .args(["/t", "REG_SZ", "/d"])
                .arg(quoted_path(&exe.to_string_lossy()))
                .arg("/f");
        } else {
            command.arg("/f");
        }
        let status = command.status().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("Unable to update Windows startup setting".to_string());
        }
    }
    #[cfg(target_os = "macos")]
    {
        let path = dirs::home_dir()
            .ok_or("Home directory unavailable")?
            .join("Library/LaunchAgents/com.artcraft.craftpulse.plist");
        if enabled {
            let exe = env::current_exe().map_err(|e| e.to_string())?;
            fs::create_dir_all(path.parent().ok_or("Invalid launch agent path")?)
                .map_err(|e| e.to_string())?;
            let escaped = exe
                .to_string_lossy()
                .replace('&', "&amp;")
                .replace('<', "&lt;");
            let content = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>com.artcraft.craftpulse</string><key>ProgramArguments</key><array><string>{escaped}</string></array><key>RunAtLoad</key><true/></dict></plist>"#
            );
            fs::write(path, content).map_err(|e| e.to_string())?;
        } else if path.exists() {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    #[cfg(target_os = "linux")]
    {
        let path: PathBuf = dirs::config_dir()
            .ok_or("XDG config directory unavailable")?
            .join("autostart/craftpulse.desktop");
        if enabled {
            // Run from an AppImage: start the AppImage file, not its temporary mount.
            let exe = env::var_os("APPIMAGE")
                .map(PathBuf::from)
                .map_or_else(env::current_exe, Ok)
                .map_err(|e| e.to_string())?;
            fs::create_dir_all(path.parent().ok_or("Invalid autostart path")?)
                .map_err(|e| e.to_string())?;
            fs::write(path, desktop_entry(&exe.to_string_lossy())).map_err(|e| e.to_string())?;
        } else if path.exists() {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = enabled;
        return Err("Launch at login is not supported on this platform".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn startup_path_is_quoted_once() {
        assert_eq!(
            super::quoted_path(r"C:\Users\John Smith\craftpulse.exe"),
            r#""C:\Users\John Smith\craftpulse.exe""#
        );
        assert_eq!(super::quoted_path(r#""C:\a.exe""#), r#""C:\a.exe""#);
    }

    #[test]
    fn autostart_entry_quotes_the_path() {
        let entry = super::desktop_entry("/home/me/My Apps/craft$pulse");
        assert!(entry.contains("Exec=\"/home/me/My Apps/craft\\$pulse\"\n"));
        assert!(entry.contains("Terminal=false"));
    }

    #[test]
    fn platform_names_are_available() {
        assert!(!std::env::consts::OS.is_empty());
    }
}
