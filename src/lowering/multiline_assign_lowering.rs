//! Multi-line Assignment Lowering
//!
//! Handles accumulation of multi-line assignments in RustS+.
//! 
//! When an assignment ends with `=`, we need to join it with the next line:
//! ```text
//! mut x Type =
//!     value
//! ```
//! Should become: `mut x Type = value`

use crate::variable::parse_rusts_assignment_ext;
use crate::helpers;
use crate::translate::assignment_translate::process_assignment;
use crate::scope::ScopeAnalyzer;
use crate::variable::VariableTracker;
use crate::function::{CurrentFunctionContext, FunctionRegistry};

/// Check if a line ends with `=` (indicating multi-line assignment start)
/// 
/// Must not match `==`, `!=`, `<=`, `>=`, or `=>`
pub fn is_multiline_assign_start(trimmed: &str) -> bool {
    if !trimmed.contains('=') || trimmed.contains("==") {
        return false;
    }
    
    trimmed.ends_with('=') 
        && !trimmed.ends_with("==") 
        && !trimmed.ends_with("!=") 
        && !trimmed.ends_with("<=") 
        && !trimmed.ends_with(">=") 
        && !trimmed.ends_with("=>")
}

/// Check if accumulated assignment is complete (doesn't end with `=` anymore)
pub fn is_multiline_assign_complete(acc: &str) -> bool {
    let trimmed = acc.trim();
    
    if !trimmed.ends_with('=') {
        return true;
    }
    
    // Check for comparison/arrow operators
    trimmed.ends_with("==") 
        || trimmed.ends_with("!=") 
        || trimmed.ends_with("<=") 
        || trimmed.ends_with(">=") 
        || trimmed.ends_with("=>")
}

/// Process completed multi-line assignment
/// 
/// # Arguments
/// * `complete_assign` - The complete accumulated assignment
/// * `leading_ws` - Leading whitespace
/// * `line_num` - Current line number
/// * `scope_analyzer` - Scope analyzer reference
/// * `tracker` - Variable tracker reference
/// * `current_fn_ctx` - Current function context
/// * `fn_registry` - Function registry
/// * `inside_multiline_expr` - Whether inside a multiline expression
/// * `next_line_is_method_chain` - Whether next line starts with `.`
/// * `next_line_closes_expr` - Whether next line closes an expression
/// * `prev_line_was_continuation` - Mutable reference to continuation tracking
/// 
/// # Returns
/// The transformed assignment as a String
pub fn process_complete_multiline_assign(
    complete_assign: &str,
    leading_ws: &str,
    line_num: usize,
    scope_analyzer: &ScopeAnalyzer,
    tracker: &VariableTracker,
    current_fn_ctx: &CurrentFunctionContext,
    fn_registry: &FunctionRegistry,
    inside_multiline_expr: bool,
    next_line_is_method_chain: bool,
    next_line_closes_expr: bool,
    prev_line_was_continuation: &mut bool,
) -> String {
    if let Some((var_name, var_type, value, is_outer, is_explicit_mut)) = parse_rusts_assignment_ext(complete_assign) {
        // Transform generic brackets in type
        let transformed_type = var_type.map(|t| helpers::transform_generic_brackets(&t));
        
        process_assignment(
            &var_name, 
            transformed_type.as_deref(), 
            &value, 
            is_outer, 
            is_explicit_mut,
            line_num, 
            leading_ws, 
            scope_analyzer, 
            tracker, 
            current_fn_ctx, 
            fn_registry,
            inside_multiline_expr, 
            next_line_is_method_chain, 
            next_line_closes_expr, 
            prev_line_was_continuation,
        )
    } else {
        // CRITICAL FIX: Check for tuple destructuring pattern
        // Pattern: `(a, b, c) = value` should become `let (a, b, c) = value;`
        // This is NOT handled by parse_rusts_assignment_ext because it rejects
        // left-hand sides containing `(`
        if let Some(output) = try_process_tuple_destructuring(complete_assign, leading_ws, current_fn_ctx, fn_registry) {
            return output;
        }
        
        // Fallback: output as-is
        format!("{}{}", leading_ws, complete_assign)
    }
}

/// Try to process tuple destructuring pattern
/// 
/// Pattern: `(a, b, c) = value` → `let (a, b, c) = value;`
/// 
/// This handles cases where `parse_rusts_assignment_ext` rejects the line
/// because the left-hand side contains `(` (tuple pattern).
fn try_process_tuple_destructuring(
    complete_assign: &str,
    leading_ws: &str,
    current_fn_ctx: &CurrentFunctionContext,
    fn_registry: &FunctionRegistry,
) -> Option<String> {
    let trimmed = complete_assign.trim();
    
    // Must start with `(` for tuple pattern
    if !trimmed.starts_with('(') {
        return None;
    }
    
    // Find the closing paren of the tuple pattern
    let paren_close = find_matching_paren(trimmed)?;
    
    // After the closing paren, there should be `=` (with optional whitespace)
    let after_paren = trimmed[paren_close + 1..].trim();
    
    // Must be assignment `=`, not `==` or `=>`
    if !after_paren.starts_with('=') || after_paren.starts_with("==") || after_paren.starts_with("=>") {
        return None;
    }
    
    let tuple_part = &trimmed[..=paren_close];
    let value_part = after_paren[1..].trim().trim_end_matches(';');
    
    // Verify it's a valid tuple pattern (has comma-separated identifiers)
    if !helpers::is_tuple_pattern(tuple_part) {
        return None;
    }
    
    // Transform value using standard transformations
    use crate::variable::expand_value;
    use crate::clone_helpers::transform_array_access_clone;
    use crate::function::{transform_string_concat, transform_call_args};
    
    let mut expanded_value = expand_value(value_part, None);
    expanded_value = transform_array_access_clone(&expanded_value);
    if current_fn_ctx.is_inside() {
        expanded_value = transform_string_concat(&expanded_value, current_fn_ctx);
    }
    expanded_value = transform_call_args(&expanded_value, fn_registry);
    
    Some(format!("{}let {} = {};", leading_ws, tuple_part, expanded_value))
}

