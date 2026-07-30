.PHONY: check test fmt

check: fmt
	jq empty schemas/app-manifest-v1.schema.json examples/hello-card/app.json
	bash -n scripts/*.sh image/build-image.sh image/pi-gen/stage-cardputerzero-os/prerun.sh image/pi-gen/stage-cardputerzero-os/00-bsp/01-run.sh
	./tests/test-patch-cm0-dtb.sh
	cargo check --workspace --all-targets
	cargo test --workspace

test:
	cargo test --workspace

fmt:
	cargo fmt --all -- --check
