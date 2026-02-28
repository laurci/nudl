mod pipeline;
mod render;

use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use nudl_core::package;

use pipeline::DumpOptions;

#[derive(Parser)]
#[command(name = "nudl", about = "The nudl programming language compiler")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compile to executable
    Build {
        /// Source file or bin target name (defaults to all bins in nudl.toml)
        source: Option<String>,

        /// Output file path
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Path to the nudl standard library directory
        #[arg(long)]
        std_path: Option<PathBuf>,

        /// Build with optimizations
        #[arg(long)]
        release: bool,

        /// Target the host CPU (like -march=native)
        #[arg(long)]
        native: bool,

        /// Extra arguments passed to the linker (e.g., object files, -L, -l)
        #[arg(long = "link")]
        link_args: Vec<String>,

        /// Dump the parsed AST
        #[arg(long)]
        dump_ast: bool,

        /// Dump the SSA IR
        #[arg(long)]
        dump_ir: bool,

        /// Dump the generated native assembly
        #[arg(long)]
        dump_asm: bool,

        /// Dump the LLVM IR
        #[arg(long)]
        dump_llvm_ir: bool,

        /// Dump all the debug info
        #[arg(long)]
        dump_all: bool,
    },

    /// Compile and run
    Run {
        /// Source file or bin target name (defaults to entry in nudl.toml)
        source: Option<String>,

        /// Run in the VM instead of compiling to native code
        #[arg(long)]
        vm: bool,

        /// Path to the nudl standard library directory
        #[arg(long)]
        std_path: Option<PathBuf>,

        /// Build with optimizations
        #[arg(long)]
        release: bool,

        /// Target the host CPU (like -march=native)
        #[arg(long)]
        native: bool,

        /// Extra arguments passed to the linker (e.g., object files, -L, -l)
        #[arg(long = "link")]
        link_args: Vec<String>,

        /// Dump the parsed AST
        #[arg(long)]
        dump_ast: bool,

        /// Dump the SSA IR
        #[arg(long)]
        dump_ir: bool,

        /// Dump the generated native assembly
        #[arg(long)]
        dump_asm: bool,

        /// Dump the LLVM IR
        #[arg(long)]
        dump_llvm_ir: bool,

        /// Dump all the debug info
        #[arg(long)]
        dump_all: bool,
    },

    /// Type-check only
    Check {
        /// Source file or bin target name (defaults to all bins in nudl.toml)
        source: Option<String>,

        /// Path to the nudl standard library directory
        #[arg(long)]
        std_path: Option<PathBuf>,

        /// Dump the parsed AST
        #[arg(long)]
        dump_ast: bool,

        /// Dump the SSA IR
        #[arg(long)]
        dump_ir: bool,

        /// Dump all the debug info
        #[arg(long)]
        dump_all: bool,
    },
}

/// A resolved build target: a name (for output) and a source path.
struct Target {
    name: String,
    source: PathBuf,
}

/// Resolved package information from `nudl.toml`.
struct PackageInfo {
    /// The directory containing `nudl.toml`.
    package_dir: PathBuf,
}

/// Resolve targets from an optional argument string.
/// The argument can be a bin target name (looked up in nudl.toml) or a file path.
/// Returns `(targets, optional_package_info)`.
fn resolve_targets(arg: Option<String>) -> (Vec<Target>, Option<PackageInfo>) {
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        eprintln!("error: could not determine current directory: {}", e);
        process::exit(1);
    });

    let package = package::discover_package(&cwd);

    if let Some(arg) = arg {
        // Check if arg matches a bin target name in the package
        if let Some((config, package_dir)) = &package {
            if let Some(bin) = config.find_bin(&arg) {
                let source = config.resolve_bin_path(bin, package_dir);
                if !source.exists() {
                    eprintln!(
                        "error: source file '{}' for bin target '{}' does not exist",
                        source.display(),
                        bin.name
                    );
                    process::exit(1);
                }
                let pkg_info = PackageInfo {
                    package_dir: package_dir.clone(),
                };
                return (
                    vec![Target {
                        name: bin.name.clone(),
                        source,
                    }],
                    Some(pkg_info),
                );
            }
        }

        // Treat as file path
        let path = PathBuf::from(&arg);
        if !path.exists() {
            eprintln!("error: source file '{}' does not exist", path.display());
            process::exit(1);
        }
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        return (vec![Target { name, source: path }], None);
    }

    // No argument: use all bins from package
    match package {
        Some((config, package_dir)) => {
            if config.bin.is_empty() {
                eprintln!("error: no [[bin]] targets defined in nudl.toml");
                process::exit(1);
            }
            let targets: Vec<Target> = config
                .bin_paths(&package_dir)
                .into_iter()
                .map(|(name, source)| {
                    if !source.exists() {
                        eprintln!(
                            "error: source file '{}' for bin target '{}' does not exist",
                            source.display(),
                            name
                        );
                        process::exit(1);
                    }
                    Target { name, source }
                })
                .collect();
            let pkg_info = PackageInfo { package_dir };
            (targets, Some(pkg_info))
        }
        None => {
            eprintln!("error: no source file provided and no nudl.toml found");
            eprintln!("usage: nudl build [SOURCE]");
            process::exit(1);
        }
    }
}

