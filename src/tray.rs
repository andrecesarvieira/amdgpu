use crate::gpu::{AmdGpu, Telemetry};
use gio::prelude::ActionGroupExt;
use glib::variant::ToVariant;
use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{MenuItem, RadioGroup, RadioItem, StandardItem, SubMenu};

const ICON_NAME: &str = "io.github.amdgpucontrol.Control-gpu-symbolic";

#[derive(Clone, Debug, Default)]
pub struct TrayGpu {
    pub pci_id: String,
    pub label: String,
}

#[derive(Clone, Debug, Default)]
pub struct TrayProfile {
    pub index: u32,
    pub label: String,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub struct TraySnapshot {
    pub gpu_title: String,
    pub tooltip: String,
    pub gpus: Vec<TrayGpu>,
    pub selected_gpu: usize,
    pub performance: String,
    pub profiles: Vec<TrayProfile>,
}

impl Default for TraySnapshot {
    fn default() -> Self {
        Self {
            gpu_title: "AMDGPU Control".to_string(),
            tooltip: "Monitoramento da GPU AMD".to_string(),
            gpus: Vec::new(),
            selected_gpu: 0,
            performance: "auto".to_string(),
            profiles: Vec::new(),
        }
    }
}

pub fn profile_label(name: &str) -> String {
    match name {
        "BOOTUP_DEFAULT" => "Padrão",
        "3D_FULL_SCREEN" => "Jogo em tela cheia",
        "POWER_SAVING" => "Economia de energia",
        "VIDEO" => "Vídeo",
        "VR" => "Realidade virtual",
        "COMPUTE" => "Computação",
        "CUSTOM" => "Personalizado",
        "WINDOW_3D" => "Jogo em janela",
        other => return other.replace('_', " ").to_lowercase(),
    }
    .to_string()
}

pub fn snapshot_for(gpus: &[AmdGpu], selected: usize, sample: Option<Telemetry>) -> TraySnapshot {
    let Some(gpu) = gpus.get(selected) else {
        return TraySnapshot::default();
    };
    let data = sample.unwrap_or_else(|| gpu.telemetry());
    let metric = |value: Option<f64>, suffix: &str, decimals: usize| {
        value
            .map(|number| format!("{number:.decimals$}{suffix}"))
            .unwrap_or_else(|| "—".to_string())
    };
    TraySnapshot {
        gpu_title: gpu.name(),
        tooltip: format!(
            "Uso {} · {} · {}",
            metric(data.utilization, "%", 0),
            metric(data.temperature, " °C", 0),
            metric(data.power, " W", 1)
        ),
        gpus: gpus
            .iter()
            .map(|gpu| TrayGpu {
                pci_id: gpu.pci_id(),
                label: format!("{} · {}", gpu.name(), gpu.pci_id()),
            })
            .collect(),
        selected_gpu: selected,
        performance: gpu.performance_level(),
        profiles: gpu
            .power_profiles()
            .into_iter()
            .map(|profile| TrayProfile {
                index: profile.index,
                label: profile_label(&profile.name),
                active: profile.active,
            })
            .collect(),
    }
}

pub struct AmdTray {
    snapshot: TraySnapshot,
}

impl AmdTray {
    fn dispatch(action: &'static str, parameter: Option<glib::Variant>) {
        glib::MainContext::default().invoke(move || {
            if let Some(application) = gio::Application::default() {
                application.activate_action(action, parameter.as_ref());
            }
        });
    }
}

impl ksni::Tray for AmdTray {
    fn id(&self) -> String {
        "amdgpu-control".to_string()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn title(&self) -> String {
        self.snapshot.gpu_title.clone()
    }

    fn icon_name(&self) -> String {
        ICON_NAME.to_string()
    }

    fn attention_icon_name(&self) -> String {
        ICON_NAME.to_string()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            icon_name: ICON_NAME.to_string(),
            title: self.snapshot.gpu_title.clone(),
            description: self.snapshot.tooltip.clone(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        Self::dispatch("show", None);
    }

    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        Self::dispatch("show", None);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut output = Vec::new();
        if self.snapshot.gpus.len() > 1 {
            let gpus = self.snapshot.gpus.clone();
            output.push(
                SubMenu {
                    label: "Placa de vídeo".to_string(),
                    submenu: vec![RadioGroup {
                        selected: self.snapshot.selected_gpu.min(gpus.len().saturating_sub(1)),
                        options: gpus
                            .iter()
                            .map(|gpu| RadioItem {
                                label: gpu.label.clone(),
                                ..Default::default()
                            })
                            .collect(),
                        select: Box::new(move |tray: &mut Self, selected| {
                            let Some(gpu) = gpus.get(selected) else {
                                return;
                            };
                            tray.snapshot.selected_gpu = selected;
                            Self::dispatch("tray-gpu", Some(gpu.pci_id.to_variant()));
                        }),
                    }
                    .into()],
                    ..Default::default()
                }
                .into(),
            );
        }

        let performances = [
            ("Automático", "auto"),
            ("Economia", "low"),
            ("Alto", "high"),
            ("Manual", "manual"),
        ];
        let selected_performance = performances
            .iter()
            .position(|(_, value)| *value == self.snapshot.performance)
            .unwrap_or(0);
        output.push(
            SubMenu {
                label: "Modo de desempenho".to_string(),
                submenu: vec![RadioGroup {
                    selected: selected_performance,
                    options: performances
                        .iter()
                        .map(|(label, _)| RadioItem {
                            label: (*label).to_string(),
                            ..Default::default()
                        })
                        .collect(),
                    select: Box::new(move |tray: &mut Self, selected| {
                        let Some((_, value)) = performances.get(selected) else {
                            return;
                        };
                        tray.snapshot.performance = (*value).to_string();
                        Self::dispatch("tray-performance", Some(value.to_variant()));
                    }),
                }
                .into()],
                ..Default::default()
            }
            .into(),
        );

        if !self.snapshot.profiles.is_empty() {
            let profiles = self.snapshot.profiles.clone();
            let selected = profiles
                .iter()
                .position(|profile| profile.active)
                .unwrap_or(0);
            output.push(
                SubMenu {
                    label: "Perfil de carga".to_string(),
                    submenu: vec![RadioGroup {
                        selected,
                        options: profiles
                            .iter()
                            .map(|profile| RadioItem {
                                label: profile.label.clone(),
                                ..Default::default()
                            })
                            .collect(),
                        select: Box::new(move |tray: &mut Self, selected| {
                            let Some(profile) = profiles.get(selected) else {
                                return;
                            };
                            for item in &mut tray.snapshot.profiles {
                                item.active = item.index == profile.index;
                            }
                            Self::dispatch("tray-profile", Some(profile.index.to_variant()));
                        }),
                    }
                    .into()],
                    ..Default::default()
                }
                .into(),
            );
        }

        output.push(MenuItem::Separator);
        output.push(
            StandardItem {
                label: "Abrir AMDGPU Control".to_string(),
                icon_name: "window-new-symbolic".to_string(),
                activate: Box::new(|_| Self::dispatch("show", None)),
                ..Default::default()
            }
            .into(),
        );
        output.push(
            StandardItem {
                label: "Sair completamente".to_string(),
                icon_name: "application-exit-symbolic".to_string(),
                activate: Box::new(|_| Self::dispatch("quit", None)),
                ..Default::default()
            }
            .into(),
        );
        output
    }
}

#[derive(Clone)]
pub struct TrayController {
    handle: Handle<AmdTray>,
}

impl TrayController {
    pub fn start(snapshot: TraySnapshot) -> Result<Self, String> {
        let tray = AmdTray { snapshot };
        let handle = tray
            .assume_sni_available(true)
            .spawn()
            .map_err(|error| format!("{error:?}"))?;
        Ok(Self { handle })
    }

    pub fn update(&self, snapshot: TraySnapshot) {
        let _ = self.handle.update(move |tray| tray.snapshot = snapshot);
    }
}
