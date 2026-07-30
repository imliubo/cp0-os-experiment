.PHONY: check test fmt image verify-image

check: fmt
	jq empty schemas/app-manifest-v1.schema.json examples/hello-card/app.json
	bash -n scripts/*.sh tests/*.sh image/build-image.sh \
		image/pi-gen/stage-cardputerzero-os/prerun.sh \
		image/pi-gen/stage-cardputerzero-os/00-bsp/01-run.sh \
		image/pi-gen/stage-cardputerzero-os/01-compositor/01-run.sh
	./tests/test-image-profile.sh
	./tests/test-compositor-profile.sh
	./tests/test-system-shell-ui.sh
	./tests/test-patch-cm0-dtb.sh
	cargo check --workspace --all-targets
	cargo test --workspace

test:
	cargo test --workspace

fmt:
	cargo fmt --all -- --check

image:
	./image/build-image.sh

verify-image:
	@cd deploy && shasum -a 256 -c SHA256SUMS
	@found=0; \
	for info_file in deploy/*.info; do \
		if [ ! -e "$$info_file" ]; then continue; fi; \
		found=1; \
		./tests/test-built-image-profile.sh "$$info_file"; \
	done; \
	if [ "$$found" -ne 1 ]; then echo "error: no deploy/*.info found" >&2; exit 1; fi
