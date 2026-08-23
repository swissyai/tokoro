.PHONY: build run check fmt lint test smoke public-surface verify audit check-windows

build:
	cargo build

run:
	cargo run --release

check:
	cargo check

fmt:
	cargo fmt --check

lint:
	cargo clippy --all-targets -- -D warnings

test:
	cargo test

smoke:
	cargo run --quiet -- help >/dev/null
	cargo run --quiet -- commands --json >/dev/null
	cargo run --quiet -- monitor --json >/dev/null
	cargo run --quiet -- visualization list --json >/dev/null
	cargo run --quiet -- visualization schema --json >/dev/null
	cargo run --quiet -- integrations --json >/dev/null
	cargo run --quiet -- handoff list --json >/dev/null

public-surface:
	@extra="$$(git ls-files | awk 'tolower($$0) ~ /\\.md$$/ && $$0 != "README.md"')"; \
	if [ -n "$$extra" ]; then \
		echo "Only README.md may be tracked as Markdown:" >&2; \
		printf '%s\n' "$$extra" >&2; \
		exit 1; \
	fi

verify: public-surface fmt lint test smoke

audit:
	cargo audit

check-windows:
	cargo check --target x86_64-pc-windows-gnu
