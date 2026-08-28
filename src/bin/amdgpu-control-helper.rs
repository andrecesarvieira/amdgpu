use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const AMD_VENDOR: &str = "0x1002";
const ALLOWED_CONTROLS: &[&str] = &[
    "power_dpm_force_performance_level",
    "pp_power_profile_mode",
    "pp_od_clk_voltage",
    "power1_cap",
    "pwm1",
    "pwm1_enable",
];

fn valid_pci_id(value: &str) -> bool {
    value.len() == 12
        && value.as_bytes()[4] == b':'
        && value.as_bytes()[7] == b':'
        && value.as_bytes()[10] == b'.'
        && value
            .chars()
            .enumerate()
            .all(|(index, ch)| matches!(index, 4 | 7 | 10) || ch.is_ascii_hexdigit())
}

fn unsigned(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit())
}

fn signed(value: &str) -> bool {
    value
        .strip_prefix('-')
        .map(unsigned)
        .unwrap_or_else(|| unsigned(value))
}

fn valid_value(control: &str, value: &str) -> bool {
    match control {
        "power_dpm_force_performance_level" => matches!(
            value,
            "auto"
                | "low"
                | "high"
                | "manual"
                | "profile_standard"
                | "profile_min_sclk"
                | "profile_min_mclk"
                | "profile_peak"
        ),
        "pp_power_profile_mode" => unsigned(value),
        "power1_cap" => value.len() <= 10 && unsigned(value),
        "pwm1" => value.parse::<u16>().is_ok_and(|number| number <= 255),
        "pwm1_enable" => matches!(value, "0" | "1" | "2"),
        "pp_od_clk_voltage" => {
            let tokens = value.split_whitespace().collect::<Vec<_>>();
            match tokens.as_slice() {
                ["c"] | ["r"] => true,
                ["s" | "m", point, clock] => unsigned(point) && unsigned(clock),
                ["s" | "m", point, clock, voltage] => {
                    unsigned(point) && unsigned(clock) && unsigned(voltage)
                }
                ["vc", point, clock, voltage] => {
                    matches!(*point, "0" | "1" | "2") && unsigned(clock) && unsigned(voltage)
                }
                ["vo", offset] => signed(offset),
                _ => false,
            }
        }
        _ => false,
    }
}

fn read_text(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path)
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn locate_device(root: &Path, pci_id: &str) -> Result<PathBuf, String> {
    if !valid_pci_id(pci_id) {
        return Err("endereço PCI inválido".to_string());
    }
    let entries = fs::read_dir(root).map_err(|_| "diretório DRM não encontrado".to_string())?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(number) = name.strip_prefix("card") else {
            continue;
        };
        if number.is_empty() || !number.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let device = entry.path().join("device");
        let current_pci = fs::canonicalize(&device).ok().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });
        if current_pci.as_deref() != Some(pci_id) {
            continue;
        }
        let driver = fs::canonicalize(device.join("driver"))
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            });
        if read_text(device.join("vendor")).to_lowercase() != AMD_VENDOR
            || driver.as_deref() != Some("amdgpu")
        {
            return Err("o dispositivo não usa o driver amdgpu".to_string());
        }
        return Ok(device);
    }
    Err("GPU AMD não encontrada".to_string())
}

fn locate_control(root: &Path, pci_id: &str, control: &str) -> Result<PathBuf, String> {
    if !ALLOWED_CONTROLS.contains(&control) {
        return Err("controle não permitido".to_string());
    }
    let device = locate_device(root, pci_id)?;
    let direct = device.join(control);
    if direct.is_file() {
        return Ok(direct);
    }
    let entries =
        fs::read_dir(device.join("hwmon")).map_err(|_| "controle não encontrado".to_string())?;
    for entry in entries.flatten() {
        if read_text(entry.path().join("name")) == "amdgpu" {
            let path = entry.path().join(control);
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    Err("controle não encontrado".to_string())
}

fn write_control(root: &Path, pci_id: &str, control: &str, value: &str) -> Result<(), io::Error> {
    if !valid_value(control, value) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "valor não permitido",
        ));
    }
    let path = locate_control(root, pci_id, control)
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.write_all(value.as_bytes())
}

fn main() -> std::process::ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 3 {
        eprintln!("uso: amdgpu-control-helper <pci> <controle> <valor>");
        return std::process::ExitCode::from(64);
    }
    match write_control(
        Path::new("/sys/class/drm"),
        &arguments[0],
        &arguments[1],
        &arguments[2],
    ) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("este ajuste precisa de autorização do Polkit");
            std::process::ExitCode::from(77)
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn validates_control_values() {
        assert!(valid_value("power_dpm_force_performance_level", "manual"));
        assert!(valid_value("pp_od_clk_voltage", "s 1 2500"));
        assert!(valid_value("pp_od_clk_voltage", "vo -50"));
        assert!(valid_value("pwm1", "255"));
        assert!(!valid_value("pwm1", "256"));
        assert!(!valid_value("pp_od_clk_voltage", "../../shadow"));
        assert!(!valid_value("power1_cap", "10W"));
    }

    #[test]
    fn validates_pci_addresses() {
        assert!(valid_pci_id("0000:03:00.0"));
        assert!(!valid_pci_id("../../../tmp"));
    }

    #[test]
    fn locates_only_an_amdgpu_device_and_writes_an_allowlisted_control() {
        let directory = tempfile::tempdir().expect("temporary DRM tree");
        let drm = directory.path().join("drm");
        let device = directory.path().join("devices/0000:03:00.0");
        let driver = directory.path().join("drivers/amdgpu");
        fs::create_dir_all(drm.join("card0")).expect("fake card");
        fs::create_dir_all(&device).expect("fake device");
        fs::create_dir_all(&driver).expect("fake driver");
        fs::write(device.join("vendor"), "0x1002\n").expect("vendor");
        fs::write(device.join("power_dpm_force_performance_level"), "auto\n").expect("control");
        symlink(&device, drm.join("card0/device")).expect("device link");
        symlink(&driver, device.join("driver")).expect("driver link");

        write_control(
            &drm,
            "0000:03:00.0",
            "power_dpm_force_performance_level",
            "high",
        )
        .expect("write control");
        assert!(
            fs::read_to_string(device.join("power_dpm_force_performance_level"))
                .expect("updated control")
                .starts_with("high")
        );
        assert!(write_control(&drm, "0000:03:00.0", "../../etc/shadow", "high").is_err());
    }
}
