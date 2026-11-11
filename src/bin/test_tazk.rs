//! Test toolkit for tazk
//!
//! This binary provides various testing utilities for tazk.
//!
//! Usage:
//!   test_tazk random-actions [OPTIONS]
//!   test_tazk --help

use anyhow::Result;
use clap::{Parser, Subcommand};
use rand::RngCore;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tazk::action_generator::{ActionExecutor, ActionGenerator, ReferenceInterpreter, TazkAction};
use tazk::types::ID_PREFIX;

/// Verbosity level for logging
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogLevel {
    Normal,
    Verbose,
}

/// Logger that buffers output and can dump on failure
#[derive(Clone)]
struct Logger {
    buffer: Arc<Mutex<Vec<String>>>,
    verbosity: LogLevel,
    buffering: bool,
}

impl Logger {
    fn new(verbosity: LogLevel, buffering: bool) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(Vec::new())),
            verbosity,
            buffering,
        }
    }

    /// Log a message at normal level
    fn log(&self, msg: String) {
        self.log_at_level(LogLevel::Normal, msg);
    }

    /// Log a message at verbose level (only shown if verbosity is Verbose)
    fn verbose(&self, msg: String) {
        self.log_at_level(LogLevel::Verbose, msg);
    }

    /// Log a message at the specified level
    fn log_at_level(&self, level: LogLevel, msg: String) {
        if self.buffering {
            // Buffer all messages regardless of level
            let mut buffer = self.buffer.lock().unwrap();
            buffer.push(msg);
        } else {
            // Only print if the message level is within our verbosity
            if level == LogLevel::Normal || self.verbosity == LogLevel::Verbose {
                println!("{}", msg);
            }
        }
    }

    /// Dump all buffered output to stdout
    fn dump(&self) {
        let buffer = self.buffer.lock().unwrap();
        for msg in buffer.iter() {
            println!("{}", msg);
        }
    }

    /// Clear the buffer without dumping
    fn clear(&self) {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.clear();
    }
}

