use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

const AUTOSTART_NAME: &str = "io.github.amdgpucontrol.Control.desktop";

pub fn user_autostart_path() -> PathBuf {
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("autostart").join(AUTOSTART_NAME)
}

pub fn is_enabled() -> bool {
    is_enabled_at(&user_autostart_path())
}

fn is_enabled_at(path: &std::path::Path) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return true;
    };
    !contents.lines().any(|line| {
        let compact = line.trim().replace(' ', "").to_ascii_lowercase();
        compact == "hidden=true"
    })
}

pub fn set_enabled(enabled: bool) -> io::Result<()> {
    set_enabled_at(&user_autostart_path(), enabled)
}

fn set_enabled_at(path: &std::path::Path, enabled: bool) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let contents = if enabled {
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=AMDGPU Control Tray\n\
         Exec=amdgpu-control --background\n\
         Icon=io.github.amdgpucontrol.Control\n\
         NoDisplay=true\n\
         OnlyShowIn=GNOME;\n\
         Hidden=false\n"
    } else {
        "[Desktop Entry]\nType=Application\nHidden=true\n"
    };
    fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enables_and_disables_the_session_autostart() {
        let directory = tempfile::tempdir().expect("temporary autostart directory");
        let path = directory.path().join("autostart.desktop");
        assert!(is_enabled_at(&path));
        set_enabled_at(&path, false).expect("disable autostart");
        assert!(!is_enabled_at(&path));
        set_enabled_at(&path, true).expect("enable autostart");
        assert!(is_enabled_at(&path));
        let contents = fs::read_to_string(path).expect("autostart file");
        assert!(contents.contains("Exec=amdgpu-control --background"));
        assert!(contents.contains("Hidden=false"));
    }
}
