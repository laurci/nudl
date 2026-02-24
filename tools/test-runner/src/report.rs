use crate::outcome::TestOutcome;

pub struct TestReport {
    pub name: String,
    pub outcome: TestOutcome,
}

pub struct Summary {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

/// Print the result of a single test.
pub fn print_test_result(report: &TestReport, use_color: bool) {
    let status = match &report.outcome {
        TestOutcome::Pass => {
            if use_color {
                "\x1b[32mok\x1b[0m"
            } else {
                "ok"
            }
        }
        TestOutcome::Skip(_) => {
            if use_color {
                "\x1b[33mskip\x1b[0m"
            } else {
                "skip"
            }
        }
        TestOutcome::Fail(_) => {
            if use_color {
                "\x1b[31mFAIL\x1b[0m"
            } else {
                "FAIL"
            }
        }
    };

    let name_padded = format!("[{}]", report.name);
    print!("  {name_padded:<45} ... {status}");

    if let TestOutcome::Skip(reason) = &report.outcome {
        print!(" ({reason})");
    }
    println!();
}

/// Print the final summary after all tests.
pub fn print_summary(reports: &[TestReport], use_color: bool) {
    let summary = compute_summary(reports);

    println!();

    // Print failures detail
    let failures: Vec<&TestReport> = reports
        .iter()
        .filter(|r| matches!(r.outcome, TestOutcome::Fail(_)))
        .collect();

    if !failures.is_empty() {
        println!("failures:");
        println!();
        for report in &failures {
            if let TestOutcome::Fail(reasons) = &report.outcome {
                println!("  [{}]", report.name);
                for reason in reasons {
                    println!("    {reason}");
                }
                println!();
            }
        }
    }

    // Print summary line
    let result_word = if summary.failed > 0 {
        if use_color {
            "\x1b[31mFAILED\x1b[0m"
        } else {
            "FAILED"
        }
    } else {
        if use_color { "\x1b[32mok\x1b[0m" } else { "ok" }
    };

    println!(
        "test result: {result_word}. {} passed, {} failed, {} skipped",
        summary.passed, summary.failed, summary.skipped
    );
}

fn compute_summary(reports: &[TestReport]) -> Summary {
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    for report in reports {
        match &report.outcome {
            TestOutcome::Pass => passed += 1,
            TestOutcome::Fail(_) => failed += 1,
            TestOutcome::Skip(_) => skipped += 1,
        }
    }

    Summary {
        passed,
        failed,
        skipped,
    }
}

/// Print just the test names (for --list mode).
pub fn print_test_list(names: &[String]) {
    println!("{} tests found:", names.len());
    println!();
    for name in names {
        println!("  [{name}]");
    }
}
