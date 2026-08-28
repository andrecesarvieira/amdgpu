use crate::autostart;
use crate::gpu::{AmdGpu, Telemetry};
use crate::settings::{self, DeviceSettings};
use crate::tray::{profile_label, snapshot_for, TrayController};
use crate::writer::{set_control, Control, ControlError};
use adw::prelude::*;
use gtk::glib;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

const PERFORMANCE_VALUES: [&str; 4] = ["auto", "low", "high", "manual"];

#[derive(Clone, Debug, Default)]
struct PendingChanges {
    performance: Option<String>,
    profile: Option<u32>,
    power_watts: Option<i32>,
    fan_automatic: Option<bool>,
    fan_percent: Option<i32>,
    sclk_min: Option<i32>,
    sclk_max: Option<i32>,
    mclk_max: Option<i32>,
    voltage_offset: Option<i32>,
}

impl PendingChanges {
    fn is_empty(&self) -> bool {
        self.performance.is_none()
            && self.profile.is_none()
            && self.power_watts.is_none()
            && self.fan_automatic.is_none()
            && self.fan_percent.is_none()
            && self.sclk_min.is_none()
            && self.sclk_max.is_none()
            && self.mclk_max.is_none()
            && self.voltage_offset.is_none()
    }
}

#[derive(Clone, Debug)]
struct GpuSnapshot {
    performance: String,
    profile: Option<u32>,
    power_watts: Option<i32>,
    fan_automatic: Option<bool>,
    fan_percent: Option<i32>,
    sclk_min: Option<i32>,
    sclk_max: Option<i32>,
    mclk_max: Option<i32>,
    voltage_offset: Option<i32>,
}

impl GpuSnapshot {
    fn capture(gpu: &AmdGpu) -> Self {
        let fan = gpu.fan_control();
        let sclk = gpu.overdrive_clock_range("sclk");
        let mclk = gpu.overdrive_clock_range("mclk");
        Self {
            performance: gpu.performance_level(),
            profile: gpu
                .power_profiles()
                .into_iter()
                .find(|profile| profile.active)
                .map(|profile| profile.index),
            power_watts: gpu.power_limit().map(|power| power.current.round() as i32),
            fan_automatic: fan.as_ref().map(|fan| fan.automatic),
            fan_percent: fan.map(|fan| fan.pwm_percent.round() as i32),
            sclk_min: sclk.as_ref().map(|clock| clock.current_minimum),
            sclk_max: sclk.map(|clock| clock.current_maximum),
            mclk_max: mclk.map(|clock| clock.current_maximum),
            voltage_offset: gpu.voltage_offset().map(|value| value.0),
        }
    }
}

struct MetricValue {
    key: &'static str,
    title: &'static str,
    icon: &'static str,
    value: String,
}

pub struct UiController {
    window: adw::ApplicationWindow,
    toast_overlay: adw::ToastOverlay,
    gpu_dropdown: gtk::DropDown,
    metrics_grid: gtk::Grid,
    metric_labels: RefCell<HashMap<&'static str, gtk::Label>>,
    tuning_box: gtk::Box,
    warning: adw::Banner,
    apply_button: gtk::Button,
    reset_button: gtk::Button,
    gpus: Vec<AmdGpu>,
    selected: Cell<usize>,
    pending: RefCell<PendingChanges>,
    updating: Cell<bool>,
    telemetry_source: RefCell<Option<glib::SourceId>>,
    last_telemetry: RefCell<Option<Telemetry>>,
    tray: Option<TrayController>,
}

