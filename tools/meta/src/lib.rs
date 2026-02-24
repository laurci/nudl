use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Expr, Fields, Lit, Meta, parse_macro_input};

/// Derives the `Diagnostic` trait for enums annotated with diagnostic attributes.
///
/// # Attributes
/// - `#[section(Lexer|Parser|Checker|Codegen)]` on the enum
/// - `#[message("...")]` on each variant (supports `{field}` interpolation)
/// - `#[severity(Error|Warning|Info)]` on each variant
///
/// Each variant must have a `span: Span` field for the primary label.
#[proc_macro_derive(Diagnostic, attributes(section, message, severity))]
pub fn derive_diagnostic(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let (section_tokens, section_base_code) = get_section_attr(&input);

    let Data::Enum(data_enum) = &input.data else {
        return syn::Error::new_spanned(&input, "Diagnostic can only be derived for enums")
            .to_compile_error()
            .into();
    };

    let mut info_arms = Vec::new();
    let mut message_arms = Vec::new();
    let mut label_arms = Vec::new();

    for (variant_index, variant) in data_enum.variants.iter().enumerate() {
        let variant_name = &variant.ident;
        let severity = get_severity_attr(variant);
        let message_fmt = get_message_attr(variant);
        let code = section_base_code + variant_index as u32;

        let Fields::Named(fields) = &variant.fields else {
            return syn::Error::new_spanned(variant, "Diagnostic variants must have named fields")
                .to_compile_error()
                .into();
        };

        let field_names: Vec<_> = fields.named.iter().map(|f| &f.ident).collect();

        // Build format args from message template
        let format_expr = build_format_expr(&message_fmt, &field_names);

        let section_tokens_clone = section_tokens.clone();

        info_arms.push(quote! {
            #name::#variant_name { .. } => ::nudl_core::diagnostic::DiagnosticInfo {
                code: #code,
                severity: ::nudl_core::diagnostic::#severity,
                section: #section_tokens_clone,
            }
        });

        message_arms.push(quote! {
            #name::#variant_name { #(#field_names),* } => #format_expr
        });

        label_arms.push(quote! {
            #name::#variant_name { span, .. } => vec![
                ::nudl_core::diagnostic::Label::new(*span, self.get_diagnostic_message())
            ]
        });
    }

    let expanded = quote! {
        #[allow(unused_variables)]
        impl ::nudl_core::diagnostic::Diagnostic for #name {
            fn get_diagnostic_info(&self) -> ::nudl_core::diagnostic::DiagnosticInfo {
                match self {
                    #(#info_arms),*
                }
            }

            fn get_diagnostic_message(&self) -> String {
                match self {
                    #(#message_arms),*
                }
            }

            fn get_diagnostic_labels(&self) -> Vec<::nudl_core::diagnostic::Label> {
                match self {
                    #(#label_arms),*
                }
            }
        }
    };

    expanded.into()
}

fn get_section_attr(input: &DeriveInput) -> (proc_macro2::TokenStream, u32) {
    for attr in &input.attrs {
        if attr.path().is_ident("section") {
            let ident: syn::Ident = attr.parse_args().expect("expected section identifier");
            let section_str = ident.to_string();
            return match section_str.as_str() {
                "Lexer" => (
                    quote! { ::nudl_core::diagnostic::DiagnosticSection::Lexer },
                    100,
                ),
                "Parser" => (
                    quote! { ::nudl_core::diagnostic::DiagnosticSection::Parser },
                    200,
                ),
                "Checker" => (
                    quote! { ::nudl_core::diagnostic::DiagnosticSection::Checker },
                    400,
                ),
                "Codegen" => (
                    quote! { ::nudl_core::diagnostic::DiagnosticSection::Codegen },
                    500,
                ),
                _ => panic!("unknown section: {}", section_str),
            };
        }
    }
    panic!("missing #[section(...)] attribute on enum");
}

fn get_severity_attr(variant: &syn::Variant) -> proc_macro2::TokenStream {
    for attr in &variant.attrs {
        if attr.path().is_ident("severity") {
            let ident: syn::Ident = attr.parse_args().expect("expected severity identifier");
            let sev_str = ident.to_string();
            return match sev_str.as_str() {
                "Error" => quote! { Severity::Error },
                "Warning" => quote! { Severity::Warning },
                "Info" => quote! { Severity::Info },
                _ => panic!("unknown severity: {}", sev_str),
            };
        }
    }
    // Default to Error
    quote! { Severity::Error }
}

fn get_message_attr(variant: &syn::Variant) -> String {
    for attr in &variant.attrs {
        if attr.path().is_ident("message") {
            let Meta::List(meta_list) = &attr.meta else {
                panic!("expected #[message(\"...\")]");
            };
            let expr: Expr =
                syn::parse2(meta_list.tokens.clone()).expect("expected string literal in message");
            if let Expr::Lit(lit) = expr {
                if let Lit::Str(s) = lit.lit {
                    return s.value();
                }
            }
            panic!("expected string literal in message");
        }
    }
    panic!("missing #[message(\"...\")] on variant {}", variant.ident);
}

fn build_format_expr(
    template: &str,
    field_names: &[&Option<syn::Ident>],
) -> proc_macro2::TokenStream {
    // Convert {field} patterns to format! arguments
    // e.g. "unexpected character '{ch}'" → format!("unexpected character '{}'", ch)
    let mut fmt_string = String::new();
    let mut args: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut chars = template.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut field_name = String::new();
            for ch in chars.by_ref() {
                if ch == '}' {
                    break;
                }
                field_name.push(ch);
            }
            fmt_string.push_str("{}");
            let ident = syn::Ident::new(&field_name, proc_macro2::Span::call_site());
            args.push(quote! { #ident });
        } else {
            fmt_string.push(ch);
        }
    }

    if args.is_empty() {
        let _ = field_names; // suppress unused warning
        quote! { #fmt_string.to_string() }
    } else {
        quote! { format!(#fmt_string, #(#args),*) }
    }
}
