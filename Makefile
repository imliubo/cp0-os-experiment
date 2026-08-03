.PHONY: check test fmt fuzz-check fuzz-smoke portal-check review-console-check store-operations-check store-control-db-check compositor compositor-builder app-runtime appd example-app malicious-apps devkit image verify-image

COMPOSITOR_BUILDER_IMAGE := cp0-phase2b-builder:weston-14

check: fmt
	jq empty schemas/app-manifest-v1.schema.json schemas/os-release-v1.schema.json \
		schemas/store-review-v1.schema.json \
		schemas/store-listing-v1.schema.json \
		schemas/store-metrics-v1.schema.json \
		schemas/store-key-ceremony-v1.schema.json \
		schemas/store-control-v1.openapi.json \
		schemas/store-portal-identity-v1.openapi.json \
		schemas/store-workforce-identity-v1.openapi.json \
		schemas/device-policy-v1.schema.json appd/device-policy.json \
		appd/device-policy-production.json \
		examples/hello-card/app.json \
		examples/neon-snake/app.json \
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
	./tests/test-store-control-api.sh
	./tests/test-store-portal-identity-api.sh
	./tests/test-store-workforce-identity-api.sh
	./tests/test-store-scan-profile.sh
	./tests/test-store-publisher-profile.sh
	./tests/test-store-resilience-profile.sh
	./tests/test-store-key-ceremony.sh
	./tests/test-image-profile.sh
	./tests/test-overlay-root-profile.sh
	./tests/test-recovery-image-profile.sh
	./tests/test-production-access-profile.sh
	./tests/test-maintenance-access.sh
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
	./tests/test-app-skill.sh
	./tests/test-app-devkit.sh
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

portal-check:
	npm --prefix developer-portal run check

review-console-check:
	npm --prefix review-console run check

store-operations-check:
	npm --prefix store-operations run check

store-control-db-check:
	@test -n "$$CP0_STORE_TEST_DATABASE_URL" || \
		(echo "CP0_STORE_TEST_DATABASE_URL is required" >&2; exit 2)
	cargo test -p cp0-store-control-server --test postgres -- --ignored --nocapture
	cargo test -p cp0-store-portal-server --test postgres -- --ignored --nocapture
	cargo test -p cp0-store-workforce-server --test postgres -- --ignored --nocapture
	cargo test -p cp0-store-scan-worker --test postgres -- --ignored --nocapture
	cargo test -p cp0-store-publisher --test postgres -- --ignored --nocapture

compositor-builder:
	docker build \
		--file containers/compositor-builder/Containerfile \
		--tag "$(COMPOSITOR_BUILDER_IMAGE)" \
		containers/compositor-builder

compositor: compositor-builder
	docker run --rm \
		-v "$(CURDIR):/work" \
		-w /work \
		"$(COMPOSITOR_BUILDER_IMAGE)" \
		./scripts/build-compositor.sh

app-runtime:
	./scripts/build-app-runtime.sh

appd:
	./scripts/build-appd.sh

example-app:
	./scripts/build-example-app.sh

malicious-apps:
	./scripts/build-malicious-apps.sh

devkit:
	./scripts/package-app-devkit.sh

image:
	./image/build-image.sh

verify-image:
	@cd deploy && shasum -a 256 -c SHA256SUMS
	@info_file="$${IMAGE_INFO:-}"; \
	if [ -z "$$info_file" ]; then \
		info_file=$$(ls -1t deploy/*.info 2>/dev/null | head -n 1); \
	fi; \
	if [ -z "$$info_file" ] || [ ! -f "$$info_file" ]; then \
		echo "error: no image info selected" >&2; \
		exit 1; \
	fi; \
	echo "Verifying image profile: $$info_file"; \
	./tests/test-built-image-profile.sh "$$info_file"
