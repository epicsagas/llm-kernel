.PHONY: test lint fmt check build clean bench bench-save bench-cmp help

test:       cargo test
lint:       cargo clippy -- -D warnings
fmt:        cargo fmt
check:      lint test
build:      cargo build --release
clean:      cargo clean

# Performance work (issue #45, ROADMAP v1.0.0 #3). Timing comparison is local-only
# — shared CI runners are too noisy for criterion's threshold. See docs/benchmarks/README.md.
bench:       cargo bench --features full                                                          ## run all benchmarks
bench-save:  cargo bench --features full -- --save-baseline main                                  ## snapshot main as the comparison baseline
bench-cmp:   cargo bench --features full -- --baseline main --noise-threshold 0.05               ## compare vs the saved 'main' baseline (5% noise floor)

help: ## Show available targets
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'