/// Find the position of the matching closing parenthesis
/// Returns the index of `)` that matches the opening `(` at position 0
fn find_matching_paren(s: &str) -> Option<usize> {
    if !s.starts_with('(') {
        return None;
    }
    
    let mut depth = 0;
    let mut in_string = false;
    let mut prev_char = ' ';
    
    for (i, c) in s.chars().enumerate() {
        // Handle string literals
        if c == '"' && prev_char != '\\' {
            in_string = !in_string;
        }
        
        if !in_string {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        
        prev_char = c;
    }
    
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_is_multiline_assign_start() {
        assert!(is_multiline_assign_start("x ="));
        assert!(is_multiline_assign_start("mut y Type ="));
        assert!(!is_multiline_assign_start("x == y"));
        assert!(!is_multiline_assign_start("x != y"));
        assert!(!is_multiline_assign_start("x => y"));
        assert!(!is_multiline_assign_start("x = 1"));
    }
    
    #[test]
    fn test_is_multiline_assign_complete() {
        assert!(is_multiline_assign_complete("x = 1"));
        assert!(is_multiline_assign_complete("x = foo()"));
        assert!(!is_multiline_assign_complete("x ="));
        assert!(is_multiline_assign_complete("x == y")); // comparison, not assignment
        assert!(is_multiline_assign_complete("x => y")); // arrow, not assignment
    }
    
    #[test]
    fn test_find_matching_paren() {
        assert_eq!(find_matching_paren("(a, b)"), Some(5));
        assert_eq!(find_matching_paren("(a, (b, c))"), Some(10));
        assert_eq!(find_matching_paren("(a, \")\", b)"), Some(10));
        assert_eq!(find_matching_paren("x = 1"), None); // doesn't start with (
        assert_eq!(find_matching_paren("(unclosed"), None);
    }
    
    #[test]
    fn test_try_process_tuple_destructuring_basic() {
        let fn_ctx = CurrentFunctionContext::new();
        let fn_registry = FunctionRegistry::new();
        
        let result = try_process_tuple_destructuring(
            "(a, b, c) = foo()",
            "    ",
            &fn_ctx,
            &fn_registry,
        );
        
        assert!(result.is_some());
        let output = result.unwrap();
        assert!(output.contains("let (a, b, c)"), "Expected 'let (a, b, c)', got: {}", output);
        assert!(output.contains("foo()"), "Expected 'foo()', got: {}", output);
        assert!(output.ends_with(';'), "Expected semicolon at end, got: {}", output);
    }
    
    #[test]
    fn test_try_process_tuple_destructuring_multiline_joined() {
        let fn_ctx = CurrentFunctionContext::new();
        let fn_registry = FunctionRegistry::new();
        
        // This simulates the joined multiline assignment:
        // (validator_slashed, delegators_slashed, total_slashed) =
        //     state.apply_full_slash(validator, SLASH_PERCENTAGE)
        let result = try_process_tuple_destructuring(
            "(validator_slashed, delegators_slashed, total_slashed) = state.apply_full_slash(validator, SLASH_PERCENTAGE)",
            "    ",
            &fn_ctx,
            &fn_registry,
        );
        
        assert!(result.is_some(), "Should handle tuple destructuring");
        let output = result.unwrap();
        assert!(output.contains("let (validator_slashed, delegators_slashed, total_slashed)"), 
            "Should add 'let' prefix, got: {}", output);
        assert!(output.contains("state.apply_full_slash"), 
            "Should preserve method call, got: {}", output);
    }
    
    #[test]
    fn test_try_process_tuple_destructuring_not_tuple() {
        let fn_ctx = CurrentFunctionContext::new();
        let fn_registry = FunctionRegistry::new();
        
        // Not a tuple pattern
        assert!(try_process_tuple_destructuring("x = 1", "", &fn_ctx, &fn_registry).is_none());
        
        // Arrow, not assignment
        assert!(try_process_tuple_destructuring("(x) => y", "", &fn_ctx, &fn_registry).is_none());
        
        // Comparison, not assignment
        assert!(try_process_tuple_destructuring("(x) == y", "", &fn_ctx, &fn_registry).is_none());
    }
}