impl UiController {
    pub fn new(
        application: &adw::Application,
        gpus: Vec<AmdGpu>,
        selected: usize,
        tray: Option<TrayController>,
    ) -> Rc<Self> {
        let window = adw::ApplicationWindow::builder()
            .application(application)
            .title("AMDGPU Control")
            .default_width(1040)
            .default_height(820)
            .build();

        let toolbar = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        let title = adw::WindowTitle::new("AMDGPU Control", "Monitoramento e ajuste");
        header.set_title_widget(Some(&title));

        let gpu_labels = gpus
            .iter()
            .map(|gpu| format!("{} · {}", gpu.name(), gpu.pci_id()))
            .collect::<Vec<_>>();
        let gpu_label_refs = gpu_labels.iter().map(String::as_str).collect::<Vec<_>>();
        let gpu_dropdown = gtk::DropDown::from_strings(&gpu_label_refs);
        gpu_dropdown.set_selected(selected as u32);
        gpu_dropdown.set_sensitive(gpus.len() > 1);
        gpu_dropdown.set_tooltip_text(Some("Selecionar placa de vídeo"));
        header.pack_start(&gpu_dropdown);

        let menu = gio::Menu::new();
        menu.append(Some("Iniciar com o sistema"), Some("app.autostart"));
        menu.append(Some("Sair completamente"), Some("app.quit"));
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .tooltip_text("Menu")
            .build();
        header.pack_end(&menu_button);
        toolbar.add_top_bar(&header);

        let page = gtk::Box::new(gtk::Orientation::Vertical, 14);
        page.set_margin_top(16);
        page.set_margin_bottom(16);
        page.set_margin_start(18);
        page.set_margin_end(18);

        let overview = gtk::Label::new(Some("Visão geral"));
        overview.set_xalign(0.0);
        overview.add_css_class("title-1");
        page.append(&overview);

        let metrics_grid = gtk::Grid::builder()
            .column_spacing(12)
            .row_spacing(12)
            .column_homogeneous(true)
            .build();
        page.append(&metrics_grid);

        let adjustments = gtk::Label::new(Some("Ajustes"));
        adjustments.set_xalign(0.0);
        adjustments.add_css_class("title-1");
        adjustments.set_margin_top(8);
        page.append(&adjustments);

        let tuning_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
        page.append(&tuning_box);

        let action_line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        action_line.set_margin_top(10);
        action_line.set_margin_bottom(10);
        action_line.set_margin_start(18);
        action_line.set_margin_end(18);
        let warning = adw::Banner::new("");
        warning.set_hexpand(true);
        warning.set_revealed(false);
        action_line.append(&warning);

        let reset_button = gtk::Button::with_label("Restaurar padrões");
        reset_button.set_valign(gtk::Align::Center);
        reset_button.add_css_class("pill");
        action_line.append(&reset_button);

        let apply_button = gtk::Button::with_label("Aplicar alterações");
        apply_button.set_valign(gtk::Align::Center);
        apply_button.set_sensitive(false);
        apply_button.add_css_class("suggested-action");
        apply_button.add_css_class("pill");
        action_line.append(&apply_button);

        let clamp = adw::Clamp::builder()
            .maximum_size(900)
            .tightening_threshold(760)
            .child(&page)
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&clamp)
            .build();
        let toast_overlay = adw::ToastOverlay::new();
        toast_overlay.set_child(Some(&scroll));
        toolbar.set_content(Some(&toast_overlay));
        toolbar.add_bottom_bar(&action_line);
        window.set_content(Some(&toolbar));

