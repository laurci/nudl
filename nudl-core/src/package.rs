use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct BinTarget {
    pub name: String,
    pub path: String,
    /// Extra arguments passed to the linker (object files, -L, -l flags, etc.)
    #[serde(default)]
    pub link: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct PackageConfig {
    pub package: PackageSection,
    #[serde(default)]
    pub bin: Vec<BinTarget>,
}

#[derive(Debug, Deserialize)]
pub struct PackageSection {
    pub name: String,
}

impl PackageConfig {
    /// Parse a `nudl.toml` file at the given path.
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read '{}': {}", path.display(), e))?;
        toml::from_str(&content)
            .map_err(|e| format!("invalid nudl.toml at '{}': {}", path.display(), e))
    }

    /// Resolve all bin target paths relative to the package directory.
    /// Returns `(name, resolved_path)` pairs.
    pub fn bin_paths(&self, package_dir: &Path) -> Vec<(String, PathBuf)> {
        self.bin
            .iter()
            .map(|b| (b.name.clone(), self.resolve_bin_path(b, package_dir)))
            .collect()
    }

    /// Find a bin target by name.
    pub fn find_bin(&self, name: &str) -> Option<&BinTarget> {
        self.bin.iter().find(|b| b.name == name)
    }

    /// Resolve one bin target's path relative to the package directory.
    pub fn resolve_bin_path(&self, bin: &BinTarget, package_dir: &Path) -> PathBuf {
        package_dir.join(&bin.path)
    }
}

/// Walk up from `start_dir` looking for a `nudl.toml` file.
pub fn find_package_file(start_dir: &Path) -> Option<PathBuf> {
    let mut dir = start_dir.to_path_buf();
    loop {
        let candidate = dir.join("nudl.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Find and load the package config starting from `start_dir`.
pub fn discover_package(start_dir: &Path) -> Option<(PackageConfig, PathBuf)> {
    let toml_path = find_package_file(start_dir)?;
    let package_dir = toml_path.parent()?.to_path_buf();
    let config = PackageConfig::load(&toml_path).ok()?;
    Some((config, package_dir))
}
