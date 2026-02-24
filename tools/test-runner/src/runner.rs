use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::annotation::TestMode;
use crate::discovery::TestCase;

#[derive(Debug)]
pub struct RunResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// Run a test with a timeout using a child process.
pub fn run_test_with_timeout(test: &TestCase, nudl_bin: &Path, timeout: Duration) -> RunResult {
    let args = build_args(&test.annotations.mode, &test.source_path);

    let child = Command::new(nudl_bin)
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    match child {
        Ok(mut child) => {
            let start = std::time::Instant::now();

            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        // Process exited — read remaining pipe contents directly
                        let mut stdout = String::new();
                        let mut stderr = String::new();
                        if let Some(mut out) = child.stdout.take() {
                            let _ = out.read_to_string(&mut stdout);
                        }
                        if let Some(mut err) = child.stderr.take() {
                            let _ = err.read_to_string(&mut stderr);
                        }
                        return RunResult {
                            exit_code: status.code().unwrap_or(-1),
                            stdout,
                            stderr,
                            timed_out: false,
                        };
                    }
                    Ok(None) => {
                        if start.elapsed() >= timeout {
                            let _ = child.kill();
                            let _ = child.wait();
                            return RunResult {
                                exit_code: -1,
                                stdout: String::new(),
                                stderr: format!("test timed out after {}s", timeout.as_secs()),
                                timed_out: true,
                            };
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => {
                        return RunResult {
                            exit_code: -1,
                            stdout: String::new(),
                            stderr: format!("failed to wait for nudl process: {e}"),
                            timed_out: false,
                        };
                    }
                }
            }
        }
        Err(e) => RunResult {
            exit_code: -1,
            stdout: String::new(),
            stderr: format!("failed to execute nudl: {e}"),
            timed_out: false,
        },
    }
}

fn build_args(mode: &TestMode, source_path: &Path) -> Vec<String> {
    let source = source_path.to_string_lossy().to_string();

    match mode {
        TestMode::Check => vec!["check".to_string(), source],
        TestMode::Build => {
            let tmp = std::env::temp_dir().join("nudl_test_output");
            vec![
                "build".to_string(),
                source,
                "-o".to_string(),
                tmp.to_string_lossy().to_string(),
            ]
        }
        TestMode::Run => vec!["run".to_string(), source],
    }
}