#[derive(Parser, Debug)]
#[command(name = "test_tazk")]
#[command(about = "Test toolkit for tazk")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Migration testing: generate numeric state, migrate to hash IDs, verify
    #[command(name = "migration-test")]
    MigrationTest {
        /// Seed for deterministic RNG
        #[arg(long)]
        seed: Option<u64>,

        /// Use entropy for random seed (non-deterministic)
        #[arg(long)]
        seed_from_entropy: bool,

        /// Number of actions to generate before migration
        #[arg(long, default_value = "50")]
        actions: usize,

        /// Enable verbose output (print action sequence and detailed checks)
        #[arg(long, short)]
        verbose: bool,
    },

    /// Random property-based testing for tazk
    #[command(name = "random-actions")]
    RandomActions {
        /// Seed for deterministic RNG
        #[arg(long)]
        seed: Option<u64>,

        /// Use entropy for random seed (non-deterministic)
        #[arg(long)]
        seed_from_entropy: bool,

        /// Number of iterations to run (each with a different seed)
        /// Mutually exclusive with --seconds
        #[arg(long, default_value = "1", conflicts_with = "seconds")]
        iters: usize,

        /// Run stress test for specified number of seconds
        /// Mutually exclusive with --iters
        #[arg(long, conflicts_with = "iters")]
        seconds: Option<u64>,

        /// Number of actions to generate per iteration
        #[arg(long, default_value = "20")]
        actions_per_iter: usize,

        /// Path to binary (overrides --impl default)
        #[arg(long)]
        binary: Option<String>,

        /// Enable verbose output (print action sequence and detailed checks)
        #[arg(long, short)]
        verbose: bool,

        /// Run stress tests in parallel on N cores (default: number of system cores)
        /// Only works with --seconds mode
        #[arg(long, value_name = "N", num_args = 0..=1, default_missing_value = "0", require_equals = true)]
        parallel: Option<usize>,

        /// ID generation mode: numeric (sequential) or hash (content-based)
        #[arg(long, default_value = "numeric")]
        ids: IdMode,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum IdMode {
    Numeric,
    Hash,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Determine worker thread count based on parallel flag
    // For parallel mode, we want worker threads = min(parallel_workers, num_cores)
    // to avoid wasting resources when parallel < cores
    let worker_threads = if let Commands::RandomActions {
        parallel: Some(n), ..
    } = &cli.command
    {
        let num_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let requested = if *n == 0 { num_cores } else { *n };
        std::cmp::min(requested, num_cores)
    } else {
        // Default: use all available cores for tokio runtime
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    };

    // Build tokio runtime with configured worker threads
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_all()
        .build()?;

    runtime.block_on(async {
        match cli.command {
            Commands::MigrationTest {
                seed,
                seed_from_entropy,
                actions,
                verbose,
            } => run_migration_test(seed, seed_from_entropy, actions, verbose).await,
            Commands::RandomActions {
                seed,
                seed_from_entropy,
                iters,
                seconds,
                actions_per_iter,
                binary,
                verbose,
                parallel,
                ids,
            } => {
                run_random_actions(
                    seed,
                    seed_from_entropy,
                    iters,
                    seconds,
                    actions_per_iter,
                    binary,
                    verbose,
                    parallel,
                    ids,
                )
                .await
            }
        }
    })
}

/// Migration test - generate numeric state, migrate to hash IDs, verify
async fn run_migration_test(
    seed: Option<u64>,
    seed_from_entropy: bool,
    num_actions: usize,
    verbose: bool,
) -> Result<()> {
    // Sample entropy ONCE at the beginning if requested
    let base_seed = if seed_from_entropy {
        let mut rng = rand::thread_rng();
        let entropy_seed = rng.next_u64();
        println!("🎲 Sampled entropy seed: {}", entropy_seed);
        println!("   (Use --seed {} to reproduce this run)\n", entropy_seed);
        entropy_seed
    } else {
        seed.unwrap_or(42u64) // Default seed
    };

    println!("🔄 Hash ID Migration Test");
    println!("Seed: {}", base_seed);
    println!("Actions: {}", num_actions);
    println!();

    // Find binary path
    let exe_path = std::env::current_exe()?;
    let exe_dir = exe_path
        .parent()
        .expect("Failed to get executable directory");
    let tazk_binary = exe_dir.join(format!("tz{}", std::env::consts::EXE_SUFFIX));

    // Validate binary exists
    if !tazk_binary.exists() {
        eprintln!("ERROR: Tazk binary not found at: {}", tazk_binary.display());
        eprintln!("Build it first with: cargo build");
        std::process::exit(1);
    }

    println!("📦 Tazk: {}", tazk_binary.display());
    println!();

    // Create working directory
    let temp_dir = tempfile::tempdir()?;
    let work_dir = temp_dir.path();
    println!("📁 Working directory: {}", work_dir.display());
    println!();

    let verbosity = if verbose {
        LogLevel::Verbose
    } else {
        LogLevel::Normal
    };
    let logger = Logger::new(verbosity, false);

    // Phase 1: Initialize with numeric IDs and populate
    println!("{}", "=".repeat(70));
    println!("Phase 1: Initialize with numeric IDs and populate");
    println!("{}", "=".repeat(70));

    let mut generator = ActionGenerator::new_with_mode(base_seed, false); // numeric mode
    let mut reference = ReferenceInterpreter::new();

    let executor = ActionExecutor::new(tazk_binary.to_str().unwrap(), work_dir.to_str().unwrap());

    let actions = generator.generate_sequence(num_actions);
    logger.log(format!("Generated {} actions", actions.len()));

    execute_actions(&executor, &actions, &mut reference, &logger)?;

    let initial_issue_count = reference.get_final_state().len();
    println!(
        "✅ Phase 1 complete: {} issues created with numeric IDs",
        initial_issue_count
    );

    // Phase 2: Migrate to hash IDs
    println!("\n{}", "=".repeat(70));
    println!("Phase 2: Migrate to hash IDs");
    println!("{}", "=".repeat(70));

    logger.log("Running tz migrate...".to_string());

    let migrate_output = std::process::Command::new(&tazk_binary)
        .current_dir(work_dir)
        .args(["migrate"])
        .output()?;

    if !migrate_output.status.success() {
        eprintln!(
            "❌ Migration failed:\n{}",
            String::from_utf8_lossy(&migrate_output.stderr)
        );
        return Err(anyhow::anyhow!("Migration failed"));
    }

    let migration_output = String::from_utf8_lossy(&migrate_output.stdout);
    logger.log(format!("Migration output:\n{}", migration_output));

    // Extract number of migrated issues from output
    let migrated_count = migration_output
        .lines()
        .find(|line| line.contains("Successfully migrated"))
        .and_then(|line| {
            line.split_whitespace()
                .find(|word| word.parse::<usize>().is_ok())
                .and_then(|num_str| num_str.parse::<usize>().ok())
        })
        .unwrap_or(0);

    println!(
        "✅ Phase 2 complete: Migrated {} issues to hash IDs",
        migrated_count
    );

    // Phase 3: Verify migration
    println!("\n{}", "=".repeat(70));
    println!("Phase 3: Verify migration");
    println!("{}", "=".repeat(70));

    logger.log("Verifying migration integrity...".to_string());

    // Count issues after migration
    let list_output = std::process::Command::new(&tazk_binary)
        .current_dir(work_dir)
        .args(["list"])
        .output()?;

    if !list_output.status.success() {
        return Err(anyhow::anyhow!(
            "Failed to list issues after migration:\n{}",
            String::from_utf8_lossy(&list_output.stderr)
        ));
    }

    let list_text = String::from_utf8_lossy(&list_output.stdout);
    let post_migration_count = list_text.lines().filter(|line| !line.is_empty()).count();

    // Verify issue count matches
    if post_migration_count != initial_issue_count {
        eprintln!(
            "❌ Issue count mismatch! Before: {}, After: {}",
            initial_issue_count, post_migration_count
        );
        return Err(anyhow::anyhow!(
            "Migration lost issues: {} before, {} after",
            initial_issue_count,
            post_migration_count
        ));
    }

    logger.log(format!(
        "✓ Issue count preserved: {} issues",
        post_migration_count
    ));

    // Verify all IDs are now hash-based and use base36 encoding
    let mut invalid_ids = Vec::new();
    let mut extended_base36_count = 0;

    for line in list_text.lines() {
        if let Some(id) = line.split(':').next() {
            let id = id.trim();
            if id.is_empty() {
                continue;
            }

            // Check if ID is a valid base36 hash ID (always contains a letter)
            if !is_base36_hash_id(id) {
                invalid_ids.push(id.to_string());
            } else if hash_suffix(id).chars().any(|c| matches!(c, 'g'..='z')) {
                // Count IDs with extended base36 chars (g-z) as confirmation
                extended_base36_count += 1;
            }
        }
    }

    if !invalid_ids.is_empty() {
        eprintln!(
            "❌ Migration failed: Found {} invalid hash IDs: {:?}",
            invalid_ids.len(),
            &invalid_ids[..std::cmp::min(3, invalid_ids.len())]
        );
        return Err(anyhow::anyhow!(
            "Migration produced invalid hash IDs: {} IDs affected",
            invalid_ids.len()
        ));
    }

    if extended_base36_count > 0 {
        logger.log(format!(
            "✓ All issue IDs use base36 encoding ({} IDs with extended chars g-z)",
            extended_base36_count
        ));
    } else {
        logger.log("✓ All issue IDs use valid base36 characters".to_string());
    }

    // Verify dependencies are intact by checking a few issues
    let show_output = std::process::Command::new(&tazk_binary)
        .current_dir(work_dir)
        .args(["stats"])
        .output()?;

    if !show_output.status.success() {
        return Err(anyhow::anyhow!(
            "Failed to get stats after migration:\n{}",
            String::from_utf8_lossy(&show_output.stderr)
        ));
    }

    logger.log(format!(
        "Stats after migration:\n{}",
        String::from_utf8_lossy(&show_output.stdout).trim()
    ));

    println!("✅ Phase 3 complete: Migration verified");

    println!("\n{}", "=".repeat(70));
    println!(
        "✅ Migration test passed! {} issues successfully migrated",
        initial_issue_count
    );
    println!("{}", "=".repeat(70));

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_random_actions(
    seed: Option<u64>,
    seed_from_entropy: bool,
    iters: usize,
    seconds: Option<u64>,
    actions_per_iter: usize,
    binary: Option<String>,
    verbose: bool,
    parallel: Option<usize>,
    ids: IdMode,
) -> Result<()> {
    // Sample entropy ONCE at the beginning if requested
    // After this point, everything is deterministic based on this seed
    let base_seed = if seed_from_entropy {
        let mut rng = rand::thread_rng();
        let entropy_seed = rng.next_u64();
        println!("🎲 Sampled entropy seed: {}", entropy_seed);
        println!("   (Use --seed {} to reproduce this run)\n", entropy_seed);
        entropy_seed
    } else {
        seed.unwrap_or(42u64) // Default seed
    };

    // Determine the binary path
    let binary_path = if let Some(path) = binary {
        // Explicit path provided
        path
    } else {
        // The tz binary lives in the same directory as test_tazk
        let exe_path = std::env::current_exe().expect("Failed to get current executable path");
        let exe_dir = exe_path
            .parent()
            .expect("Failed to get executable directory");
        let binary_name = format!("tz{}", std::env::consts::EXE_SUFFIX);
        exe_dir.join(&binary_name).to_str().unwrap().to_string()
    };

    // Check if binary exists
    if !PathBuf::from(&binary_path).exists() {
        eprintln!("ERROR: Binary not found at: {}", binary_path);
        eprintln!("Build it first with: cargo build");
        std::process::exit(1);
    }

    println!("Binary: {}", binary_path);

    // Check for parallel execution
    if let Some(num_workers) = parallel {
        // Parallel execution only works with --seconds mode
        if seconds.is_none() {
            eprintln!("ERROR: --parallel requires --seconds mode");
            std::process::exit(1);
        }

        // Determine number of workers (0 means use all cores)
        let workers = if num_workers == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        } else {
            num_workers
        };

        println!("\n🚀 Parallel stress test mode: {} workers", workers);
        println!("Duration: {} seconds", seconds.unwrap());
        println!("Base seed: {}", base_seed);
        println!("Will stop on first failure across all workers\n");

        return run_parallel_stress_test(
            base_seed,
            seconds.unwrap(),
            actions_per_iter,
            &binary_path,
            verbose,
            workers,
            ids,
        )
        .await;
    }

    // Unified sequential testing loop (handles both time-based and iteration-based modes)
    let start_time = seconds.map(|_| std::time::Instant::now());
    let duration = seconds.map(std::time::Duration::from_secs);

    // Print mode-specific header
    if let Some(duration_secs) = seconds {
        println!(
            "\n⏱️  Stress test mode: running for {} seconds",
            duration_secs
        );
        println!("Will stop on first failure or when time expires\n");
    }

    let verbosity = if verbose {
        LogLevel::Verbose
    } else {
        LogLevel::Normal
    };

    let mut iter = 0usize;
    loop {
        // Check stopping condition
        if let Some(d) = duration {
            if start_time.unwrap().elapsed() >= d {
                break;
            }
        } else if iter >= iters {
            break;
        }

        iter += 1;

        // Deterministic seed based on base_seed + iteration
        let iter_seed = if iters == 1 && seconds.is_none() {
            base_seed // Single iteration mode: use exact seed
        } else {
            base_seed.wrapping_add(iter as u64)
        };

        // Print iteration header
        println!("\n{}", "=".repeat(60));
        if let Some(duration_secs) = seconds {
            let elapsed = start_time.unwrap().elapsed().as_secs();
            println!(
                "Iteration {} | Elapsed: {}s / {}s",
                iter, elapsed, duration_secs
            );
        } else {
            println!("Iteration {}/{}", iter, iters);
        }
        println!("SEED: {}", iter_seed);
        println!("{}\n", "=".repeat(60));

        // Run the test
        let logger = Logger::new(verbosity, false);
        if let Err(e) = run_test(iter_seed, actions_per_iter, &binary_path, &logger, ids) {
            eprintln!("\n❌ TEST FAILED with SEED: {}", iter_seed);
            eprintln!("Error: {:?}", e);
            eprintln!("\nTo reproduce this failure, run:");
            eprintln!("  test_tazk random-actions --seed {}", iter_seed);
            if seconds.is_some() {
                eprintln!("\nStopping on first failure after {} iterations", iter);
            }
            std::process::exit(1);
        }

        println!("✅ Iteration {} completed successfully", iter);
    }

    // Print summary
    println!("\n{}", "=".repeat(60));
    if seconds.is_some() {
        let total_elapsed = start_time.unwrap().elapsed();
        println!(
            "✅ Stress test completed! {} iterations in {:.1}s",
            iter,
            total_elapsed.as_secs_f64()
        );
    } else {
        println!("✅ All {} iterations passed!", iters);
    }
    println!("{}", "=".repeat(60));

    Ok(())
}

/// Run parallel stress tests with multiple async worker tasks
#[allow(clippy::too_many_arguments)]
async fn run_parallel_stress_test(
    base_seed: u64,
    duration_secs: u64,
    actions_per_iter: usize,
    binary_path: &str,
    verbose: bool,
    workers: usize,
    ids: IdMode,
) -> Result<()> {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;

    let start_time = std::time::Instant::now();
    let duration = tokio::time::Duration::from_secs(duration_secs);

    // Shared state for coordinating workers
    let should_stop = Arc::new(AtomicBool::new(false));
    let total_iterations = Arc::new(AtomicU64::new(0));

    // Spawn progress reporter task
    let progress_should_stop = Arc::clone(&should_stop);
    let progress_iterations = Arc::clone(&total_iterations);
    let progress_handle = tokio::spawn(async move {
        while !progress_should_stop.load(Ordering::Relaxed) {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            let count = progress_iterations.load(Ordering::Relaxed);
            print!("\r⏱️  Progress: {} iterations completed", count);
            std::io::stdout().flush().ok();
        }
        // Clear the progress line when done
        print!("\r{}\r", " ".repeat(50));
        std::io::stdout().flush().ok();
    });

    // Spawn async worker tasks
    let mut handles = vec![];

    for worker_id in 0..workers {
        let should_stop = Arc::clone(&should_stop);
        let total_iterations = Arc::clone(&total_iterations);
        let binary_path = binary_path.to_string();

        let handle = tokio::spawn(async move {
            let mut local_iterations = 0u64;

            // Create buffering logger for parallel workers
            let verbosity = if verbose {
                LogLevel::Verbose
            } else {
                LogLevel::Normal
            };
            let logger = Logger::new(verbosity, true);

            // Each worker gets its own deterministic seed offset from the base seed
            // Worker 0: base_seed + 0
            // Worker 1: base_seed + 1000000
            // Worker 2: base_seed + 2000000, etc.
            let worker_base_seed = base_seed.wrapping_add((worker_id as u64) * 1000000);

            while start_time.elapsed() < duration && !should_stop.load(Ordering::Relaxed) {
                local_iterations += 1;
                let iter_seed = worker_base_seed.wrapping_add(local_iterations);

                // Run the test
                if let Err(e) = run_test(iter_seed, actions_per_iter, &binary_path, &logger, ids) {
                    // Signal all workers to stop
                    should_stop.store(true, Ordering::Relaxed);

                    // Dump buffered output on failure
                    logger.dump();

                    // Update total before returning
                    total_iterations.fetch_add(local_iterations, Ordering::Relaxed);

                    // Return error info
                    return Err((worker_id, iter_seed, e, local_iterations));
                }

                // Clear buffer after successful iteration to avoid memory buildup
                logger.clear();

                // Update progress counter after each successful iteration
                total_iterations.fetch_add(1, Ordering::Relaxed);
            }
            Ok::<_, (usize, u64, anyhow::Error, u64)>(local_iterations)
        });

        handles.push(handle);
    }

    // Wait for all workers to complete
    let mut failure: Option<(usize, u64, anyhow::Error, u64)> = None;

    for handle in handles {
        match handle.await {
            Ok(Ok(_)) => {}
            Ok(Err(err_info)) => {
                if failure.is_none() {
                    failure = Some(err_info);
                }
            }
            Err(e) => {
                eprintln!("Worker task panicked or was cancelled: {:?}", e);
            }
        }
    }

    // Signal progress reporter to stop
    should_stop.store(true, Ordering::Relaxed);

    let total_elapsed = start_time.elapsed();
    let total_iters = total_iterations.load(Ordering::Relaxed);

    // Wait for progress reporter to finish
    progress_handle.await.ok();

    // Report failure if any
    if let Some((worker_id, iter_seed, error, worker_iters)) = failure {
        eprintln!("\n❌ TEST FAILED in worker {}", worker_id);
        eprintln!("SEED: {}", iter_seed);
        eprintln!("Error: {:?}", error);
        eprintln!(
            "\nWorker {} completed {} iterations before failure",
            worker_id, worker_iters
        );
        eprintln!("Total iterations across all workers: {}", total_iters);
        eprintln!("\nTo reproduce this failure, run:");
        eprintln!("  test_tazk random-actions --seed {}", iter_seed);
        std::process::exit(1);
    }

    // Success!
    println!("\n{}", "=".repeat(60));
    println!(
        "✅ Parallel stress test completed! {} iterations in {:.1}s",
        total_iters,
        total_elapsed.as_secs_f64()
    );
    println!(
        "   Throughput: {:.1} iterations/second",
        total_iters as f64 / total_elapsed.as_secs_f64()
    );
    println!("{}", "=".repeat(60));

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_test(
    seed: u64,
    num_actions: usize,
    binary_path: &str,
    logger: &Logger,
    ids: IdMode,
) -> Result<()> {
    // Create a temporary directory for this test
    let temp_dir = tempfile::tempdir()?;
    let work_dir = temp_dir.path().to_str().unwrap();

    logger.log(format!("Working directory: {}", work_dir));

    // Create action generator
    let use_hash_ids = ids == IdMode::Hash;
    let mut generator = ActionGenerator::new_with_mode(seed, use_hash_ids);

    // Generate action sequence
    let actions = generator.generate_sequence(num_actions);
    logger.log(format!("Generated {} actions", actions.len()));

    // Print action sequence if verbose
    logger.verbose("\n📋 Generated Action Sequence:".to_string());
    for (i, action) in actions.iter().enumerate() {
        logger.verbose(format!("  {}. {}", i + 1, action));
    }
    logger.verbose(String::new());

    // Create executor
    let executor = ActionExecutor::new(binary_path, work_dir);

    // Create reference interpreter to maintain golden state
    let mut reference = match ids {
        IdMode::Numeric => ReferenceInterpreter::new(),
        IdMode::Hash => ReferenceInterpreter::new_with_hash_ids(),
    };

    // Execute actions
    logger.log("\nExecuting actions (use --verbose for details)...".to_string());
    let mut success_count = 0;
    let mut failure_count = 0;

    for (i, action) in actions.iter().enumerate() {
        logger.verbose(format!("{:3}. {:?}", i + 1, action));

        match executor.execute(action) {
            Ok(result) => {
                if result.success {
                    success_count += 1;
                    // Update reference interpreter with successful actions
                    // For hash mode Create actions, replace the placeholder ID with actual ID
                    let action_for_ref = if let Some(ref actual_id) = result.actual_issue_id {
                        if let TazkAction::Create {
                            expected_id,
                            title,
                            priority,
                            issue_type,
                            description,
                        } = action
                        {
                            if expected_id.contains("HASH") {
                                // Replace placeholder with actual ID for hash mode
                                TazkAction::Create {
                                    expected_id: actual_id.clone(),
                                    title: title.clone(),
                                    priority: *priority,
                                    issue_type: *issue_type,
                                    description: description.clone(),
                                }
                            } else {
                                action.clone()
                            }
                        } else {
                            action.clone()
                        }
                    } else {
                        action.clone()
                    };

                    if let Err(e) = reference.execute(&action_for_ref) {
                        let err_msg = format!("     ❌ Reference interpreter failed: {:?}", e);
                        logger.log(err_msg.clone());
                        logger.dump(); // Dump buffer on failure
                        eprintln!("{}", err_msg);
                        return Err(e);
                    }
                } else {
                    // Some failures are expected (e.g., dependency cycles, duplicate deps)
                    // Only fail the test for unexpected errors
                    if is_critical_failure(&result) {
                        let err_msgs = vec![
                            "     ❌ CRITICAL FAILURE".to_string(),
                            format!("     Exit code: {:?}", result.exit_code),
                            format!("     Stderr: {}", result.stderr),
                        ];
                        for msg in &err_msgs {
                            logger.log(msg.clone());
                        }
                        logger.dump(); // Dump buffer on failure
                        for msg in &err_msgs {
                            eprintln!("{}", msg);
                        }
                        return Err(anyhow::anyhow!(
                            "Critical failure on action {}: {:?}",
                            i + 1,
                            action
                        ));
                    } else {
                        // Expected failure (e.g., validation error)
                        failure_count += 1;
                        logger.verbose(format!(
                            "     ⚠️  Expected failure: {}",
                            result.stderr.lines().next().unwrap_or("")
                        ));
                    }
                }
            }
            Err(e) => {
                let err_msg = format!("     ❌ Failed to execute action: {:?}", e);
                logger.log(err_msg.clone());
                logger.dump(); // Dump buffer on failure
                eprintln!("{}", err_msg);
                return Err(e);
            }
        }
    }

    logger.log(format!(
        "\nResults: {} successful, {} expected failures",
        success_count, failure_count
    ));

    // Verify final state consistency
    let (bytes_written, num_issues) =
        verify_consistency(&executor, work_dir, logger, &mut reference)?;

    // Print summary stats
    logger.log(format!("📊 Issues generated: {}", num_issues));
    logger.log(format!(
        "💾 Bytes written: {} ({:.1} KB)",
        bytes_written,
        bytes_written as f64 / 1024.0
    ));

    Ok(())
}

/// Check if a failure is critical (unexpected) vs expected (validation error)
fn is_critical_failure(result: &tazk::action_generator::ExecutionResult) -> bool {
    // These are expected validation errors, not critical failures
    let expected_errors = [
        "already exists",
        "not found",
        "cycle detected",
        "Invalid status",
        "Invalid priority",
        "already has dependency",
        "already has a parent",
        "cannot depend on itself",
    ];

    for expected in &expected_errors {
        if result.stderr.contains(expected) {
            return false;
        }
    }

    // If exit code is non-zero and it's not an expected error, it's critical
    result.exit_code.unwrap_or(1) != 0
}

/// Check if an ID is a valid base36 hash ID (prefix followed by 0-9, a-z with at least one letter)
///
/// Base36 encoding uses characters [0-9a-z].
///
/// Examples of valid base36 hash IDs:
/// - "test-3s9"
/// - "tazk-0qeg"
/// - "tz-12f8393"
///
/// Examples of invalid (uppercase or non-base36 characters):
/// - "test-ABCD" (uppercase not allowed)
/// - "test-xyz!" (! is not base36)
fn is_base36_hash_id(id: &str) -> bool {
    // Must start with the ID prefix, then the hash part
    let Some(hash_part) = id.strip_prefix(ID_PREFIX) else {
        return false;
    };

    // Hash part must contain at least one letter, and only base36 characters
    // (0-9, a-z lowercase only)
    hash_part.chars().any(|c| c.is_ascii_lowercase())
        && hash_part
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// The part of an ID after the fixed prefix (the whole ID if it has none).
fn hash_suffix(id: &str) -> &str {
    id.strip_prefix(ID_PREFIX).unwrap_or(id)
}

/// Validate that all hash IDs in a collection use base36 encoding
///
/// Returns Ok if all IDs use valid base36 characters (0-9, a-z).
fn validate_hash_ids_are_base36(
    issues: &std::collections::HashMap<String, tazk::action_generator::ReferenceIssue>,
    logger: &Logger,
) -> Result<()> {
    let mut non_base36_ids = Vec::new();
    let mut extended_base36_ids = Vec::new();

    for id in issues.keys() {
        // Skip if it's a numeric ID (doesn't contain alphabetic chars)
        let id_suffix = hash_suffix(id);
        if id_suffix.chars().all(|c| c.is_ascii_digit()) {
            continue; // Numeric ID, skip validation
        }

        if !is_base36_hash_id(id) {
            non_base36_ids.push(id.clone());
        } else if id_suffix.chars().any(|c| matches!(c, 'g'..='z')) {
            extended_base36_ids.push(id.clone());
        }
    }

    // Only invalid characters are a hard error
    if !non_base36_ids.is_empty() {
        return Err(anyhow::anyhow!(
            "Found {} invalid hash IDs (not base36): {:?}",
            non_base36_ids.len(),
            &non_base36_ids[..std::cmp::min(5, non_base36_ids.len())]
        ));
    }

    // Log statistics about the hash distribution
    logger.verbose(format!(
        "   ✓ All {} hash IDs use valid base36 characters",
        issues.len()
    ));

    if !extended_base36_ids.is_empty() {
        logger.verbose(format!(
            "     {} IDs use extended base36 chars (g-z) - confirms base36 encoding",
            extended_base36_ids.len()
        ));
    }

    Ok(())
}

/// Detect if we're in hash ID mode by checking if any ID contains non-numeric characters
fn is_hash_id_mode(
    issues: &std::collections::HashMap<String, tazk::action_generator::ReferenceIssue>,
) -> bool {
    for id in issues.keys() {
        let id_suffix = hash_suffix(id);
        // If any ID has alphabetic characters, we're in hash mode
        if id_suffix.chars().any(|c| c.is_alphabetic()) {
            return true;
        }
    }
    false
}

/// Verify the final state is consistent with the reference interpreter
/// Returns (bytes_written, num_issues) on success
fn verify_consistency(
    _executor: &ActionExecutor,
    work_dir: &str,
    logger: &Logger,
    reference: &mut ReferenceInterpreter,
) -> Result<(u64, usize)> {
    logger.verbose(
        "\n🔍 Deep verification: comparing actual state with reference interpreter...".to_string(),
    );

    // Check that the implementation's storage directory exists.
    let tazk_dir = PathBuf::from(work_dir).join(".tazk");
    if !tazk_dir.exists() {
        return Err(anyhow::anyhow!(
            "{} directory does not exist",
            tazk_dir.display()
        ));
    }

    // Step 1: Recursively walk storage directory and report files
    let (files, total_size) = walk_tazk_directory(&tazk_dir, logger)?;

    logger.verbose(format!(
        "   Found {} files, total size: {} bytes",
        files.len(),
        total_size
    ));

    // Step 2: Verify config.yaml is loadable
    verify_config(&tazk_dir, logger)?;

    // Step 3: Compare actual issues with reference state
    verify_tazk_state(&tazk_dir, reference, logger)?;

    logger.log("✅ Verification passed".to_string());

    // Return metrics
    let num_issues = reference.get_final_state().len();
    Ok((total_size, num_issues))
}

#[derive(Debug)]
#[allow(dead_code)]
struct FileInfo {
    path: PathBuf,
    size: u64,
}

/// Recursively walk storage directory and collect file info
fn walk_tazk_directory(
    tazk_dir: &std::path::Path,
    logger: &Logger,
) -> Result<(Vec<FileInfo>, u64)> {
    let mut files = Vec::new();
    let mut total_size = 0u64;

    fn walk_recursive(
        dir: &std::path::Path,
        files: &mut Vec<FileInfo>,
        total_size: &mut u64,
        logger: &Logger,
    ) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = entry.metadata()?;

            if metadata.is_dir() {
                walk_recursive(&path, files, total_size, logger)?;
            } else {
                let size = metadata.len();
                *total_size += size;
                logger.verbose(format!("      {:6} bytes  {}", size, path.display()));
                files.push(FileInfo { path, size });
            }
        }
        Ok(())
    }

    walk_recursive(tazk_dir, &mut files, &mut total_size, logger)?;
    Ok((files, total_size))
}

/// Parse and verify config.yaml using shared Storage code
fn verify_config(tazk_dir: &std::path::Path, logger: &Logger) -> Result<()> {
    use tazk::storage::Storage;

    // Opening the store validates config.yaml (including rejecting removed keys)
    Storage::open(tazk_dir.to_path_buf())?;
    logger.verbose("   ✓ config.yaml: store opens".to_string());

    Ok(())
}

/// Verify tazk state (markdown files)
fn verify_tazk_state(
    tazk_dir: &std::path::Path,
    reference: &ReferenceInterpreter,
    logger: &Logger,
) -> Result<()> {
    let issues_dir = tazk_dir.join("issues");
    if !issues_dir.exists() {
        // No issues created yet is valid
        if reference.get_final_state().is_empty() {
            logger.verbose("   ✓ No issues directory (no issues created)".to_string());
            return Ok(());
        } else {
            return Err(anyhow::anyhow!(
                "Reference has {} issues but issues/ directory does not exist",
                reference.get_final_state().len()
            ));
        }
    }

    // Read all markdown files. Tazk supports both flat and sharded issue
    // layouts, so verification must recurse through issues/.
    let mut actual_issues = std::collections::HashMap::new();
    let mut stack = vec![issues_dir];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && path.extension().map(|e| e == "md").unwrap_or(false) {
                let issue = parse_tazk_issue(&path)?;
                actual_issues.insert(issue.id.clone(), issue);
            }
        }
    }

    // Validate hash ID encoding if in hash mode
    if !actual_issues.is_empty() && is_hash_id_mode(&actual_issues) {
        logger.verbose("   Validating hash ID encoding format...".to_string());
        validate_hash_ids_are_base36(&actual_issues, logger)?;
    }

    // Compare with reference
    compare_issue_states(&actual_issues, reference.get_final_state(), logger)?;

    Ok(())
}

