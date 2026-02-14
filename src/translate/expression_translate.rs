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
    transform_string_concat, transform_call_args_with_ctx, should_be_tail_return,
};
use crate::control_flow::transform_enum_struct_init;
use crate::clone_helpers::transform_array_access_clone;
use crate::helpers::{ends_with_continuation_operator, needs_semicolon};
use crate::transform_literal::is_string_literal;
use crate::translate::assignment_translate::parse_var_type_annotation;
use crate::type_resolution::{TypeResolutionPass, has_explicit_result_handling};

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
            expanded_value = transform_call_args_with_ctx(&expanded_value, fn_registry, Some(current_fn_ctx));
            
            return format!("{}let mut {}{} = {};", leading_ws, var_name, type_annotation, expanded_value);
        }
    }
    
    // Apply transformations
    if current_fn_ctx.is_inside() {
        transformed = transform_string_concat(&transformed, current_fn_ctx);
    }
    transformed = transform_call_args_with_ctx(&transformed, fn_registry, Some(current_fn_ctx));
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
pub fn process_tuple_destructuring(
    trimmed: &str,
    leading_ws: &str,
    current_fn_ctx: &CurrentFunctionContext,
    fn_registry: &FunctionRegistry,
    type_resolution: &TypeResolutionPass,
    next_line_is_method_chain: bool,
) -> Option<String> {
    let (expr, had_let_prefix) = if let Some(rest) = trimmed.strip_prefix("let ") {
        (rest.trim_start(), true)
    } else {
        (trimmed, false)
    };

    if !expr.starts_with('(') || !expr.contains(')') || !expr.contains('=') {
        return None;
    }
    
    // Find the closing paren and check if = follows
    let paren_close = expr.find(')')?;
    let after_paren = expr[paren_close + 1..].trim();
    
    if !after_paren.starts_with('=') || after_paren.starts_with("==") || after_paren.starts_with("=>") {
        return None;
    }
    
    let tuple_part = &expr[..=paren_close];
    let value_part = after_paren[1..].trim().trim_end_matches(';');
    
    // Verify it's a valid tuple pattern
    if !crate::helpers::is_tuple_pattern(tuple_part) {
        return None;
    }

    // Type-aware Result tuple destructuring check
    if type_resolution.return_type_is_result(value_part) && !has_explicit_result_handling(value_part) && !next_line_is_method_chain {
        return Some(format!(
            "{}compile_error!(\"RustS+ semantic error: tuple destructuring from Result requires .expect(...), .unwrap(...), or ?\");",
            leading_ws
        ));
    }
    
    // Transform value
    let mut expanded_value = expand_value(value_part, None);
    expanded_value = transform_array_access_clone(&expanded_value);
    if current_fn_ctx.is_inside() {
        expanded_value = transform_string_concat(&expanded_value, current_fn_ctx);
    }
    expanded_value = transform_call_args_with_ctx(&expanded_value, fn_registry, Some(current_fn_ctx));
    
    let semi = if next_line_is_method_chain { "" } else { ";" };
    if had_let_prefix {
        Some(format!("{}let {} = {}{}", leading_ws, tuple_part, expanded_value, semi))
    } else {
        Some(format!("{}let {} = {}{}", leading_ws, tuple_part, expanded_value, semi))
    }
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
            &TypeResolutionPass::new(fn_registry.clone()),
            false,
        );
        
        assert!(result.is_some());
        let output = result.unwrap();
        assert!(output.contains("let (a, b)"));
        assert!(output.contains("foo()"));
    }
    
    #[test]
    fn test_not_tuple_destructuring() {
        let fn_ctx = CurrentFunctionContext::new();
        let fn_registry = FunctionRegistry::new();
        
        // Not a tuple pattern
        assert!(process_tuple_destructuring(
            "x = 1",
            "",
            &fn_ctx,
            &fn_registry,
            &TypeResolutionPass::new(fn_registry.clone()),
            false,
        ).is_none());
        
        // Arrow, not assignment
        assert!(process_tuple_destructuring(
            "(x) => y",
            "",
            &fn_ctx,
            &fn_registry,
            &TypeResolutionPass::new(fn_registry.clone()),
            false,
        ).is_none());
    }
}