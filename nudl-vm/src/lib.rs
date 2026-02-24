use std::collections::HashMap;
use std::fmt;

use nudl_bc::ir::*;
use nudl_core::intern::Symbol;

/// Runtime value in the VM.
#[derive(Debug, Clone)]
pub enum Value {
    Unit,
    I32(i32),
    I64(i64),
    U64(u64),
    Bool(bool),
    /// String constant (index into Program::string_constants).
    String(u32),
    /// Synthetic raw pointer (not dereferenceable, only for VM-internal tracking).
    RawPtr(u64),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Unit => write!(f, "()"),
            Value::I32(v) => write!(f, "{}", v),
            Value::I64(v) => write!(f, "{}", v),
            Value::U64(v) => write!(f, "{}", v),
            Value::Bool(v) => write!(f, "{}", v),
            Value::String(idx) => write!(f, "string[{}]", idx),
            Value::RawPtr(v) => write!(f, "ptr(0x{:x})", v),
        }
    }
}

/// VM execution error.
#[derive(Debug)]
pub enum VmError {
    /// Attempted to call an extern function, which is not allowed in the VM.
    ExternCallNotAllowed { function_name: String },
    /// Function not found.
    UndefinedFunction { symbol: Symbol },
    /// Execution exceeded the step limit.
    StepLimitExceeded { limit: u64 },
    /// Hit an unreachable terminator.
    Unreachable,
    /// No entry function (main) found.
    NoEntryFunction,
    /// Invalid block index.
    InvalidBlock { function_name: String, block_id: u32 },
    /// Stack overflow (too many nested calls).
    StackOverflow { depth: usize },
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VmError::ExternCallNotAllowed { function_name } =>
                write!(f, "cannot call extern function '{}' in the VM", function_name),
            VmError::UndefinedFunction { symbol } =>
                write!(f, "undefined function (symbol {})", symbol.0),
            VmError::StepLimitExceeded { limit } =>
                write!(f, "execution exceeded step limit of {}", limit),
            VmError::Unreachable =>
                write!(f, "hit unreachable code"),
            VmError::NoEntryFunction =>
                write!(f, "no entry function (main) found"),
            VmError::InvalidBlock { function_name, block_id } =>
                write!(f, "invalid block b{} in function '{}'", block_id, function_name),
            VmError::StackOverflow { depth } =>
                write!(f, "stack overflow at depth {}", depth),
        }
    }
}

const DEFAULT_STEP_LIMIT: u64 = 1_000_000;
const MAX_CALL_DEPTH: usize = 256;

/// Register-based SSA bytecode interpreter.
pub struct Vm {
    step_count: u64,
    step_limit: u64,
    call_depth: usize,
}

impl Vm {
    pub fn new() -> Self {
        Self {
            step_count: 0,
            step_limit: DEFAULT_STEP_LIMIT,
            call_depth: 0,
        }
    }

    pub fn with_step_limit(limit: u64) -> Self {
        Self {
            step_count: 0,
            step_limit: limit,
            call_depth: 0,
        }
    }

    /// Run the program starting from the entry function.
    pub fn run(&mut self, program: &Program) -> Result<Value, VmError> {
        let entry_id = program.entry_function.ok_or(VmError::NoEntryFunction)?;

        // Build function lookup: Symbol -> index in program.functions
        let func_map: HashMap<Symbol, usize> = program.functions.iter()
            .enumerate()
            .map(|(i, f)| (f.name, i))
            .collect();

        let entry_idx = program.functions.iter()
            .position(|f| f.id == entry_id)
            .ok_or(VmError::NoEntryFunction)?;

        self.execute_function(program, &func_map, entry_idx, vec![])
    }

    fn execute_function(
        &mut self,
        program: &Program,
        func_map: &HashMap<Symbol, usize>,
        func_idx: usize,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let func = &program.functions[func_idx];
        let func_name = program.interner.resolve(func.name).to_string();

        // Check for extern function
        if func.is_extern {
            return Err(VmError::ExternCallNotAllowed { function_name: func_name });
        }

        // Check call depth
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(VmError::StackOverflow { depth: self.call_depth });
        }
        self.call_depth += 1;

        // Initialize registers
        let mut registers = vec![Value::Unit; func.register_count as usize];

