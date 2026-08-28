PREFIX ?= /usr
DESTDIR ?=
CARGO ?= cargo

.PHONY: build install uninstall test run rpm

build:
	$(CARGO) build --release --locked --offline

install: build
	install -Dm755 target/release/amdgpu-control "$(DESTDIR)$(PREFIX)/bin/amdgpu-control"
	install -Dm755 target/release/amdgpu-control-helper "$(DESTDIR)$(PREFIX)/libexec/amdgpu-control-helper"
	install -Dm644 data/io.github.amdgpucontrol.Control.desktop "$(DESTDIR)$(PREFIX)/share/applications/io.github.amdgpucontrol.Control.desktop"
	install -Dm644 data/io.github.amdgpucontrol.Control.metainfo.xml "$(DESTDIR)$(PREFIX)/share/metainfo/io.github.amdgpucontrol.Control.metainfo.xml"
	install -Dm644 data/io.github.amdgpucontrol.Control.svg "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/io.github.amdgpucontrol.Control.svg"
	install -Dm644 data/io.github.amdgpucontrol.Control-symbolic.svg "$(DESTDIR)$(PREFIX)/share/icons/hicolor/symbolic/apps/io.github.amdgpucontrol.Control-symbolic.svg"
	install -Dm644 data/io.github.amdgpucontrol.Control-gpu-symbolic.svg "$(DESTDIR)$(PREFIX)/share/icons/hicolor/symbolic/apps/io.github.amdgpucontrol.Control-gpu-symbolic.svg"
	install -Dm644 data/io.github.amdgpucontrol.Control-autostart.desktop "$(DESTDIR)/etc/xdg/autostart/io.github.amdgpucontrol.Control.desktop"
	install -Dm644 data/io.github.amdgpucontrol.policy "$(DESTDIR)$(PREFIX)/share/polkit-1/actions/io.github.amdgpucontrol.policy"

uninstall:
	rm -f "$(DESTDIR)$(PREFIX)/bin/amdgpu-control"
	rm -f "$(DESTDIR)$(PREFIX)/libexec/amdgpu-control-helper"
	rm -f "$(DESTDIR)$(PREFIX)/share/applications/io.github.amdgpucontrol.Control.desktop"
	rm -f "$(DESTDIR)$(PREFIX)/share/metainfo/io.github.amdgpucontrol.Control.metainfo.xml"
	rm -f "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/io.github.amdgpucontrol.Control.svg"
	rm -f "$(DESTDIR)$(PREFIX)/share/icons/hicolor/symbolic/apps/io.github.amdgpucontrol.Control-symbolic.svg"
	rm -f "$(DESTDIR)$(PREFIX)/share/icons/hicolor/symbolic/apps/io.github.amdgpucontrol.Control-gpu-symbolic.svg"
	rm -f "$(DESTDIR)/etc/xdg/autostart/io.github.amdgpucontrol.Control.desktop"
	rm -f "$(DESTDIR)$(PREFIX)/share/polkit-1/actions/io.github.amdgpucontrol.policy"

test:
	$(CARGO) fmt --all -- --check
	$(CARGO) test --all-targets --locked --offline
	$(CARGO) clippy --all-targets --locked --offline -- -D warnings

run:
	$(CARGO) run --release --locked --offline --bin amdgpu-control

rpm:
	./scripts/build-rpm.sh
