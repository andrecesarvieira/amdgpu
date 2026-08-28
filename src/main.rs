use adw::prelude::*;
use amdgpu_control::autostart;
use amdgpu_control::gpu::{discover_gpus, preferred_gpu_index, AmdGpu};
use amdgpu_control::settings;
use amdgpu_control::tray::{snapshot_for, TrayController};
use amdgpu_control::ui::{set_autostart_action_state, UiController};
use amdgpu_control::writer::{set_control, Control};
use amdgpu_control::{APP_ID, APP_NAME};
use glib::variant::{StaticVariantType, ToVariant};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn main() -> glib::ExitCode {
    let start_hidden = std::env::args().any(|argument| argument == "--background");
    let application = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::empty())
        .build();

    let gpus = Rc::new(discover_gpus());
    restore_settings(&gpus);
    let saved = settings::load();
    let initial_selected = preferred_gpu_index(&gpus, saved.app.selected_gpu.as_deref());
    if let Some(gpu) = gpus.get(initial_selected) {
        let _ = settings::set_selected_gpu(&gpu.pci_id());
    }

    let tray = TrayController::start(snapshot_for(&gpus, initial_selected, None))
        .map_err(|error| eprintln!("{APP_NAME}: tray indisponível: {error}"))
        .ok();
    let ui: Rc<RefCell<Option<Rc<UiController>>>> = Rc::new(RefCell::new(None));
    let first_activation = Rc::new(Cell::new(true));

    install_css(&application);
    install_actions(&application, Rc::clone(&gpus), Rc::clone(&ui), tray.clone());

    {
        let gpus = Rc::clone(&gpus);
        let ui = Rc::clone(&ui);
        let tray = tray.clone();
        let first_activation = Rc::clone(&first_activation);
        application.connect_activate(move |application| {
            let first = first_activation.replace(false);
            if first && start_hidden {
                return;
            }
            show_window(application, &gpus, &ui, tray.clone());
        });
    }

    let _hold_guard = application.hold();
    let arguments = std::env::args()
        .filter(|argument| argument != "--background")
        .collect::<Vec<_>>();
    application.run_with_args(&arguments)
}

fn show_window(
    application: &adw::Application,
    gpus: &[AmdGpu],
    slot: &Rc<RefCell<Option<Rc<UiController>>>>,
    tray: Option<TrayController>,
) {
    if slot.borrow().is_none() {
        let saved = settings::load();
        let selected = preferred_gpu_index(gpus, saved.app.selected_gpu.as_deref());
        slot.replace(Some(UiController::new(
            application,
            gpus.to_vec(),
            selected,
            tray,
        )));
    }
    if let Some(controller) = slot.borrow().as_ref() {
        controller.present();
    }
}

fn install_css(application: &adw::Application) {
    let application = application.clone();
    application.connect_startup(move |_| {
        let Some(display) = gtk::gdk::Display::default() else {
            return;
        };
        let provider = gtk::CssProvider::new();
        provider.load_from_string(
            ".metric-card {\n\
                 background-color: alpha(currentColor, 0.08);\n\
                 border-radius: 12px;\n\
                 padding: 14px;\n\
                 min-height: 64px;\n\
             }\n\
             .metric-card image { opacity: 0.72; }",
        );
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });
}

fn install_actions(
    application: &adw::Application,
    gpus: Rc<Vec<AmdGpu>>,
    ui: Rc<RefCell<Option<Rc<UiController>>>>,
    tray: Option<TrayController>,
) {
    let quit = gio::SimpleAction::new("quit", None);
    let application_quit = application.clone();
    quit.connect_activate(move |_, _| application_quit.quit());
    application.add_action(&quit);
    application.set_accels_for_action("app.quit", &["<Primary>q"]);

    let show = gio::SimpleAction::new("show", None);
    let application_show = application.clone();
    let gpus_show = Rc::clone(&gpus);
    let ui_show = Rc::clone(&ui);
    let tray_show = tray.clone();
    show.connect_activate(move |_, _| {
        show_window(&application_show, &gpus_show, &ui_show, tray_show.clone())
    });
    application.add_action(&show);

    let autostart_action =
        gio::SimpleAction::new_stateful("autostart", None, &autostart::is_enabled().to_variant());
    autostart_action.connect_activate(move |action, _| {
        let enabled = action
            .state()
            .and_then(|value| value.get::<bool>())
            .unwrap_or(true);
        set_autostart_action_state(action, !enabled);
    });
    application.add_action(&autostart_action);

    let gpu_action = gio::SimpleAction::new("tray-gpu", Some(&String::static_variant_type()));
    let gpus_gpu = Rc::clone(&gpus);
    let ui_gpu = Rc::clone(&ui);
    let tray_gpu = tray.clone();
    gpu_action.connect_activate(move |_, parameter| {
        let Some(pci_id) = parameter.and_then(|value| value.get::<String>()) else {
            return;
        };
        let Some(index) = gpus_gpu.iter().position(|gpu| gpu.pci_id() == pci_id) else {
            return;
        };
        let _ = settings::set_selected_gpu(&pci_id);
        if let Some(controller) = ui_gpu.borrow().as_ref() {
            controller.select_gpu_by_pci(&pci_id);
        }
        if let Some(tray) = &tray_gpu {
            tray.update(snapshot_for(&gpus_gpu, index, None));
        }
    });
    application.add_action(&gpu_action);

    let performance_action =
        gio::SimpleAction::new("tray-performance", Some(&String::static_variant_type()));
    let gpus_performance = Rc::clone(&gpus);
    let ui_performance = Rc::clone(&ui);
    let tray_performance = tray.clone();
    performance_action.connect_activate(move |_, parameter| {
        let Some(value) = parameter.and_then(|value| value.get::<String>()) else {
            return;
        };
        let selected = current_selected_index(&gpus_performance);
        let Some(gpu) = gpus_performance.get(selected) else {
            return;
        };
        match set_control(gpu, Control::Performance, &value) {
            Ok(()) => {
                let _ =
                    settings::update_device(&gpu.pci_id(), |saved| saved.performance = Some(value));
                refresh_external(
                    &ui_performance,
                    &tray_performance,
                    &gpus_performance,
                    selected,
                );
            }
            Err(error) => eprintln!("{APP_NAME}: {error}"),
        }
    });
    application.add_action(&performance_action);

    let profile_action = gio::SimpleAction::new("tray-profile", Some(&u32::static_variant_type()));
    let gpus_profile = Rc::clone(&gpus);
    let ui_profile = Rc::clone(&ui);
    let tray_profile = tray;
    profile_action.connect_activate(move |_, parameter| {
        let Some(value) = parameter.and_then(|value| value.get::<u32>()) else {
            return;
        };
        let selected = current_selected_index(&gpus_profile);
        let Some(gpu) = gpus_profile.get(selected) else {
            return;
        };
        match set_control(gpu, Control::WorkloadProfile, value.to_string()) {
            Ok(()) => {
                let _ = settings::update_device(&gpu.pci_id(), |saved| saved.profile = Some(value));
                refresh_external(&ui_profile, &tray_profile, &gpus_profile, selected);
            }
            Err(error) => eprintln!("{APP_NAME}: {error}"),
        }
    });
    application.add_action(&profile_action);
}

