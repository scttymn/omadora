PLUGIN ?= $(HOME)/.config/omarchy/plugins/scttymn.omadora

.PHONY: engine install pin

# Build the engine into bin/. Outside a git clone (as `make install` copies
# it), the widget prefers it to a downloaded release.
engine:
	cargo build --release --manifest-path engine/Cargo.toml
	install -Dm755 engine/target/release/omadora-engine bin/omadora-engine

# Omarchy will not load a symlinked plugin, so copy it: what git would keep,
# plus the engine just built. A running engine is stopped so the widget
# starts the new one. Reinstall from GitHub to get updates again.
install: engine
	rm -rf "$(PLUGIN)"
	mkdir -p "$(PLUGIN)"
	git ls-files -z --cached --others --exclude-standard | tar --null -T - -cf - | tar -C "$(PLUGIN)" -xf -
	install -Dm755 bin/omadora-engine "$(PLUGIN)/bin/omadora-engine"
	pkill -u "$$(id -u)" -x omadora-engine || true

# After tagging vX.Y.Z and the release workflow finishing:
#   make pin VERSION=X.Y.Z
pin:
	scripts/pin-engine "$(VERSION)"
