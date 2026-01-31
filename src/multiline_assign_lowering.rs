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
use crate::assignment_translate::process_assignment;
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
        // Fallback: output as-is
        format!("{}{}", leading_ws, complete_assign)
    }
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
}