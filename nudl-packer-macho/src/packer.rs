use std::path::Path;
use std::process::Command;

use object::write::{Object, Symbol, SymbolSection, Relocation as ObjRelocation};
use object::{
    Architecture, BinaryFormat, Endianness, RelocationFlags, SectionKind,
    SymbolFlags, SymbolKind, SymbolScope,
};

use nudl_backend_arm64::codegen::{CodegenResult, RelocKind, RelocTarget};

#[derive(Debug)]
pub enum PackError {
    ObjectWrite(String),
    IoError(std::io::Error),
    LinkError(String),
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackError::ObjectWrite(msg) => write!(f, "object write error: {}", msg),
            PackError::IoError(e) => write!(f, "I/O error: {}", e),
            PackError::LinkError(msg) => write!(f, "link error: {}", msg),
        }
    }
}

impl std::error::Error for PackError {}

impl From<std::io::Error> for PackError {
    fn from(e: std::io::Error) -> Self {
        PackError::IoError(e)
    }
}

pub fn pack(codegen: &CodegenResult, output: &Path) -> Result<(), PackError> {
    let obj_path = output.with_extension("o");

    // Build object file
    write_object(codegen, &obj_path)?;

    // Link using system linker
    link(&obj_path, output)?;

    // Clean up object file
    let _ = std::fs::remove_file(&obj_path);

    Ok(())
}

fn write_object(codegen: &CodegenResult, obj_path: &Path) -> Result<(), PackError> {
    let mut obj = Object::new(BinaryFormat::MachO, Architecture::Aarch64, Endianness::Little);

    // Create __TEXT,__text section
    let text_section = obj.section_id(object::write::StandardSection::Text);

    // Create __TEXT,__const section for string constants
    let cstring_section = obj.add_section(
        b"__TEXT".to_vec(),
        b"__const".to_vec(),
        SectionKind::ReadOnlyData,
    );

    // Write code to text section
    let code_offset = obj.append_section_data(text_section, &codegen.code, 4);

    // Write string data to cstring section
    let data_offset = obj.append_section_data(cstring_section, &codegen.data, 1);

    // Add symbols for each string constant in the data section
    let mut string_symbol_ids = Vec::new();
    for (i, &(offset, _len)) in codegen.string_offsets.iter().enumerate() {
        let sym_id = obj.add_symbol(Symbol {
            name: format!("l_.str.{}", i).into_bytes(),
            value: data_offset + offset as u64,
            size: 0,
            kind: SymbolKind::Data,
            scope: SymbolScope::Compilation,
            weak: false,
            section: SymbolSection::Section(cstring_section),
            flags: SymbolFlags::None,
        });
        string_symbol_ids.push(sym_id);
    }

    // Add function symbols from codegen
    for func_sym in &codegen.function_symbols {
        let scope = if func_sym.is_entry {
            SymbolScope::Dynamic
        } else {
            SymbolScope::Compilation
        };
        // The entry function gets the name "main" (object crate adds _ prefix for Mach-O)
        let name = if func_sym.is_entry {
            "main".to_string()
        } else {
            func_sym.name.clone()
        };
        obj.add_symbol(Symbol {
            name: name.into_bytes(),
            value: code_offset + func_sym.offset as u64,
            size: func_sym.size as u64,
            kind: SymbolKind::Text,
            scope,
            weak: false,
            section: SymbolSection::Section(text_section),
            flags: SymbolFlags::None,
        });
    }

    // Add extern symbols (undefined)
    // The object crate auto-adds the Mach-O _ prefix
    let mut extern_symbol_ids = Vec::new();
    for name in &codegen.extern_symbols {
        let sym_id = obj.add_symbol(Symbol {
            name: name.as_bytes().to_vec(),
            value: 0,
            size: 0,
            kind: SymbolKind::Text,
            scope: SymbolScope::Dynamic,
            weak: false,
            section: SymbolSection::Undefined,
            flags: SymbolFlags::None,
        });
        extern_symbol_ids.push(sym_id);
    }

    // Add relocations
    for reloc in &codegen.relocations {
        let flags = match reloc.kind {
            RelocKind::Page21 => RelocationFlags::MachO {
                r_type: object::macho::ARM64_RELOC_PAGE21,
                r_pcrel: true,
                r_length: 2,
            },
            RelocKind::PageOff12 => RelocationFlags::MachO {
                r_type: object::macho::ARM64_RELOC_PAGEOFF12,
                r_pcrel: false,
                r_length: 2,
            },
            RelocKind::Branch26 => RelocationFlags::MachO {
                r_type: object::macho::ARM64_RELOC_BRANCH26,
                r_pcrel: true,
                r_length: 2,
            },
        };

        let (symbol, addend) = match &reloc.target {
            RelocTarget::DataSection(offset) => {
                let str_idx = codegen.string_offsets.iter().position(|&(o, _)| o == *offset)
                    .expect("relocation targets unknown data offset");
                (string_symbol_ids[str_idx], 0i64)
            }
            RelocTarget::ExternSymbol(idx) => {
                (extern_symbol_ids[*idx], 0)
            }
        };

        obj.add_relocation(text_section, ObjRelocation {
            offset: code_offset + reloc.offset as u64,
            symbol,
            addend,
            flags,
        }).map_err(|e| PackError::ObjectWrite(e.to_string()))?;
    }

    let data = obj.write().map_err(|e| PackError::ObjectWrite(e.to_string()))?;
    std::fs::write(obj_path, data)?;

    Ok(())
}

fn link(obj_path: &Path, output: &Path) -> Result<(), PackError> {
    let status = Command::new("cc")
        .arg("-o")
        .arg(output)
        .arg(obj_path)
        .arg("-lSystem")
        .arg("-arch")
        .arg("arm64")
        .status()?;

    if !status.success() {
        return Err(PackError::LinkError(format!(
            "linker exited with status: {}",
            status
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    use nudl_ast::lexer::Lexer;
    use nudl_ast::parser::Parser;
    use nudl_bc::checker::Checker;
    use nudl_bc::lower::Lowerer;
    use nudl_backend_arm64::codegen::Codegen;
    use nudl_core::span::FileId;

    fn compile_and_run(source: &str) -> (String, bool) {
        let (tokens, _) = Lexer::new(source, FileId(0)).tokenize();
        let (module, _) = Parser::new(tokens).parse_module();
        let (checked, diags) = Checker::new().check(&module);
        assert!(!diags.has_errors(), "checker errors: {:?}", diags.reports());
        let program = Lowerer::new(checked).lower(&module);
        let codegen_result = Codegen::new().generate(&program);

        let output = std::env::temp_dir().join("nudl_test_hello");
        pack(&codegen_result, &output).expect("packing failed");

        assert!(output.exists(), "output binary should exist");

        let result = Command::new(&output)
            .output()
            .expect("failed to run binary");

        let stdout = String::from_utf8_lossy(&result.stdout).to_string();
        let success = result.status.success();

        let _ = std::fs::remove_file(&output);

        (stdout, success)
    }

    #[test]
    fn pack_and_run_hello_world() {
        let (stdout, success) = compile_and_run(r#"
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}

fn print(s: string) {
    write(1, __str_ptr(s), __str_len(s));
}

fn println(s: string) {
    print(s);
    print("\n");
}

fn main() {
    println("Hello, world!");
}
"#);

        assert_eq!(stdout, "Hello, world!\n");
        assert!(success, "binary should exit with 0");
    }
}
