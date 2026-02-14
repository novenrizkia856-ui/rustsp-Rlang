//! Expression Translation
//!
//! Handles non-assignment expressions and statements in RustS+.
//!
//! This includes:
//! - Function calls
//! - Method calls
//! - Return expressions
//! - Bare expressions

use crate::variable::expand_value;
use crate::function::{
    CurrentFunctionContext, FunctionRegistry,
    transform_string_concat, transform_call_args, should_be_tail_return,
};
use crate::control_flow::transform_enum_struct_init;
use crate::clone_helpers::transform_array_access_clone;
use crate::helpers::{ends_with_continuation_operator, needs_semicolon};
use crate::transform_literal::is_string_literal;
use crate::translate::assignment_translate::parse_var_type_annotation;

/// Process a non-assignment expression
pub fn process_non_assignment(
    trimmed: &str,
    leading_ws: &str,
    _line_num: usize,
    current_fn_ctx: &CurrentFunctionContext,
    fn_registry: &FunctionRegistry,
    is_before_closing_brace: bool,
    inside_multiline_expr: bool,
    next_line_is_method_chain: bool,
    next_line_closes_expr: bool,
    prev_line_was_continuation: &mut bool,
) -> String {
    let mut transformed = trimmed.to_string();
    
    // Handle bare mut (e.g., `mut x = 1`)
    if trimmed.starts_with("mut ") && trimmed.contains('=') && !trimmed.contains("==") {
        let rest = trimmed.strip_prefix("mut ").unwrap().trim();
        if let Some(eq_pos) = rest.find('=') {
            let var_part = rest[..eq_pos].trim();
            let val_part = rest[eq_pos + 1..].trim().trim_end_matches(';');
            
            let (var_name, type_annotation) = parse_var_type_annotation(var_part);
            
            let mut expanded_value = expand_value(val_part, None);
            expanded_value = transform_array_access_clone(&expanded_value);
            if current_fn_ctx.is_inside() {
                expanded_value = transform_string_concat(&expanded_value, current_fn_ctx);
            }
            expanded_value = transform_call_args(&expanded_value, fn_registry);
            
            return format!("{}let mut {}{} = {};", leading_ws, var_name, type_annotation, expanded_value);
        }
    }
    
    // Apply transformations
    if current_fn_ctx.is_inside() {
        transformed = transform_string_concat(&transformed, current_fn_ctx);
    }
    transformed = transform_call_args(&transformed, fn_registry);
    transformed = transform_enum_struct_init(&transformed);
    
    // Check if this is a return expression
    let is_return_expr = should_be_tail_return(&transformed, current_fn_ctx, is_before_closing_brace);
    
    // Transform string literals to owned if return type is String
    if is_return_expr {
        if let Some(ref ret_type) = current_fn_ctx.return_type {
            if ret_type == "String" && is_string_literal(&transformed) {
                let inner = &transformed[1..transformed.len()-1];
                transformed = format!("String::from(\"{}\")", inner);
            }
        }
    }
    
    let this_line_ends_with_continuation = ends_with_continuation_operator(&transformed);
    *prev_line_was_continuation = this_line_ends_with_continuation;
    
    // CRITICAL FIX: Semicolon logic for non-assignment expressions
    // 1. If ends with continuation → no semicolon
    // 2. If return expression → no semicolon
    // 3. If next line is method chain → no semicolon
    // 4. If inside multiline expr AND next line closes it → no semicolon (last arg)
    // 5. Otherwise → add semicolon if needed
    let suppress_semi = this_line_ends_with_continuation
        || is_return_expr
        || next_line_is_method_chain
        || (inside_multiline_expr && next_line_closes_expr);
    
    let should_add_semi = !suppress_semi && needs_semicolon(&transformed);
    
    if should_add_semi {
        format!("{}{};", leading_ws, transformed)
    } else {
        format!("{}{}", leading_ws, transformed)
    }
}

