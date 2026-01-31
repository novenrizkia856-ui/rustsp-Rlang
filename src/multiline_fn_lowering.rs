//! Multi-line Function Signature Lowering
//!
//! Handles accumulation and processing of multi-line function signatures.
//! 
//! RustS+ allows function signatures to span multiple lines:
//! ```text
//! pub fn validate_stateful[F1, F2](
//!     &self,
//!     mut get_balance F1,
//!     mut get_nonce F2
//! ) effects(panic) Result[()]
//! where
//!     F1: FnMut(&Address) -> u128,
//! {
//!     // body
//! }
//! ```

use crate::helpers::strip_inline_comment;
use crate::function::{
    parse_function_line, signature_to_rust_with_where, FunctionParseResult,
    CurrentFunctionContext,
};
use crate::lookahead_lowering::check_next_line_is_where;

/// Result of processing a multi-line function signature accumulation
pub enum MultilineFnResult {
    /// Still accumulating - need more lines
    Continue,
    /// Signature complete - here's the output
    Complete {
        output: String,
        has_body: bool,
    },
}

/// Check if a line starts a multi-line function signature
/// 
/// Returns true if line starts with `fn ` or `pub fn ` and has unbalanced parentheses
pub fn is_multiline_fn_start(trimmed: &str) -> bool {
    if !trimmed.starts_with("fn ") && !trimmed.starts_with("pub fn ") {
        return false;
    }
    if !trimmed.contains('(') {
        return false;
    }
    
    let paren_opens = trimmed.matches('(').count();
    let paren_closes = trimmed.matches(')').count();
    
    paren_opens > paren_closes
}

/// Process accumulated multi-line function signature
/// 
/// # Arguments
/// * `acc` - Accumulated signature string
/// * `lines` - All source lines for look-ahead
/// * `line_num` - Current line number
/// * `leading_ws` - Leading whitespace for output
/// * `current_fn_ctx` - Function context to update
/// * `brace_depth` - Current brace depth
/// 
/// # Returns
/// `MultilineFnResult` indicating whether to continue accumulating or the signature is complete
pub fn process_multiline_fn_signature(
    acc: &str,
    lines: &[&str],
    line_num: usize,
    leading_ws: &str,
    current_fn_ctx: &mut CurrentFunctionContext,
    brace_depth: usize,
) -> MultilineFnResult {
    let paren_opens = acc.matches('(').count();
    let paren_closes = acc.matches(')').count();
    
    // Check if signature is complete
    let parens_balanced = paren_opens == paren_closes;
    let has_body = acc.ends_with('{');
    
    // CRITICAL FIX: Look ahead to check if next line is a `where` clause
    let next_line_is_where = check_next_line_is_where(lines, line_num);
    
    // CRITICAL FIX: Trait methods don't have `{` at the end
    // Detect trait method: balanced parens + no `{` + not ending with continuation
    // BUT: If next line is `where`, this is NOT a trait method - it has a body after the where!
    let is_trait_method = parens_balanced && !has_body && 
        !acc.trim().ends_with(',') && 
        !acc.trim().ends_with('(') &&
        !acc.trim().ends_with('[') &&
        !acc.trim().ends_with('+') &&
        !next_line_is_where;
    
    // Signature is complete when: has body, OR is trait method, OR has where clause following
    let signature_complete = parens_balanced && (has_body || is_trait_method || next_line_is_where);
    
    if !signature_complete {
        return MultilineFnResult::Continue;
    }
    
    // Update function context if this has a body
    if has_body {
        if let FunctionParseResult::RustSPlusSignature(ref sig) = parse_function_line(acc) {
            current_fn_ctx.enter(sig, brace_depth + 1);
        }
    }
    
    // Generate output
    let output = match parse_function_line(acc) {
        FunctionParseResult::RustSPlusSignature(sig) => {
            if is_trait_method {
                // Trait method declaration - add semicolon
                format!("{}{};", leading_ws, signature_to_rust_with_where(&sig, true))
            } else {
                // Function with where clause or regular function
                format!("{}{}", leading_ws, signature_to_rust_with_where(&sig, next_line_is_where))
            }
        }
        FunctionParseResult::RustPassthrough => {
            format!("{}{}", leading_ws, acc)
        }
        FunctionParseResult::Error(e) => {
            format!("{}// COMPILE ERROR: {}\n{}{}", leading_ws, e, leading_ws, acc)
        }
        FunctionParseResult::NotAFunction => {
            format!("{}{}", leading_ws, acc)
        }
    };
    
    MultilineFnResult::Complete { output, has_body }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_is_multiline_fn_start() {
        assert!(is_multiline_fn_start("pub fn foo("));
        assert!(is_multiline_fn_start("fn bar(x: i32,"));
        assert!(!is_multiline_fn_start("fn baz() {"));
        assert!(!is_multiline_fn_start("let x = 1"));
    }
}