use nudl_ast::ast::*;

use crate::ir::*;

use super::context::FunctionLowerCtx;

impl<'a> FunctionLowerCtx<'a> {
    pub(super) fn lower_builtin_call(&mut self, name: &str, args: &[CallArg]) -> Register {
        let call_span = self.current_span;
        match name {
            "__str_ptr" => {
                let arg_reg = self.lower_expr(&args[0].value);
                self.current_span = call_span;
                let dst = self.alloc_register();
                self.push_inst(Instruction::StringPtr(dst, arg_reg));
                dst
            }
            "__str_len" => {
                let arg_reg = self.lower_expr(&args[0].value);
                self.current_span = call_span;
                let dst = self.alloc_register();
                self.push_inst(Instruction::StringLen(dst, arg_reg));
                dst
            }
            "__str_concat" | "__i32_to_str" | "__i64_to_str" | "__f64_to_str"
            | "__bool_to_str" | "__char_to_str" => {
                let arg_regs: Vec<Register> =
                    args.iter().map(|a| self.lower_expr(&a.value)).collect();
                self.current_span = call_span;
                let sym = self.interner.intern(name);
                let string_ty = self.types.string();
                let dst = self.alloc_typed_register(string_ty);
                self.push_inst(Instruction::Call(
                    dst,
                    FunctionRef::Builtin(sym),
                    arg_regs,
                ));
                dst
            }
            "panic" => {
                let arg_reg = self.lower_expr(&args[0].value);
                self.current_span = call_span;
                let sym = self.interner.intern("panic");
                let never_ty = self.types.never();
                let dst = self.alloc_typed_register(never_ty);
                self.push_inst(Instruction::Call(
                    dst,
                    FunctionRef::Builtin(sym),
                    vec![arg_reg],
                ));
                dst
            }
            "assert" => {
                let cond_reg = self.lower_expr(&args[0].value);
                let msg_reg = self.lower_expr(&args[1].value);
                self.current_span = call_span;
                let sym = self.interner.intern("assert");
                let unit_ty = self.types.unit();
                let dst = self.alloc_typed_register(unit_ty);
                self.push_inst(Instruction::Call(
                    dst,
                    FunctionRef::Builtin(sym),
                    vec![cond_reg, msg_reg],
                ));
                dst
            }
            "exit" => {
                let arg_reg = self.lower_expr(&args[0].value);
                self.current_span = call_span;
                let sym = self.interner.intern("exit");
                let never_ty = self.types.never();
                let dst = self.alloc_typed_register(never_ty);
                self.push_inst(Instruction::Call(
                    dst,
                    FunctionRef::Builtin(sym),
                    vec![arg_reg],
                ));
                dst
            }
            _ => {
                let reg = self.alloc_register();
                self.push_inst(Instruction::ConstUnit(reg));
                reg
            }
        }
    }

    pub(super) fn lower_generic_call(
        &mut self,
        name: &str,
        args: &[CallArg],
        is_extern: bool,
    ) -> Register {
        let call_span = self.current_span;
        // Lower all arguments
        let arg_regs: Vec<Register> = args.iter().map(|arg| self.lower_expr(&arg.value)).collect();
        self.current_span = call_span;

        // Caller-retain: for struct-typed args, emit Retain so callee's Release doesn't free them
        if !is_extern {
            if let Some(sig) = self.function_sigs.get(name).cloned() {
                for (i, (_pname, pty)) in sig.params.iter().enumerate() {
                    if self.types.is_struct(*pty) && i < arg_regs.len() {
                        self.push_inst(Instruction::Retain(arg_regs[i]));
                    }
                }
            }
        }

        let sym = self.interner.intern(name);

        let func_ref = if is_extern {
            FunctionRef::Extern(sym)
        } else {
            FunctionRef::Named(sym)
        };

        let ret_type = self
            .function_sigs
            .get(name)
            .map(|s| s.return_type)
            .unwrap_or(self.types.i64());
        let dst = self.alloc_typed_register(ret_type);
        self.push_inst(Instruction::Call(dst, func_ref, arg_regs));
        dst
    }

