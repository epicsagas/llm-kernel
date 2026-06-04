.PHONY: test lint fmt check build clean help

test:       cargo test
lint:       cargo clippy -- -D warnings
fmt:        cargo fmt
check:      lint test
build:      cargo build --release
clean:      cargo clean

help: ## Show available targets
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'
