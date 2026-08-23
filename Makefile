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
GOLDEN := tests/fixtures/golden.ndjson

# The measured prefix. With docs/state-schema.md's waived rows resynced from
# the trace, disc-core reproduces 10 ticks of the golden fixture before
# players[0].state_index goes 0 -> 20 without it -- a transition bead discr-75o
# owns. The gate is on the LENGTH of that prefix, not on the run being clean:
# a divergence that a bead owns is a boundary, and a gate that is red by design
# gets ignored. Raise this when the prefix grows; reports/core-report.md
# carries the reasoning and scripts/oracle_diff.py uses the same --min-agree
# idiom for the oracle's 275-frame boundary.
TRACE_MIN_AGREE := 10

.PHONY: core-check fmt tracecheck
core-check:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo test
	cargo run -q -p disc-tools --bin tracecheck -- \
	    $(GOLDEN) --skip-waived --min-agree $(TRACE_MIN_AGREE)
# The honest, ungated view: exits 1 on the divergence above.
tracecheck:
	cargo run -q -p disc-tools --bin tracecheck -- $(GOLDEN) --skip-waived
fmt:
	cargo fmt
