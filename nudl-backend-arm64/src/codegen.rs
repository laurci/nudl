use std::collections::HashMap;

use nudl_bc::ir::*;
use nudl_core::types::{TypeInterner, TypeKind};

/// Relocation kinds for ARM64
#[derive(Debug, Clone)]
pub enum RelocKind {
    /// ADRP instruction — page-relative 21-bit offset
    Page21,
    /// ADD/LDR instruction — page offset 12-bit
    PageOff12,
    /// BL instruction — 26-bit PC-relative branch
    Branch26,
}

/// What a relocation points to
#[derive(Debug, Clone)]
pub enum RelocTarget {
    /// Offset within the data section
    DataSection(u32),
    /// Index into extern_symbols
    ExternSymbol(usize),
}

#[derive(Debug, Clone)]
pub struct Relocation {
    /// Offset within the code section where the instruction lives
    pub offset: u32,
    pub kind: RelocKind,
    pub target: RelocTarget,
}

#[derive(Debug)]
pub struct FunctionSymbol {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    pub is_entry: bool,
}

#[derive(Debug)]
pub struct CodegenResult {
    pub code: Vec<u8>,
    pub data: Vec<u8>,
    pub entry_offset: u32,
    pub relocations: Vec<Relocation>,
    pub extern_symbols: Vec<String>,
    /// (offset, length) for each string constant in data section
    pub string_offsets: Vec<(u32, u32)>,
    pub function_symbols: Vec<FunctionSymbol>,
}

/// Tracks what an SSA register holds so StringPtr/StringLen can resolve correctly
#[derive(Debug, Clone)]
enum RegInfo {
    /// Holds a string literal (index into string_constants)
    StringLiteral(u32),
    /// Holds a string parameter (ptr_arm_reg, len_arm_reg)
    StringParam(u32, u32),
    /// Holds a general value in the given ARM64 register
    General(u32),
}

/// Maps parameter index to ARM64 register layout accounting for string pairs
struct ParamLayout {
    /// For each SSA param index: (first arm64 register, count of arm64 regs used)
    entries: Vec<(u32, u32)>,
    /// Total ARM64 registers consumed by all params
    total_arm_regs: u32,
}

impl ParamLayout {
    fn compute(func: &Function, types: &TypeInterner) -> Self {
        let mut entries = Vec::new();
        let mut arm_reg = 0u32;
        for (_name, type_id) in &func.params {
            let kind = types.resolve(*type_id);
            match kind {
                TypeKind::String => {
                    entries.push((arm_reg, 2)); // ptr, len
                    arm_reg += 2;
                }
                _ => {
                    entries.push((arm_reg, 1));
                    arm_reg += 1;
                }
            }
        }
        ParamLayout {
            entries,
            total_arm_regs: arm_reg,
        }
    }
}

/// Context built once, passed to emit_function for call resolution
struct CodegenContext {
    /// FunctionId → ParamLayout
    layouts: HashMap<u32, ParamLayout>,
    /// Symbol.0 (from FunctionRef::Named) → FunctionId
    named_sym_to_func_id: HashMap<u32, u32>,
    /// Symbol.0 (from FunctionRef::Extern) → (FunctionId, extern_symbol_name)
    extern_sym_to_info: HashMap<u32, (u32, String)>,
}

/// ARM64 code generator
pub struct Codegen {
    code: Vec<u8>,
    data: Vec<u8>,
    relocations: Vec<Relocation>,
    extern_symbols: Vec<String>,
    extern_symbol_map: HashMap<String, usize>,
    string_offsets: Vec<(u32, u32)>,
    function_symbols: Vec<FunctionSymbol>,
    /// FunctionId → code offset
    function_offsets: HashMap<u32, u32>,
    /// Pending internal calls: (code_offset_of_bl, target_function_id)
    pending_internal_calls: Vec<(u32, u32)>,
    /// Types for resolving param layouts
    types: TypeInterner,
}

