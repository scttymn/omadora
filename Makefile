PLUGIN ?= $(HOME)/.config/omarchy/plugins/scttymn.omadora

.PHONY: engine install pin

# Build the engine into bin/, where the widget looks for it. Without the
# release mark, the widget keeps this build instead of downloading one.
engine:
	cargo build --release --manifest-path engine/Cargo.toml
	install -Dm755 engine/target/release/omadora-engine bin/omadora-engine
	rm -f bin/omadora-engine.release

# Omarchy will not load a symlinked plugin, so copy it. A running engine is
# stopped so the widget starts the new one.
install: engine
	rm -rf $(PLUGIN)
	mkdir -p $(PLUGIN)
	tar --exclude .git --exclude engine/target -cf - . | tar -C $(PLUGIN) -xf -
	pkill -x omadora-engine || true

# After tagging vX.Y.Z and the release workflow finishing:
#   make pin VERSION=X.Y.Z
pin:
	scripts/pin-engine $(VERSION)
