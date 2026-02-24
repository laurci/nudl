mod annotation;
mod discovery;
mod outcome;
mod report;
mod runner;

use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;

use discovery::{discover_tests, filter_tests};
use outcome::{evaluate, TestOutcome};
use report::{print_summary, print_test_list, print_test_result, TestReport};
use runner::run_test_with_timeout;

#[derive(Parser)]
#[command(name = "nudl-test", about = "End-to-end test runner for the nudl compiler")]
struct Cli {
    /// Substring filter(s) on test names (e.g. "functions" or "core-types/integers")
    filters: Vec<String>,

    /// Path to the nudl binary
    #[arg(long, default_value = "target/debug/nudl")]
    nudl_bin: PathBuf,

    /// Path to the tests directory
    #[arg(long, default_value = "tests")]
    tests_dir: PathBuf,

    /// Per-test timeout in seconds
    #[arg(long, default_value = "5")]
    timeout: u64,

    /// Disable ANSI colors
    #[arg(long)]
    no_color: bool,

    /// List tests without running them
    #[arg(long)]
    list: bool,

    /// Stop on first failure
    #[arg(long)]
    fail_fast: bool,
}

fn main() {
    let cli = Cli::parse();
    let use_color = !cli.no_color;
    let timeout = Duration::from_secs(cli.timeout);

    // Check that the nudl binary exists
    if !cli.nudl_bin.exists() {
        eprintln!(
            "error: nudl binary not found at '{}'. Build it first with `cargo build`.",
            cli.nudl_bin.display()
        );
        std::process::exit(1);
    }

    // Discover and filter tests
    let tests = discover_tests(&cli.tests_dir);
    let tests = filter_tests(tests, &cli.filters);

    if tests.is_empty() {
        eprintln!("no tests found");
        std::process::exit(1);
    }

    // List mode
    if cli.list {
        let names: Vec<String> = tests.iter().map(|t| t.name.clone()).collect();
        print_test_list(&names);
        return;
    }

    // Run tests
    println!("running {} tests", tests.len());
    println!();

    let mut reports = Vec::with_capacity(tests.len());
    let mut had_failure = false;

    for test in &tests {
        let outcome = if test.annotations.skip.is_some() {
            TestOutcome::Skip(test.annotations.skip.clone().unwrap())
        } else {
            let result = run_test_with_timeout(test, &cli.nudl_bin, timeout);
            evaluate(&test.annotations, &result)
        };

        let is_failure = matches!(outcome, TestOutcome::Fail(_));

        let report = TestReport {
            name: test.name.clone(),
            outcome,
        };

        print_test_result(&report, use_color);
        reports.push(report);

        if is_failure {
            had_failure = true;
            if cli.fail_fast {
                break;
            }
        }
    }

    print_summary(&reports, use_color);

    if had_failure {
        std::process::exit(1);
    }
}