impl Codegen {
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            data: Vec::new(),
            relocations: Vec::new(),
            extern_symbols: Vec::new(),
            extern_symbol_map: HashMap::new(),
            string_offsets: Vec::new(),
            function_symbols: Vec::new(),
            function_offsets: HashMap::new(),
            pending_internal_calls: Vec::new(),
            types: TypeInterner::new(),
        }
    }

    pub fn generate(mut self, program: &Program) -> CodegenResult {
        // Layout string constants in data section
        for s in &program.string_constants {
            let offset = self.data.len() as u32;
            let bytes = s.as_bytes();
            self.data.extend_from_slice(bytes);
            self.string_offsets.push((offset, bytes.len() as u32));
        }

        // Build lookup maps for function resolution
        let mut named_sym_to_func_id: HashMap<u32, u32> = HashMap::new();
        let mut extern_sym_to_info: HashMap<u32, (u32, String)> = HashMap::new();
        let mut layouts: HashMap<u32, ParamLayout> = HashMap::new();

        for func in &program.functions {
            named_sym_to_func_id.insert(func.name.0, func.id.0);
            layouts.insert(func.id.0, ParamLayout::compute(func, &self.types));

            if func.is_extern {
                if let Some(ref ext_sym) = func.extern_symbol {
                    let idx = self.extern_symbols.len();
                    self.extern_symbol_map.insert(ext_sym.clone(), idx);
                    self.extern_symbols.push(ext_sym.clone());

                    // FunctionRef::Extern uses the same symbol as func.name
                    extern_sym_to_info.insert(func.name.0, (func.id.0, ext_sym.clone()));
                }
            }
        }

        let ctx = CodegenContext {
            layouts,
            named_sym_to_func_id,
            extern_sym_to_info,
        };

        let entry_id = program.entry_function;

        // Emit code for each non-extern function
        for func in &program.functions {
            if func.is_extern {
                continue;
            }

            let func_offset = self.code.len() as u32;
            self.function_offsets.insert(func.id.0, func_offset);

            let is_entry = entry_id == Some(func.id);
            let layout = ParamLayout::compute(func, &self.types);

            self.emit_function(func, program, &layout, &ctx, is_entry);

            let func_size = self.code.len() as u32 - func_offset;

            self.function_symbols.push(FunctionSymbol {
                name: format!("__func_{}", func.id.0),
                offset: func_offset,
                size: func_size,
                is_entry,
            });
        }

        // Resolve internal calls
        for (bl_offset, target_func_id) in &self.pending_internal_calls {
            if let Some(&target_offset) = self.function_offsets.get(target_func_id) {
                let rel_offset = (target_offset as i32 - *bl_offset as i32) / 4;
                let bl_instr = encode_bl(rel_offset);
                let off = *bl_offset as usize;
                self.code[off..off + 4].copy_from_slice(&bl_instr.to_le_bytes());
            }
        }

        let entry_offset = entry_id
            .and_then(|eid| self.function_offsets.get(&eid.0))
            .copied()
            .unwrap_or(0);

        CodegenResult {
            code: self.code,
            data: self.data,
            entry_offset,
            relocations: self.relocations,
            extern_symbols: self.extern_symbols,
            string_offsets: self.string_offsets,
            function_symbols: self.function_symbols,
        }
    }

    fn emit_function(
        &mut self,
        func: &Function,
        _program: &Program,
        layout: &ParamLayout,
        ctx: &CodegenContext,
        is_entry: bool,
    ) {
        // Frame size: save X29/X30 + callee-saved regs for params
        let num_callee_saved = layout.total_arm_regs;
        let save_pairs = 1 + ((num_callee_saved + 1) / 2); // +1 for X29/X30 pair
        let frame_size = save_pairs * 16;

        // Prologue: STP X29, X30, [SP, #-frame_size]!
        self.emit_u32(encode_stp_pre(29, 30, frame_size));
        // MOV X29, SP
        self.emit_u32(0x910003fd);

        let callee_saved_base = 19u32;

        // Save callee-saved registers we'll use
        for i in (0..num_callee_saved).step_by(2) {
            let callee_reg = callee_saved_base + i;
            let slot_offset = (1 + i / 2) * 16;
            if i + 1 < num_callee_saved {
                self.emit_u32(encode_stp_offset(
                    callee_reg,
                    callee_reg + 1,
                    29,
                    slot_offset,
                ));
            } else {
                self.emit_u32(encode_str_offset(callee_reg, 29, slot_offset));
            }
        }

        // Move parameter values from X0-X7 to callee-saved X19+
        for i in 0..layout.total_arm_regs {
            self.emit_u32(encode_mov_reg(callee_saved_base + i, i));
        }

        // Build RegInfo for SSA param registers
        let mut reg_info: HashMap<u32, RegInfo> = HashMap::new();
        for (param_idx, &(first_arm, count)) in layout.entries.iter().enumerate() {
            let callee_first = callee_saved_base + first_arm;
            if count == 2 {
                reg_info.insert(
                    param_idx as u32,
                    RegInfo::StringParam(callee_first, callee_first + 1),
                );
            } else {
                reg_info.insert(param_idx as u32, RegInfo::General(callee_first));
            }
        }

        // Temp register allocator
        let mut next_temp = 9u32;
        let mut alloc_temp = || -> u32 {
            let r = next_temp;
            next_temp += 1;
            if next_temp > 15 {
                next_temp = 9;
            }
            r
        };

        for block in &func.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::Const(reg, ConstValue::StringLiteral(idx)) => {
                        reg_info.insert(reg.0, RegInfo::StringLiteral(*idx));
                    }

                    Instruction::Const(reg, ConstValue::I32(val)) => {
                        let arm_reg = alloc_temp();
                        self.emit_mov_imm(arm_reg, *val as u64);
                        reg_info.insert(reg.0, RegInfo::General(arm_reg));
                    }

                    Instruction::Const(reg, ConstValue::U64(val)) => {
                        let arm_reg = alloc_temp();
                        self.emit_mov_imm(arm_reg, *val);
                        reg_info.insert(reg.0, RegInfo::General(arm_reg));
                    }

                    Instruction::Const(reg, ConstValue::I64(val)) => {
                        let arm_reg = alloc_temp();
                        self.emit_mov_imm(arm_reg, *val as u64);
                        reg_info.insert(reg.0, RegInfo::General(arm_reg));
                    }

                    Instruction::Const(reg, ConstValue::Bool(val)) => {
                        let arm_reg = alloc_temp();
                        self.emit_mov_imm(arm_reg, if *val { 1 } else { 0 });
                        reg_info.insert(reg.0, RegInfo::General(arm_reg));
                    }

                    Instruction::Const(_, ConstValue::Unit) => {}

                    Instruction::StringPtr(dst, src) => {
                        let arm_dst = alloc_temp();
                        match reg_info.get(&src.0) {
                            Some(RegInfo::StringLiteral(idx)) => {
                                let (offset, _) = self.string_offsets[*idx as usize];
                                let code_offset = self.code.len() as u32;
                                self.emit_u32(encode_adrp(arm_dst, 0));
                                self.relocations.push(Relocation {
                                    offset: code_offset,
                                    kind: RelocKind::Page21,
                                    target: RelocTarget::DataSection(offset),
                                });
                                let code_offset = self.code.len() as u32;
                                self.emit_u32(encode_add_imm(arm_dst, arm_dst, 0));
                                self.relocations.push(Relocation {
                                    offset: code_offset,
                                    kind: RelocKind::PageOff12,
                                    target: RelocTarget::DataSection(offset),
                                });
                            }
                            Some(RegInfo::StringParam(ptr_reg, _)) => {
                                self.emit_u32(encode_mov_reg(arm_dst, *ptr_reg));
                            }
                            _ => {
                                self.emit_u32(encode_movz(arm_dst, 0, 0));
                            }
                        }
                        reg_info.insert(dst.0, RegInfo::General(arm_dst));
                    }

                    Instruction::StringLen(dst, src) => {
                        let arm_dst = alloc_temp();
                        match reg_info.get(&src.0) {
                            Some(RegInfo::StringLiteral(idx)) => {
                                let (_, len) = self.string_offsets[*idx as usize];
                                self.emit_mov_imm(arm_dst, len as u64);
                            }
                            Some(RegInfo::StringParam(_, len_reg)) => {
                                self.emit_u32(encode_mov_reg(arm_dst, *len_reg));
                            }
                            _ => {
                                self.emit_u32(encode_movz(arm_dst, 0, 0));
                            }
                        }
                        reg_info.insert(dst.0, RegInfo::General(arm_dst));
                    }

                    Instruction::Call(result_reg, func_ref, args) => {
                        // Resolve callee info
                        let (callee_func_id, is_extern, extern_sym) = match func_ref {
                            FunctionRef::Named(sym) => {
                                let fid = ctx.named_sym_to_func_id.get(&sym.0).copied();
                                (fid, false, None)
                            }
                            FunctionRef::Extern(sym) => {
                                if let Some(&(fid, ref es)) = ctx.extern_sym_to_info.get(&sym.0) {
                                    (Some(fid), true, Some(es.clone()))
                                } else {
                                    (None, true, None)
                                }
                            }
                            FunctionRef::Builtin(_) => (None, false, None),
                        };

                        // Get callee's param layout
                        let callee_layout = callee_func_id.and_then(|fid| ctx.layouts.get(&fid));

                        // Marshal arguments
                        if let Some(cl) = callee_layout {
                            for (i, arg_reg) in args.iter().enumerate() {
                                if i >= cl.entries.len() {
                                    break;
                                }
                                let (first_arm, count) = cl.entries[i];
                                match reg_info.get(&arg_reg.0) {
                                    Some(RegInfo::StringLiteral(idx)) if count == 2 => {
                                        let (offset, len) = self.string_offsets[*idx as usize];
                                        let code_offset = self.code.len() as u32;
                                        self.emit_u32(encode_adrp(first_arm, 0));
                                        self.relocations.push(Relocation {
                                            offset: code_offset,
                                            kind: RelocKind::Page21,
                                            target: RelocTarget::DataSection(offset),
                                        });
                                        let code_offset = self.code.len() as u32;
                                        self.emit_u32(encode_add_imm(first_arm, first_arm, 0));
                                        self.relocations.push(Relocation {
                                            offset: code_offset,
                                            kind: RelocKind::PageOff12,
                                            target: RelocTarget::DataSection(offset),
                                        });
                                        self.emit_mov_imm(first_arm + 1, len as u64);
                                    }
                                    Some(RegInfo::StringParam(ptr_reg, len_reg)) if count == 2 => {
                                        if first_arm != *ptr_reg {
                                            self.emit_u32(encode_mov_reg(first_arm, *ptr_reg));
                                        }
                                        if first_arm + 1 != *len_reg {
                                            self.emit_u32(encode_mov_reg(first_arm + 1, *len_reg));
                                        }
                                    }
                                    Some(RegInfo::General(arm_reg)) => {
                                        if first_arm != *arm_reg {
                                            self.emit_u32(encode_mov_reg(first_arm, *arm_reg));
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        } else {
                            // No layout — simple positional marshalling
                            for (i, arg_reg) in args.iter().enumerate() {
                                if let Some(RegInfo::General(arm_reg)) = reg_info.get(&arg_reg.0) {
                                    if i as u32 != *arm_reg {
                                        self.emit_u32(encode_mov_reg(i as u32, *arm_reg));
                                    }
                                }
                            }
                        }

                        // Emit BL
                        if is_extern {
                            if let Some(ref ext_sym) = extern_sym {
                                if let Some(&ext_idx) = self.extern_symbol_map.get(ext_sym) {
                                    let code_offset = self.code.len() as u32;
                                    self.emit_u32(encode_bl(0));
                                    self.relocations.push(Relocation {
                                        offset: code_offset,
                                        kind: RelocKind::Branch26,
                                        target: RelocTarget::ExternSymbol(ext_idx),
                                    });
                                }
                            }
                        } else if let Some(target_fid) = callee_func_id {
                            let code_offset = self.code.len() as u32;
                            self.emit_u32(encode_bl(0)); // placeholder
                            self.pending_internal_calls.push((code_offset, target_fid));
                        }

                        // Result in X0
                        let arm_dst = alloc_temp();
                        if arm_dst != 0 {
                            self.emit_u32(encode_mov_reg(arm_dst, 0));
                        }
                        reg_info.insert(result_reg.0, RegInfo::General(arm_dst));
                    }

                    Instruction::ConstUnit(_) | Instruction::Nop => {}

                    Instruction::Copy(dst, src) => {
                        if let Some(info) = reg_info.get(&src.0).cloned() {
                            reg_info.insert(dst.0, info);
                        }
                    }

                    // Legacy instructions
                    Instruction::StringConstPtr(reg, str_idx) => {
                        let arm_reg = alloc_temp();
                        let (offset, _) = self.string_offsets[*str_idx as usize];
                        let code_offset = self.code.len() as u32;
                        self.emit_u32(encode_adrp(arm_reg, 0));
                        self.relocations.push(Relocation {
                            offset: code_offset,
                            kind: RelocKind::Page21,
                            target: RelocTarget::DataSection(offset),
                        });
                        let code_offset = self.code.len() as u32;
                        self.emit_u32(encode_add_imm(arm_reg, arm_reg, 0));
                        self.relocations.push(Relocation {
                            offset: code_offset,
                            kind: RelocKind::PageOff12,
                            target: RelocTarget::DataSection(offset),
                        });
                        reg_info.insert(reg.0, RegInfo::General(arm_reg));
                    }

                    Instruction::StringConstLen(reg, str_idx) => {
                        let arm_reg = alloc_temp();
                        let (_, len) = self.string_offsets[*str_idx as usize];
                        self.emit_mov_imm(arm_reg, len as u64);
                        reg_info.insert(reg.0, RegInfo::General(arm_reg));
                    }
                }
            }

            // Emit terminator
            match &block.terminator {
                Terminator::Return(_) => {
                    if is_entry {
                        // main: set exit code 0
                        self.emit_u32(encode_movz(0, 0, 0));
                    }

                    // Restore callee-saved registers
                    for i in (0..num_callee_saved).step_by(2) {
                        let callee_reg = callee_saved_base + i;
                        let slot_offset = (1 + i / 2) * 16;
                        if i + 1 < num_callee_saved {
                            self.emit_u32(encode_ldp_offset(
                                callee_reg,
                                callee_reg + 1,
                                29,
                                slot_offset,
                            ));
                        } else {
                            self.emit_u32(encode_ldr_offset(callee_reg, 29, slot_offset));
                        }
                    }

                    // Epilogue: LDP X29, X30, [SP], #frame_size
                    self.emit_u32(encode_ldp_post(29, 30, frame_size));
                    // RET
                    self.emit_u32(0xd65f03c0);
                }
                _ => {}
            }
        }
    }

    fn emit_mov_imm(&mut self, rd: u32, value: u64) {
        if value <= 0xFFFF {
            self.emit_u32(encode_movz(rd, value as u16, 0));
        } else if value <= 0xFFFF_FFFF {
            self.emit_u32(encode_movz(rd, (value & 0xFFFF) as u16, 0));
            self.emit_u32(encode_movk(rd, ((value >> 16) & 0xFFFF) as u16, 1));
        } else {
            self.emit_u32(encode_movz(rd, (value & 0xFFFF) as u16, 0));
            if (value >> 16) & 0xFFFF != 0 {
                self.emit_u32(encode_movk(rd, ((value >> 16) & 0xFFFF) as u16, 1));
            }
            if (value >> 32) & 0xFFFF != 0 {
                self.emit_u32(encode_movk(rd, ((value >> 32) & 0xFFFF) as u16, 2));
            }
            if (value >> 48) & 0xFFFF != 0 {
                self.emit_u32(encode_movk(rd, ((value >> 48) & 0xFFFF) as u16, 3));
            }
        }
    }

    fn emit_u32(&mut self, val: u32) {
        self.code.extend_from_slice(&val.to_le_bytes());
    }
}

