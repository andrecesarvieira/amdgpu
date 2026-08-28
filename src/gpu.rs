use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

const AMD_VENDOR: &str = "0x1002";

fn read_text(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path)
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn number(path: impl AsRef<Path>, divisor: f64) -> Option<f64> {
    read_text(path)
        .parse::<f64>()
        .ok()
        .map(|value| value / divisor)
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClockState {
    pub index: i32,
    pub mhz: i32,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PowerProfile {
    pub index: u32,
    pub name: String,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PowerLimit {
    pub current: f64,
    pub default: f64,
    pub minimum: f64,
    pub maximum: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FanControl {
    pub automatic: bool,
    pub pwm_percent: f64,
    pub rpm: Option<u32>,
    pub maximum_rpm: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClockRange {
    pub current_minimum: i32,
    pub current_maximum: i32,
    pub allowed_minimum: i32,
    pub allowed_maximum: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Telemetry {
    pub utilization: Option<f64>,
    pub temperature: Option<f64>,
    pub junction_temperature: Option<f64>,
    pub power: Option<f64>,
    pub fan_rpm: Option<f64>,
    pub vram_used: Option<f64>,
    pub vram_total: Option<f64>,
    pub core_clock: Option<i32>,
    pub memory_clock: Option<i32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub performance: bool,
    pub workload_profiles: bool,
    pub core_clock: bool,
    pub memory_clock: bool,
    pub voltage: bool,
    pub power_limit: bool,
    pub fan: bool,
}

impl Capabilities {
    pub fn score(&self) -> usize {
        [
            self.performance,
            self.workload_profiles,
            self.core_clock,
            self.memory_clock,
            self.voltage,
            self.power_limit,
            self.fan,
        ]
        .into_iter()
        .filter(|value| *value)
        .count()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmdGpu {
    card_path: PathBuf,
    device_path: PathBuf,
}

impl AmdGpu {
    pub fn new(card_path: impl Into<PathBuf>) -> Self {
        let card_path = card_path.into();
        let device_path = card_path.join("device");
        Self {
            card_path,
            device_path,
        }
    }

    pub fn card_path(&self) -> &Path {
        &self.card_path
    }

    pub fn device_path(&self) -> &Path {
        &self.device_path
    }

    pub fn pci_id(&self) -> String {
        fs::canonicalize(&self.device_path)
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| {
                self.card_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
    }

    pub fn name(&self) -> String {
        let product = read_text(self.device_path.join("product_name"));
        if !product.is_empty() {
            return product;
        }
        let device = read_text(self.device_path.join("device"))
            .trim_start_matches("0x")
            .to_uppercase();
        if device.is_empty() {
            "AMD Radeon".to_string()
        } else {
            format!("AMD Radeon ({device})")
        }
    }

    pub fn performance_level(&self) -> String {
        let value = read_text(self.device_path.join("power_dpm_force_performance_level"));
        if value.is_empty() {
            "auto".to_string()
        } else {
            value
        }
    }

    pub fn control_path(&self, name: &str) -> Option<PathBuf> {
        let direct = self.device_path.join(name);
        if direct.is_file() {
            return Some(direct);
        }
        self.hwmon_path()
            .map(|path| path.join(name))
            .filter(|path| path.is_file())
    }

    pub fn hwmon_path(&self) -> Option<PathBuf> {
        let mut candidates = fs::read_dir(self.device_path.join("hwmon"))
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        candidates.sort();
        candidates
            .into_iter()
            .find(|candidate| read_text(candidate.join("name")) == "amdgpu")
    }

    pub fn clock_states(&self, kind: &str) -> Vec<ClockState> {
        if !matches!(kind, "sclk" | "mclk") {
            return Vec::new();
        }
        read_text(self.device_path.join(format!("pp_dpm_{kind}")))
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let active = line.ends_with('*');
                let (index, rest) = line.split_once(':')?;
                let index = if index == "S" {
                    -1
                } else {
                    index.parse().ok()?
                };
                let mhz = rest
                    .trim()
                    .trim_end_matches('*')
                    .trim()
                    .trim_end_matches(|ch: char| ch.is_ascii_alphabetic())
                    .trim()
                    .parse()
                    .ok()?;
                Some(ClockState { index, mhz, active })
            })
            .collect()
    }

    pub fn overdrive_clock_range(&self, kind: &str) -> Option<ClockRange> {
        if !matches!(kind, "sclk" | "mclk") {
            return None;
        }
        let output = read_text(self.device_path.join("pp_od_clk_voltage"));
        let section_name = if kind == "sclk" {
            "OD_SCLK:"
        } else {
            "OD_MCLK:"
        };
        let range_name = if kind == "sclk" { "SCLK:" } else { "MCLK:" };
        let section_start = output.find(section_name)? + section_name.len();
        let section_tail = &output[section_start..];
        let section_end = section_tail.find("\nOD_").unwrap_or(section_tail.len());
        let section = &section_tail[..section_end];
        let point_re = Regex::new(r"(?im)^\s*(\d+):\s*(\d+)\s*Mhz").ok()?;
        let mut points = std::collections::BTreeMap::new();
        for captures in point_re.captures_iter(section) {
            points.insert(
                captures[1].parse::<u32>().ok()?,
                captures[2].parse::<i32>().ok()?,
            );
        }
        let range_re = Regex::new(&format!(
            r"(?im)^\s*{}\s*(\d+)\s*Mhz\s+(\d+)\s*Mhz",
            regex::escape(range_name)
        ))
        .ok()?;
        let allowed = range_re.captures(&output)?;
        let allowed_minimum = allowed[1].parse().ok()?;
        let allowed_maximum = allowed[2].parse().ok()?;
        let current_maximum = *points.get(&1)?;
        let current_minimum = if kind == "sclk" {
            *points.get(&0)?
        } else {
            points.get(&0).copied().unwrap_or(allowed_minimum)
        };
        Some(ClockRange {
            current_minimum,
            current_maximum,
            allowed_minimum,
            allowed_maximum,
        })
    }

    pub fn voltage_offset(&self) -> Option<(i32, i32, i32)> {
        let output = read_text(self.device_path.join("pp_od_clk_voltage"));
        let current_re = Regex::new(r"(?im)OD_VDDGFX_OFFSET:\s*\n\s*(-?\d+)\s*mV").ok()?;
        let range_re = Regex::new(r"(?im)VDDGFX_OFFSET:\s*(-?\d+)\s*mV\s+(-?\d+)\s*mV").ok()?;
        let current = current_re.captures(&output)?[1].parse().ok()?;
        let range = range_re.captures(&output)?;
        Some((current, range[1].parse().ok()?, range[2].parse().ok()?))
    }

    pub fn power_profiles(&self) -> Vec<PowerProfile> {
        let Ok(pattern) = Regex::new(r"(?im)^\s*(\d+)\s+([A-Z0-9_]+)\s*(\*)?\s*:") else {
            return Vec::new();
        };
        pattern
            .captures_iter(&read_text(self.device_path.join("pp_power_profile_mode")))
            .filter_map(|captures| {
                Some(PowerProfile {
                    index: captures[1].parse().ok()?,
                    name: captures[2].to_string(),
                    active: captures.get(3).is_some(),
                })
            })
            .collect()
    }

    pub fn power_limit(&self) -> Option<PowerLimit> {
        let hwmon = self.hwmon_path()?;
        Some(PowerLimit {
            current: number(hwmon.join("power1_cap"), 1_000_000.0)?,
            default: number(hwmon.join("power1_cap_default"), 1_000_000.0)?,
            minimum: number(hwmon.join("power1_cap_min"), 1_000_000.0)?,
            maximum: number(hwmon.join("power1_cap_max"), 1_000_000.0)?,
        })
    }

    pub fn fan_control(&self) -> Option<FanControl> {
        let hwmon = self.hwmon_path()?;
        let pwm = number(hwmon.join("pwm1"), 1.0)?;
        let mode = number(hwmon.join("pwm1_enable"), 1.0)? as u32;
        Some(FanControl {
            automatic: mode == 2,
            pwm_percent: (pwm * 100.0 / 255.0).clamp(0.0, 100.0),
            rpm: number(hwmon.join("fan1_input"), 1.0).map(|value| value as u32),
            maximum_rpm: number(hwmon.join("fan1_max"), 1.0).map(|value| value as u32),
        })
    }

    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            performance: self
                .control_path("power_dpm_force_performance_level")
                .is_some(),
            workload_profiles: !self.power_profiles().is_empty()
                && self.control_path("pp_power_profile_mode").is_some(),
            core_clock: self.overdrive_clock_range("sclk").is_some(),
            memory_clock: self.overdrive_clock_range("mclk").is_some(),
            voltage: self.voltage_offset().is_some(),
            power_limit: self.power_limit().is_some() && self.control_path("power1_cap").is_some(),
            fan: self.fan_control().is_some()
                && self.control_path("pwm1").is_some()
                && self.control_path("pwm1_enable").is_some(),
        }
    }

    pub fn telemetry(&self) -> Telemetry {
        let hwmon = self.hwmon_path();
        let core_sensor = hwmon
            .as_ref()
            .and_then(|path| number(path.join("freq1_input"), 1_000_000.0));
        let memory_sensor = hwmon
            .as_ref()
            .and_then(|path| number(path.join("freq2_input"), 1_000_000.0));
        let core_clock = core_sensor.map(|value| value as i32).or_else(|| {
            self.clock_states("sclk")
                .into_iter()
                .find(|state| state.active)
                .map(|state| state.mhz)
        });
        let memory_clock = memory_sensor.map(|value| value as i32).or_else(|| {
            self.clock_states("mclk")
                .into_iter()
                .find(|state| state.active)
                .map(|state| state.mhz)
        });
        Telemetry {
            utilization: number(self.device_path.join("gpu_busy_percent"), 1.0),
            temperature: hwmon
                .as_ref()
                .and_then(|path| number(path.join("temp1_input"), 1_000.0)),
            junction_temperature: hwmon
                .as_ref()
                .and_then(|path| number(path.join("temp2_input"), 1_000.0)),
            power: hwmon
                .as_ref()
                .and_then(|path| number(path.join("power1_average"), 1_000_000.0)),
            fan_rpm: hwmon
                .as_ref()
                .and_then(|path| number(path.join("fan1_input"), 1.0)),
            vram_used: number(
                self.device_path.join("mem_info_vram_used"),
                1024_f64.powi(3),
            ),
            vram_total: number(
                self.device_path.join("mem_info_vram_total"),
                1024_f64.powi(3),
            ),
            core_clock,
            memory_clock,
        }
    }
}

pub fn discover_gpus() -> Vec<AmdGpu> {
    discover_gpus_in(Path::new("/sys/class/drm"))
}

pub fn discover_gpus_in(root: &Path) -> Vec<AmdGpu> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut cards = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let suffix = name.strip_prefix("card")?;
            if suffix.is_empty() || !suffix.chars().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            Some((suffix.parse::<u32>().ok()?, entry.path()))
        })
        .collect::<Vec<_>>();
    cards.sort_by_key(|(number, _)| *number);
    cards
        .into_iter()
        .map(|(_, card)| card)
        .filter(|card| {
            let device = card.join("device");
            if read_text(device.join("vendor")).to_lowercase() != AMD_VENDOR {
                return false;
            }
            fs::canonicalize(device.join("driver"))
                .ok()
                .and_then(|path| path.file_name().map(|name| name == "amdgpu"))
                .unwrap_or(false)
        })
        .map(AmdGpu::new)
        .collect()
}

pub fn preferred_gpu_index(gpus: &[AmdGpu], saved_pci: Option<&str>) -> usize {
    if let Some(saved) = saved_pci {
        if let Some(index) = gpus.iter().position(|gpu| gpu.pci_id() == saved) {
            return index;
        }
    }
    gpus.iter()
        .enumerate()
        .max_by_key(|(_, gpu)| gpu.capabilities().score())
        .map(|(index, _)| index)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn fixture() -> (tempfile::TempDir, AmdGpu) {
        let directory = tempfile::tempdir().expect("temporary GPU fixture");
        let card = directory.path().join("card0");
        let device = card.join("device");
        fs::create_dir_all(device.join("hwmon/hwmon0")).expect("fake hwmon");
        let write = |relative: &str, value: &str| {
            fs::write(device.join(relative), value).expect("fake sysfs value");
        };
        write("pp_dpm_sclk", "0: 500Mhz\n1: 2400Mhz *\n");
        write("pp_dpm_mclk", "0: 96Mhz *\n1: 1000Mhz\n");
        write("gpu_busy_percent", "42\n");
        write("mem_info_vram_used", &(2_u64 * 1024_u64.pow(3)).to_string());
        write(
            "mem_info_vram_total",
            &(16_u64 * 1024_u64.pow(3)).to_string(),
        );
        write("hwmon/hwmon0/name", "amdgpu\n");
        write("hwmon/hwmon0/temp1_input", "51500\n");
        write("hwmon/hwmon0/temp2_input", "69000\n");
        write("hwmon/hwmon0/power1_average", "123000000\n");
        write("hwmon/hwmon0/power1_cap", "265000000\n");
        write("hwmon/hwmon0/power1_cap_default", "250000000\n");
        write("hwmon/hwmon0/power1_cap_min", "200000000\n");
        write("hwmon/hwmon0/power1_cap_max", "300000000\n");
        write("hwmon/hwmon0/fan1_input", "1450\n");
        write("hwmon/hwmon0/fan1_max", "3000\n");
        write("hwmon/hwmon0/pwm1", "128\n");
        write("hwmon/hwmon0/pwm1_enable", "2\n");
        (directory, AmdGpu::new(card))
    }

    #[test]
    fn parses_clocks_telemetry_power_and_fan() {
        let (_directory, gpu) = fixture();
        assert_eq!(
            gpu.clock_states("sclk"),
            vec![
                ClockState {
                    index: 0,
                    mhz: 500,
                    active: false,
                },
                ClockState {
                    index: 1,
                    mhz: 2400,
                    active: true,
                },
            ]
        );
        let telemetry = gpu.telemetry();
        assert_eq!(telemetry.utilization, Some(42.0));
        assert_eq!(telemetry.temperature, Some(51.5));
        assert_eq!(telemetry.junction_temperature, Some(69.0));
        assert_eq!(telemetry.power, Some(123.0));
        assert_eq!(telemetry.vram_used, Some(2.0));
        assert_eq!(telemetry.vram_total, Some(16.0));
        assert_eq!(telemetry.core_clock, Some(2400));

        let power = gpu.power_limit().expect("power limits");
        assert_eq!(
            (power.current, power.default, power.minimum, power.maximum),
            (265.0, 250.0, 200.0, 300.0)
        );
        let fan = gpu.fan_control().expect("fan control");
        assert!(fan.automatic);
        assert!((fan.pwm_percent - 50.196).abs() < 0.01);
        assert_eq!((fan.rpm, fan.maximum_rpm), (Some(1450), Some(3000)));
    }

    #[test]
    fn parses_firmware_profiles_and_overdrive_ranges() {
        let (_directory, gpu) = fixture();
        fs::write(
            gpu.device_path().join("pp_power_profile_mode"),
            "PROFILE_INDEX(NAME)\n 0 BOOTUP_DEFAULT*:\n 1 3D_FULL_SCREEN :\n 2 POWER_SAVING :\n",
        )
        .expect("profiles");
        fs::write(
            gpu.device_path().join("pp_od_clk_voltage"),
            "OD_SCLK:\n0: 500Mhz\n1: 2500Mhz\nOD_MCLK:\n0: 96Mhz\n1: 1200Mhz\nOD_VDDGFX_OFFSET:\n-50mV\nOD_RANGE:\nSCLK: 300Mhz 3000Mhz\nMCLK: 90Mhz 1400Mhz\nVDDGFX_OFFSET: -100mV 0mV\n",
        )
        .expect("overdrive");
        let profiles = gpu.power_profiles();
        assert_eq!(profiles.len(), 3);
        assert_eq!(profiles[1].name, "3D_FULL_SCREEN");
        assert!(profiles[0].active);
        assert_eq!(
            gpu.overdrive_clock_range("sclk"),
            Some(ClockRange {
                current_minimum: 500,
                current_maximum: 2500,
                allowed_minimum: 300,
                allowed_maximum: 3000,
            })
        );
        assert_eq!(
            gpu.overdrive_clock_range("mclk")
                .map(|range| (range.current_minimum, range.current_maximum)),
            Some((96, 1200))
        );
        assert_eq!(gpu.voltage_offset(), Some((-50, -100, 0)));
    }

    #[test]
    fn discovery_filters_vendor_driver_and_sorts_card_numbers() {
        let directory = tempfile::tempdir().expect("temporary DRM fixture");
        let drm = directory.path().join("drm");
        let devices = directory.path().join("devices");
        let drivers = directory.path().join("drivers");
        fs::create_dir_all(&drm).expect("drm");
        fs::create_dir_all(drivers.join("amdgpu")).expect("amdgpu driver");
        fs::create_dir_all(drivers.join("i915")).expect("i915 driver");

        for (card, pci, vendor, driver) in [
            ("card10", "0000:10:00.0", "0x1002", "amdgpu"),
            ("card2", "0000:02:00.0", "0x1002", "amdgpu"),
            ("card1", "0000:00:02.0", "0x8086", "i915"),
        ] {
            let device = devices.join(pci);
            fs::create_dir_all(&device).expect("device");
            fs::write(device.join("vendor"), vendor).expect("vendor");
            symlink(drivers.join(driver), device.join("driver")).expect("driver link");
            fs::create_dir_all(drm.join(card)).expect("card");
            symlink(&device, drm.join(card).join("device")).expect("device link");
        }

        let found = discover_gpus_in(&drm);
        assert_eq!(
            found.iter().map(AmdGpu::pci_id).collect::<Vec<_>>(),
            ["0000:02:00.0", "0000:10:00.0"]
        );
    }
}