/// Compute the output path for a build target.
/// When a package is present, outputs go to `<package_dir>/.nudl/{debug,release}/<name>`.
/// Otherwise, outputs go to the current directory.
/// Creates the output directory if it doesn't exist.
fn resolve_output(
    output: Option<PathBuf>,
    package: &Option<PackageInfo>,
    target: &Target,
    release: bool,
) -> PathBuf {
    let path = if let Some(output) = output {
        output
    } else if let Some(pkg) = package {
        let profile = if release { "release" } else { "debug" };
        pkg.package_dir
            .join(".nudl")
            .join(profile)
            .join(&target.name)
    } else {
        PathBuf::from(&target.name)
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    path
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Build {
            source,
            output,
            std_path,
            release,
            native,
            link_args,
            dump_ast,
            dump_ir,
            dump_asm,
            dump_llvm_ir,
            dump_all,
        } => {
            let (targets, package) = resolve_targets(source);

            let dump = DumpOptions {
                dump_ast: dump_ast || dump_all,
                dump_ir: dump_ir || dump_all,
                dump_asm: dump_asm || dump_all,
                dump_llvm_ir: dump_llvm_ir || dump_all,
            };

            let mut any_failed = false;
            for target in &targets {
                let out = resolve_output(output.clone(), &package, target, release);
                let result = pipeline::build(
                    &target.source,
                    &out,
                    std_path.as_deref(),
                    release,
                    native,
                    &link_args,
                    &dump,
                );
                render::render_diagnostics(&result.diagnostics, &result.source_map);

                if !result.success {
                    any_failed = true;
                }
            }

            if any_failed {
                process::exit(1);
            }
        }

        Command::Run {
            source,
            vm,
            std_path,
            release,
            native,
            link_args,
            dump_ast,
            dump_ir,
            dump_asm,
            dump_llvm_ir,
            dump_all,
        } => {
            let (targets, package) = resolve_targets(source);

            // `run` requires exactly one target
            if targets.len() > 1 {
                eprintln!("error: multiple bin targets found; specify which one to run:");
                for t in &targets {
                    eprintln!("  nudl run {}", t.name);
                }
                process::exit(1);
            }

            let target = &targets[0];

            if vm {
                let dump = DumpOptions {
                    dump_ast: dump_ast || dump_all,
                    dump_ir: dump_ir || dump_all,
                    dump_asm: false,
                    dump_llvm_ir: false,
                };
                let result = pipeline::run_vm(&target.source, std_path.as_deref(), &dump);
                render::render_diagnostics(&result.diagnostics, &result.source_map);

                if !result.success {
                    process::exit(1);
                }
            } else {
                let (output, is_temp) = if package.is_some() {
                    let output = resolve_output(None, &package, target, release);
                    (output, false)
                } else {
                    let tmp_dir = std::env::temp_dir();
                    (tmp_dir.join("nudl_run_output"), true)
                };

                let dump = DumpOptions {
                    dump_ast: dump_ast || dump_all,
                    dump_ir: dump_ir || dump_all,
                    dump_asm: dump_asm || dump_all,
                    dump_llvm_ir: dump_llvm_ir || dump_all,
                };
                let result = pipeline::build(
                    &target.source,
                    &output,
                    std_path.as_deref(),
                    release,
                    native,
                    &link_args,
                    &dump,
                );
                render::render_diagnostics(&result.diagnostics, &result.source_map);

                if !result.success {
                    process::exit(1);
                }

                let status = std::process::Command::new(&output)
                    .status()
                    .unwrap_or_else(|e| {
                        eprintln!("error: failed to execute '{}': {}", output.display(), e);
                        process::exit(1);
                    });

                if is_temp {
                    let _ = std::fs::remove_file(&output);
                }

                process::exit(status.code().unwrap_or(1));
            }
        }

        Command::Check {
            source,
            std_path,
            dump_ast,
            dump_ir,
            dump_all,
        } => {
            let (targets, _) = resolve_targets(source);

            let dump = DumpOptions {
                dump_ast: dump_ast || dump_all,
                dump_ir: dump_ir || dump_all,
                dump_asm: false,
                dump_llvm_ir: false,
            };

            let mut any_failed = false;
            for target in &targets {
                let result = pipeline::check(&target.source, std_path.as_deref(), &dump);
                render::render_diagnostics(&result.diagnostics, &result.source_map);

                if result.diagnostics.has_errors() {
                    any_failed = true;
                }
            }

            if any_failed {
                process::exit(1);
            }
        }
    }
}