// ARM64 instruction encoders

/// ADRP Xd, #imm — encodes with imm=0, to be patched by relocation
fn encode_adrp(rd: u32, _imm: i32) -> u32 {
    0x90000000 | (rd & 0x1f)
}

/// ADD Xd, Xn, #imm12
fn encode_add_imm(rd: u32, rn: u32, imm12: u32) -> u32 {
    0x91000000 | ((imm12 & 0xfff) << 10) | ((rn & 0x1f) << 5) | (rd & 0x1f)
}

/// MOV Xd, Xn (encoded as ORR Xd, XZR, Xn)
fn encode_mov_reg(rd: u32, rm: u32) -> u32 {
    0xaa0003e0 | ((rm & 0x1f) << 16) | (rd & 0x1f)
}

/// BL #offset (26-bit signed, in units of 4 bytes)
fn encode_bl(offset: i32) -> u32 {
    let imm26 = (offset as u32) & 0x03ffffff;
    0x94000000 | imm26
}

/// MOVZ Xd, #imm16, LSL #(hw*16)
fn encode_movz(rd: u32, imm16: u16, hw: u32) -> u32 {
    0xd2800000 | ((hw & 0x3) << 21) | ((imm16 as u32) << 5) | (rd & 0x1f)
}

/// MOVK Xd, #imm16, LSL #(hw*16)
fn encode_movk(rd: u32, imm16: u16, hw: u32) -> u32 {
    0xf2800000 | ((hw & 0x3) << 21) | ((imm16 as u32) << 5) | (rd & 0x1f)
}