/// Parse a tazk markdown issue file to ReferenceIssue using shared format code
fn parse_tazk_issue(path: &PathBuf) -> Result<tazk::action_generator::ReferenceIssue> {
    use tazk::action_generator::ReferenceIssue;
    use tazk::format::markdown_to_issue;

    // Extract issue ID from filename
    let issue_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("Cannot extract ID from filename {:?}", path))?;

    // Read file content
    let content = std::fs::read_to_string(path)?;

    // Use the standard markdown parser from the main codebase
    let issue = markdown_to_issue(issue_id, &content)?;

    // Convert from full Issue to simplified ReferenceIssue
    Ok(ReferenceIssue {
        id: issue.id,
        title: issue.title,
        description: issue.description,
        status: issue.status,
        priority: issue.priority,
        issue_type: issue.issue_type,
        depends_on: issue.depends_on,
    })
}

/// Compare actual issues with reference state
fn compare_issue_states(
    actual: &std::collections::HashMap<String, tazk::action_generator::ReferenceIssue>,
    expected: &std::collections::HashMap<String, tazk::action_generator::ReferenceIssue>,
    logger: &Logger,
) -> Result<()> {
    use similar_asserts::assert_eq;

    // Check counts match
    if actual.len() != expected.len() {
        eprintln!("\n❌ Issue count mismatch!");
        eprintln!(
            "Expected {} issues, but got {} issues\n",
            expected.len(),
            actual.len()
        );

        // Show which issues are in expected but not actual, and vice versa
        let expected_ids: std::collections::HashSet<_> = expected.keys().collect();
        let actual_ids: std::collections::HashSet<_> = actual.keys().collect();

        let missing: Vec<_> = expected_ids.difference(&actual_ids).collect();
        let extra: Vec<_> = actual_ids.difference(&expected_ids).collect();

        if !missing.is_empty() {
            eprintln!(
                "Missing issues (in reference but not in actual): {:?}",
                missing
            );
        }
        if !extra.is_empty() {
            eprintln!("Extra issues (in actual but not in reference): {:?}", extra);
        }

        return Err(anyhow::anyhow!(
            "Issue count mismatch: expected {}, got {}",
            expected.len(),
            actual.len()
        ));
    }

    if !expected.is_empty() {
        logger.verbose(format!("   ✓ Issue count matches: {}", expected.len()));
    }

    // Compare each issue
    for (id, expected_issue) in expected {
        let actual_issue = actual.get(id).ok_or_else(|| {
            anyhow::anyhow!("Issue {} exists in reference but not in actual state", id)
        })?;

        // Use similar_asserts for colorful diffs on mismatch
        // Wrap in a catch_unwind to convert panic to Result
        let comparison_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Compare title
            if actual_issue.title != expected_issue.title {
                eprintln!("\n❌ Title mismatch for issue {}:", id);
                assert_eq!(
                    &expected_issue.title, &actual_issue.title,
                    "Title mismatch for {}",
                    id
                );
            }

            // Compare status
            if actual_issue.status != expected_issue.status {
                eprintln!("\n❌ Status mismatch for issue {}:", id);
                assert_eq!(
                    &expected_issue.status, &actual_issue.status,
                    "Status mismatch for {}",
                    id
                );
            }

            // Compare priority
            if actual_issue.priority != expected_issue.priority {
                eprintln!("\n❌ Priority mismatch for issue {}:", id);
                assert_eq!(
                    &expected_issue.priority, &actual_issue.priority,
                    "Priority mismatch for {}",
                    id
                );
            }

            // Compare issue_type
            if actual_issue.issue_type != expected_issue.issue_type {
                eprintln!("\n❌ IssueType mismatch for issue {}:", id);
                assert_eq!(
                    &expected_issue.issue_type, &actual_issue.issue_type,
                    "IssueType mismatch for {}",
                    id
                );
            }

            // Compare dependencies
            if actual_issue.depends_on != expected_issue.depends_on {
                eprintln!("\n❌ Dependencies mismatch for issue {}:", id);

                // Convert to sorted vectors for better diff display
                let mut expected_deps: Vec<_> = expected_issue.depends_on.iter().collect();
                let mut actual_deps: Vec<_> = actual_issue.depends_on.iter().collect();
                expected_deps.sort_by_key(|(k, _)| *k);
                actual_deps.sort_by_key(|(k, _)| *k);

                assert_eq!(
                    &expected_deps, &actual_deps,
                    "Dependencies mismatch for {}",
                    id
                );
            }
        }));

        // If comparison panicked (assertion failed), convert to error
        if let Err(panic_info) = comparison_result {
            // The panic from similar_asserts already printed colorful output
            // Just return an error to stop the test
            if let Some(s) = panic_info.downcast_ref::<String>() {
                return Err(anyhow::anyhow!(
                    "Verification failed for issue {}: {}",
                    id,
                    s
                ));
            } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                return Err(anyhow::anyhow!(
                    "Verification failed for issue {}: {}",
                    id,
                    s
                ));
            } else {
                return Err(anyhow::anyhow!("Verification failed for issue {}", id));
            }
        }

        logger.verbose(format!("   ✓ Issue {} matches reference", id));
    }

    Ok(())
}

