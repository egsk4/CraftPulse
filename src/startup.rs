use std::env;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::process::Command;

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let exe = env::current_exe().map_err(|e| e.to_string())?;
        let mut command = Command::new("reg.exe");
        command.args([
            if enabled { "add" } else { "delete" },
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "CraftPulse",
        ]);
        if enabled {
            command.args(["/t", "REG_SZ", "/d"]).arg(exe).arg("/f");
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
            let exe = env::current_exe().map_err(|e| e.to_string())?;
            fs::create_dir_all(path.parent().ok_or("Invalid autostart path")?)
                .map_err(|e| e.to_string())?;
            fs::write(
                path,
                format!(
                    "[Desktop Entry]\nType=Application\nName=CraftPulse\nExec=\"{}\"\nX-GNOME-Autostart-enabled=true\n",
                    exe.display()
                ),
            )
            .map_err(|e| e.to_string())?;
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
    fn platform_names_are_available() {
        assert!(!std::env::consts::OS.is_empty());
    }
}