/// STP Xt1, Xt2, [SP, #-imm]! (pre-indexed)
fn encode_stp_pre(rt1: u32, rt2: u32, frame_size: u32) -> u32 {
    let imm7 = ((-(frame_size as i32)) / 8) as u32 & 0x7f;
    0xa9800000 | (imm7 << 15) | ((rt2 & 0x1f) << 10) | (31 << 5) | (rt1 & 0x1f)
}

/// LDP Xt1, Xt2, [SP], #imm (post-indexed)
fn encode_ldp_post(rt1: u32, rt2: u32, frame_size: u32) -> u32 {
    let imm7 = (frame_size / 8) & 0x7f;
    0xa8c00000 | (imm7 << 15) | ((rt2 & 0x1f) << 10) | (31 << 5) | (rt1 & 0x1f)
}

/// STP Xt1, Xt2, [Xn, #offset] (signed offset)
fn encode_stp_offset(rt1: u32, rt2: u32, rn: u32, offset: u32) -> u32 {
    let imm7 = (offset / 8) & 0x7f;
    0xa9000000 | (imm7 << 15) | ((rt2 & 0x1f) << 10) | ((rn & 0x1f) << 5) | (rt1 & 0x1f)
}

/// LDP Xt1, Xt2, [Xn, #offset] (signed offset)
fn encode_ldp_offset(rt1: u32, rt2: u32, rn: u32, offset: u32) -> u32 {
    let imm7 = (offset / 8) & 0x7f;
    0xa9400000 | (imm7 << 15) | ((rt2 & 0x1f) << 10) | ((rn & 0x1f) << 5) | (rt1 & 0x1f)
}

