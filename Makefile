.PHONY: check test fmt

check: fmt
	jq empty schemas/app-manifest-v1.schema.json examples/hello-card/app.json
	cargo check --workspace --all-targets
	cargo test --workspace

test:
	cargo test --workspace

fmt:
	cargo fmt --all -- --check