/// Execute a sequence of actions and update reference
fn execute_actions(
    executor: &ActionExecutor,
    actions: &[TazkAction],
    reference: &mut ReferenceInterpreter,
    logger: &Logger,
) -> Result<()> {
    let mut success_count = 0;
    let mut failure_count = 0;

    for (i, action) in actions.iter().enumerate() {
        logger.verbose(format!("{:3}. {:?}", i + 1, action));

        match executor.execute(action) {
            Ok(result) => {
                if result.success {
                    success_count += 1;
                    // Update reference interpreter with successful actions
                    let action_for_ref = if let Some(ref actual_id) = result.actual_issue_id {
                        if let TazkAction::Create {
                            expected_id,
                            title,
                            priority,
                            issue_type,
                            description,
                        } = action
                        {
                            if expected_id.contains("HASH") {
                                TazkAction::Create {
                                    expected_id: actual_id.clone(),
                                    title: title.clone(),
                                    priority: *priority,
                                    issue_type: *issue_type,
                                    description: description.clone(),
                                }
                            } else {
                                action.clone()
                            }
                        } else {
                            action.clone()
                        }
                    } else {
                        action.clone()
                    };

                    if let Err(e) = reference.execute(&action_for_ref) {
                        logger.log(format!("❌ Reference interpreter failed: {:?}", e));
                        return Err(e);
                    }
                } else if is_critical_failure(&result) {
                    logger.log(format!("❌ CRITICAL FAILURE: {}", result.stderr));
                    return Err(anyhow::anyhow!(
                        "Critical failure on action {}: {:?}",
                        i + 1,
                        action
                    ));
                } else {
                    failure_count += 1;
                    logger.verbose(format!(
                        "     ⚠️  Expected failure: {}",
                        result.stderr.lines().next().unwrap_or("")
                    ));
                }
            }
            Err(e) => {
                logger.log(format!("❌ Failed to execute action: {:?}", e));
                return Err(e);
            }
        }
    }

    logger.log(format!(
        "Results: {} successful, {} expected failures",
        success_count, failure_count
    ));
    Ok(())
}