        let controller = Rc::new(Self {
            window,
            toast_overlay,
            gpu_dropdown,
            metrics_grid,
            metric_labels: RefCell::new(HashMap::new()),
            tuning_box,
            warning,
            apply_button,
            reset_button,
            gpus,
            selected: Cell::new(selected),
            pending: RefCell::new(PendingChanges::default()),
            updating: Cell::new(false),
            telemetry_source: RefCell::new(None),
            last_telemetry: RefCell::new(None),
            tray,
        });
        controller.connect_signals();
        controller.load_selected_gpu();
        controller
    }

    pub fn present(self: &Rc<Self>) {
        self.window.present();
        self.refresh_telemetry();
        self.start_telemetry();
    }

    pub fn select_gpu_by_pci(self: &Rc<Self>, pci_id: &str) {
        if let Some(index) = self.gpus.iter().position(|gpu| gpu.pci_id() == pci_id) {
            self.gpu_dropdown.set_selected(index as u32);
        }
    }

    pub fn selected_gpu(&self) -> Option<AmdGpu> {
        self.gpus.get(self.selected.get()).cloned()
    }

    pub fn refresh_after_external_change(self: &Rc<Self>) {
        self.rebuild_controls();
        self.refresh_telemetry();
    }

    fn connect_signals(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.gpu_dropdown.connect_selected_notify(move |dropdown| {
            let Some(controller) = weak.upgrade() else {
                return;
            };
            let selected = dropdown.selected() as usize;
            if selected >= controller.gpus.len() || selected == controller.selected.get() {
                return;
            }
            let discarded = !controller.pending.borrow().is_empty();
            controller.selected.set(selected);
            controller.pending.replace(PendingChanges::default());
            controller.apply_button.set_sensitive(false);
            if discarded {
                controller.toast("Alterações pendentes descartadas ao trocar de GPU", 4);
            }
            if let Some(gpu) = controller.gpus.get(selected) {
                if let Err(error) = settings::set_selected_gpu(&gpu.pci_id()) {
                    controller.toast(
                        &format!("Não foi possível salvar a GPU selecionada: {error}"),
                        5,
                    );
                }
            }
            controller.load_selected_gpu();
        });

        let weak = Rc::downgrade(self);
        self.apply_button.connect_clicked(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.apply_pending();
            }
        });

        let weak = Rc::downgrade(self);
        self.reset_button.connect_clicked(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.reset_gpu();
            }
        });

        let weak = Rc::downgrade(self);
        self.window.connect_is_active_notify(move |window| {
            let Some(controller) = weak.upgrade() else {
                return;
            };
            if window.is_active() {
                controller.refresh_telemetry();
                controller.start_telemetry();
            } else {
                controller.stop_telemetry();
            }
        });

        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |window| {
            if let Some(controller) = weak.upgrade() {
                controller.stop_telemetry();
            }
            window.set_visible(false);
            glib::Propagation::Stop
        });
    }

    fn load_selected_gpu(self: &Rc<Self>) {
        let Some(gpu) = self.selected_gpu() else {
            return;
        };
        let data = gpu.telemetry();
        self.last_telemetry.replace(Some(data.clone()));
        self.rebuild_metrics(&data);
        self.rebuild_controls();
        self.update_tray(Some(data));
    }

    fn telemetry_values(data: &Telemetry) -> Vec<MetricValue> {
        let format = |value: Option<f64>, suffix: &str, decimals: usize| {
            value.map(|number| format!("{number:.decimals$}{suffix}"))
        };
        let mut values = Vec::new();
        if let Some(value) = format(data.utilization, "%", 0) {
            values.push(MetricValue {
                key: "utilization",
                title: "Uso",
                icon: "power-profile-performance-symbolic",
                value,
            });
        }
        if let Some(value) = format(data.temperature, " °C", 0) {
            values.push(MetricValue {
                key: "temperature",
                title: "Temperatura",
                icon: "temperature-symbolic",
                value,
            });
        }
        if let Some(value) = format(data.power, " W", 1) {
            values.push(MetricValue {
                key: "power",
                title: "Potência",
                icon: "battery-level-100-charged-symbolic",
                value,
            });
        }
        if let Some(value) = format(data.fan_rpm, " RPM", 0) {
            values.push(MetricValue {
                key: "fan",
                title: "Ventoinha",
                icon: "weather-windy-symbolic",
                value,
            });
        }
        if let Some(clock) = data.core_clock {
            values.push(MetricValue {
                key: "core",
                title: "Clock",
                icon: "applications-system-symbolic",
                value: format!("{clock} MHz"),
            });
        }
        if let (Some(used), Some(total)) = (data.vram_used, data.vram_total) {
            values.push(MetricValue {
                key: "vram",
                title: "VRAM",
                icon: "drive-harddisk-symbolic",
                value: format!("{used:.1} / {total:.1} GiB"),
            });
        }
        if let Some(value) = format(data.junction_temperature, " °C", 0) {
            values.push(MetricValue {
                key: "hotspot",
                title: "Hotspot",
                icon: "find-location-symbolic",
                value,
            });
        }
        if let Some(clock) = data.memory_clock {
            values.push(MetricValue {
                key: "memory",
                title: "Clock da memória",
                icon: "media-flash-symbolic",
                value: format!("{clock} MHz"),
            });
        }
        values
    }

    fn rebuild_metrics(&self, data: &Telemetry) {
        while let Some(child) = self.metrics_grid.first_child() {
            self.metrics_grid.remove(&child);
        }
        self.metric_labels.borrow_mut().clear();
        for (index, metric) in Self::telemetry_values(data).into_iter().enumerate() {
            let card = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            card.add_css_class("metric-card");
            card.set_hexpand(true);
            let icon = gtk::Image::from_icon_name(metric.icon);
            icon.set_pixel_size(28);
            icon.add_css_class("dim-label");
            card.append(&icon);
            let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
            let title = gtk::Label::new(Some(metric.title));
            title.set_xalign(0.0);
            title.add_css_class("dim-label");
            let value = gtk::Label::new(Some(&metric.value));
            value.set_xalign(0.0);
            value.add_css_class("title-2");
            text.append(&title);
            text.append(&value);
            card.append(&text);
            self.metrics_grid
                .attach(&card, (index % 4) as i32, (index / 4) as i32, 1, 1);
            self.metric_labels.borrow_mut().insert(metric.key, value);
        }
    }

    fn refresh_metric_values(&self, data: &Telemetry) {
        let labels = self.metric_labels.borrow();
        for metric in Self::telemetry_values(data) {
            if let Some(label) = labels.get(metric.key) {
                label.set_label(&metric.value);
            }
        }
    }

    fn rebuild_controls(self: &Rc<Self>) {
        let Some(gpu) = self.selected_gpu() else {
            return;
        };
        self.updating.set(true);
        while let Some(child) = self.tuning_box.first_child() {
            self.tuning_box.remove(&child);
        }
        let capabilities = gpu.capabilities();
        let group = adw::PreferencesGroup::new();
        group.set_description(Some(
            "Somente controles oferecidos por esta GPU e pelo driver atual.",
        ));

        if capabilities.performance {
            let row = adw::ComboRow::builder()
                .title("Modo de desempenho")
                .subtitle("Controle global de energia e clocks do driver")
                .model(&gtk::StringList::new(&[
                    "Automático",
                    "Economia",
                    "Alto",
                    "Manual",
                ]))
                .build();
            let current = PERFORMANCE_VALUES
                .iter()
                .position(|value| *value == gpu.performance_level())
                .unwrap_or(0);
            row.set_selected(current as u32);
            let weak = Rc::downgrade(self);
            row.connect_selected_notify(move |row| {
                let Some(controller) = weak.upgrade() else {
                    return;
                };
                if controller.updating.get() {
                    return;
                }
                if let Some(value) = PERFORMANCE_VALUES.get(row.selected() as usize) {
                    controller.pending.borrow_mut().performance = Some((*value).to_string());
                    controller.mark_dirty();
                }
            });
            group.add(&row);
        }

        if capabilities.workload_profiles {
            let profiles = gpu.power_profiles();
            let labels = profiles
                .iter()
                .map(|profile| profile_label(&profile.name))
                .collect::<Vec<_>>();
            let label_refs = labels.iter().map(String::as_str).collect::<Vec<_>>();
            let row = adw::ComboRow::builder()
                .title("Perfil de carga")
                .subtitle("Otimiza o firmware para jogos, vídeo, VR ou computação")
                .model(&gtk::StringList::new(&label_refs))
                .build();
            row.set_selected(
                profiles
                    .iter()
                    .position(|profile| profile.active)
                    .unwrap_or(0) as u32,
            );
            let profile_indices = profiles
                .iter()
                .map(|profile| profile.index)
                .collect::<Vec<_>>();
            let weak = Rc::downgrade(self);
            row.connect_selected_notify(move |row| {
                let Some(controller) = weak.upgrade() else {
                    return;
                };
                if controller.updating.get() {
                    return;
                }
                if let Some(index) = profile_indices.get(row.selected() as usize) {
                    controller.pending.borrow_mut().profile = Some(*index);
                    controller.mark_dirty();
                }
            });
            group.add(&row);
        }

        if let Some(power) = capabilities
            .power_limit
            .then(|| gpu.power_limit())
            .flatten()
        {
            let row = adw::ActionRow::builder()
                .title("Limite de potência")
                .subtitle(format!(
                    "Padrão {:.0} W · faixa {:.0}–{:.0} W",
                    power.default, power.minimum, power.maximum
                ))
                .build();
            let spin = gtk::SpinButton::with_range(power.minimum, power.maximum, 1.0);
            spin.set_value(power.current);
            spin.set_numeric(true);
            spin.set_width_chars(5);
            spin.set_valign(gtk::Align::Center);
            row.add_suffix(&spin);
            row.add_suffix(&gtk::Label::new(Some("W")));
            let weak = Rc::downgrade(self);
            spin.connect_value_changed(move |spin| {
                let Some(controller) = weak.upgrade() else {
                    return;
                };
                if controller.updating.get() {
                    return;
                }
                controller.pending.borrow_mut().power_watts = Some(spin.value().round() as i32);
                controller.mark_dirty();
            });
            group.add(&row);
        }

        if let Some(fan) = capabilities.fan.then(|| gpu.fan_control()).flatten() {
            let fan_row = adw::ComboRow::builder()
                .title("Controle da ventoinha")
                .subtitle("O firmware ajusta a rotação conforme a temperatura")
                .model(&gtk::StringList::new(&["Automático", "Manual"]))
                .build();
            fan_row.set_selected(if fan.automatic { 0 } else { 1 });
            group.add(&fan_row);

            let speed_row = adw::ActionRow::builder()
                .title("Velocidade manual")
                .subtitle(format!(
                    "PWM fixo{}",
                    fan.rpm
                        .map(|rpm| format!(" · rotação atual {rpm} RPM"))
                        .unwrap_or_default()
                ))
                .visible(!fan.automatic)
                .build();
            let speed = gtk::SpinButton::with_range(0.0, 100.0, 1.0);
            speed.set_value(fan.pwm_percent);
            speed.set_width_chars(4);
            speed.set_valign(gtk::Align::Center);
            speed_row.add_suffix(&speed);
            speed_row.add_suffix(&gtk::Label::new(Some("%")));
            group.add(&speed_row);

            let weak = Rc::downgrade(self);
            let speed_row_clone = speed_row.clone();
            fan_row.connect_selected_notify(move |row| {
                let Some(controller) = weak.upgrade() else {
                    return;
                };
                if controller.updating.get() {
                    return;
                }
                let automatic = row.selected() == 0;
                speed_row_clone.set_visible(!automatic);
                controller.pending.borrow_mut().fan_automatic = Some(automatic);
                controller.mark_dirty();
            });
            let weak = Rc::downgrade(self);
            speed.connect_value_changed(move |speed| {
                let Some(controller) = weak.upgrade() else {
                    return;
                };
                if controller.updating.get() {
                    return;
                }
                controller.pending.borrow_mut().fan_percent = Some(speed.value().round() as i32);
                controller.mark_dirty();
            });
        }

        if let Some(range) = capabilities
            .core_clock
            .then(|| gpu.overdrive_clock_range("sclk"))
            .flatten()
        {
            let row = adw::ActionRow::builder()
                .title("Clock da GPU")
                .subtitle(format!(
                    "Faixa do firmware: {}–{} MHz",
                    range.allowed_minimum, range.allowed_maximum
                ))
                .build();
            let minimum = gtk::SpinButton::with_range(
                range.allowed_minimum as f64,
                range.allowed_maximum as f64,
                10.0,
            );
            minimum.set_value(range.current_minimum as f64);
            minimum.set_width_chars(5);
            minimum.set_valign(gtk::Align::Center);
            let maximum = gtk::SpinButton::with_range(
                range.allowed_minimum as f64,
                range.allowed_maximum as f64,
                10.0,
            );
            maximum.set_value(range.current_maximum as f64);
            maximum.set_width_chars(5);
            maximum.set_valign(gtk::Align::Center);
            row.add_suffix(&minimum);
            row.add_suffix(&gtk::Label::new(Some("a")));
            row.add_suffix(&maximum);
            row.add_suffix(&gtk::Label::new(Some("MHz")));
            let weak = Rc::downgrade(self);
            minimum.connect_value_changed(move |spin| {
                if let Some(controller) = weak.upgrade() {
                    if !controller.updating.get() {
                        controller.pending.borrow_mut().sclk_min =
                            Some(spin.value().round() as i32);
                        controller.mark_dirty();
                    }
                }
            });
            let weak = Rc::downgrade(self);
            maximum.connect_value_changed(move |spin| {
                if let Some(controller) = weak.upgrade() {
                    if !controller.updating.get() {
                        controller.pending.borrow_mut().sclk_max =
                            Some(spin.value().round() as i32);
                        controller.mark_dirty();
                    }
                }
            });
            group.add(&row);
        }

        if let Some(range) = capabilities
            .memory_clock
            .then(|| gpu.overdrive_clock_range("mclk"))
            .flatten()
        {
            let row = adw::ActionRow::builder()
                .title("Clock da VRAM")
                .subtitle(format!(
                    "Faixa do firmware: {}–{} MHz",
                    range.allowed_minimum, range.allowed_maximum
                ))
                .build();
            let maximum = gtk::SpinButton::with_range(
                range.allowed_minimum as f64,
                range.allowed_maximum as f64,
                10.0,
            );
            maximum.set_value(range.current_maximum as f64);
            maximum.set_width_chars(5);
            maximum.set_valign(gtk::Align::Center);
            row.add_suffix(&maximum);
            row.add_suffix(&gtk::Label::new(Some("MHz")));
            let weak = Rc::downgrade(self);
            maximum.connect_value_changed(move |spin| {
                if let Some(controller) = weak.upgrade() {
                    if !controller.updating.get() {
                        controller.pending.borrow_mut().mclk_max =
                            Some(spin.value().round() as i32);
                        controller.mark_dirty();
                    }
                }
            });
            group.add(&row);
        }

        if let Some((current, minimum, maximum)) =
            capabilities.voltage.then(|| gpu.voltage_offset()).flatten()
        {
            let row = adw::ActionRow::builder()
                .title("Offset de voltagem")
                .subtitle(format!("Faixa permitida: {minimum} a {maximum} mV"))
                .build();
            let spin = gtk::SpinButton::with_range(minimum as f64, maximum as f64, 5.0);
            spin.set_value(current as f64);
            spin.set_width_chars(6);
            spin.set_valign(gtk::Align::Center);
            row.add_suffix(&spin);
            row.add_suffix(&gtk::Label::new(Some("mV")));
            let weak = Rc::downgrade(self);
            spin.connect_value_changed(move |spin| {
                if let Some(controller) = weak.upgrade() {
                    if !controller.updating.get() {
                        controller.pending.borrow_mut().voltage_offset =
                            Some(spin.value().round() as i32);
                        controller.mark_dirty();
                    }
                }
            });
            group.add(&row);
        }

        self.tuning_box.append(&group);
        let clock_tuning = capabilities.core_clock || capabilities.memory_clock;
        if clock_tuning && capabilities.voltage {
            self.warning
                .set_title("Overclock e undervolt podem causar instabilidade.");
            self.warning.set_revealed(true);
        } else if capabilities.voltage {
            self.warning
                .set_title("Ajustes de voltagem podem causar instabilidade.");
            self.warning.set_revealed(true);
        } else if clock_tuning {
            self.warning
                .set_title("Ajustes de clock podem causar instabilidade.");
            self.warning.set_revealed(true);
        } else if capabilities.power_limit {
            self.warning
                .set_title("Ajustes de potência podem afetar a estabilidade.");
            self.warning.set_revealed(true);
        } else {
            self.warning.set_revealed(false);
        }
        self.updating.set(false);
    }

    fn mark_dirty(&self) {
        self.apply_button
            .set_sensitive(!self.pending.borrow().is_empty());
    }

    fn apply_pending(self: &Rc<Self>) {
        let Some(gpu) = self.selected_gpu() else {
            return;
        };
        let pending = self.pending.borrow().clone();
        if pending.is_empty() {
            return;
        }
        let snapshot = GpuSnapshot::capture(&gpu);
        if let Err(error) = apply_changes(&gpu, &pending) {
            let _ = restore_snapshot(&gpu, &snapshot);
            self.toast(&format!("Ajustes desfeitos após uma falha: {error}"), 6);
            self.rebuild_controls();
            return;
        }
        if let Err(error) =
            settings::update_device(&gpu.pci_id(), |saved| merge_settings(saved, &pending))
        {
            self.toast(
                &format!("Ajustes aplicados, mas não foi possível salvá-los: {error}"),
                6,
            );
        } else {
            self.toast("Alterações aplicadas", 3);
        }
        self.pending.replace(PendingChanges::default());
        self.apply_button.set_sensitive(false);
        self.rebuild_controls();
        self.refresh_telemetry();
    }

    fn reset_gpu(self: &Rc<Self>) {
        let Some(gpu) = self.selected_gpu() else {
            return;
        };
        let snapshot = GpuSnapshot::capture(&gpu);
        if let Err(error) = reset_controls(&gpu) {
            let _ = restore_snapshot(&gpu, &snapshot);
            self.toast(&format!("Restauração desfeita após uma falha: {error}"), 6);
            return;
        }
        if let Err(error) = settings::clear_device(&gpu.pci_id()) {
            self.toast(
                &format!("Controles restaurados, mas o perfil salvo não foi apagado: {error}"),
                6,
            );
        } else {
            self.toast("Controles restaurados para os padrões", 3);
        }
        self.pending.replace(PendingChanges::default());
        self.apply_button.set_sensitive(false);
        self.rebuild_controls();
        self.refresh_telemetry();
    }

    fn refresh_telemetry(self: &Rc<Self>) {
        if !self.window.is_active() && self.last_telemetry.borrow().is_some() {
            return;
        }
        let Some(gpu) = self.selected_gpu() else {
            return;
        };
        let data = gpu.telemetry();
        self.refresh_metric_values(&data);
        self.last_telemetry.replace(Some(data.clone()));
        self.update_tray(Some(data));
    }

    fn start_telemetry(self: &Rc<Self>) {
        if self.telemetry_source.borrow().is_some() || !self.window.is_active() {
            return;
        }
        let weak: Weak<Self> = Rc::downgrade(self);
        let source = glib::timeout_add_seconds_local(2, move || {
            let Some(controller) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !controller.window.is_active() {
                controller.telemetry_source.borrow_mut().take();
                return glib::ControlFlow::Break;
            }
            controller.refresh_telemetry();
            glib::ControlFlow::Continue
        });
        self.telemetry_source.replace(Some(source));
    }

    fn stop_telemetry(&self) {
        if let Some(source) = self.telemetry_source.borrow_mut().take() {
            source.remove();
        }
    }

    fn update_tray(&self, sample: Option<Telemetry>) {
        if let Some(tray) = &self.tray {
            tray.update(snapshot_for(&self.gpus, self.selected.get(), sample));
        }
    }

    fn toast(&self, title: &str, seconds: u32) {
        self.toast_overlay
            .add_toast(adw::Toast::builder().title(title).timeout(seconds).build());
    }
}

