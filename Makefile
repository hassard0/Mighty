# Mighty top-level Makefile.
#
# v0.30 Track B introduced this file primarily as the home for the
# SWE-bench harness targets. Other workflows (build, test, fmt,
# clippy) still go through `cargo` directly per CONTRIBUTING.md;
# they're aliased here for convenience.

.PHONY: help build test fmt lint bench-smoke bench-full bench-clean

SWE_DIR := bench/swe
SMOKE_OUT := $(SWE_DIR)/results/smoke_$(shell date -u +%Y%m%d_%H%M%S).json
FULL_OUT := $(SWE_DIR)/results/full_$(shell date -u +%Y%m%d_%H%M%S).json

help: ## Show this help.
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

# --------------------------------------------------------------------
# Build / test / lint — thin wrappers around cargo.
# --------------------------------------------------------------------

build: ## Build the full Mighty workspace (release).
	cargo build --release --workspace

test: ## Run the full Mighty test suite.
	cargo test --workspace

fmt: ## Run rustfmt on the workspace + bench/swe crate.
	cargo fmt --all
	cd $(SWE_DIR) && cargo fmt --all

lint: ## Run clippy with `-D warnings` on workspace + bench/swe.
	cargo clippy --workspace --all-targets -- -D warnings
	cd $(SWE_DIR) && cargo clippy --all-targets -- -D warnings

# --------------------------------------------------------------------
# SWE-bench Verified — adoption-proof harness (v0.30 Track B).
# --------------------------------------------------------------------

bench-smoke: ## Run the 10-problem SWE-bench Verified smoke (~$$5-20, ~15-40min).
	@if [ -z "$$ANTHROPIC_API_KEY" ]; then \
		echo "ANTHROPIC_API_KEY required for smoke run. Set it and retry, or use 'make bench-full' (gated)."; \
		exit 1; \
	fi
	@echo "Smoke run starting — dollar cap $$25, per-instance cap $$3."
	cd $(SWE_DIR) && cargo run --release -- \
		--num-problems 10 \
		--member anthropic:claude-opus-4-7 \
		--output ../../$(SMOKE_OUT)

bench-full: ## Run the full SWE-bench Verified set (~500 problems, ~$$300-500). GATED.
	@if [ -z "$$ANTHROPIC_API_KEY" ]; then \
		echo "ANTHROPIC_API_KEY required for full run."; \
		exit 1; \
	fi
	@echo ""
	@echo "  ============================================================"
	@echo "  WARNING: FULL RUN will cost ~\$$300-500 in LLM credits."
	@echo "  Expected wall-clock: 8-16 hours."
	@echo "  Press Ctrl-C now to abort. Press Enter to continue."
	@echo "  ============================================================"
	@read confirm
	MTY_BENCH_FULL_CONFIRM=1 $(MAKE) -C $(SWE_DIR) -f /dev/null \
		.bench-full-impl OUTFILE=../../$(FULL_OUT) || \
	(cd $(SWE_DIR) && MTY_BENCH_FULL_CONFIRM=1 cargo run --release -- \
		--all \
		--member anthropic:claude-opus-4-7 \
		--dollar-cap 500 \
		--output ../../$(FULL_OUT))

bench-clean: ## Drop cached dataset rows + ephemeral checkouts (keeps results/).
	rm -rf $(SWE_DIR)/.swe-work $(SWE_DIR)/data/instances
	@echo "cleared $(SWE_DIR)/.swe-work + $(SWE_DIR)/data/instances"
