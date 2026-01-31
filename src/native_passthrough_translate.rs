//! Native Passthrough Translation
//!
//! Detects and handles lines that are already in native Rust syntax
//! and should pass through with minimal transformation.
//!
//! This includes:
//! - `let` statements
//! - `use` imports
//! - `mod` declarations
//! - `impl` blocks
//! - `trait` definitions
//! - `type` aliases
//! - Comments
//! - Attributes
//! - Control flow keywords
//! - `where` clauses and trait bounds

use crate::function::{
    CurrentFunctionContext, transform_string_concat, transform_call_args, 
    should_be_tail_return, FunctionRegistry,
};
use crate::control_flow::transform_enum_struct_init;
use crate::helpers::needs_semicolon;

/// Check if a line is native Rust syntax that should pass through
pub fn is_rust_native_line(trimmed: &str) -> bool {
    // NOTE: const and static are NOT included here because RustS+ uses different syntax:
    // RustS+: `const NAME TYPE = VALUE` (no colon)
    // Rust:   `const NAME: TYPE = VALUE;` (has colon)
    // These are handled separately by transform_const_or_static()
    
    // CRITICAL FIX: `where` clause and trait bounds should pass through unchanged
    // These appear after function signatures with generic constraints
    if trimmed == "where" || trimmed.starts_with("where ") {
        return true;
    }
    
    // Trait bounds in where clauses: `T: Trait,` or `F1: FnMut(...) -> Type,`
    // These have `:` but no `=`, so they're not variable declarations
    // Pattern: identifier `:` type (no `=`)
    if !trimmed.contains('=') && trimmed.contains(':') && !trimmed.contains("::") {
        // Check if it looks like a trait bound: starts with uppercase identifier followed by `:`
        let first_colon = trimmed.find(':').unwrap();
        let before_colon = trimmed[..first_colon].trim();
        if !before_colon.is_empty() {
            let first_char = before_colon.chars().next().unwrap();
            // Trait bound identifiers typically start with uppercase
            // Or could be 'impl' keyword for impl Trait bounds
            if first_char.is_uppercase() || before_colon.starts_with("impl ") {
                return true;
            }
        }
    }
    
    trimmed.starts_with("let ")
        || trimmed.starts_with("use ")
        || trimmed.starts_with("mod ")
        || trimmed.starts_with("impl ")
        || trimmed.starts_with("trait ")
        || trimmed.starts_with("type ")
        || trimmed.starts_with("//")
        || trimmed.starts_with("/*")
        || trimmed.starts_with("*")
        || trimmed.starts_with('#')
        || trimmed == "{"
        || trimmed == "}"
        || trimmed.starts_with("if ")
        || trimmed.starts_with("else")
        || trimmed.starts_with("for ")
        || trimmed.starts_with("while ")
        || trimmed.starts_with("loop")
        || trimmed.starts_with("match ")
        || trimmed.starts_with("return ")
        || trimmed.starts_with("break")
        || trimmed.starts_with("continue")
        || trimmed.starts_with("pub ")
}

/// Process a native Rust line with minimal transformation
pub fn process_native_line(
    trimmed: &str,
    leading_ws: &str,
    current_fn_ctx: &CurrentFunctionContext,
    fn_registry: &FunctionRegistry,
    is_before_closing_brace: bool,
) -> String {
    let mut transformed = trimmed.to_string();
    
    // Apply function context transformations if inside function
    if current_fn_ctx.is_inside() {
        transformed = transform_string_concat(&transformed, current_fn_ctx);
        transformed = transform_call_args(&transformed, fn_registry);
    }
    
    // Transform enum struct init patterns
    transformed = transform_enum_struct_init(&transformed);
    
    // Check if this is a return expression
    let is_return_expr = should_be_tail_return(&transformed, current_fn_ctx, is_before_closing_brace);
    
    // Add semicolon if needed
    if needs_semicolon(&transformed) && !is_return_expr {
        format!("{}{};", leading_ws, transformed)
    } else {
        format!("{}{}", leading_ws, transformed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rust_native_detection() {
        assert!(is_rust_native_line("let x = 1"));
        assert!(is_rust_native_line("use std::io"));
        assert!(is_rust_native_line("mod tests"));
        assert!(is_rust_native_line("impl Foo for Bar"));
        assert!(is_rust_native_line("trait MyTrait"));
        assert!(is_rust_native_line("type Alias = i32"));
        assert!(is_rust_native_line("// comment"));
        assert!(is_rust_native_line("#[derive(Debug)]"));
        assert!(is_rust_native_line("if x > 0"));
        assert!(is_rust_native_line("for i in 0..10"));
        assert!(is_rust_native_line("pub struct Foo"));
    }
    
    #[test]
    fn test_where_clause_detection() {
        assert!(is_rust_native_line("where"));
        assert!(is_rust_native_line("where T: Clone"));
    }
    
    #[test]
    fn test_trait_bound_detection() {
        assert!(is_rust_native_line("T: Clone,"));
        assert!(is_rust_native_line("F1: FnMut(&Address) -> u128,"));
    }
    
    #[test]
    fn test_not_native() {
        // RustS+ assignment - not native
        assert!(!is_rust_native_line("x = 1"));
        assert!(!is_rust_native_line("config Config = Config {}"));
    }
}