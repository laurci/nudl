/// Test annotations parsed from `//!` comments at the top of `.nudl` files.
///
/// Supported annotations:
/// ```text
/// //! mode: check|build|run       (default: check)
/// //! expect: pass|fail            (default: pass)
/// //! exit: N                      (run mode only, default: 0)
/// //! stdout: "exact output"       (run mode, repeatable for multiline)
/// //! stdout-contains: "substring" (run mode, repeatable)
/// //! stderr-contains: "substring" (any mode, repeatable)
/// //! error: E0401                 (expect:fail, repeatable)
/// //! error-contains: "text"       (expect:fail, repeatable)
/// //! skip: reason                 (skip this test)
/// ```

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestMode {
    Check,
    Build,
    Run,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expectation {
    Pass,
    Fail,
}

#[derive(Debug, Clone)]
pub struct TestAnnotations {
    pub mode: TestMode,
    pub expect: Expectation,
    pub exit_code: i32,
    pub stdout_lines: Vec<String>,
    pub stdout_contains: Vec<String>,
    pub stderr_contains: Vec<String>,
    pub error_codes: Vec<String>,
    pub error_contains: Vec<String>,
    pub skip: Option<String>,
}

impl Default for TestAnnotations {
    fn default() -> Self {
        Self {
            mode: TestMode::Check,
            expect: Expectation::Pass,
            exit_code: 0,
            stdout_lines: Vec::new(),
            stdout_contains: Vec::new(),
            stderr_contains: Vec::new(),
            error_codes: Vec::new(),
            error_contains: Vec::new(),
            skip: None,
        }
    }
}

/// Strip surrounding quotes from a string value if present.
fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

impl TestAnnotations {
    /// Parse annotations from the source text of a `.nudl` file.
    /// Only reads `//!` lines from the beginning of the file (stops at first non-annotation line).
    pub fn parse(source: &str) -> Self {
        let mut annotations = Self::default();

        for line in source.lines() {
            let trimmed = line.trim();

            // Skip empty lines at the top
            if trimmed.is_empty() {
                continue;
            }

            // Stop at first non-annotation line
            if !trimmed.starts_with("//!") {
                break;
            }

            // Strip the `//!` prefix
            let content = trimmed[3..].trim();

            // Parse key: value
            if let Some((key, value)) = content.split_once(':') {
                let key = key.trim();
                let value = value.trim();

                match key {
                    "mode" => {
                        annotations.mode = match value {
                            "check" => TestMode::Check,
                            "build" => TestMode::Build,
                            "run" => TestMode::Run,
                            _ => TestMode::Check,
                        };
                    }
                    "expect" => {
                        annotations.expect = match value {
                            "pass" => Expectation::Pass,
                            "fail" => Expectation::Fail,
                            _ => Expectation::Pass,
                        };
                    }
                    "exit" => {
                        annotations.exit_code = value.parse().unwrap_or(0);
                    }
                    "stdout" => {
                        annotations
                            .stdout_lines
                            .push(strip_quotes(value).to_string());
                    }
                    "stdout-contains" => {
                        annotations
                            .stdout_contains
                            .push(strip_quotes(value).to_string());
                    }
                    "stderr-contains" => {
                        annotations
                            .stderr_contains
                            .push(strip_quotes(value).to_string());
                    }
                    "error" => {
                        annotations.error_codes.push(value.to_string());
                    }
                    "error-contains" => {
                        annotations
                            .error_contains
                            .push(strip_quotes(value).to_string());
                    }
                    "skip" => {
                        annotations.skip = Some(value.to_string());
                    }
                    _ => {}
                }
            }
        }

        annotations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_annotations() {
        let ann = TestAnnotations::parse("fn main() {}");
        assert_eq!(ann.mode, TestMode::Check);
        assert_eq!(ann.expect, Expectation::Pass);
        assert_eq!(ann.exit_code, 0);
        assert!(ann.skip.is_none());
    }

    #[test]
    fn test_run_mode_with_stdout() {
        let source = r#"//! mode: run
//! expect: pass
//! stdout: "hello world"
//! stdout: "second line"
fn main() {}"#;
        let ann = TestAnnotations::parse(source);
        assert_eq!(ann.mode, TestMode::Run);
        assert_eq!(ann.expect, Expectation::Pass);
        assert_eq!(ann.stdout_lines, vec!["hello world", "second line"]);
    }

    #[test]
    fn test_expect_fail_with_error() {
        let source = r#"//! expect: fail
//! error: E0401
//! error-contains: "undefined variable"
fn main() {}"#;
        let ann = TestAnnotations::parse(source);
        assert_eq!(ann.expect, Expectation::Fail);
        assert_eq!(ann.error_codes, vec!["E0401"]);
        assert_eq!(ann.error_contains, vec!["undefined variable"]);
    }

    #[test]
    fn test_skip() {
        let source = "//! skip: not yet implemented\nfn main() {}";
        let ann = TestAnnotations::parse(source);
        assert_eq!(ann.skip, Some("not yet implemented".to_string()));
    }

    #[test]
    fn test_empty_lines_before_annotations() {
        let source = "\n\n//! mode: build\nfn main() {}";
        let ann = TestAnnotations::parse(source);
        assert_eq!(ann.mode, TestMode::Build);
    }
}
