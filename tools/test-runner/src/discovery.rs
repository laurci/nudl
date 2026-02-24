use std::fs;
use std::path::{Path, PathBuf};

use crate::annotation::TestAnnotations;

#[derive(Debug, Clone)]
pub struct TestCase {
    /// Display name like "core-types/integers"
    pub name: String,
    /// Path to the .nudl source file
    pub source_path: PathBuf,
    /// Parsed annotations
    pub annotations: TestAnnotations,
}

/// Discover all test cases under the given tests directory.
///
/// Test structure:
/// - `tests/<category>/<name>.nudl` → single-file test named "category/name"
/// - `tests/<category>/<name>/main.nudl` → module test named "category/name"
pub fn discover_tests(tests_dir: &Path) -> Vec<TestCase> {
    let mut tests = Vec::new();

    let Ok(categories) = fs::read_dir(tests_dir) else {
        eprintln!(
            "warning: cannot read tests directory: {}",
            tests_dir.display()
        );
        return tests;
    };

    let mut category_entries: Vec<_> = categories
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    category_entries.sort_by_key(|e| e.file_name());

    for category_entry in category_entries {
        let category_name = category_entry.file_name();
        let category_name = category_name.to_string_lossy();
        let category_path = category_entry.path();

        let Ok(entries) = fs::read_dir(&category_path) else {
            continue;
        };

        let mut file_entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        file_entries.sort_by_key(|e| e.file_name());

        for entry in file_entries {
            let path = entry.path();
            let file_type = entry.file_type().unwrap_or_else(|_| {
                // Fallback: won't match either branch below
                fs::metadata(&path)
                    .map(|m| m.file_type())
                    .unwrap_or_else(|_| entry.file_type().unwrap())
            });

            if file_type.is_file() && path.extension().is_some_and(|ext| ext == "nudl") {
                // Single-file test: tests/<category>/<name>.nudl
                let test_name = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let source = fs::read_to_string(&path).unwrap_or_default();
                let annotations = TestAnnotations::parse(&source);

                tests.push(TestCase {
                    name: format!("{category_name}/{test_name}"),
                    source_path: path,
                    annotations,
                });
            } else if file_type.is_dir() {
                // Module test: tests/<category>/<name>/main.nudl
                let main_path = path.join("main.nudl");
                if main_path.exists() {
                    let dir_name = entry.file_name();
                    let test_name = dir_name.to_string_lossy();

                    let source = fs::read_to_string(&main_path).unwrap_or_default();
                    let annotations = TestAnnotations::parse(&source);

                    tests.push(TestCase {
                        name: format!("{category_name}/{test_name}"),
                        source_path: main_path,
                        annotations,
                    });
                }
            }
        }
    }

    tests
}

/// Filter test cases by substring match on test name.
pub fn filter_tests(tests: Vec<TestCase>, filters: &[String]) -> Vec<TestCase> {
    if filters.is_empty() {
        return tests;
    }

    tests
        .into_iter()
        .filter(|test| filters.iter().any(|f| test.name.contains(f.as_str())))
        .collect()
}
