#[cfg(test)]
mod tests {
    use crate::ast::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use nudl_core::span::FileId;

    fn parse(source: &str) -> Module {
        let (tokens, lex_diags) = Lexer::new(source, FileId(0)).tokenize();
        assert!(
            !lex_diags.has_errors(),
            "lex errors: {:?}",
            lex_diags.reports()
        );
        let (module, parse_diags) = Parser::new(tokens).parse_module();
        assert!(
            !parse_diags.has_errors(),
            "parse errors: {:?}",
            parse_diags.reports()
        );
        module
    }

    #[test]
    fn parse_hello_world() {
        let module = parse(
            r#"fn main() {
    println("Hello, world!");
}"#,
        );
        assert_eq!(module.items.len(), 1);
        match &module.items[0].node {
            Item::FnDef {
                name,
                params,
                return_type,
                ..
            } => {
                assert_eq!(name, "main");
                assert!(params.is_empty());
                assert!(return_type.is_none());
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_fn_with_params_and_return() {
        let module = parse("fn add(a: i32, b: i32) -> i32 { a }");
        match &module.items[0].node {
            Item::FnDef {
                name,
                params,
                return_type,
                ..
            } => {
                assert_eq!(name, "add");
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].name, "a");
                assert_eq!(params[1].name, "b");
                assert!(return_type.is_some());
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_extern_block() {
        let module = parse(
            r#"extern "C" {
    fn write(fd: i32, buf: RawPtr, len: u64) -> i64;
}"#,
        );
        match &module.items[0].node {
            Item::ExternBlock { library, items } => {
                assert_eq!(library.as_deref(), Some("C"));
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].node.name, "write");
            }
            _ => panic!("expected ExternBlock"),
        }
    }

    #[test]
    fn parse_error_missing_brace() {
        let (tokens, _) = Lexer::new("fn main() {", FileId(0)).tokenize();
        let (_, diags) = Parser::new(tokens).parse_module();
        assert!(diags.has_errors());
    }