        // Copy arguments into parameter registers
        for (i, arg) in args.into_iter().enumerate() {
            if i < registers.len() {
                registers[i] = arg;
            }
        }

        // Execute blocks
        let mut block_idx: usize = 0;
        let result = loop {
            if block_idx >= func.blocks.len() {
                break Err(VmError::InvalidBlock {
                    function_name: func_name.clone(),
                    block_id: block_idx as u32,
                });
            }

            let block = &func.blocks[block_idx];

            // Execute instructions
            for inst in &block.instructions {
                self.step_count += 1;
                if self.step_count > self.step_limit {
                    break;
                }
                self.execute_instruction(program, func_map, inst, &mut registers)?;
            }

            if self.step_count > self.step_limit {
                break Err(VmError::StepLimitExceeded { limit: self.step_limit });
            }

            // Execute terminator
            match &block.terminator {
                Terminator::Return(reg) => {
                    break Ok(registers[reg.0 as usize].clone());
                }
                Terminator::Jump(target) => {
                    block_idx = func.blocks.iter()
                        .position(|b| b.id == *target)
                        .unwrap_or(target.0 as usize);
                }
                Terminator::Branch(cond, then_block, else_block) => {
                    let cond_val = &registers[cond.0 as usize];
                    let target = if is_truthy(cond_val) { then_block } else { else_block };
                    block_idx = func.blocks.iter()
                        .position(|b| b.id == *target)
                        .unwrap_or(target.0 as usize);
                }
                Terminator::Unreachable => {
                    break Err(VmError::Unreachable);
                }
            }
        };

        self.call_depth -= 1;
        result
    }

    fn execute_instruction(
        &mut self,
        program: &Program,
        func_map: &HashMap<Symbol, usize>,
        inst: &Instruction,
        registers: &mut [Value],
    ) -> Result<(), VmError> {
        match inst {
            Instruction::Const(reg, val) => {
                registers[reg.0 as usize] = match val {
                    ConstValue::Unit => Value::Unit,
                    ConstValue::I32(v) => Value::I32(*v),
                    ConstValue::I64(v) => Value::I64(*v),
                    ConstValue::U64(v) => Value::U64(*v),
                    ConstValue::Bool(v) => Value::Bool(*v),
                    ConstValue::StringLiteral(idx) => Value::String(*idx),
                };
            }

            Instruction::ConstUnit(reg) => {
                registers[reg.0 as usize] = Value::Unit;
            }

            Instruction::StringPtr(dst, src) => {
                // Extract a synthetic pointer from a string value.
                // For string literals, use the constant index as a synthetic address.
                let val = match &registers[src.0 as usize] {
                    Value::String(idx) => Value::RawPtr(*idx as u64),
                    _ => Value::RawPtr(0),
                };
                registers[dst.0 as usize] = val;
            }

            Instruction::StringLen(dst, src) => {
                // Extract the length from a string value.
                let val = match &registers[src.0 as usize] {
                    Value::String(idx) => {
                        let len = program.string_constants.get(*idx as usize)
                            .map(|s| s.len() as u64)
                            .unwrap_or(0);
                        Value::U64(len)
                    }
                    _ => Value::U64(0),
                };
                registers[dst.0 as usize] = val;
            }

            Instruction::StringConstPtr(dst, idx) => {
                registers[dst.0 as usize] = Value::RawPtr(*idx as u64);
            }

            Instruction::StringConstLen(dst, idx) => {
                let len = program.string_constants.get(*idx as usize)
                    .map(|s| s.len() as u64)
                    .unwrap_or(0);
                registers[dst.0 as usize] = Value::U64(len);
            }

            Instruction::Call(dst, func_ref, args) => {
                let arg_values: Vec<Value> = args.iter()
                    .map(|r| registers[r.0 as usize].clone())
                    .collect();

                match func_ref {
                    FunctionRef::Named(sym) => {
                        let idx = func_map.get(sym)
                            .ok_or(VmError::UndefinedFunction { symbol: *sym })?;
                        let result = self.execute_function(program, func_map, *idx, arg_values)?;
                        registers[dst.0 as usize] = result;
                    }
                    FunctionRef::Extern(sym) => {
                        let name = program.interner.resolve(*sym);
                        return Err(VmError::ExternCallNotAllowed {
                            function_name: name.to_string(),
                        });
                    }
                    FunctionRef::Builtin(sym) => {
                        // Builtins should be lowered to specific instructions,
                        // but handle them here as a fallback.
                        let name = program.interner.resolve(*sym);
                        match name {
                            "__str_ptr" => {
                                let val = match arg_values.first() {
                                    Some(Value::String(idx)) => Value::RawPtr(*idx as u64),
                                    _ => Value::RawPtr(0),
                                };
                                registers[dst.0 as usize] = val;
                            }
                            "__str_len" => {
                                let val = match arg_values.first() {
                                    Some(Value::String(idx)) => {
                                        let len = program.string_constants.get(*idx as usize)
                                            .map(|s| s.len() as u64)
                                            .unwrap_or(0);
                                        Value::U64(len)
                                    }
                                    _ => Value::U64(0),
                                };
                                registers[dst.0 as usize] = val;
                            }
                            _ => {
                                registers[dst.0 as usize] = Value::Unit;
                            }
                        }
                    }
                }
            }

            Instruction::Copy(dst, src) => {
                registers[dst.0 as usize] = registers[src.0 as usize].clone();
            }

            Instruction::Nop => {}
        }

        Ok(())
    }
}

