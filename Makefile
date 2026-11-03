.PHONY: all build test stress-test validate validate-inner clean install help

# Default target
all: build

# Build the project in debug mode
build:
	cargo build

# Build release version
release:
	cargo build --release

# Run all tests
test: build
	cargo test --locked --lib --bins --test e2e_tests --test migration_description_truncation --test regressions
	cargo test --locked --test random_tazk harness_initialization

# Run longer randomized/stress suites
stress-test:
	cargo test --test random_tazk

# Validation target - runs all checks before commit with timing
validate:
	@time $(MAKE) validate-inner

# Internal validation target that does the actual work
validate-inner: test
	@echo "Running cargo fmt check..."
	cargo fmt -- --check
	@echo "Running clippy..."
	cargo clippy -- -D warnings
	@echo "All validation checks passed!"

# Format code
fmt:
	cargo fmt

# Clean build artifacts
clean:
	cargo clean

# Install binary to ~/.local/bin
install: release
	mkdir -p ~/.local/bin
	cp target/release/tz ~/.local/bin/

# Show help
help:
	@echo "Tazk Makefile targets:"
	@echo "  make build     - Build debug binary"
	@echo "  make release   - Build release binary"
	@echo "  make test      - Build debug binary, then run Rust tests and shell e2e tests"
	@echo "  make validate  - Run all validation checks (test, e2e, fmt, clippy)"
	@echo "  make stress-test - Run longer randomized/stress tests"
	@echo "  make fmt       - Format code with rustfmt"
	@echo "  make clean     - Clean build artifacts"
	@echo "  make install   - Install release binary to ~/.local/bin"
	@echo "  make help      - Show this help message"