fn merge_settings(saved: &mut DeviceSettings, pending: &PendingChanges) {
    if let Some(value) = &pending.performance {
        saved.performance = Some(value.clone());
    }
    if let Some(value) = pending.profile {
        saved.profile = Some(value);
    }
    if let Some(value) = pending.power_watts {
        saved.power_watts = Some(value);
    }
    if let Some(value) = pending.fan_automatic {
        saved.fan_automatic = Some(value);
    }
    if let Some(value) = pending.fan_percent {
        saved.fan_percent = Some(value);
    }
    if let Some(value) = pending.sclk_min {
        saved.sclk_min = Some(value);
        saved.performance = Some("manual".to_string());
    }
    if let Some(value) = pending.sclk_max {
        saved.sclk_max = Some(value);
        saved.performance = Some("manual".to_string());
    }
    if let Some(value) = pending.mclk_max {
        saved.mclk_max = Some(value);
        saved.performance = Some("manual".to_string());
    }
    if let Some(value) = pending.voltage_offset {
        saved.voltage_offset = Some(value);
        saved.performance = Some("manual".to_string());
    }
}

fn apply_changes(gpu: &AmdGpu, pending: &PendingChanges) -> Result<(), ControlError> {
    let overdrive = pending.sclk_min.is_some()
        || pending.sclk_max.is_some()
        || pending.mclk_max.is_some()
        || pending.voltage_offset.is_some();
    if overdrive {
        set_control(gpu, Control::Performance, "manual")?;
        if let Some(value) = pending.sclk_min {
            set_control(gpu, Control::Overdrive, format!("s 0 {value}"))?;
        }
        if let Some(value) = pending.sclk_max {
            set_control(gpu, Control::Overdrive, format!("s 1 {value}"))?;
        }
        if let Some(value) = pending.mclk_max {
            set_control(gpu, Control::Overdrive, format!("m 1 {value}"))?;
        }
        if let Some(value) = pending.voltage_offset {
            set_control(gpu, Control::Overdrive, format!("vo {value}"))?;
        }
        set_control(gpu, Control::Overdrive, "c")?;
    }
    if let Some(value) = &pending.performance {
        set_control(gpu, Control::Performance, value)?;
    }
    if let Some(value) = pending.profile {
        set_control(gpu, Control::WorkloadProfile, value.to_string())?;
    }
    if let Some(value) = pending.power_watts {
        set_control(
            gpu,
            Control::PowerLimit,
            (i64::from(value) * 1_000_000).to_string(),
        )?;
    }
    if let Some(automatic) = pending.fan_automatic {
        set_control(gpu, Control::FanMode, if automatic { "2" } else { "1" })?;
    }
    if let Some(percent) = pending.fan_percent {
        let pwm = (percent.clamp(0, 100) * 255 + 50) / 100;
        set_control(gpu, Control::FanMode, "1")?;
        set_control(gpu, Control::FanPwm, pwm.to_string())?;
    }
    Ok(())
}

