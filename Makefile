# The repo's tasks in one place. Cargo does the building; the scripts do the
# procedures (a live smoke test, a release, the plugin's start-up).

.PHONY: build release test lint check smoke smoke-press install uninstall plugin plugin-reload plugin-remove dist clean

PLUGIN_ID = io.github.iluxav.omaestro
PLUGIN_LINK = $(HOME)/.config/omarchy/plugins/$(PLUGIN_ID)

build:            ## debug binary at target/debug/om
	cargo build

release:          ## optimized binary at target/release/om
	cargo build --release

test:             ## unit tests, no display or Hyprland needed
	cargo test

lint:             ## formatting and clippy, as CI would run them
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

check: lint test  ## everything that runs without a session

smoke:            ## live checks in a nested Hyprland (run inside a session)
	scripts/smoke.sh

smoke-press:      ## the same, plus a hotkey you press by hand
	scripts/smoke.sh --press

install:          ## om into ~/.cargo/bin and the systemd user unit, enabled
	cargo install --path .
	install -Dm644 systemd/omaestro.service $(HOME)/.config/systemd/user/omaestro.service
	systemctl --user daemon-reload
	systemctl --user enable --now omaestro

uninstall:        ## the reverse; your rules in ~/.config/omaestro stay
	-systemctl --user disable --now omaestro
	rm -f $(HOME)/.config/systemd/user/omaestro.service
	systemctl --user daemon-reload
	-cargo uninstall omaestro

plugin:           ## link this checkout into the Omarchy shell as the plugin (panel + service) and enable it
	ln -sfn $(CURDIR) $(PLUGIN_LINK)
	-omarchy-shell shell rescanPlugins
	@# The rescan runs in the background; enabling an unknown id fails.
	@for _ in $$(seq 50); do \
	  omarchy-plugin-catalog 2>/dev/null | jq -e --arg id '$(PLUGIN_ID)' 'any(.[]; .id == $$id)' >/dev/null && break; \
	  sleep 0.1; \
	done
	omarchy plugin enable $(PLUGIN_ID)
	@echo "open the panel with: omarchy-shell shell toggle $(PLUGIN_ID)"

plugin-reload:    ## make the shell re-read Panel.qml and Service.qml (its file watch does not follow the link)
	omarchy-shell shell rescanPlugins

plugin-remove:    ## the reverse: disable it and remove the link; this checkout stays
	-omarchy plugin disable $(PLUGIN_ID)
	rm -f $(PLUGIN_LINK)
	-omarchy-shell shell rescanPlugins

dist:             ## release binary for this machine's arch plus its SHA256 in release.sha256
	scripts/release.sh

clean:
	cargo clean
	rm -rf dist

help:
	@grep -E '^[a-z-]+:.*##' $(MAKEFILE_LIST) | sed 's/:.*##/:/' | column -t -s:
