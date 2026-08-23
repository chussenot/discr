# Disc reverse-engineering pipeline.
.PHONY: oracle oracle-check clean
oracle:
	$(MAKE) -C oracle
oracle-check: oracle
	./scripts/oracle_check.sh
clean:
	$(MAKE) -C oracle clean

# --- Rust workspace (disc-core / disc-tools). disc-app is excluded from
# --- default-members so a missing macroquad system dep cannot break this.
.PHONY: core-check fmt
core-check:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo test
fmt:
	cargo fmt