fn current_selected_index(gpus: &[AmdGpu]) -> usize {
    let saved = settings::load();
    preferred_gpu_index(gpus, saved.app.selected_gpu.as_deref())
}

fn refresh_external(
    ui: &Rc<RefCell<Option<Rc<UiController>>>>,
    tray: &Option<TrayController>,
    gpus: &[AmdGpu],
    selected: usize,
) {
    if let Some(controller) = ui.borrow().as_ref() {
        controller.refresh_after_external_change();
    } else if let Some(tray) = tray {
        tray.update(snapshot_for(gpus, selected, None));
    }
}

fn restore_settings(gpus: &[AmdGpu]) {
    let saved = settings::load();
    for gpu in gpus {
        let Some(values) = saved.devices.get(&gpu.pci_id()) else {
            continue;
        };
        let apply = |control, value: String| {
            if let Err(error) = set_control(gpu, control, value) {
                eprintln!(
                    "{APP_NAME}: falha ao restaurar {}: {error}",
                    control.as_str()
                );
                false
            } else {
                true
            }
        };

        if let Some(profile) = values.profile {
            if gpu
                .power_profiles()
                .iter()
                .any(|item| item.index == profile)
            {
                apply(Control::WorkloadProfile, profile.to_string());
            }
        }
        if let (Some(watts), Some(limit)) = (values.power_watts, gpu.power_limit()) {
            let watts = watts.clamp(limit.minimum.round() as i32, limit.maximum.round() as i32);
            apply(
                Control::PowerLimit,
                (i64::from(watts) * 1_000_000).to_string(),
            );
        }
        if let Some(automatic) = values.fan_automatic {
            if apply(
                Control::FanMode,
                if automatic { "2" } else { "1" }.to_string(),
            ) && !automatic
            {
                if let Some(percent) = values.fan_percent {
                    let pwm = (percent.clamp(0, 100) * 255 + 50) / 100;
                    apply(Control::FanPwm, pwm.to_string());
                }
            }
        }

        let mut overdrive_changed = false;
        if let Some(range) = gpu.overdrive_clock_range("sclk") {
            if let (Some(minimum), Some(maximum)) = (values.sclk_min, values.sclk_max) {
                let minimum = minimum.clamp(range.allowed_minimum, range.allowed_maximum);
                let maximum = maximum.clamp(minimum, range.allowed_maximum);
                apply(Control::Performance, "manual".to_string());
                overdrive_changed |= apply(Control::Overdrive, format!("s 0 {minimum}"));
                overdrive_changed |= apply(Control::Overdrive, format!("s 1 {maximum}"));
            }
        }
        if let (Some(maximum), Some(range)) = (values.mclk_max, gpu.overdrive_clock_range("mclk")) {
            let maximum = maximum.clamp(range.allowed_minimum, range.allowed_maximum);
            apply(Control::Performance, "manual".to_string());
            overdrive_changed |= apply(Control::Overdrive, format!("m 1 {maximum}"));
        }
        if let (Some(offset), Some((_, minimum, maximum))) =
            (values.voltage_offset, gpu.voltage_offset())
        {
            apply(Control::Performance, "manual".to_string());
            overdrive_changed |= apply(
                Control::Overdrive,
                format!("vo {}", offset.clamp(minimum, maximum)),
            );
        }
        if overdrive_changed {
            apply(Control::Overdrive, "c".to_string());
        }
        if let Some(performance) = values.performance.as_deref() {
            if matches!(performance, "auto" | "low" | "high" | "manual") {
                apply(Control::Performance, performance.to_string());
            }
        }
    }
}
