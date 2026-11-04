# Tests

Use debug builds for local development and validation.

- `make validate` runs the routine Rust tests, the shell-test fixtures (`tests/*.sh`),
  migration/regression suites, deterministic harness-initialization checks,
  formatting, and Clippy.
- `cargo test --locked --test regressions` exercises CLI/storage integration
  regressions in isolated temporary databases.
- `cargo test --locked --bin tz issue_merge diff3 merge_driver` runs the git
  merge-driver suites: explicit conflict cases plus seeded property tests
  (commutativity, one-side identity, re-merge idempotence against a
  paragraph-diff3 oracle, machine-resolvable hunks, and exactly the expected
  comment set over thousands of generated edit pairs). `merge_driver.sh` drives
  real `git merge`s through an installed driver.
- `cargo test --locked --test random_tazk harness_initialization` checks
  concurrent test-binary initialization without running the stress workload.
- `cargo test --locked --test random_tazk tazk` runs the native
  numeric/hash random and parallel stress cases. The wrapper builds both debug
  tools together once before any worker executes them, so another test cannot
  replace a binary while it is running.
- `cargo test --locked --test random_tazk test_migration_stress` runs the
  randomized migration case.

The wrapper respects Cargo's target-directory configuration. A plain
`cargo test` runs the full random-wrapper suite, while routine validation
includes only its deterministic initialization regressions. Add regressions to
routine validation rather than relying on temporary manual test commands.
