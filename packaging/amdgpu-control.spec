%global debug_package %{nil}

Name:           amdgpu-control
Version:        2.0.0
Release:        1%{?dist}
Summary:        Native GNOME control panel for AMD GPUs

License:        GPL-3.0-or-later
URL:            https://github.com/amdgpucontrol/amdgpu-control
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo >= 1.87
BuildRequires:  rust >= 1.87
BuildRequires:  gcc
BuildRequires:  pkgconfig(gtk4) >= 4.18
BuildRequires:  pkgconfig(libadwaita-1) >= 1.7
BuildRequires:  desktop-file-utils
BuildRequires:  appstream

Requires:       gtk4 >= 4.18
Requires:       libadwaita >= 1.7
Requires:       polkit
Requires:       gnome-shell-extension-appindicator

%description
AMDGPU Control is a Rust, GTK4 and libadwaita application for monitoring and
controlling AMD GPUs through the Linux amdgpu sysfs interface. It exposes only
the controls supported by each GPU and driver, provides live telemetry,
firmware workload profiles, power-limit, fan and OverDrive controls, a
Polkit-protected helper, persistent per-GPU settings and a GNOME tray icon.

%prep
%autosetup
chmod 0644 LICENSE README.md data/*

%build
cargo build --release --locked --offline

%install
install -Dm755 target/release/amdgpu-control \
  %{buildroot}%{_bindir}/amdgpu-control
install -Dm755 target/release/amdgpu-control-helper \
  %{buildroot}%{_libexecdir}/amdgpu-control-helper
install -Dm644 data/io.github.amdgpucontrol.Control.desktop \
  %{buildroot}%{_datadir}/applications/io.github.amdgpucontrol.Control.desktop
install -Dm644 data/io.github.amdgpucontrol.Control-autostart.desktop \
  %{buildroot}%{_sysconfdir}/xdg/autostart/io.github.amdgpucontrol.Control.desktop
install -Dm644 data/io.github.amdgpucontrol.Control.metainfo.xml \
  %{buildroot}%{_metainfodir}/io.github.amdgpucontrol.Control.metainfo.xml
install -Dm644 data/io.github.amdgpucontrol.Control.svg \
  %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.amdgpucontrol.Control.svg
install -Dm644 data/io.github.amdgpucontrol.Control-symbolic.svg \
  %{buildroot}%{_datadir}/icons/hicolor/symbolic/apps/io.github.amdgpucontrol.Control-symbolic.svg
install -Dm644 data/io.github.amdgpucontrol.Control-gpu-symbolic.svg \
  %{buildroot}%{_datadir}/icons/hicolor/symbolic/apps/io.github.amdgpucontrol.Control-gpu-symbolic.svg
install -Dm644 data/io.github.amdgpucontrol.policy \
  %{buildroot}%{_datadir}/polkit-1/actions/io.github.amdgpucontrol.policy

%check
cargo test --all-targets --release --locked --offline
desktop-file-validate \
  %{buildroot}%{_datadir}/applications/io.github.amdgpucontrol.Control.desktop
desktop-file-validate \
  %{buildroot}%{_sysconfdir}/xdg/autostart/io.github.amdgpucontrol.Control.desktop
appstreamcli validate --no-net \
  %{buildroot}%{_metainfodir}/io.github.amdgpucontrol.Control.metainfo.xml

%files
%license LICENSE
%doc README.md
%{_bindir}/amdgpu-control
%{_libexecdir}/amdgpu-control-helper
%{_datadir}/applications/io.github.amdgpucontrol.Control.desktop
%config(noreplace) %{_sysconfdir}/xdg/autostart/io.github.amdgpucontrol.Control.desktop
%{_metainfodir}/io.github.amdgpucontrol.Control.metainfo.xml
%{_datadir}/icons/hicolor/scalable/apps/io.github.amdgpucontrol.Control.svg
%{_datadir}/icons/hicolor/symbolic/apps/io.github.amdgpucontrol.Control-symbolic.svg
%{_datadir}/icons/hicolor/symbolic/apps/io.github.amdgpucontrol.Control-gpu-symbolic.svg
%{_datadir}/polkit-1/actions/io.github.amdgpucontrol.policy

%changelog
* Fri Aug 28 2026 AMDGPU Control contributors <maintainers@example.invalid> - 2.0.0-1
- Rewrite the application, privileged helper and tray integration in Rust
- Redesign the native GTK interface around detected hardware capabilities
- Keep the tray idle and suspend telemetry reads while the window is unfocused
- Preserve settings compatibility and per-GPU restoration at session startup