/// Check if a value is truthy (for branch conditions).
fn is_truthy(val: &Value) -> bool {
    match val {
        Value::Unit => false,
        Value::I32(v) => *v != 0,
        Value::I64(v) => *v != 0,
        Value::U64(v) => *v != 0,
        Value::Bool(v) => *v,
        Value::String(_) => true,
        Value::RawPtr(v) => *v != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nudl_ast::lexer::Lexer;
    use nudl_ast::parser::Parser;
    use nudl_bc::checker::Checker;
    use nudl_bc::lower::Lowerer;
    use nudl_core::span::FileId;

    fn compile(source: &str) -> Program {
        let (tokens, _) = Lexer::new(source, FileId(0)).tokenize();
        let (module, _) = Parser::new(tokens).parse_module();
        let (checked, diags) = Checker::new().check(&module);
        assert!(!diags.has_errors(), "checker errors: {:?}", diags.reports());
        Lowerer::new(checked).lower(&module)
    }

    #[test]
    fn run_empty_main() {
        let program = compile("fn main() {}");
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), Value::Unit));
    }

    #[test]
    fn run_function_call() {
        let program = compile(r#"
fn greet(s: string) {}
fn main() {
    greet("hello");
}
"#);
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_ok());
    }

    #[test]
    fn run_nested_calls() {
        let program = compile(r#"
fn inner(s: string) {}
fn outer(s: string) {
    inner(s);
}
fn main() {
    outer("hello");
}
"#);
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_ok());
    }

    #[test]
    fn extern_call_fails() {
        let program = compile(r#"
extern {
    fn write(fd: i32, buf: RawPtr, count: u64) -> i64;
}
fn print(s: string) {
    write(1, __str_ptr(s), __str_len(s));
}
fn main() {
    print("hello");
}
"#);
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_err());
        match result.unwrap_err() {
            VmError::ExternCallNotAllowed { function_name } => {
                assert_eq!(function_name, "write");
            }
            other => panic!("expected ExternCallNotAllowed, got {:?}", other),
        }
    }

    #[test]
    fn step_limit_exceeded() {
        // Create a program that calls itself to blow the step limit
        // Since we don't have recursion in the test (main calls greet, greet doesn't recurse),
        // use a very low step limit
        let program = compile(r#"
fn a(s: string) {}
fn b(s: string) { a(s); }
fn c(s: string) { b(s); }
fn d(s: string) { c(s); }
fn main() {
    d("x");
}
"#);
        let mut vm = Vm::with_step_limit(5);
        let result = vm.run(&program);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), VmError::StepLimitExceeded { .. }));
    }

    #[test]
    fn string_builtins_work() {
        let program = compile(r#"
fn main() {
    __str_ptr("hello");
    __str_len("hello");
}
"#);
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_ok());
    }

    #[test]
    fn no_entry_function_error() {
        // Create a program manually with no entry function
        let program = Program {
            functions: vec![],
            string_constants: vec![],
            entry_function: None,
            extern_libs: vec![],
            interner: nudl_core::intern::StringInterner::new(),
        };
        let mut vm = Vm::new();
        let result = vm.run(&program);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), VmError::NoEntryFunction));
    }
}
