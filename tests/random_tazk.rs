//! Cargo test wrapper for random property-based testing
//!
//! This test invokes the test_tazk binary with the random-actions subcommand.
//! For manual testing with custom parameters, use the binary directly:
//!
//!   cargo build --locked --bin tz --bin test_tazk --features test-tools
//!   ./target/debug/test_tazk random-actions --seed 42 --verbose
//!   ./target/debug/test_tazk random-actions --seed-from-entropy --iters 10

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

#[derive(serde::Deserialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
enum CargoMessage {
    CompilerArtifact {
        target: CargoTarget,
        executable: Option<PathBuf>,
    },
    #[serde(other)]
    Other,
}

#[derive(serde::Deserialize)]
struct CargoTarget {
    name: CargoBinary,
}

#[derive(serde::Deserialize)]
enum CargoBinary {
    #[serde(rename = "test_tazk")]
    Harness,
    #[serde(other)]
    Other,
}

fn harness_build_command() -> Command {
    let mut command = Command::new(env!("CARGO"));
    command.current_dir(env!("CARGO_MANIFEST_DIR")).args([
        "build",
        "--locked",
        "--profile",
        "dev",
        "--bin",
        "tz",
        "--bin",
        "test_tazk",
        "--features",
        "test-tools",
        "--message-format=json",
    ]);
    command
}

fn harness_executable(messages: &[u8]) -> anyhow::Result<PathBuf> {
    for line in messages
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        if let CargoMessage::CompilerArtifact {
            target: CargoTarget {
                name: CargoBinary::Harness,
            },
            executable: Some(path),
        } = serde_json::from_slice(line)?
        {
            return Ok(path);
        }
    }
    anyhow::bail!("Cargo did not report the test_tazk executable");
}

fn initialize_harness(
    build: &OnceLock<anyhow::Result<PathBuf>>,
    builder: impl FnOnce() -> anyhow::Result<PathBuf>,
) -> Result<&Path, &anyhow::Error> {
    build.get_or_init(builder).as_deref()
}

fn build_tazk() -> &'static Path {
    static BUILD: OnceLock<anyhow::Result<PathBuf>> = OnceLock::new();
    initialize_harness(&BUILD, || {
        let output = harness_build_command().output()?;
        anyhow::ensure!(
            output.status.success(),
            "Failed to build tazk binaries: {}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        harness_executable(&output.stdout)
    })
    .expect("Failed to initialize tazk test harness")
}

/// Run test_tazk random-actions with specified arguments
fn run_test(binary_path: &Path, args: &[&str], test_name: &str) {
    println!(
        "\nRunning: {} random-actions {}",
        binary_path.display(),
        args.join(" ")
    );

    let output = Command::new(binary_path)
        .arg("random-actions")
        .args(args)
        .output()
        .expect("Failed to execute test_tazk");

    // Print output for debugging
    println!("{}", String::from_utf8_lossy(&output.stdout));
    if !output.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    }

    assert!(
        output.status.success(),
        "{} failed with exit code: {:?}",
        test_name,
        output.status.code()
    );
}

#[test]
#[cfg(not(tarpaulin))] // Skip under coverage - this test invokes external binaries
fn test_random_actions_tazk_numeric() {
    let binary_path = build_tazk();
    run_test(
        binary_path,
        &["--seed", "42", "--ids", "numeric"],
        "Random test against tazk with numeric IDs",
    );
}

#[test]
#[cfg(not(tarpaulin))] // Skip under coverage - this test invokes external binaries
fn test_random_actions_tazk_hash() {
    let binary_path = build_tazk();
    run_test(
        binary_path,
        &["--seed", "42", "--ids", "hash"],
        "Random test against tazk with hash IDs",
    );
}

#[test]
#[cfg(not(tarpaulin))] // Skip under coverage - this test invokes external binaries
fn test_stress_tazk_parallel_numeric() {
    let binary_path = build_tazk();
    run_test(
        binary_path,
        &[
            "--seed",
            "42",
            "--seconds",
            "3",
            "--parallel=3",
            "--ids",
            "numeric",
        ],
        "Parallel stress test against tazk with numeric IDs",
    );
}

