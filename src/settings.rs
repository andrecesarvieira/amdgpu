use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct DeviceSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_watts: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fan_automatic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fan_percent: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sclk_min: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sclk_max: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mclk_max: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_offset: Option<i32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct AppSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_gpu: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct SettingsFile {
    #[serde(
        rename = "__app__",
        default,
        skip_serializing_if = "app_settings_empty"
    )]
    pub app: AppSettings,
    #[serde(flatten)]
    pub devices: BTreeMap<String, DeviceSettings>,
}

fn app_settings_empty(settings: &AppSettings) -> bool {
    settings.selected_gpu.is_none()
}

pub fn settings_path() -> PathBuf {
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("amdgpu-control/settings.json")
}

pub fn load() -> SettingsFile {
    load_from(&settings_path()).unwrap_or_default()
}

pub fn load_from(path: &Path) -> io::Result<SettingsFile> {
    let contents = fs::read_to_string(path)?;
    serde_json::from_str(&contents)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub fn save(settings: &SettingsFile) -> io::Result<()> {
    save_to(&settings_path(), settings)
}

pub fn save_to(path: &Path, settings: &SettingsFile) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    let payload = serde_json::to_vec_pretty(settings)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(&payload)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temporary, path)
}

pub fn update_device(pci_id: &str, update: impl FnOnce(&mut DeviceSettings)) -> io::Result<()> {
    let mut settings = load();
    update(settings.devices.entry(pci_id.to_string()).or_default());
    save(&settings)
}

pub fn clear_device(pci_id: &str) -> io::Result<()> {
    let mut settings = load();
    settings.devices.remove(pci_id);
    save(&settings)
}

pub fn set_selected_gpu(pci_id: &str) -> io::Result<()> {
    let mut settings = load();
    settings.app.selected_gpu = Some(pci_id.to_string());
    save(&settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn reads_legacy_python_shape_and_writes_selection() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(
            &path,
            r#"{"0000:03:00.0":{"performance":"high","power_watts":290}}"#,
        )
        .unwrap();
        let mut value = load_from(&path).unwrap();
        assert_eq!(value.devices["0000:03:00.0"].power_watts, Some(290));
        value.app.selected_gpu = Some("0000:03:00.0".to_string());
        save_to(&path, &value).unwrap();
        let roundtrip = load_from(&path).unwrap();
        assert_eq!(roundtrip, value);
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