    /// Resolve call arguments to positional registers, handling named args and defaults.
    /// `skip_params` is the number of leading params to skip for argument matching (e.g., self).
    /// `fn_key` is the key used to look up default values in `param_defaults`.
    pub(super) fn resolve_call_args(
        &mut self,
        fn_key: &str,
        sig: &crate::checker::FunctionSig,
        call_args: &[CallArg],
        skip_params: usize,
    ) -> Vec<Register> {
        let call_span = self.current_span;
        let callable_params = &sig.params[skip_params..];
        let callable_defaults = &sig.has_default[skip_params..];

        // Build array of resolved registers, one per callable param
        let mut resolved: Vec<Option<Register>> = vec![None; callable_params.len()];

        // Process positional args first
        let mut positional_idx = 0;
        for arg in call_args {
            if arg.name.is_some() {
                break;
            }
            if positional_idx < callable_params.len() {
                let reg = self.lower_expr(&arg.value);
                resolved[positional_idx] = Some(reg);
                positional_idx += 1;
            }
        }

        // Process named args
        for arg in call_args.iter().skip(positional_idx) {
            if let Some(arg_name) = &arg.name {
                if let Some(pos) = callable_params.iter().position(|(n, _)| n == arg_name) {
                    let reg = self.lower_expr(&arg.value);
                    resolved[pos] = Some(reg);
                }
            } else {
                // Positional arg that came after named
                if positional_idx < callable_params.len() {
                    let reg = self.lower_expr(&arg.value);
                    resolved[positional_idx] = Some(reg);
                    positional_idx += 1;
                }
            }
        }

        self.current_span = call_span;

        // Fill in defaults for missing args
        let defaults = self.param_defaults.get(fn_key).cloned();

        for (i, resolved_reg) in resolved.iter_mut().enumerate() {
            if resolved_reg.is_none() && callable_defaults[i] {
                // Try to lower the default expression from the AST
                let param_idx = i + skip_params;
                let did_lower = if let Some(ref defaults_vec) = defaults {
                    if let Some(Some(default_expr)) = defaults_vec.get(param_idx) {
                        let reg = self.lower_expr(default_expr);
                        *resolved_reg = Some(reg);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !did_lower {
                    let reg = self.alloc_register();
                    self.push_inst(Instruction::ConstUnit(reg));
                    *resolved_reg = Some(reg);
                }
            }
        }

        resolved
            .into_iter()
            .map(|r| {
                r.unwrap_or_else(|| {
                    let reg = self.alloc_register();
                    self.push_inst(Instruction::ConstUnit(reg));
                    reg
                })
            })
            .collect()
    }

    /// Lower a function call with named arg resolution and default filling.
    pub(super) fn lower_resolved_call(
        &mut self,
        name: &str,
        sig: &crate::checker::FunctionSig,
        args: &[CallArg],
        is_extern: bool,
        skip_params: usize,
    ) -> Register {
        // For extern or simple calls without named args and with all args provided, fast path
        let has_named = args.iter().any(|a| a.name.is_some());
        let all_provided = args.len() + skip_params == sig.params.len();

        if !has_named && all_provided {
            return self.lower_generic_call(name, args, is_extern);
        }

        let call_span = self.current_span;
        let arg_regs = self.resolve_call_args(name, sig, args, skip_params);
        self.current_span = call_span;

        // Caller-retain for struct-typed args
        if !is_extern {
            for (i, (_pname, pty)) in sig.params[skip_params..].iter().enumerate() {
                if self.types.is_struct(*pty) && i < arg_regs.len() {
                    self.push_inst(Instruction::Retain(arg_regs[i]));
                }
            }
        }

        let sym = self.interner.intern(name);
        let func_ref = if is_extern {
            FunctionRef::Extern(sym)
        } else {
            FunctionRef::Named(sym)
        };

        let dst = self.alloc_typed_register(sig.return_type);
        self.push_inst(Instruction::Call(dst, func_ref, arg_regs));
        dst
    }

    /// Lower a method call: self is already lowered, pass it as first arg
    pub(super) fn lower_method_call(
        &mut self,
        mangled_name: &str,
        sig: &crate::checker::FunctionSig,
        self_reg: Register,
        args: &[CallArg],
    ) -> Register {
        let call_span = self.current_span;

        // Resolve the rest of the args (skip self)
        let mut arg_regs = vec![self_reg];
        let rest = self.resolve_call_args(mangled_name, sig, args, 1);
        arg_regs.extend(rest);

        self.current_span = call_span;

        // Caller-retain for struct-typed args
        for (i, (_pname, pty)) in sig.params.iter().enumerate() {
            if self.types.is_struct(*pty) && i < arg_regs.len() {
                self.push_inst(Instruction::Retain(arg_regs[i]));
            }
        }

        let sym = self.interner.intern(mangled_name);
        let func_ref = FunctionRef::Named(sym);

        let dst = self.alloc_typed_register(sig.return_type);
        self.push_inst(Instruction::Call(dst, func_ref, arg_regs));
        dst
    }
}