/// STR Xt, [Xn, #offset] (unsigned offset)
fn encode_str_offset(rt: u32, rn: u32, offset: u32) -> u32 {
    let imm12 = (offset / 8) & 0xfff;
    0xf9000000 | (imm12 << 10) | ((rn & 0x1f) << 5) | (rt & 0x1f)
}

/// LDR Xt, [Xn, #offset] (unsigned offset)
fn encode_ldr_offset(rt: u32, rn: u32, offset: u32) -> u32 {
    let imm12 = (offset / 8) & 0xfff;
    0xf9400000 | (imm12 << 10) | ((rn & 0x1f) << 5) | (rt & 0x1f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nudl_ast::lexer::Lexer;
    use nudl_ast::parser::Parser;
    use nudl_bc::checker::Checker;
    use nudl_bc::lower::Lowerer;
    use nudl_core::span::FileId;

    fn generate_from_source(source: &str) -> CodegenResult {
        let (tokens, _) = Lexer::new(source, FileId(0)).tokenize();
        let (module, _) = Parser::new(tokens).parse_module();
        let (checked, diags) = Checker::new().check(&module);
        assert!(!diags.has_errors(), "checker errors: {:?}", diags.reports());
        let program = Lowerer::new(checked).lower(&module);
        Codegen::new().generate(&program)
    }

    #[test]
    fn codegen_target_program() {
        let result = generate_from_source(
            r#"
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
"#,
        );

        assert!(!result.code.is_empty());
        assert!(!result.data.is_empty());

        // 3 function symbols (print, println, main)
        assert_eq!(
            result.function_symbols.len(),
            3,
            "expected 3 function symbols, got {}: {:?}",
            result.function_symbols.len(),
            result
                .function_symbols
                .iter()
                .map(|s| &s.name)
                .collect::<Vec<_>>()
        );

        assert!(result.extern_symbols.contains(&"write".to_string()));
        assert!(!result.relocations.is_empty());
        assert!(result.function_symbols.iter().any(|s| s.is_entry));
    }

    #[test]
    fn codegen_simple_main() {
        let result = generate_from_source(
            r#"
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}

fn main() {
    write(1, __str_ptr("hi"), __str_len("hi"));
}
"#,
        );
        assert!(!result.code.is_empty());
        assert!(result.function_symbols.iter().any(|s| s.is_entry));
    }
}