#[test]
#[cfg(not(tarpaulin))] // Skip under coverage - this test invokes external binaries
fn test_stress_tazk_parallel_hash() {
    let binary_path = build_tazk();
    run_test(
        binary_path,
        &[
            "--seed",
            "42",
            "--seconds",
            "3",
            "--parallel=3",
            "--ids",
            "hash",
        ],
        "Parallel stress test against tazk with hash IDs",
    );
}

#[test]
#[cfg(not(tarpaulin))] // Skip under coverage - this test invokes external binaries
fn test_migration_stress() {
    let binary_path = build_tazk();
    run_migration_test(
        binary_path,
        &["--seed", "54321", "--actions", "50"],
        "Hash ID migration stress test (50 actions, then migrate)",
    );
}

/// Run migration test: generate numeric state, migrate to hash, verify
fn run_migration_test(binary_path: &Path, args: &[&str], test_name: &str) {
    println!(
        "\nRunning: {} migration-test {}",
        binary_path.display(),
        args.join(" ")
    );

    let output = Command::new(binary_path)
        .arg("migration-test")
        .args(args)
        .output()
        .expect("Failed to execute test_tazk migration-test");

    // Print output for debugging
    println!("{}", String::from_utf8_lossy(&output.stdout));
    if !output.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    }

    assert!(
        output.status.success(),
        "{} failed with exit code: {:?}",
        test_name,
        output.status.code()
    );
}

mod harness_initialization {
    use super::*;
    use std::ffi::OsStr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;

    #[test]
    fn concurrent_callers_build_once() {
        let build = OnceLock::new();
        let calls = AtomicUsize::new(0);
        let start = Barrier::new(16);

        std::thread::scope(|scope| {
            for _ in 0..16 {
                scope.spawn(|| {
                    start.wait();
                    let binary = initialize_harness(&build, || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(PathBuf::from("custom-target/debug/test_tazk"))
                    })
                    .unwrap();
                    assert_eq!(binary, Path::new("custom-target/debug/test_tazk"));
                });
            }
        });

        initialize_harness(&build, || panic!("Completed build must be reused")).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failed_build_is_not_retried() {
        let build = OnceLock::new();
        assert!(initialize_harness(&build, || anyhow::bail!("build failed")).is_err());
        let error =
            initialize_harness(&build, || panic!("Failed build must be cached")).unwrap_err();
        assert_eq!(error.to_string(), "build failed");
    }

    #[test]
    fn both_binaries_share_one_debug_build() {
        let command = harness_build_command();
        assert_eq!(
            command.get_current_dir(),
            Some(Path::new(env!("CARGO_MANIFEST_DIR")))
        );
        assert!(command.get_args().eq([
            "build",
            "--locked",
            "--profile",
            "dev",
            "--bin",
            "tz",
            "--bin",
            "test_tazk",
            "--features",
            "test-tools",
            "--message-format=json",
        ]
        .map(OsStr::new)));
    }

    #[test]
    fn cargo_artifact_preserves_target_directory_and_executable_suffix() {
        for executable in [
            "custom target/debug/test_tazk",
            "custom target/debug/test_tazk.exe",
        ] {
            let messages = format!(
                "{{\"reason\":\"compiler-message\"}}\n\
                 {{\"reason\":\"compiler-artifact\",\"target\":{{\"name\":\"tz\"}},\"executable\":\"custom target/debug/tz\"}}\n\
                 {{\"reason\":\"compiler-artifact\",\"target\":{{\"name\":\"test_tazk\"}},\"executable\":\"{executable}\"}}\n"
            );
            assert_eq!(
                harness_executable(messages.as_bytes()).unwrap(),
                Path::new(executable)
            );
        }
    }

    #[test]
    fn missing_harness_artifact_is_rejected() {
        assert!(harness_executable(b"{\"reason\":\"build-finished\",\"success\":true}\n").is_err());
    }
}