/// Process tuple destructuring assignment
/// Pattern: `(a, b) = value` → `let (a, b) = value;`
///
/// CRITICAL FIX (Bug #1): Must respect method chain continuation!
/// When the next line is `.method(...)`, we must NOT add semicolon.
/// Example:
///   `(phrase, secret) = mnemonic::generate_mnemonic()`
///       `.expect("...")`
/// Must become:
///   `let (phrase, secret) = mnemonic::generate_mnemonic()`  ← NO semicolon
///       `.expect("...");`                                    ← semicolon here
pub fn process_tuple_destructuring(
    trimmed: &str,
    leading_ws: &str,
    current_fn_ctx: &CurrentFunctionContext,
    fn_registry: &FunctionRegistry,
    next_line_is_method_chain: bool,
    inside_multiline_expr: bool,
    next_line_closes_expr: bool,
) -> Option<String> {
    if !trimmed.starts_with('(') || !trimmed.contains(')') || !trimmed.contains('=') {
        return None;
    }
    
    // Find the closing paren and check if = follows
    let paren_close = trimmed.find(')')?;
    let after_paren = trimmed[paren_close + 1..].trim();
    
    if !after_paren.starts_with('=') || after_paren.starts_with("==") || after_paren.starts_with("=>") {
        return None;
    }
    
    let tuple_part = &trimmed[..=paren_close];
    let value_part = after_paren[1..].trim().trim_end_matches(';');
    
    // Verify it's a valid tuple pattern
    if !crate::helpers::is_tuple_pattern(tuple_part) {
        return None;
    }
    
    // Transform value
    let mut expanded_value = expand_value(value_part, None);
    expanded_value = transform_array_access_clone(&expanded_value);
    if current_fn_ctx.is_inside() {
        expanded_value = transform_string_concat(&expanded_value, current_fn_ctx);
    }
    expanded_value = transform_call_args(&expanded_value, fn_registry);
    
    // CRITICAL FIX (Bug #1): Semicolon suppression for method chain continuation
    // If the next line starts with `.expect(...)`, `.map(...)`, etc., the expression
    // continues on the next line and we must NOT insert a semicolon here.
    let suppress_semi = next_line_is_method_chain
        || ends_with_continuation_operator(&expanded_value)
        || (inside_multiline_expr && next_line_closes_expr);
    let semi = if suppress_semi { "" } else { ";" };
    
    Some(format!("{}let {} = {}{}", leading_ws, tuple_part, expanded_value, semi))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_tuple_destructuring() {
        let fn_ctx = CurrentFunctionContext::new();
        let fn_registry = FunctionRegistry::new();
        
        let result = process_tuple_destructuring(
            "(a, b) = foo()",
            "    ",
            &fn_ctx,
            &fn_registry,
            false, // next_line_is_method_chain
            false, // inside_multiline_expr
            false, // next_line_closes_expr
        );
        
        assert!(result.is_some());
        let output = result.unwrap();
        assert!(output.contains("let (a, b)"));
        assert!(output.contains("foo()"));
        assert!(output.ends_with(";"), "Should have semicolon when no chain: {}", output);
    }
    
    #[test]
    fn test_tuple_destructuring_method_chain() {
        // Bug #1 regression test:
        // (phrase, secret) = mnemonic::generate_mnemonic()
        //     .expect("BIP39 mnemonic generation should not fail")
        // When next line is .expect(...), must NOT add semicolon!
        let fn_ctx = CurrentFunctionContext::new();
        let fn_registry = FunctionRegistry::new();
        
        let result = process_tuple_destructuring(
            "(phrase, secret) = mnemonic::generate_mnemonic()",
            "        ",
            &fn_ctx,
            &fn_registry,
            true,  // next_line_is_method_chain = .expect(...)
            false,
            false,
        );
        
        assert!(result.is_some());
        let output = result.unwrap();
        assert!(output.contains("let (phrase, secret) = mnemonic::generate_mnemonic()"));
        assert!(!output.ends_with(";"), "Must NOT have semicolon when chain continues: {}", output);
    }
    
    #[test]
    fn test_not_tuple_destructuring() {
        let fn_ctx = CurrentFunctionContext::new();
        let fn_registry = FunctionRegistry::new();
        
        // Not a tuple pattern
        assert!(process_tuple_destructuring(
            "x = 1", "", &fn_ctx, &fn_registry,
            false, false, false,
        ).is_none());
        
        // Arrow, not assignment
        assert!(process_tuple_destructuring(
            "(x) => y", "", &fn_ctx, &fn_registry,
            false, false, false,
        ).is_none());
    }
}