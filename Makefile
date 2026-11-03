.PHONY: fetch-spec test fmt clippy coverage docs generated sim check

fetch-spec:
	./scripts/fetch-spec.sh

test:
	cargo test --workspace

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

coverage:
	./scripts/coverage.sh

docs:
	cargo run -p uci-docs --bin build-site
	python3 scripts/check_site.py
	python3 scripts/contrast.py

generated:
	cargo run -p uci-codegen --bin gen
	cargo test --manifest-path examples/generated/uci-types/Cargo.toml

sim:
	cargo run -p uci-cli -- sim examples/scenarios/tasking.toml --embedded

check: fmt clippy test generated coverage docs