    #[test]
    fn parse_binary_precedence() {
        let module = parse("fn main() { let x = 1 + 2 * 3; }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                let stmt = &body.node.stmts[0].node;
                match stmt {
                    Stmt::Let { value, .. } => match &value.node {
                        Expr::Binary { op, right, .. } => {
                            assert_eq!(*op, BinOp::Add);
                            match &right.node {
                                Expr::Binary { op, .. } => assert_eq!(*op, BinOp::Mul),
                                _ => panic!("expected Binary(Mul)"),
                            }
                        }
                        _ => panic!("expected Binary"),
                    },
                    _ => panic!("expected Let"),
                }
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_if_else() {
        let module = parse(
            r#"fn main() {
    if x > 0 { 1 } else { 2 }
}"#,
        );
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                // The if/else is a tail expression or statement
                let has_if = body.node.tail_expr.is_some()
                    || body.node.stmts.iter().any(|s| {
                        matches!(
                            &s.node,
                            Stmt::Expr(e) if matches!(&e.node, Expr::If { .. })
                        )
                    });
                assert!(has_if, "expected If expression in body");
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_while_loop() {
        let module = parse("fn main() { while x < 10 { x = x + 1; } }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                let in_stmts = body.node.stmts.iter().any(|s| {
                    matches!(
                        &s.node,
                        Stmt::Expr(e) if matches!(&e.node, Expr::While { .. })
                    )
                });
                let in_tail = body
                    .node
                    .tail_expr
                    .as_ref()
                    .is_some_and(|e| matches!(&e.node, Expr::While { .. }));
                assert!(in_stmts || in_tail, "expected While expression");
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_loop_break() {
        let module = parse("fn main() { loop { break; } }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                let in_stmts = body.node.stmts.iter().any(|s| {
                    matches!(
                        &s.node,
                        Stmt::Expr(e) if matches!(&e.node, Expr::Loop { .. })
                    )
                });
                let in_tail = body
                    .node
                    .tail_expr
                    .as_ref()
                    .is_some_and(|e| matches!(&e.node, Expr::Loop { .. }));
                assert!(in_stmts || in_tail, "expected Loop expression");
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_tail_expression() {
        let module = parse("fn foo() -> i32 { 42 }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                assert!(body.node.tail_expr.is_some(), "expected tail expression");
                match &body.node.tail_expr.as_ref().unwrap().node {
                    Expr::Literal(Literal::Int(s, None)) => assert_eq!(s, "42"),
                    _ => panic!("expected Int literal tail"),
                }
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_assignment() {
        let module = parse("fn main() { x = 42; }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                let has_assign = body.node.stmts.iter().any(|s| {
                    matches!(
                        &s.node,
                        Stmt::Expr(e) if matches!(&e.node, Expr::Assign { .. })
                    )
                });
                assert!(has_assign, "expected Assign expression");
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_compound_assign() {
        let module = parse("fn main() { x += 1; }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                let has_compound = body.node.stmts.iter().any(|s| {
                    matches!(
                        &s.node,
                        Stmt::Expr(e) if matches!(&e.node, Expr::CompoundAssign { .. })
                    )
                });
                assert!(has_compound, "expected CompoundAssign expression");
            }
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_unary_negation() {
        let module = parse("fn main() { let x = -42; }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => match &body.node.stmts[0].node {
                Stmt::Let { value, .. } => {
                    assert!(
                        matches!(
                            &value.node,
                            Expr::Unary {
                                op: UnaryOp::Neg,
                                ..
                            }
                        ),
                        "expected Unary Neg"
                    );
                }
                _ => panic!("expected Let"),
            },
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_grouped_expression() {
        let module = parse("fn main() { let x = (1 + 2) * 3; }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => match &body.node.stmts[0].node {
                Stmt::Let { value, .. } => {
                    // Should be Mul at top level with grouped Add inside
                    assert!(
                        matches!(&value.node, Expr::Binary { op: BinOp::Mul, .. }),
                        "expected Mul at top"
                    );
                }
                _ => panic!("expected Let"),
            },
            _ => panic!("expected FnDef"),
        }
    }

    #[test]
    fn parse_add_function_with_tail() {
        let module = parse("fn add(a: i32, b: i32) -> i32 { a + b }");
        match &module.items[0].node {
            Item::FnDef { body, .. } => {
                assert!(body.node.tail_expr.is_some());
                match &body.node.tail_expr.as_ref().unwrap().node {
                    Expr::Binary { op: BinOp::Add, .. } => {}
                    _ => panic!("expected Binary Add tail"),
                }
            }
            _ => panic!("expected FnDef"),
        }
    }

    // --- Phase 3 tests ---

    #[test]
    fn parse_named_args() {
        let module = parse("fn main() { add(1, b: 2); }");
        assert_eq!(module.items.len(), 1);
    }

    #[test]
    fn parse_default_params() {
        let module = parse("fn greet(name: string, times: i32 = 1) -> i32 { times }");
        if let Item::FnDef { params, .. } = &module.items[0].node {
            assert_eq!(params.len(), 2);
            assert!(params[0].default_value.is_none());
            assert!(params[1].default_value.is_some());
        } else {
            panic!("expected FnDef");
        }
    }

    #[test]
    fn parse_self_param() {
        let module = parse("fn get(self) -> i32 { 0 }");
        if let Item::FnDef { params, .. } = &module.items[0].node {
            assert_eq!(params.len(), 1);
            assert!(params[0].is_self);
            assert!(!params[0].is_mut);
        } else {
            panic!("expected FnDef");
        }
    }

    #[test]
    fn parse_mut_self_param() {
        let module = parse("fn incr(mut self) { }");
        if let Item::FnDef { params, .. } = &module.items[0].node {
            assert_eq!(params.len(), 1);
            assert!(params[0].is_self);
            assert!(params[0].is_mut);
        } else {
            panic!("expected FnDef");
        }
    }

    #[test]
    fn parse_impl_block() {
        let module = parse(
            r#"
struct Point { x: i32, y: i32 }
impl Point {
    fn new(x: i32, y: i32) -> Point { Point { x: x, y: y } }
    fn get_x(self) -> i32 { 0 }
}
"#,
        );
        assert_eq!(module.items.len(), 2);
        if let Item::ImplBlock {
            type_name, methods, ..
        } = &module.items[1].node
        {
            assert_eq!(type_name, "Point");
            assert_eq!(methods.len(), 2);
        } else {
            panic!("expected ImplBlock");
        }
    }

    #[test]
    fn parse_method_call() {
        let module = parse("fn main() { p.get_x(); }");
        assert_eq!(module.items.len(), 1);
    }

    #[test]
    fn parse_static_call() {
        let module = parse("fn main() { Point::new(1, y: 2); }");
        assert_eq!(module.items.len(), 1);
    }

    #[test]
    fn parse_struct_field_shorthand() {
        let module = parse(
            r#"
struct Point { x: i32, y: i32 }
fn main() {
    let x = 1;
    let y = 2;
    let p = Point { x, y };
}
"#,
        );
        assert_eq!(module.items.len(), 2);
    }

    #[test]
    fn parse_trailing_lambda() {
        let module = parse(
            r#"
fn main() {
    let a = foo(1, 2) |x: i32| x + 1;
    let b = bar() { it + 1 };
    let c = baz(1) || { 42 };
}
"#,
        );
        // Just check that it parses without error
        assert_eq!(module.items.len(), 1);
        if let Item::FnDef { body, .. } = &module.items[0].node {
            assert_eq!(body.node.stmts.len(), 3);
        } else {
            panic!("expected function");
        }
    }
}
