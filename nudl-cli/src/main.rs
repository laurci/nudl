mod pipeline;
mod render;

use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

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
        /// Source file to compile
        source: PathBuf,

        /// Output file path
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Dump the parsed AST
        #[arg(long)]
        dump_ast: bool,

        /// Dump the SSA IR
        #[arg(long)]
        dump_ir: bool,

        /// Dump the generated ARM64 assembly
        #[arg(long)]
        dump_asm: bool,

        /// Dump all the debug info
        #[arg(long)]
        dump_all: bool,
    },

    /// Compile and run
    Run {
        /// Source file to compile and run
        source: PathBuf,

        /// Run in the VM instead of compiling to native code
        #[arg(long)]
        vm: bool,

        /// Dump the parsed AST
        #[arg(long)]
        dump_ast: bool,

        /// Dump the SSA IR
        #[arg(long)]
        dump_ir: bool,

        /// Dump the generated ARM64 assembly
        #[arg(long)]
        dump_asm: bool,

        /// Dump all the debug info
        #[arg(long)]
        dump_all: bool,
    },

    /// Type-check only
    Check {
        /// Source file to check
        source: PathBuf,

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

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Build {
            source,
            output,
            dump_ast,
            dump_ir,
            dump_asm,
            dump_all,
        } => {
            let output = output.unwrap_or_else(|| {
                let stem = source.file_stem().unwrap_or_default();
                PathBuf::from(stem)
            });

            let dump = DumpOptions {
                dump_ast: dump_ast || dump_all,
                dump_ir: dump_ir || dump_all,
                dump_asm: dump_asm || dump_all,
            };
            let result = pipeline::build(&source, &output, &dump);
            render::render_diagnostics(&result.diagnostics, &result.source_map);

            if !result.success {
                process::exit(1);
            }
        }

        Command::Run {
            source,
            vm,
            dump_ast,
            dump_ir,
            dump_asm,
            dump_all,
        } => {
            if vm {
                let dump = DumpOptions {
                    dump_ast: dump_ast || dump_all,
                    dump_ir: dump_ir || dump_all,
                    dump_asm: false,
                };
                let result = pipeline::run_vm(&source, &dump);
                render::render_diagnostics(&result.diagnostics, &result.source_map);

                if !result.success {
                    process::exit(1);
                }
            } else {
                let tmp_dir = std::env::temp_dir();
                let output = tmp_dir.join("nudl_run_output");

                let dump = DumpOptions {
                    dump_ast: dump_ast || dump_all,
                    dump_ir: dump_ir || dump_all,
                    dump_asm: dump_asm || dump_all,
                };
                let result = pipeline::build(&source, &output, &dump);
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

                let _ = std::fs::remove_file(&output);

                process::exit(status.code().unwrap_or(1));
            }
        }

        Command::Check {
            source,
            dump_ast,
            dump_ir,
            dump_all,
        } => {
            let dump = DumpOptions {
                dump_ast: dump_ast || dump_all,
                dump_ir: dump_ir || dump_all,
                dump_asm: false,
            };
            let result = pipeline::check(&source, &dump);
            render::render_diagnostics(&result.diagnostics, &result.source_map);

            if result.diagnostics.has_errors() {
                process::exit(1);
            }
        }
    }
}
