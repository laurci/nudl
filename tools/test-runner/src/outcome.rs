use crate::annotation::{Expectation, TestAnnotations, TestMode};
use crate::runner::RunResult;

#[derive(Debug)]
pub enum TestOutcome {
    Pass,
    Skip(String),
    Fail(Vec<String>),
}

/// Compare actual run result against expected annotations.
pub fn evaluate(annotations: &TestAnnotations, result: &RunResult) -> TestOutcome {
    // Check skip first
    if let Some(reason) = &annotations.skip {
        return TestOutcome::Skip(reason.clone());
    }

    let mut failures = Vec::new();

    // Check timeout
    if result.timed_out {
        failures.push("test timed out".to_string());
        return TestOutcome::Fail(failures);
    }

    match annotations.expect {
        Expectation::Pass => {
            // Expect success (exit code 0 for check/build, annotations.exit_code for run)
            let expected_exit = if annotations.mode == TestMode::Run {
                annotations.exit_code
            } else {
                0
            };

            if result.exit_code != expected_exit {
                failures.push(format!(
                    "expected exit code {expected_exit}, got {}",
                    result.exit_code
                ));
                // Include stderr for context on unexpected failures
                if !result.stderr.is_empty() {
                    let stderr_preview = truncate(&result.stderr, 500);
                    failures.push(format!("stderr: {stderr_preview}"));
                }
            }

            // Check stdout expectations
            check_stdout(annotations, result, &mut failures);
        }

        Expectation::Fail => {
            // Expect failure (non-zero exit)
            if result.exit_code == 0 {
                failures.push("expected failure (non-zero exit), but got exit code 0".to_string());
            }

            // Check expected error codes
            for code in &annotations.error_codes {
                if !result.stderr.contains(code.as_str()) {
                    failures.push(format!("expected error code {code} in stderr"));
                }
            }

            // Check error-contains
            for text in &annotations.error_contains {
                if !result.stderr.contains(text.as_str()) {
                    failures.push(format!("expected stderr to contain: {text}"));
                }
            }
        }
    }

    // Check stderr-contains (applicable to both pass and fail)
    for text in &annotations.stderr_contains {
        if !result.stderr.contains(text.as_str()) {
            failures.push(format!("expected stderr to contain: {text}"));
        }
    }

    if failures.is_empty() {
        TestOutcome::Pass
    } else {
        TestOutcome::Fail(failures)
    }
}

fn check_stdout(annotations: &TestAnnotations, result: &RunResult, failures: &mut Vec<String>) {
    // Check exact stdout lines
    if !annotations.stdout_lines.is_empty() {
        let expected = annotations.stdout_lines.join("\n");
        let actual = result.stdout.trim_end();

        if actual != expected {
            failures.push(format!(
                "stdout mismatch:\n  expected: {expected}\n  actual:   {actual}"
            ));
        }
    }

    // Check stdout-contains
    for text in &annotations.stdout_contains {
        if !result.stdout.contains(text.as_str()) {
            failures.push(format!("expected stdout to contain: {text}"));
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        // Find a valid char boundary at or before `max`
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}