fn restore_snapshot(gpu: &AmdGpu, snapshot: &GpuSnapshot) -> Result<(), ControlError> {
    if gpu.control_path("pp_od_clk_voltage").is_some() {
        set_control(gpu, Control::Performance, "manual")?;
        set_control(gpu, Control::Overdrive, "r")?;
        if let Some(value) = snapshot.sclk_min {
            set_control(gpu, Control::Overdrive, format!("s 0 {value}"))?;
        }
        if let Some(value) = snapshot.sclk_max {
            set_control(gpu, Control::Overdrive, format!("s 1 {value}"))?;
        }
        if let Some(value) = snapshot.mclk_max {
            set_control(gpu, Control::Overdrive, format!("m 1 {value}"))?;
        }
        if let Some(value) = snapshot.voltage_offset {
            set_control(gpu, Control::Overdrive, format!("vo {value}"))?;
        }
        set_control(gpu, Control::Overdrive, "c")?;
    }
    if let Some(value) = snapshot.profile {
        set_control(gpu, Control::WorkloadProfile, value.to_string())?;
    }
    if let Some(value) = snapshot.power_watts {
        set_control(
            gpu,
            Control::PowerLimit,
            (i64::from(value) * 1_000_000).to_string(),
        )?;
    }
    if let Some(automatic) = snapshot.fan_automatic {
        set_control(gpu, Control::FanMode, if automatic { "2" } else { "1" })?;
        if !automatic {
            if let Some(percent) = snapshot.fan_percent {
                let pwm = (percent.clamp(0, 100) * 255 + 50) / 100;
                set_control(gpu, Control::FanPwm, pwm.to_string())?;
            }
        }
    }
    set_control(gpu, Control::Performance, &snapshot.performance)
}

fn reset_controls(gpu: &AmdGpu) -> Result<(), ControlError> {
    if gpu.control_path("pp_od_clk_voltage").is_some() {
        set_control(gpu, Control::Overdrive, "r")?;
        set_control(gpu, Control::Overdrive, "c")?;
    }
    if let Some(power) = gpu.power_limit() {
        set_control(
            gpu,
            Control::PowerLimit,
            ((power.default * 1_000_000.0).round() as i64).to_string(),
        )?;
    }
    if gpu.control_path("pwm1_enable").is_some() {
        set_control(gpu, Control::FanMode, "2")?;
    }
    if gpu.control_path("pp_power_profile_mode").is_some() {
        set_control(gpu, Control::WorkloadProfile, "0")?;
    }
    if gpu
        .control_path("power_dpm_force_performance_level")
        .is_some()
    {
        set_control(gpu, Control::Performance, "auto")?;
    }
    Ok(())
}

pub fn set_autostart_action_state(action: &gio::SimpleAction, enabled: bool) {
    if autostart::set_enabled(enabled).is_ok() {
        action.set_state(&enabled.to_variant());
    }
}
