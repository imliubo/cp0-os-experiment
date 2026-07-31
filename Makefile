.PHONY: check test fmt fuzz-check fuzz-smoke compositor app-runtime appd example-app malicious-apps image verify-image

check: fmt
	jq empty schemas/app-manifest-v1.schema.json schemas/os-release-v1.schema.json \
		schemas/store-review-v1.schema.json \
		schemas/device-policy-v1.schema.json appd/device-policy.json \
		appd/device-policy-production.json \
		examples/hello-card/app.json \
		examples/device-capability-probe/app.json \
		examples/storage-isolation-probe/app.json \
		examples/store-acceptance-v1/app.json \
		examples/store-acceptance-v2/app.json
	bash -n scripts/*.sh tests/*.sh image/build-image.sh \
		image/pi-gen/export-image-prerun.sh \
		image/pi-gen/stage-cardputerzero-os/prerun.sh \
		image/pi-gen/stage-cardputerzero-os/00-bsp/01-run.sh \
		image/pi-gen/stage-cardputerzero-os/01-compositor/01-run.sh \
		image/pi-gen/stage-cardputerzero-os/02-app-platform/01-run.sh
	node --check scripts/test-store-origin.mjs
	./tests/test-image-profile.sh
	./tests/test-overlay-root-profile.sh
	./tests/test-recovery-image-profile.sh
	./tests/test-production-access-profile.sh
	./tests/test-os-update-profile.sh
	./tests/test-recovery-data.sh
	./tests/test-compositor-profile.sh
	./tests/test-system-shell-ui.sh
	./tests/test-appd-profile.sh
	./tests/test-app-platform-image.sh
	./tests/test-sdk-abi.sh
	./tests/test-sdk-c.sh
	./tests/test-sdk-lvgl.sh
	./tests/test-sdk-examples.sh
	./tests/test-simulator.sh
	./tests/test-runtime-display.sh
	./tests/test-device-diagnostics.sh
	./tests/test-stability-evidence.sh
	./tests/test-device-capability-acceptance.sh
	./tests/test-store-acceptance.sh
	./tests/test-store-origin.sh
	./tests/test-device-deployment.sh
	./tests/test-malicious-apps.sh
	./tests/test-security-validation.sh
	./tests/test-patch-cm0-dtb.sh
	cargo check --workspace --all-targets
	cargo test --workspace

test:
	cargo test --workspace

fmt:
	cargo fmt --all -- --check

fuzz-check:
	PATH="$(CURDIR)/target/fuzz-tools/bin:$$PATH" cargo +nightly fuzz check

fuzz-smoke:
	./scripts/fuzz-smoke.sh

compositor:
	docker run --rm \
		-v "$(CURDIR):/work" \
		-w /work \
		cp0-phase2b-builder:weston-14 \
		./scripts/build-compositor.sh

app-runtime:
	./scripts/build-app-runtime.sh

appd:
	./scripts/build-appd.sh

example-app:
	./scripts/build-example-app.sh

malicious-apps:
	./scripts/build-malicious-apps.sh

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
