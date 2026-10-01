# The repo's tasks in one place. Cargo does the building; the scripts do the
# procedures (a live smoke test, a release, the plugin's start-up).

.PHONY: build build-release test lint check smoke smoke-press install uninstall plugin plugin-reload plugin-remove release release-dry preview dist clean

PLUGIN_ID = io.github.iluxav.omaestro
PLUGIN_LINK = $(HOME)/.config/omarchy/plugins/$(PLUGIN_ID)

build:            ## debug binary at target/debug/om
	cargo build

build-release:    ## optimized binary at target/release/om
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
	@echo
	@echo "omaestro is running. Plugins to start with:"
	@echo "  om plugin available                              what there is"
	@echo "  om plugin add panel window-halves text-tools      the starter set (SUPER+ALT+O opens the panel)"

uninstall:        ## the reverse; your rules in ~/.config/omaestro stay
	-systemctl --user disable --now omaestro
	rm -f $(HOME)/.config/systemd/user/omaestro.service
	systemctl --user daemon-reload
	-cargo uninstall omaestro

plugin:           ## link this checkout into the Omarchy shell as the plugin (panel + service) and enable it
	@if [ -d "$(PLUGIN_LINK)" ] && [ ! -L "$(PLUGIN_LINK)" ]; then \
	  echo "$(PLUGIN_LINK) is a directory (a marketplace install?), not a link;"; \
	  echo "remove it first: omarchy plugin remove $(PLUGIN_ID) --yes"; exit 1; fi
	ln -sfn $(CURDIR) $(PLUGIN_LINK)
	-omarchy-shell shell rescanPlugins
	@# The rescan runs in the background; enabling an unknown id fails.
	@for _ in $$(seq 50); do \
	  omarchy-plugin-catalog 2>/dev/null | jq -e --arg id '$(PLUGIN_ID)' 'any(.[]; .id == $$id)' >/dev/null && break; \
	  sleep 0.1; \
	done
	omarchy plugin enable $(PLUGIN_ID)
	@echo "open the panel with: omarchy-shell shell toggle $(PLUGIN_ID)"

plugin-reload:    ## make the shell re-read Panel.qml and Service.qml: a shell restart (the plugin is keepLoaded)
	omarchy restart shell

plugin-remove:    ## the reverse: disable it and remove the link; this checkout stays
	-omarchy plugin disable $(PLUGIN_ID)
	rm -f $(PLUGIN_LINK)
	-omarchy-shell shell rescanPlugins

release:          ## bump the version, commit, tag vX.Y.Z, push; CI builds and publishes. BUMP=patch|minor|major or VERSION=x.y.z
	scripts/release.sh $(if $(VERSION),--version $(VERSION),--bump $(or $(BUMP),patch))

release-dry:      ## the checks and the plan for `make release`, without changing anything
	scripts/release.sh --dry-run $(if $(VERSION),--version $(VERSION),--bump $(or $(BUMP),patch))

preview:          ## preview.png for the marketplace: the panel in a nested Hyprland (run inside a session)
	scripts/preview.sh

dist:             ## release binary for this machine's arch plus its SHA256 in release.sha256, by hand
	scripts/dist.sh

clean:
	cargo clean
	rm -rf dist

help:
	@grep -E '^[a-z-]+:.*##' $(MAKEFILE_LIST) | sed 's/:.*##/:/' | column -t -s:
