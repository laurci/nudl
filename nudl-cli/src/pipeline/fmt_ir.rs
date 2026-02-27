use nudl_bc::ir::*;

pub(super) fn fmt_ir(program: &Program) -> String {
    let mut out = String::new();

    // String constants
    out.push_str("string_constants:\n");
    for (i, s) in program.string_constants.iter().enumerate() {
        out.push_str(&format!("  [{}] {:?}\n", i, s));
    }
    out.push('\n');

    // Functions
    for func in &program.functions {
        let func_name = program.interner.resolve(func.name);
        if func.is_extern {
            out.push_str(&format!("function {}(", func_name));
            for (i, (pname, _pty)) in func.params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("{}", program.interner.resolve(*pname)));
            }
            out.push(')');
            if let Some(ref ext_sym) = func.extern_symbol {
                out.push_str(&format!("  [extern: {}]", ext_sym));
            }
            out.push('\n');
        } else {
            out.push_str(&format!("function {}(", func_name));
            for (i, (_pname, _pty)) in func.params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("r{}", i));
            }
            out.push_str("):\n");

            for block in &func.blocks {
                out.push_str(&format!("  block b{}:\n", block.id.0));
                for inst in &block.instructions {
                    out.push_str("    ");
                    fmt_instruction(inst, &mut out, &program.interner);
                    out.push('\n');
                }
                out.push_str("    ");
                fmt_terminator(&block.terminator, &mut out);
                out.push('\n');
            }
        }
        out.push('\n');
    }

    out
}

