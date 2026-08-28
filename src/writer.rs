use crate::gpu::AmdGpu;
use std::env;
use std::path::PathBuf;
use std::process::{Command, Output};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Performance,
    WorkloadProfile,
    Overdrive,
    PowerLimit,
    FanPwm,
    FanMode,
}

impl Control {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Performance => "power_dpm_force_performance_level",
            Self::WorkloadProfile => "pp_power_profile_mode",
            Self::Overdrive => "pp_od_clk_voltage",
            Self::PowerLimit => "power1_cap",
            Self::FanPwm => "pwm1",
            Self::FanMode => "pwm1_enable",
        }
    }
}

#[derive(Debug, Error)]
pub enum ControlError {
    #[error("O helper amdgpu-control-helper não está instalado.")]
    HelperMissing,
    #[error("O pkexec não está instalado; não foi possível solicitar autorização.")]
    PkexecMissing,
    #[error("Este controle não é oferecido pelo driver atual.")]
    Unsupported,
    #[error("{0}")]
    Rejected(String),
    #[error("Não foi possível executar o helper: {0}")]
    Spawn(#[from] std::io::Error),
}

fn helper_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("AMDGPU_CONTROL_HELPER").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }
    let sibling = env::current_exe()
        .ok()
        .map(|path| path.with_file_name("amdgpu-control-helper"))
        .filter(|path| path.is_file());
    if sibling.is_some() {
        return sibling;
    }
    let installed = PathBuf::from("/usr/libexec/amdgpu-control-helper");
    installed.is_file().then_some(installed)
}

fn run_helper(
    helper: &PathBuf,
    gpu: &AmdGpu,
    control: Control,
    value: &str,
) -> Result<Output, std::io::Error> {
    Command::new(helper)
        .args([
            gpu.pci_id(),
            control.as_str().to_string(),
            value.to_string(),
        ])
        .output()
}

pub fn set_control(
    gpu: &AmdGpu,
    control: Control,
    value: impl AsRef<str>,
) -> Result<(), ControlError> {
    if gpu.control_path(control.as_str()).is_none() {
        return Err(ControlError::Unsupported);
    }
    let helper = helper_path().ok_or(ControlError::HelperMissing)?;
    let value = value.as_ref();
    let direct = run_helper(&helper, gpu, control, value)?;
    if direct.status.success() {
        return Ok(());
    }
    if direct.status.code() != Some(77) {
        return Err(ControlError::Rejected(message(&direct)));
    }
    let pkexec = PathBuf::from("/usr/bin/pkexec");
    if !pkexec.is_file() {
        return Err(ControlError::PkexecMissing);
    }
    let output = Command::new(pkexec)
        .arg(&helper)
        .args([
            gpu.pci_id(),
            control.as_str().to_string(),
            value.to_string(),
        ])
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ControlError::Rejected(message(&output)))
    }
}

fn message(output: &Output) -> String {
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if detail.is_empty() {
        "A alteração foi cancelada ou rejeitada pelo driver.".to_string()
    } else {
        detail
    }
}
