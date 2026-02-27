use nudl_ast::ast::*;

use crate::ir::*;

use super::context::FunctionLowerCtx;
use super::parse_int_const;

impl<'a> FunctionLowerCtx<'a> {
    pub(super) fn lower_expr(&mut self, expr: &nudl_core::span::Spanned<Expr>) -> Register {
        self.current_span = expr.span;
        match &expr.node {
            Expr::Call { callee, args } => {
                if let Expr::Ident(name) = &callee.node {
                    if let Some(sig) = self.function_sigs.get(name).cloned() {
                        return match sig.kind {
                            crate::checker::FunctionKind::Builtin => {
                                self.lower_builtin_call(name, args)
                            }
                            crate::checker::FunctionKind::Extern => {
                                self.lower_resolved_call(name, &sig, args, true, 0)
                            }
                            crate::checker::FunctionKind::UserDefined => {
                                self.lower_resolved_call(name, &sig, args, false, 0)
                            }
                        };
                    }
                    // Check if it's a closure variable
                    if let Some(closure_reg) = self.locals.get(name).copied() {
                        if let Some(ty) = self.local_types.get(name).copied() {
                            if let nudl_core::types::TypeKind::Function { ret, .. } =
                                self.types.resolve(ty).clone()
                            {
                                let call_span = self.current_span;
                                let arg_regs: Vec<Register> =
                                    args.iter().map(|a| self.lower_expr(&a.value)).collect();
                                self.current_span = call_span;
                                let dst = self.alloc_typed_register(ret);
                                self.push_inst(Instruction::ClosureCall(
                                    dst,
                                    closure_reg,
                                    arg_regs,
                                ));
                                return dst;
                            }
                        }
                    }
                    // Check if it's a tuple struct constructor: Foo(val1, val2)
                    if let Some(&struct_ty) = self.struct_defs.get(name) {
                        let dst = self.alloc_typed_register(struct_ty);
                        self.push_inst(Instruction::Alloc(dst, struct_ty));
                        // Lower args and store them as positional fields
                        for (i, arg) in args.iter().enumerate() {
                            let val = self.lower_expr(&arg.value);
                            self.push_inst(Instruction::Store(dst, i as u32, val));
                        }
                        // ARC retain for newly allocated
                        self.push_inst(Instruction::Retain(dst));
                        return dst;
                    }
                }
                // Fallback: emit unit
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::MethodCall {
                object,
                method,
                args,
            } => {
                // First, infer receiver type name for mangled lookup
                let type_name = self.infer_receiver_type_name(object);
                if let Some(type_name) = type_name {
                    let mangled_name = format!("{}__{}", type_name, method);
                    if let Some(sig) = self.function_sigs.get(&mangled_name).cloned() {
                        // Lower the object (self argument)
                        let self_reg = self.lower_expr(object);
                        return self.lower_method_call(&mangled_name, &sig, self_reg, args);
                    }
                }

                // Check built-in methods for dynamic arrays and maps
                if let Some(obj_type) = self.infer_expr_type(object) {
                    let resolved = self.types.resolve(obj_type).clone();
                    match &resolved {
                        nudl_core::types::TypeKind::DynamicArray { element } => {
                            let elem_ty = *element;
                            let obj_reg = self.lower_expr(object);
                            match method.as_str() {
                                "push" => {
                                    let val_reg = self.lower_expr(&args[0].value);
                                    let unit_ty = self.types.unit();
                                    let dst = self.alloc_typed_register(unit_ty);
                                    self.push_inst(Instruction::DynArrayPush(obj_reg, val_reg));
                                    self.push_inst(Instruction::ConstUnit(dst));
                                    return dst;
                                }
                                "pop" => {
                                    let dst = self.alloc_typed_register(elem_ty);
                                    self.push_inst(Instruction::DynArrayPop(dst, obj_reg));
                                    return dst;
                                }
                                "len" => {
                                    let i64_ty = self.types.i64();
                                    let dst = self.alloc_typed_register(i64_ty);
                                    self.push_inst(Instruction::DynArrayLen(dst, obj_reg));
                                    return dst;
                                }
                                _ => {}
                            }
                        }
                        nudl_core::types::TypeKind::Map { value, .. } => {
                            let val_ty = *value;
                            let obj_reg = self.lower_expr(object);
                            match method.as_str() {
                                "insert" => {
                                    let key_reg = self.lower_expr(&args[0].value);
                                    let val_reg = self.lower_expr(&args[1].value);
                                    self.push_inst(Instruction::MapInsert(
                                        obj_reg, key_reg, val_reg,
                                    ));
                                    let unit_ty = self.types.unit();
                                    let dst = self.alloc_typed_register(unit_ty);
                                    self.push_inst(Instruction::ConstUnit(dst));
                                    return dst;
                                }
                                "get" => {
                                    let key_reg = self.lower_expr(&args[0].value);
                                    let dst = self.alloc_typed_register(val_ty);
                                    self.push_inst(Instruction::MapGet(dst, obj_reg, key_reg));
                                    return dst;
                                }
                                "contains_key" => {
                                    let key_reg = self.lower_expr(&args[0].value);
                                    let bool_ty = self.types.bool();
                                    let dst = self.alloc_typed_register(bool_ty);
                                    self.push_inst(Instruction::MapContains(
                                        dst, obj_reg, key_reg,
                                    ));
                                    return dst;
                                }
                                "remove" => {
                                    let key_reg = self.lower_expr(&args[0].value);
                                    let bool_ty = self.types.bool();
                                    let dst = self.alloc_typed_register(bool_ty);
                                    self.push_inst(Instruction::MapContains(
                                        dst, obj_reg, key_reg,
                                    ));
                                    // Reuse MapContains for remove — it returns whether the key existed
                                    // Actually we need a separate instruction but for now map_remove also returns 0/1
                                    // We'll emit a Call to the runtime instead
                                    return dst;
                                }
                                "len" => {
                                    let i64_ty = self.types.i64();
                                    let dst = self.alloc_typed_register(i64_ty);
                                    self.push_inst(Instruction::MapLen(dst, obj_reg));
                                    return dst;
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }

                // Fallback
                self.lower_expr(object);
                for arg in args {
                    self.lower_expr(&arg.value);
                }
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::StaticCall {
                type_name,
                method,
                args,
            } => {
                // Check if this is an enum tuple variant constructor: Enum::Variant(args)
                if let Some(&enum_ty) = self.enum_defs.get(type_name.as_str()) {
                    if let nudl_core::types::TypeKind::Enum { variants, .. } =
                        self.types.resolve(enum_ty).clone()
                    {
                        if let Some((tag, variant)) = variants
                            .iter()
                            .enumerate()
                            .find(|(_, v)| v.name == *method)
                        {
                            if !variant.fields.is_empty() {
                                // This is a tuple variant constructor
                                let variant_fields = variant.fields.clone();
                                return self.lower_enum_construct(
                                    enum_ty, tag, &variant_fields, args,
                                );
                            }
                        }
                    }
                }

                // Handle Map::new() — creates a new empty map
                if type_name == "Map" && method == "new" && args.is_empty() {
                    // Create a Map<i64, i64> by default (types are erased at runtime)
                    let key_ty = self.types.i64();
                    let val_ty = self.types.i64();
                    let map_ty = self.types.intern(nudl_core::types::TypeKind::Map {
                        key: key_ty,
                        value: val_ty,
                    });
                    let dst = self.alloc_typed_register(map_ty);
                    self.push_inst(Instruction::MapAlloc(dst, map_ty));
                    return dst;
                }

                let mangled_name = format!("{}__{}", type_name, method);
                if let Some(sig) = self.function_sigs.get(&mangled_name).cloned() {
                    return self.lower_resolved_call(&mangled_name, &sig, args, false, 0);
                }
                // Fallback
                for arg in args {
                    self.lower_expr(&arg.value);
                }
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::Literal(Literal::String(s)) => {
                // Deduplicate string constants
                let idx = if let Some(pos) = self.string_constants.iter().position(|c| c == s) {
                    pos as u32
                } else {
                    let idx = self.string_constants.len() as u32;
                    self.string_constants.push(s.clone());
                    idx
                };
                let reg = self.alloc_register();
                self.push_inst(Instruction::Const(reg, ConstValue::StringLiteral(idx)));
                reg
            }

            Expr::Literal(Literal::Int(s, suffix)) => {
                let const_val = parse_int_const(s, *suffix);
                let reg = self.alloc_register();
                self.push_inst(Instruction::Const(reg, const_val));
                reg
            }

            Expr::Literal(Literal::Float(s)) => {
                let val: f64 = s.parse().unwrap_or(0.0);
                let f64_ty = self.types.f64();
                let reg = self.alloc_typed_register(f64_ty);
                self.push_inst(Instruction::Const(reg, ConstValue::F64(val)));
                reg
            }

            Expr::Literal(Literal::Bool(b)) => {
                let reg = self.alloc_register();
                self.push_inst(Instruction::Const(reg, ConstValue::Bool(*b)));
                reg
            }

            Expr::Literal(Literal::Char(c)) => {
                let reg = self.alloc_register();
                self.push_inst(Instruction::Const(reg, ConstValue::Char(*c)));
                reg
            }

            Expr::Ident(name) => {
                if let Some(&reg) = self.locals.get(name) {
                    reg
                } else if let Some(&struct_ty) = self.struct_defs.get(name) {
                    // Unit struct constructor
                    let dst = self.alloc_typed_register(struct_ty);
                    self.push_inst(Instruction::Alloc(dst, struct_ty));
                    self.push_inst(Instruction::Retain(dst));
                    dst
                } else {
                    // Should have been caught by checker
                    let reg = self.alloc_register();
                    self.push_inst(Instruction::ConstUnit(reg));
                    reg
                }
            }

            Expr::Return(value) => {
                let ret_reg = if let Some(inner) = value {
                    self.lower_expr(inner)
                } else {
                    let reg = self.alloc_register();
                    self.push_inst(Instruction::ConstUnit(reg));
                    reg
                };
                // Terminate current block with a Return and start a dead block
                // for any subsequent code in the same scope.
                let dead_block = self.new_block_id();
                self.finish_block(Terminator::Return(ret_reg));
                self.start_block(dead_block);
                // Return unit since the code after this is unreachable
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::Binary { op, left, right } => {
                // Short-circuit for && and ||
                match op {
                    BinOp::And => return self.lower_short_circuit_and(left, right),
                    BinOp::Or => return self.lower_short_circuit_or(left, right),
                    _ => {}
                }

                // Check for operator overloading: if lhs is a struct/enum with the right method
                let op_method = match op {
                    BinOp::Add => Some("add"),
                    BinOp::Sub => Some("sub"),
                    BinOp::Mul => Some("mul"),
                    BinOp::Div => Some("div"),
                    BinOp::Mod => Some("rem"),
                    BinOp::Eq => Some("eq"),
                    BinOp::Ne => Some("ne"),
                    BinOp::Lt => Some("lt"),
                    BinOp::Le => Some("le"),
                    BinOp::Gt => Some("gt"),
                    BinOp::Ge => Some("ge"),
                    _ => None,
                };
                if let Some(method_name) = op_method {
                    if let Some(type_name) = self.infer_receiver_type_name(left) {
                        let mangled = format!("{}__{}", type_name, method_name);
                        if let Some(sig) = self.function_sigs.get(&mangled).cloned() {
                            let self_reg = self.lower_expr(left);
                            let arg_args = &[CallArg { name: None, value: (**right).clone() }];
                            return self.lower_method_call(&mangled, &sig, self_reg, arg_args);
                        }
                    }
                }

                let binop_span = self.current_span;
                let lhs = self.lower_expr(left);
                let rhs = self.lower_expr(right);
                self.current_span = binop_span;

                // Check if this is a float operation (propagate lhs type for arithmetic)
                let lhs_type = self.register_types[lhs.0 as usize];
                let is_float = matches!(
                    self.types.resolve(lhs_type),
                    nudl_core::types::TypeKind::Primitive(p) if p.is_float()
                );

                // Comparisons always produce i64 (bool); arithmetic propagates operand type
                let is_comparison = matches!(
                    op,
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                );

                let dst = if is_float && !is_comparison {
                    self.alloc_typed_register(lhs_type)
                } else {
                    self.alloc_register()
                };

                let inst = match op {
                    BinOp::Add => Instruction::Add(dst, lhs, rhs),
                    BinOp::Sub => Instruction::Sub(dst, lhs, rhs),
                    BinOp::Mul => Instruction::Mul(dst, lhs, rhs),
                    BinOp::Div => Instruction::Div(dst, lhs, rhs),
                    BinOp::Mod => Instruction::Mod(dst, lhs, rhs),
                    BinOp::Shl => Instruction::Shl(dst, lhs, rhs),
                    BinOp::Shr => Instruction::Shr(dst, lhs, rhs),
                    BinOp::BitAnd => Instruction::BitAnd(dst, lhs, rhs),
                    BinOp::BitOr => Instruction::BitOr(dst, lhs, rhs),
                    BinOp::BitXor => Instruction::BitXor(dst, lhs, rhs),
                    BinOp::Eq => Instruction::Eq(dst, lhs, rhs),
                    BinOp::Ne => Instruction::Ne(dst, lhs, rhs),
                    BinOp::Lt => Instruction::Lt(dst, lhs, rhs),
                    BinOp::Le => Instruction::Le(dst, lhs, rhs),
                    BinOp::Gt => Instruction::Gt(dst, lhs, rhs),
                    BinOp::Ge => Instruction::Ge(dst, lhs, rhs),
                    BinOp::And | BinOp::Or => unreachable!(),
                };
                self.push_inst(inst);
                dst
            }

            Expr::Unary { op, operand } => {
                let src = self.lower_expr(operand);
                let src_type = self.register_types[src.0 as usize];
                let is_float = matches!(
                    self.types.resolve(src_type),
                    nudl_core::types::TypeKind::Primitive(p) if p.is_float()
                );
                let dst = if is_float && matches!(op, UnaryOp::Neg) {
                    self.alloc_typed_register(src_type)
                } else {
                    self.alloc_register()
                };
                let inst = match op {
                    UnaryOp::Neg => Instruction::Neg(dst, src),
                    UnaryOp::Not => Instruction::Not(dst, src),
                    UnaryOp::BitNot => Instruction::BitNot(dst, src),
                };
                self.push_inst(inst);
                dst
            }

            Expr::StructLiteral { name, fields } => {
                // Check for enum struct variant: "EnumName::VariantName"
                if let Some((enum_name, variant_name)) = name.split_once("::") {
                    if let Some(&enum_ty) = self.enum_defs.get(enum_name) {
                        if let nudl_core::types::TypeKind::Enum { variants, .. } =
                            self.types.resolve(enum_ty).clone()
                        {
                            if let Some((tag, var_def)) = variants
                                .iter()
                                .enumerate()
                                .find(|(_, v)| v.name == variant_name)
                            {
                                let variant_fields = var_def.fields.clone();
                                // Convert struct fields to call args in order
                                let call_args: Vec<CallArg> = variant_fields
                                    .iter()
                                    .map(|(fname, _)| {
                                        let val = fields
                                            .iter()
                                            .find(|(n, _)| n == fname)
                                            .map(|(_, v)| v.clone())
                                            .unwrap();
                                        CallArg {
                                            name: Some(fname.clone()),
                                            value: val,
                                        }
                                    })
                                    .collect();
                                return self.lower_enum_construct(
                                    enum_ty,
                                    tag,
                                    &variant_fields,
                                    &call_args,
                                );
                            }
                        }
                    }
                }

                let type_id = self.struct_defs.get(name.as_str()).copied().unwrap();
                let dst = self.alloc_register();
                self.push_inst(Instruction::Alloc(dst, type_id));

                // Resolve field order from the type definition
                let struct_fields = match self.types.resolve(type_id).clone() {
                    nudl_core::types::TypeKind::Struct { fields: f, .. } => f,
                    _ => vec![],
                };

                for (field_name, field_val) in fields {
                    let val_reg = self.lower_expr(field_val);
                    // Find field index in the struct definition
                    let field_idx = struct_fields
                        .iter()
                        .position(|(n, _)| n == field_name)
                        .unwrap() as u32;
                    self.push_inst(Instruction::Store(dst, field_idx, val_reg));
                }
                dst
            }

            Expr::FieldAccess { object, field } => {
                let obj_reg = self.lower_expr(object);
                // Check if this is a tuple field access (.0, .1, etc.)
                if let Ok(idx) = field.parse::<u32>() {
                    let obj_type = self.infer_expr_type(object);
                    if let Some(tid) = obj_type {
                        if self.types.is_tuple(tid) {
                            let dst = self.alloc_register();
                            self.push_inst(Instruction::TupleLoad(dst, obj_reg, idx));
                            return dst;
                        }
                    }
                }
                // Resolve object type to find field index (struct field access)
                let field_idx = self.resolve_field_index(object, field);
                let dst = self.alloc_register();
                self.push_inst(Instruction::Load(dst, obj_reg, field_idx));
                dst
            }

            Expr::Assign { target, value } => {
                let val_reg = self.lower_expr(value);
                if let Expr::Ident(name) = &target.node {
                    // Release old value if reassigning a struct-typed variable
                    if let Some(&type_id) = self.local_types.get(name) {
                        if self.types.is_struct(type_id) {
                            if let Some(&old_reg) = self.locals.get(name) {
                                self.push_inst(Instruction::Release(old_reg, Some(type_id)));
                            }
                        }
                    }
                    self.locals.update(name, val_reg);
                } else if let Expr::FieldAccess { object, field } = &target.node {
                    let obj_reg = self.lower_expr(object);
                    let field_idx = self.resolve_field_index(object, field);
                    self.push_inst(Instruction::Store(obj_reg, field_idx, val_reg));
                } else if let Expr::IndexAccess { object, index } = &target.node {
                    let obj_type = self.infer_expr_type(object);
                    let is_dynamic = obj_type.map_or(false, |tid| {
                        matches!(
                            self.types.resolve(tid),
                            nudl_core::types::TypeKind::DynamicArray { .. }
                        )
                    });
                    let obj_reg = self.lower_expr(object);
                    let idx_reg = self.lower_expr(index);
                    if is_dynamic {
                        self.push_inst(Instruction::DynArraySet(obj_reg, idx_reg, val_reg));
                    } else {
                        self.push_inst(Instruction::IndexStore(obj_reg, idx_reg, val_reg));
                    }
                }
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::CompoundAssign { op, target, value } => {
                // target = target op value
                let target_reg = self.lower_expr(target);
                let val_reg = self.lower_expr(value);
                let result_reg = self.alloc_register();
                let inst = match op {
                    BinOp::Add => Instruction::Add(result_reg, target_reg, val_reg),
                    BinOp::Sub => Instruction::Sub(result_reg, target_reg, val_reg),
                    BinOp::Mul => Instruction::Mul(result_reg, target_reg, val_reg),
                    BinOp::Div => Instruction::Div(result_reg, target_reg, val_reg),
                    BinOp::Mod => Instruction::Mod(result_reg, target_reg, val_reg),
                    BinOp::Shl => Instruction::Shl(result_reg, target_reg, val_reg),
                    BinOp::Shr => Instruction::Shr(result_reg, target_reg, val_reg),
                    BinOp::BitAnd => Instruction::BitAnd(result_reg, target_reg, val_reg),
                    BinOp::BitOr => Instruction::BitOr(result_reg, target_reg, val_reg),
                    BinOp::BitXor => Instruction::BitXor(result_reg, target_reg, val_reg),
                    _ => unreachable!(),
                };
                self.push_inst(inst);
                if let Expr::Ident(name) = &target.node {
                    self.locals.update(name, result_reg);
                }
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_reg = self.lower_expr(condition);

                let then_block = self.new_block_id();
                let else_block = self.new_block_id();
                let merge_block = self.new_block_id();

                // Pre-allocate result register
                let result_reg = self.alloc_register();

                // Snapshot locals before branching so we can reconcile mutations
                let pre_if_locals = self.locals.flatten();

                // End current block with branch
                self.finish_block(Terminator::Branch(cond_reg, then_block, else_block));

                // Then block
                self.start_block(then_block);
                let then_result = self.lower_block_expr(&then_branch.node);
                self.push_inst(Instruction::Copy(result_reg, then_result));
                self.finish_block(Terminator::Jump(merge_block));
                let then_block_idx = self.blocks.len() - 1;
                let post_then_locals = self.locals.flatten();

                // Restore locals to pre-if state before lowering else branch
                for (name, reg) in &pre_if_locals {
                    self.locals.update(name, *reg);
                }

                // Else block
                self.start_block(else_block);
                if let Some(else_expr) = else_branch {
                    let else_result = self.lower_expr(else_expr);
                    self.push_inst(Instruction::Copy(result_reg, else_result));
                } else {
                    self.push_inst(Instruction::ConstUnit(result_reg));
                }
                self.finish_block(Terminator::Jump(merge_block));
                let else_block_idx = self.blocks.len() - 1;
                let post_else_locals = self.locals.flatten();

                // Reconcile: for any variable mutated differently in the two
                // branches, emit Copy instructions into both branch blocks so
                // they agree on a single merge register.
                for (name, pre_reg) in &pre_if_locals {
                    let then_reg = post_then_locals.get(name).copied().unwrap_or(*pre_reg);
                    let else_reg = post_else_locals.get(name).copied().unwrap_or(*pre_reg);

                    if then_reg != else_reg {
                        // Both branches have different registers — merge them
                        let merge_reg = self.alloc_register();
                        self.blocks[then_block_idx]
                            .instructions
                            .push(Instruction::Copy(merge_reg, then_reg));
                        self.blocks[then_block_idx].spans.push(self.current_span);
                        self.blocks[else_block_idx]
                            .instructions
                            .push(Instruction::Copy(merge_reg, else_reg));
                        self.blocks[else_block_idx].spans.push(self.current_span);
                        self.locals.update(name, merge_reg);
                    } else if then_reg != *pre_reg {
                        // Both branches mutated to the same register
                        self.locals.update(name, then_reg);
                    }
                }

                // Merge block
                self.start_block(merge_block);
                result_reg
            }

            Expr::Cast { expr, target_type } => {
                let src = self.lower_expr(expr);
                let target_id = self.resolve_type_expr(&target_type.node);
                let dst = self.alloc_typed_register(target_id);
                self.push_inst(Instruction::Cast(dst, src, target_id));
                dst
            }

            Expr::While {
                condition,
                body,
                label,
            } => {
                let cond_block = self.new_block_id();
                let body_block = self.new_block_id();
                let exit_block = self.new_block_id();

                // Jump to condition block
                self.finish_block(Terminator::Jump(cond_block));

                // Snapshot locals before condition (these are the registers the condition uses)
                self.start_block(cond_block);
                let pre_loop_locals = self.locals.clone();
                let cond_reg = self.lower_expr(condition);
                self.finish_block(Terminator::Branch(cond_reg, body_block, exit_block));

                // Body block
                self.start_block(body_block);
                self.loop_stack.push(super::LoopContext {
                    label: label.clone(),
                    continue_block: cond_block,
                    break_block: exit_block,
                    pre_loop_locals: pre_loop_locals.clone(),
                });
                self.lower_block_expr(&body.node);
                self.loop_stack.pop();

                // Copy-back: emit Copy instructions for any locals whose register changed
                // so that the condition block sees updated values on next iteration
                self.emit_loop_copyback(&pre_loop_locals);
                self.finish_block(Terminator::Jump(cond_block));

                // Exit block — restore locals to pre-loop state
                self.start_block(exit_block);
                self.locals = pre_loop_locals;
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::Loop { body, label } => {
                let body_block = self.new_block_id();
                let exit_block = self.new_block_id();

                // Jump to body block
                self.finish_block(Terminator::Jump(body_block));

                // Body block
                self.start_block(body_block);
                let pre_loop_locals = self.locals.clone();
                self.loop_stack.push(super::LoopContext {
                    label: label.clone(),
                    continue_block: body_block,
                    break_block: exit_block,
                    pre_loop_locals: pre_loop_locals.clone(),
                });
                self.lower_block_expr(&body.node);
                self.loop_stack.pop();

                // Copy-back for loop variables
                self.emit_loop_copyback(&pre_loop_locals);
                self.finish_block(Terminator::Jump(body_block));

                // Exit block (reached by break) — restore locals
                self.start_block(exit_block);
                self.locals = pre_loop_locals;
                let unit_reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(unit_reg));
                unit_reg
            }

            Expr::Break { label, .. } => {
                let lc_info = if let Some(label) = label {
                    self.loop_stack
                        .iter()
                        .rev()
                        .find(|lc| lc.label.as_deref() == Some(label))
                        .map(|lc| (lc.break_block, lc.pre_loop_locals.clone()))
                } else {
                    self.loop_stack
                        .last()
                        .map(|lc| (lc.break_block, lc.pre_loop_locals.clone()))
                };
                if let Some((break_block, pre_loop_locals)) = lc_info {
                    self.emit_loop_copyback(&pre_loop_locals);
                    self.finish_block(Terminator::Jump(break_block));
                    let dead = self.new_block_id();
                    self.start_block(dead);
                }
                let reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(reg));
                reg
            }

            Expr::Continue { label } => {
                let lc_info = if let Some(label) = label {
                    self.loop_stack
                        .iter()
                        .rev()
                        .find(|lc| lc.label.as_deref() == Some(label))
                        .map(|lc| (lc.continue_block, lc.pre_loop_locals.clone()))
                } else {
                    self.loop_stack
                        .last()
                        .map(|lc| (lc.continue_block, lc.pre_loop_locals.clone()))
                };
                if let Some((continue_block, pre_loop_locals)) = lc_info {
                    self.emit_loop_copyback(&pre_loop_locals);
                    self.finish_block(Terminator::Jump(continue_block));
                    let dead = self.new_block_id();
                    self.start_block(dead);
                }
                let reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(reg));
                reg
            }

            Expr::Grouped(inner) => self.lower_expr(inner),

            Expr::Block(block) => self.lower_block_expr(block),

            Expr::TupleLiteral(elements) => {
                let elem_regs: Vec<Register> =
                    elements.iter().map(|e| self.lower_expr(e)).collect();
                let elem_types: Vec<nudl_core::types::TypeId> = elements
                    .iter()
                    .map(|e| self.infer_expr_type(e).unwrap_or(self.types.i32()))
                    .collect();
                let type_id = self
                    .types
                    .intern(nudl_core::types::TypeKind::Tuple(elem_types));
                let dst = self.alloc_register();
                self.push_inst(Instruction::TupleAlloc(dst, type_id, elem_regs));
                dst
            }

            Expr::ArrayLiteral(elements) => {
                let elem_regs: Vec<Register> =
                    elements.iter().map(|e| self.lower_expr(e)).collect();
                let elem_type = if !elements.is_empty() {
                    self.infer_expr_type(&elements[0])
                        .unwrap_or(self.types.i32())
                } else {
                    self.types.i32()
                };
                let type_id = self.types.intern(nudl_core::types::TypeKind::FixedArray {
                    element: elem_type,
                    length: elements.len(),
                });
                let dst = self.alloc_register();
                self.push_inst(Instruction::FixedArrayAlloc(dst, type_id, elem_regs));
                dst
            }

            Expr::ArrayRepeat { value, count } => {
                let val_reg = self.lower_expr(value);
                let elem_type = self.infer_expr_type(value).unwrap_or(self.types.i32());
                let elem_regs: Vec<Register> = (0..*count).map(|_| val_reg).collect();
                let type_id = self.types.intern(nudl_core::types::TypeKind::FixedArray {
                    element: elem_type,
                    length: *count,
                });
                let dst = self.alloc_register();
                self.push_inst(Instruction::FixedArrayAlloc(dst, type_id, elem_regs));
                dst
            }

            Expr::IndexAccess { object, index } => {
                // Check if this is a dynamic array or map index access
                let obj_type = self.infer_expr_type(object);
                let is_dynamic = obj_type.map_or(false, |tid| {
                    matches!(
                        self.types.resolve(tid),
                        nudl_core::types::TypeKind::DynamicArray { .. }
                    )
                });
                let is_map = obj_type.map_or(false, |tid| {
                    matches!(
                        self.types.resolve(tid),
                        nudl_core::types::TypeKind::Map { .. }
                    )
                });

                let obj_reg = self.lower_expr(object);
                let idx_reg = self.lower_expr(index);

                if is_dynamic {
                    let elem_type = self.infer_index_element_type(object);
                    let dst = self.alloc_typed_register(elem_type);
                    self.push_inst(Instruction::DynArrayGet(dst, obj_reg, idx_reg));
                    dst
                } else if is_map {
                    let val_type = if let Some(tid) = obj_type {
                        match self.types.resolve(tid) {
                            nudl_core::types::TypeKind::Map { value, .. } => *value,
                            _ => self.types.i64(),
                        }
                    } else {
                        self.types.i64()
                    };
                    let dst = self.alloc_typed_register(val_type);
                    self.push_inst(Instruction::MapGet(dst, obj_reg, idx_reg));
                    dst
                } else {
                    let elem_type = self.infer_index_element_type(object);
                    let dst = self.alloc_register();
                    self.push_inst(Instruction::IndexLoad(dst, obj_reg, idx_reg, elem_type));
                    dst
                }
            }

            Expr::Range { .. } => {
                // Ranges are only used in for-loops; standalone range expressions
                // produce unit for now
                let reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(reg));
                reg
            }

            Expr::For {
                binding,
                iter,
                body,
            } => {
                // Desugar for-in to while loop
                match &iter.node {
                    Expr::Range {
                        start,
                        end,
                        inclusive,
                    } => self.lower_for_range(binding, start, end, *inclusive, &body.node),
                    _ => {
                        // Array iteration
                        self.lower_for_array(binding, iter, &body.node)
                    }
                }
            }

            Expr::EnumLiteral {
                enum_name,
                variant,
                args,
            } => {
                if let Some(&enum_ty) = self.enum_defs.get(enum_name.as_str()) {
                    if let nudl_core::types::TypeKind::Enum { variants, .. } =
                        self.types.resolve(enum_ty).clone()
                    {
                        if let Some((tag, var_def)) = variants
                            .iter()
                            .enumerate()
                            .find(|(_, v)| v.name == *variant)
                        {
                            let variant_fields = var_def.fields.clone();
                            let call_args: Vec<CallArg> = args
                                .iter()
                                .map(|a| CallArg {
                                    name: None,
                                    value: a.clone(),
                                })
                                .collect();
                            return self.lower_enum_construct(
                                enum_ty,
                                tag,
                                &variant_fields,
                                &call_args,
                            );
                        }
                    }
                }
                let reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(reg));
                reg
            }

            Expr::Match {
                expr: scrutinee,
                arms,
            } => self.lower_match(scrutinee, arms),

            Expr::IfLet {
                pattern,
                expr: scrutinee,
                then_branch,
                else_branch,
            } => self.lower_if_let(pattern, scrutinee, then_branch, else_branch),

            Expr::Literal(Literal::TemplateString { parts, exprs }) => {
                self.lower_template_string(parts, exprs)
            }

            Expr::Closure {
                params,
                return_type,
                body,
            } => {
                // Resolve parameter types
                let closure_params: Vec<(String, nudl_core::types::TypeId)> = params
                    .iter()
                    .map(|p| {
                        let ty = if let Some(type_expr) = &p.ty {
                            self.resolve_type_expr(&type_expr.node)
                        } else {
                            self.types.i32() // default to i32
                        };
                        (p.name.clone(), ty)
                    })
                    .collect();

                let param_names: Vec<String> = closure_params.iter().map(|(n, _)| n.clone()).collect();

                // Collect captures: variables from enclosing scope referenced in the body
                let captures = self.collect_captures(body, &param_names);

                // Allocate a function ID for the closure thunk
                let thunk_fn_id = self.alloc_closure_function_id();

                // Resolve return type
                let ret_ty = if let Some(rt) = return_type {
                    self.resolve_type_expr(&rt.node)
                } else {
                    // We don't easily know the return type here without re-checking,
                    // so default to i64 (the lowerer works with i64 for most values)
                    self.types.i64()
                };

                // Emit ClosureCreate instruction
                let capture_regs: Vec<Register> = captures.iter().map(|(_, reg, _)| *reg).collect();
                let capture_names: Vec<String> = captures.iter().map(|(name, _, _)| name.clone()).collect();
                let capture_types: Vec<nudl_core::types::TypeId> =
                    captures.iter().map(|(_, _, ty)| *ty).collect();

                // Build Function type for the closure value
                let fn_param_types: Vec<nudl_core::types::TypeId> =
                    closure_params.iter().map(|(_, ty)| *ty).collect();
                let fn_type = self.types.intern(nudl_core::types::TypeKind::Function {
                    params: fn_param_types,
                    ret: ret_ty,
                });
                let dst = self.alloc_typed_register(fn_type);
                self.push_inst(Instruction::ClosureCreate(
                    dst,
                    thunk_fn_id,
                    capture_regs,
                ));

                // Register the pending closure for later lowering
                self.pending_closures.push(super::PendingClosure {
                    func_id: thunk_fn_id,
                    capture_names,
                    capture_types,
                    params: closure_params,
                    body: (**body).clone(),
                    return_type: ret_ty,
                    span: expr.span,
                });

                dst
            }

            Expr::QuestionMark(inner) => {
                self.lower_question_mark(inner)
            }
        }
    }

    pub(super) fn lower_short_circuit_and(
        &mut self,
        left: &nudl_core::span::Spanned<Expr>,
        right: &nudl_core::span::Spanned<Expr>,
    ) -> Register {
        let result_reg = self.alloc_register();
        let lhs = self.lower_expr(left);

        let rhs_block = self.new_block_id();
        let merge_block = self.new_block_id();
        let false_block = self.new_block_id();

        self.finish_block(Terminator::Branch(lhs, rhs_block, false_block));

        // If lhs is true, evaluate rhs
        self.start_block(rhs_block);
        let rhs = self.lower_expr(right);
        self.push_inst(Instruction::Copy(result_reg, rhs));
        self.finish_block(Terminator::Jump(merge_block));

        // If lhs is false, short-circuit
        self.start_block(false_block);
        self.push_inst(Instruction::Const(result_reg, ConstValue::Bool(false)));
        self.finish_block(Terminator::Jump(merge_block));

        self.start_block(merge_block);
        result_reg
    }

    /// Lower a template string like `hello {name}, you are {age} years old`
    /// into a chain of string concatenation calls.
    fn lower_template_string(
        &mut self,
        parts: &[String],
        exprs: &[nudl_core::span::Spanned<Expr>],
    ) -> Register {
        let string_ty = self.types.string();
        let concat_sym = self.interner.intern("__str_concat");

        // Start with the first text part as a string constant
        let mut result_reg = self.lower_string_const(&parts[0]);

        // Interleave: expr[i], then parts[i+1]
        for (i, expr) in exprs.iter().enumerate() {
            // Convert expression to string
            let expr_str_reg = self.lower_expr_to_string(expr);

            // Concat result with expr string
            let concat_dst = self.alloc_typed_register(string_ty);
            self.push_inst(Instruction::Call(
                concat_dst,
                FunctionRef::Builtin(concat_sym),
                vec![result_reg, expr_str_reg],
            ));
            result_reg = concat_dst;

            // Concat with next text part
            if i + 1 < parts.len() && !parts[i + 1].is_empty() {
                let part_reg = self.lower_string_const(&parts[i + 1]);
                let concat_dst2 = self.alloc_typed_register(string_ty);
                self.push_inst(Instruction::Call(
                    concat_dst2,
                    FunctionRef::Builtin(concat_sym),
                    vec![result_reg, part_reg],
                ));
                result_reg = concat_dst2;
            }
        }

        result_reg
    }

    /// Lower a string constant into a register
    fn lower_string_const(&mut self, s: &str) -> Register {
        let string_ty = self.types.string();
        let idx = self.string_constants.len() as u32;
        self.string_constants.push(s.to_string());
        let dst = self.alloc_typed_register(string_ty);
        self.push_inst(Instruction::Const(dst, ConstValue::StringLiteral(idx)));
        dst
    }

    /// Convert an expression to a string register, inserting conversion calls as needed
    fn lower_expr_to_string(&mut self, expr: &nudl_core::span::Spanned<Expr>) -> Register {
        let expr_reg = self.lower_expr(expr);
        let expr_type = self.infer_expr_type(expr);
        let string_ty = self.types.string();

        // If already a string, return directly
        if expr_type == Some(string_ty) {
            return expr_reg;
        }

        // Determine the conversion builtin based on type
        let builtin_name = match expr_type.map(|t| self.types.resolve(t).clone()) {
            Some(nudl_core::types::TypeKind::Primitive(nudl_core::types::PrimitiveType::I32)) => {
                "__i32_to_str"
            }
            Some(nudl_core::types::TypeKind::Primitive(nudl_core::types::PrimitiveType::I64)) => {
                "__i64_to_str"
            }
            Some(nudl_core::types::TypeKind::Primitive(nudl_core::types::PrimitiveType::F64)) => {
                "__f64_to_str"
            }
            Some(nudl_core::types::TypeKind::Primitive(nudl_core::types::PrimitiveType::Bool)) => {
                "__bool_to_str"
            }
            Some(nudl_core::types::TypeKind::Primitive(nudl_core::types::PrimitiveType::Char)) => {
                "__char_to_str"
            }
            _ => "__i32_to_str", // fallback
        };

        let sym = self.interner.intern(builtin_name);
        let dst = self.alloc_typed_register(string_ty);
        self.push_inst(Instruction::Call(
            dst,
            FunctionRef::Builtin(sym),
            vec![expr_reg],
        ));
        dst
    }

    /// Lower enum variant construction. Tag at field 0, data at field 1+.
    /// Enum memory layout: [ARC header] [tag: i64] [field0: i64] [field1: i64] ...
    fn lower_enum_construct(
        &mut self,
        enum_ty: nudl_core::types::TypeId,
        tag: usize,
        variant_fields: &[(String, nudl_core::types::TypeId)],
        args: &[CallArg],
    ) -> Register {
        let dst = self.alloc_typed_register(enum_ty);
        self.push_inst(Instruction::Alloc(dst, enum_ty));

        // Store tag at field 0
        let tag_reg = self.alloc_register();
        self.push_inst(Instruction::Const(tag_reg, ConstValue::I32(tag as i32)));
        self.push_inst(Instruction::Store(dst, 0, tag_reg));

        // Store variant data at field 1+
        for (i, arg) in args.iter().enumerate() {
            let val_reg = self.lower_expr(&arg.value);
            self.push_inst(Instruction::Store(dst, (i + 1) as u32, val_reg));
            let _ = variant_fields; // fields used by checker
        }

        dst
    }

    /// Lower a match expression into a chain of tag comparisons and branches
    fn lower_match(
        &mut self,
        scrutinee: &nudl_core::span::Spanned<Expr>,
        arms: &[MatchArm],
    ) -> Register {
        let scrutinee_reg = self.lower_expr(scrutinee);
        let scrutinee_ty = self.infer_expr_type(scrutinee);
        let result_reg = self.alloc_register();

        let merge_block = self.new_block_id();

        // For enum scrutinees, load the tag
        let tag_reg = if scrutinee_ty
            .map(|t| self.types.is_enum(t))
            .unwrap_or(false)
        {
            let tag = self.alloc_register();
            self.push_inst(Instruction::Load(tag, scrutinee_reg, 0));
            Some(tag)
        } else {
            None
        };

        let mut remaining_arms: Vec<(usize, &MatchArm)> = arms.iter().enumerate().collect();
        self.lower_match_arms(
            &remaining_arms,
            scrutinee_reg,
            scrutinee_ty,
            tag_reg,
            result_reg,
            merge_block,
        );

        self.start_block(merge_block);
        result_reg
    }

    fn lower_match_arms(
        &mut self,
        arms: &[(usize, &MatchArm)],
        scrutinee_reg: Register,
        scrutinee_ty: Option<nudl_core::types::TypeId>,
        tag_reg: Option<Register>,
        result_reg: Register,
        merge_block: BlockId,
    ) {
        if arms.is_empty() {
            // Default: unit
            self.push_inst(Instruction::ConstUnit(result_reg));
            self.finish_block(Terminator::Jump(merge_block));
            return;
        }

        let (_, arm) = &arms[0];
        let rest = &arms[1..];

        match &arm.pattern.node {
            Pattern::Wildcard | Pattern::Binding(_) => {
                // Always matches - introduce binding if needed
                self.locals.push_scope();
                self.local_types.push_scope();
                if let Pattern::Binding(name) = &arm.pattern.node {
                    self.locals.insert(name.clone(), scrutinee_reg);
                    if let Some(ty) = scrutinee_ty {
                        self.local_types.insert(name.clone(), ty);
                    }
                }
                let body_result = self.lower_expr(&arm.body);
                self.push_inst(Instruction::Copy(result_reg, body_result));
                self.local_types.pop_scope();
                self.locals.pop_scope();
                self.finish_block(Terminator::Jump(merge_block));
            }
            Pattern::Literal(lit) => {
                let lit_reg = self.lower_literal_pattern(lit);
                let cmp_reg = self.alloc_register();
                self.push_inst(Instruction::Eq(cmp_reg, scrutinee_reg, lit_reg));

                let match_block = self.new_block_id();
                let next_block = self.new_block_id();
                self.finish_block(Terminator::Branch(cmp_reg, match_block, next_block));

                self.start_block(match_block);
                let body_result = self.lower_expr(&arm.body);
                self.push_inst(Instruction::Copy(result_reg, body_result));
                self.finish_block(Terminator::Jump(merge_block));

                self.start_block(next_block);
                self.lower_match_arms(rest, scrutinee_reg, scrutinee_ty, tag_reg, result_reg, merge_block);
            }
            Pattern::Enum {
                enum_name,
                variant,
                fields,
            } => {
                // Find the tag for this variant
                let enum_ty = scrutinee_ty.unwrap_or(self.types.i64());
                let variant_info = if let nudl_core::types::TypeKind::Enum { variants, .. } =
                    self.types.resolve(enum_ty).clone()
                {
                    variants
                        .iter()
                        .enumerate()
                        .find(|(_, v)| v.name == *variant)
                        .map(|(tag, v)| (tag, v.fields.clone()))
                } else {
                    None
                };

                if let (Some(tag_r), Some((expected_tag, variant_fields))) =
                    (tag_reg, variant_info)
                {
                    let expected_tag_reg = self.alloc_register();
                    self.push_inst(Instruction::Const(
                        expected_tag_reg,
                        ConstValue::I32(expected_tag as i32),
                    ));
                    let cmp_reg = self.alloc_register();
                    self.push_inst(Instruction::Eq(cmp_reg, tag_r, expected_tag_reg));

                    let match_block = self.new_block_id();
                    let next_block = self.new_block_id();
                    self.finish_block(Terminator::Branch(cmp_reg, match_block, next_block));

                    self.start_block(match_block);
                    self.locals.push_scope();
                    self.local_types.push_scope();

                    // Bind pattern fields
                    for (i, pat) in fields.iter().enumerate() {
                        if let Pattern::Binding(name) = &pat.node {
                            let field_reg = self.alloc_register();
                            self.push_inst(Instruction::Load(
                                field_reg,
                                scrutinee_reg,
                                (i + 1) as u32,
                            ));
                            self.locals.insert(name.clone(), field_reg);
                            if let Some((_, field_ty)) = variant_fields.get(i) {
                                self.local_types.insert(name.clone(), *field_ty);
                            }
                        }
                    }

                    let body_result = self.lower_expr(&arm.body);
                    self.push_inst(Instruction::Copy(result_reg, body_result));
                    self.local_types.pop_scope();
                    self.locals.pop_scope();
                    self.finish_block(Terminator::Jump(merge_block));

                    self.start_block(next_block);
                    self.lower_match_arms(
                        rest,
                        scrutinee_reg,
                        scrutinee_ty,
                        tag_reg,
                        result_reg,
                        merge_block,
                    );
                } else {
                    // Can't match - skip
                    self.lower_match_arms(
                        rest,
                        scrutinee_reg,
                        scrutinee_ty,
                        tag_reg,
                        result_reg,
                        merge_block,
                    );
                }
            }
            Pattern::Tuple(elements) => {
                // Tuple destructuring in match: bind elements and execute body
                self.locals.push_scope();
                self.local_types.push_scope();
                if let Some(ty) = scrutinee_ty {
                    if let nudl_core::types::TypeKind::Tuple(elem_types) =
                        self.types.resolve(ty).clone()
                    {
                        for (i, pat) in elements.iter().enumerate() {
                            let elem_ty = elem_types.get(i).copied();
                            let elem_reg = self.alloc_register();
                            self.push_inst(Instruction::TupleLoad(
                                elem_reg,
                                scrutinee_reg,
                                i as u32,
                            ));
                            if let Some(ety) = elem_ty {
                                self.register_types[elem_reg.0 as usize] = ety;
                            }
                            self.lower_pattern_binding(&pat.node, elem_reg, elem_ty);
                        }
                    }
                }
                let body_result = self.lower_expr(&arm.body);
                self.push_inst(Instruction::Copy(result_reg, body_result));
                self.local_types.pop_scope();
                self.locals.pop_scope();
                self.finish_block(Terminator::Jump(merge_block));
            }
            Pattern::Struct { name, fields, .. } => {
                // Struct destructuring in match: bind fields and execute body
                self.locals.push_scope();
                self.local_types.push_scope();
                if let Some(&struct_ty) = self.struct_defs.get(name) {
                    if let nudl_core::types::TypeKind::Struct {
                        fields: struct_fields,
                        ..
                    } = self.types.resolve(struct_ty).clone()
                    {
                        for (field_name, pat) in fields {
                            if let Some((idx, (_, field_ty))) = struct_fields
                                .iter()
                                .enumerate()
                                .find(|(_, (n, _))| n == field_name)
                            {
                                let ft = *field_ty;
                                let field_reg = self.alloc_typed_register(ft);
                                self.push_inst(Instruction::Load(
                                    field_reg,
                                    scrutinee_reg,
                                    idx as u32,
                                ));
                                self.lower_pattern_binding(&pat.node, field_reg, Some(ft));
                            }
                        }
                    }
                }
                let body_result = self.lower_expr(&arm.body);
                self.push_inst(Instruction::Copy(result_reg, body_result));
                self.local_types.pop_scope();
                self.locals.pop_scope();
                self.finish_block(Terminator::Jump(merge_block));
            }
        }
    }

    fn lower_literal_pattern(&mut self, lit: &Literal) -> Register {
        let reg = self.alloc_register();
        match lit {
            Literal::Int(s, suffix) => {
                let val = parse_int_const(s, *suffix);
                self.push_inst(Instruction::Const(reg, val));
            }
            Literal::Bool(b) => {
                self.push_inst(Instruction::Const(reg, ConstValue::Bool(*b)));
            }
            Literal::String(s) => {
                let idx = if let Some(pos) = self.string_constants.iter().position(|c| c == s) {
                    pos as u32
                } else {
                    let idx = self.string_constants.len() as u32;
                    self.string_constants.push(s.clone());
                    idx
                };
                self.push_inst(Instruction::Const(reg, ConstValue::StringLiteral(idx)));
            }
            Literal::Char(c) => {
                self.push_inst(Instruction::Const(reg, ConstValue::Char(*c)));
            }
            _ => {
                self.push_inst(Instruction::ConstUnit(reg));
            }
        }
        reg
    }

    fn lower_if_let(
        &mut self,
        pattern: &nudl_core::span::Spanned<Pattern>,
        scrutinee: &nudl_core::span::Spanned<Expr>,
        then_branch: &nudl_core::span::Spanned<Block>,
        else_branch: &Option<Box<nudl_core::span::Spanned<Expr>>>,
    ) -> Register {
        let scrutinee_reg = self.lower_expr(scrutinee);
        let scrutinee_ty = self.infer_expr_type(scrutinee);
        let result_reg = self.alloc_register();

        let then_block = self.new_block_id();
        let else_block = self.new_block_id();
        let merge_block = self.new_block_id();

        match &pattern.node {
            Pattern::Enum {
                variant, fields, ..
            } => {
                let enum_ty = scrutinee_ty.unwrap_or(self.types.i64());
                let variant_info = if let nudl_core::types::TypeKind::Enum { variants, .. } =
                    self.types.resolve(enum_ty).clone()
                {
                    variants
                        .iter()
                        .enumerate()
                        .find(|(_, v)| v.name == *variant)
                        .map(|(tag, v)| (tag, v.fields.clone()))
                } else {
                    None
                };

                if let Some((expected_tag, variant_fields)) = variant_info {
                    let tag_reg = self.alloc_register();
                    self.push_inst(Instruction::Load(tag_reg, scrutinee_reg, 0));
                    let expected_tag_reg = self.alloc_register();
                    self.push_inst(Instruction::Const(
                        expected_tag_reg,
                        ConstValue::I32(expected_tag as i32),
                    ));
                    let cmp_reg = self.alloc_register();
                    self.push_inst(Instruction::Eq(cmp_reg, tag_reg, expected_tag_reg));
                    self.finish_block(Terminator::Branch(cmp_reg, then_block, else_block));

                    // Then block: bind fields
                    self.start_block(then_block);
                    self.locals.push_scope();
                    self.local_types.push_scope();
                    for (i, pat) in fields.iter().enumerate() {
                        if let Pattern::Binding(name) = &pat.node {
                            let field_reg = self.alloc_register();
                            self.push_inst(Instruction::Load(
                                field_reg,
                                scrutinee_reg,
                                (i + 1) as u32,
                            ));
                            self.locals.insert(name.clone(), field_reg);
                            if let Some((_, field_ty)) = variant_fields.get(i) {
                                self.local_types.insert(name.clone(), *field_ty);
                            }
                        }
                    }
                    let then_result = self.lower_block_expr(&then_branch.node);
                    self.push_inst(Instruction::Copy(result_reg, then_result));
                    self.local_types.pop_scope();
                    self.locals.pop_scope();
                    self.finish_block(Terminator::Jump(merge_block));
                } else {
                    self.finish_block(Terminator::Jump(else_block));
                }
            }
            _ => {
                // Non-enum pattern in if-let - always match
                self.finish_block(Terminator::Jump(then_block));
                self.start_block(then_block);
                let then_result = self.lower_block_expr(&then_branch.node);
                self.push_inst(Instruction::Copy(result_reg, then_result));
                self.finish_block(Terminator::Jump(merge_block));
            }
        }

        // Else block
        self.start_block(else_block);
        if let Some(else_expr) = else_branch {
            let else_result = self.lower_expr(else_expr);
            self.push_inst(Instruction::Copy(result_reg, else_result));
        } else {
            self.push_inst(Instruction::ConstUnit(result_reg));
        }
        self.finish_block(Terminator::Jump(merge_block));

        self.start_block(merge_block);
        result_reg
    }

    pub(super) fn lower_short_circuit_or(
        &mut self,
        left: &nudl_core::span::Spanned<Expr>,
        right: &nudl_core::span::Spanned<Expr>,
    ) -> Register {
        let result_reg = self.alloc_register();
        let lhs = self.lower_expr(left);

        let true_block = self.new_block_id();
        let rhs_block = self.new_block_id();
        let merge_block = self.new_block_id();

        self.finish_block(Terminator::Branch(lhs, true_block, rhs_block));

        // If lhs is true, short-circuit
        self.start_block(true_block);
        self.push_inst(Instruction::Const(result_reg, ConstValue::Bool(true)));
        self.finish_block(Terminator::Jump(merge_block));

        // If lhs is false, evaluate rhs
        self.start_block(rhs_block);
        let rhs = self.lower_expr(right);
        self.push_inst(Instruction::Copy(result_reg, rhs));
        self.finish_block(Terminator::Jump(merge_block));

        self.start_block(merge_block);
        result_reg
    }

    /// Lower the `?` operator for Option and Result types.
    /// For Option: if None, early return None; else extract Some(T) value.
    /// For Result: if Err(e), early return Err(e); else extract Ok(T) value.
    fn lower_question_mark(
        &mut self,
        inner: &nudl_core::span::Spanned<Expr>,
    ) -> Register {
        let inner_reg = self.lower_expr(inner);
        let inner_ty = self.infer_expr_type(inner);
        let result_reg = self.alloc_register();

        // Check if this is an Option or Result enum
        let enum_info = inner_ty.and_then(|ty| {
            if let nudl_core::types::TypeKind::Enum { name, variants } =
                self.types.resolve(ty).clone()
            {
                Some((name, variants))
            } else {
                None
            }
        });

        let (name, variants) = match enum_info {
            Some(info) => info,
            None => {
                // Not an enum — just pass through
                self.push_inst(Instruction::Copy(result_reg, inner_reg));
                return result_reg;
            }
        };

        // Load the tag from the enum
        let tag_reg = self.alloc_register();
        self.push_inst(Instruction::Load(tag_reg, inner_reg, 0));

        let success_block = self.new_block_id();
        let error_block = self.new_block_id();
        let merge_block = self.new_block_id();

        if name == "Option" {
            // Option: Some = tag 0, None = tag 1
            let some_tag = variants
                .iter()
                .enumerate()
                .find(|(_, v)| v.name == "Some")
                .map(|(i, _)| i)
                .unwrap_or(0);
            let some_tag_reg = self.alloc_register();
            self.push_inst(Instruction::Const(
                some_tag_reg,
                ConstValue::I32(some_tag as i32),
            ));
            let is_some = self.alloc_register();
            self.push_inst(Instruction::Eq(is_some, tag_reg, some_tag_reg));
            self.finish_block(Terminator::Branch(is_some, success_block, error_block));

            // Success: extract the value from Some(T)
            self.start_block(success_block);
            self.push_inst(Instruction::Load(result_reg, inner_reg, 1));
            self.finish_block(Terminator::Jump(merge_block));

            // Error: early return None
            self.start_block(error_block);
            // Construct a None value to return
            let none_tag = variants
                .iter()
                .enumerate()
                .find(|(_, v)| v.name == "None")
                .map(|(i, _)| i)
                .unwrap_or(1);
            let ret_ty = self.return_type;
            let none_reg = self.alloc_register();
            self.push_inst(Instruction::Alloc(none_reg, ret_ty));
            let none_tag_val = self.alloc_register();
            self.push_inst(Instruction::Const(
                none_tag_val,
                ConstValue::I32(none_tag as i32),
            ));
            self.push_inst(Instruction::Store(none_reg, 0, none_tag_val));
            self.finish_block(Terminator::Return(none_reg));
        } else if name == "Result" {
            // Result: Ok = tag 0, Err = tag 1
            let ok_tag = variants
                .iter()
                .enumerate()
                .find(|(_, v)| v.name == "Ok")
                .map(|(i, _)| i)
                .unwrap_or(0);
            let ok_tag_reg = self.alloc_register();
            self.push_inst(Instruction::Const(
                ok_tag_reg,
                ConstValue::I32(ok_tag as i32),
            ));
            let is_ok = self.alloc_register();
            self.push_inst(Instruction::Eq(is_ok, tag_reg, ok_tag_reg));
            self.finish_block(Terminator::Branch(is_ok, success_block, error_block));

            // Success: extract value from Ok(T)
            self.start_block(success_block);
            self.push_inst(Instruction::Load(result_reg, inner_reg, 1));
            self.finish_block(Terminator::Jump(merge_block));

            // Error: early return Err(E) — propagate the original error
            self.start_block(error_block);
            let ret_ty = self.return_type;
            let err_reg = self.alloc_register();
            self.push_inst(Instruction::Alloc(err_reg, ret_ty));
            // Copy the tag
            let err_tag = variants
                .iter()
                .enumerate()
                .find(|(_, v)| v.name == "Err")
                .map(|(i, _)| i)
                .unwrap_or(1);
            let err_tag_val = self.alloc_register();
            self.push_inst(Instruction::Const(
                err_tag_val,
                ConstValue::I32(err_tag as i32),
            ));
            self.push_inst(Instruction::Store(err_reg, 0, err_tag_val));
            // Copy the error value from the original
            let err_val = self.alloc_register();
            self.push_inst(Instruction::Load(err_val, inner_reg, 1));
            self.push_inst(Instruction::Store(err_reg, 1, err_val));
            self.finish_block(Terminator::Return(err_reg));
        } else {
            // Unknown enum — just pass through
            self.push_inst(Instruction::Copy(result_reg, inner_reg));
            self.finish_block(Terminator::Jump(merge_block));
        }

        self.start_block(merge_block);
        result_reg
    }
}