fn fmt_instruction(
    inst: &Instruction,
    out: &mut String,
    interner: &nudl_core::intern::StringInterner,
) {
    match inst {
        Instruction::Const(reg, val) => {
            out.push_str(&format!("r{} = Const(", reg.0));
            match val {
                ConstValue::Unit => out.push_str("Unit"),
                ConstValue::I32(v) => out.push_str(&format!("I32({})", v)),
                ConstValue::I64(v) => out.push_str(&format!("I64({})", v)),
                ConstValue::U64(v) => out.push_str(&format!("U64({})", v)),
                ConstValue::Bool(v) => out.push_str(&format!("Bool({})", v)),
                ConstValue::F32(v) => out.push_str(&format!("F32({})", v)),
                ConstValue::F64(v) => out.push_str(&format!("F64({})", v)),
                ConstValue::Char(v) => out.push_str(&format!("Char({:?})", v)),
                ConstValue::StringLiteral(idx) => out.push_str(&format!("StringLiteral({})", idx)),
            }
            out.push(')');
        }
        Instruction::ConstUnit(reg) => {
            out.push_str(&format!("r{} = ConstUnit", reg.0));
        }
        Instruction::StringPtr(dst, src) => {
            out.push_str(&format!("r{} = StringPtr(r{})", dst.0, src.0));
        }
        Instruction::StringLen(dst, src) => {
            out.push_str(&format!("r{} = StringLen(r{})", dst.0, src.0));
        }
        Instruction::StringConstPtr(reg, idx) => {
            out.push_str(&format!("r{} = StringConstPtr({})", reg.0, idx));
        }
        Instruction::StringConstLen(reg, idx) => {
            out.push_str(&format!("r{} = StringConstLen({})", reg.0, idx));
        }
        Instruction::Call(dst, func_ref, args) => {
            out.push_str(&format!("r{} = Call(", dst.0));
            match func_ref {
                FunctionRef::Named(sym) => {
                    out.push_str(&format!("Named(\"{}\")", interner.resolve(*sym)))
                }
                FunctionRef::Extern(sym) => {
                    out.push_str(&format!("Extern(\"{}\")", interner.resolve(*sym)))
                }
                FunctionRef::Builtin(sym) => {
                    out.push_str(&format!("Builtin(\"{}\")", interner.resolve(*sym)))
                }
            }
            out.push_str(", [");
            for (i, r) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("r{}", r.0));
            }
            out.push_str("])");
        }
        Instruction::Copy(dst, src) => {
            out.push_str(&format!("r{} = Copy(r{})", dst.0, src.0));
        }
        Instruction::Nop => {
            out.push_str("Nop");
        }
        // Arithmetic
        Instruction::Add(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Add(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Sub(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Sub(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Mul(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Mul(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Div(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Div(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Mod(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Mod(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Shl(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Shl(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Shr(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Shr(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Neg(dst, src) => {
            out.push_str(&format!("r{} = Neg(r{})", dst.0, src.0));
        }
        // Bitwise
        Instruction::BitAnd(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = BitAnd(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::BitOr(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = BitOr(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::BitXor(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = BitXor(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::BitNot(dst, src) => {
            out.push_str(&format!("r{} = BitNot(r{})", dst.0, src.0));
        }
        Instruction::Cast(dst, src, type_id) => {
            out.push_str(&format!(
                "r{} = Cast(r{}, type_id={})",
                dst.0, src.0, type_id.0
            ));
        }
        // Comparison
        Instruction::Eq(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Eq(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Ne(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Ne(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Lt(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Lt(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Le(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Le(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Gt(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Gt(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        Instruction::Ge(dst, lhs, rhs) => {
            out.push_str(&format!("r{} = Ge(r{}, r{})", dst.0, lhs.0, rhs.0));
        }
        // Logical
        Instruction::Not(dst, src) => {
            out.push_str(&format!("r{} = Not(r{})", dst.0, src.0));
        }
        // ARC / heap operations
        Instruction::Alloc(dst, type_id) => {
            out.push_str(&format!("r{} = Alloc(type_id={})", dst.0, type_id.0));
        }
        Instruction::Load(dst, ptr, offset) => {
            out.push_str(&format!("r{} = Load(r{}, offset={})", dst.0, ptr.0, offset));
        }
        Instruction::Store(ptr, offset, src) => {
            out.push_str(&format!("Store(r{}, offset={}, r{})", ptr.0, offset, src.0));
        }
        Instruction::Retain(reg) => {
            out.push_str(&format!("Retain(r{})", reg.0));
        }
        Instruction::Release(reg, type_id) => {
            if let Some(tid) = type_id {
                out.push_str(&format!("Release(r{}, type_id={})", reg.0, tid.0));
            } else {
                out.push_str(&format!("Release(r{})", reg.0));
            }
        }
        // Tuple/Array operations
        Instruction::TupleAlloc(dst, type_id, elems) => {
            let regs: Vec<String> = elems.iter().map(|r| format!("r{}", r.0)).collect();
            out.push_str(&format!(
                "r{} = TupleAlloc(type_id={}, [{}])",
                dst.0,
                type_id.0,
                regs.join(", ")
            ));
        }
        Instruction::FixedArrayAlloc(dst, type_id, elems) => {
            let regs: Vec<String> = elems.iter().map(|r| format!("r{}", r.0)).collect();
            out.push_str(&format!(
                "r{} = FixedArrayAlloc(type_id={}, [{}])",
                dst.0,
                type_id.0,
                regs.join(", ")
            ));
        }
        Instruction::TupleLoad(dst, ptr, offset) => {
            out.push_str(&format!(
                "r{} = TupleLoad(r{}, offset={})",
                dst.0, ptr.0, offset
            ));
        }
        Instruction::TupleStore(ptr, offset, src) => {
            out.push_str(&format!(
                "TupleStore(r{}, offset={}, r{})",
                ptr.0, offset, src.0
            ));
        }
        Instruction::IndexLoad(dst, ptr, idx, _elem_type) => {
            out.push_str(&format!("r{} = IndexLoad(r{}, r{})", dst.0, ptr.0, idx.0));
        }
        Instruction::IndexStore(ptr, idx, src) => {
            out.push_str(&format!("IndexStore(r{}, r{}, r{})", ptr.0, idx.0, src.0));
        }
        // Closure operations
        Instruction::ClosureCreate(dst, func_id, captures) => {
            out.push_str(&format!("r{} = ClosureCreate(fn#{}, [", dst.0, func_id.0));
            for (i, r) in captures.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("r{}", r.0));
            }
            out.push_str("])");
        }
        Instruction::ClosureCall(dst, closure, args) => {
            out.push_str(&format!("r{} = ClosureCall(r{}, [", dst.0, closure.0));
            for (i, r) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("r{}", r.0));
            }
            out.push_str("])");
        }
        // Dynamic array operations
        Instruction::DynArrayAlloc(dst, _ty) => {
            out.push_str(&format!("r{} = DynArrayAlloc", dst.0));
        }
        Instruction::DynArrayPush(arr, val) => {
            out.push_str(&format!("DynArrayPush(r{}, r{})", arr.0, val.0));
        }
        Instruction::DynArrayPop(dst, arr) => {
            out.push_str(&format!("r{} = DynArrayPop(r{})", dst.0, arr.0));
        }
        Instruction::DynArrayLen(dst, arr) => {
            out.push_str(&format!("r{} = DynArrayLen(r{})", dst.0, arr.0));
        }
        Instruction::DynArrayGet(dst, arr, idx) => {
            out.push_str(&format!("r{} = DynArrayGet(r{}, r{})", dst.0, arr.0, idx.0));
        }
        Instruction::DynArraySet(arr, idx, val) => {
            out.push_str(&format!("DynArraySet(r{}, r{}, r{})", arr.0, idx.0, val.0));
        }
        // Map operations
        Instruction::MapAlloc(dst, _ty) => {
            out.push_str(&format!("r{} = MapAlloc", dst.0));
        }
        Instruction::MapInsert(map, key, val) => {
            out.push_str(&format!("MapInsert(r{}, r{}, r{})", map.0, key.0, val.0));
        }
        Instruction::MapGet(dst, map, key) => {
            out.push_str(&format!("r{} = MapGet(r{}, r{})", dst.0, map.0, key.0));
        }
        Instruction::MapLen(dst, map) => {
            out.push_str(&format!("r{} = MapLen(r{})", dst.0, map.0));
        }
        Instruction::MapContains(dst, map, key) => {
            out.push_str(&format!("r{} = MapContains(r{}, r{})", dst.0, map.0, key.0));
        }
    }
}

fn fmt_terminator(term: &Terminator, out: &mut String) {
    match term {
        Terminator::Return(reg) => out.push_str(&format!("Return(r{})", reg.0)),
        Terminator::Jump(block) => out.push_str(&format!("Jump(b{})", block.0)),
        Terminator::Branch(cond, t, f) => {
            out.push_str(&format!("Branch(r{}, b{}, b{})", cond.0, t.0, f.0))
        }
        Terminator::Unreachable => out.push_str("Unreachable"),
    }
}
