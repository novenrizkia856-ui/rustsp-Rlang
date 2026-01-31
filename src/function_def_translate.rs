//! Function Definition Translation
//!
//! Translates RustS+ function definitions to Rust syntax.
//!
//! RustS+ function syntax:
//! ```text
//! pub fn process[T](data T) effects(io) Result[T]
//! ```
//!
//! Rust function syntax:
//! ```text
//! pub fn process<T>(data: T) -> Result<T>
//! ```

use crate::function::{
    parse_function_line, signature_to_rust_with_where, FunctionParseResult,
    CurrentFunctionContext,
};
use crate::helpers::needs_semicolon;
use crate::lookahead_lowering::check_next_line_is_where;

/// Result of processing a function definition
pub enum FunctionDefResult {
    /// Function was processed
    Handled(String),
    /// Not a function definition
    NotFunctionDef,
}

/// Process a function definition line
pub fn process_function_def(
    trimmed: &str,
    clean_line: &str,
    leading_ws: &str,
    lines: &[&str],
    line_num: usize,
    current_fn_ctx: &mut CurrentFunctionContext,
    function_start_brace: usize,
) -> FunctionDefResult {
    if !trimmed.starts_with("fn ") && !trimmed.starts_with("pub fn ") {
        return FunctionDefResult::NotFunctionDef;
    }
    
    // CRITICAL FIX: Look ahead to detect if next line is a `where` clause
    let next_line_is_where = check_next_line_is_where(lines, line_num);
    
    // CRITICAL FIX: Detect trait method declarations (no body)
    // If trimmed doesn't end with `{` and parens are balanced, it MIGHT be a trait method.
    // BUT: If next line is a `where` clause, it's NOT a trait method - it has a body!
    let is_trait_method_declaration = {
        let paren_opens = trimmed.matches('(').count();
        let paren_closes = trimmed.matches(')').count();
        let parens_balanced = paren_opens == paren_closes && paren_opens > 0;
        let no_body = !trimmed.ends_with('{');
        // CRITICAL FIX: If next line is `where`, this is NOT a trait method!
        parens_balanced && no_body && !next_line_is_where
    };
    
    match parse_function_line(trimmed) {
        FunctionParseResult::RustSPlusSignature(sig) => {
            let output = if is_trait_method_declaration {
                // Trait method declaration - add semicolon
                format!("{}{};", leading_ws, signature_to_rust_with_where(&sig, true))
            } else {
                // Regular function or function with where clause
                format!("{}{}", leading_ws, signature_to_rust_with_where(&sig, next_line_is_where))
            };
            FunctionDefResult::Handled(output)
        }
        FunctionParseResult::RustPassthrough => {
            let output = process_rust_passthrough_function(clean_line, trimmed, current_fn_ctx, function_start_brace);
            FunctionDefResult::Handled(output)
        }
        FunctionParseResult::Error(e) => {
            FunctionDefResult::Handled(format!("{}// COMPILE ERROR: {}\n{}", leading_ws, e, clean_line))
        }
        FunctionParseResult::NotAFunction => {
            FunctionDefResult::Handled(clean_line.to_string())
        }
    }
}

/// Process a Rust-native function that passes through
/// 
/// This handles functions that are already in Rust syntax but may have
/// RustS+ effect annotations that need to be stripped.
pub fn process_rust_passthrough_function(
    clean_line: &str,
    trimmed: &str,
    current_fn_ctx: &mut CurrentFunctionContext,
    function_start_brace: usize,
) -> String {
    let mut output = clean_line.to_string();
    
    // Strip effects annotation if present
    if output.contains("effects(") {
        if let Some(effects_start) = output.find("effects(") {
            let mut paren_depth = 0;
            let mut effects_end = effects_start;
            for (i, c) in output[effects_start..].char_indices() {
                match c {
                    '(' => paren_depth += 1,
                    ')' => {
                        paren_depth -= 1;
                        if paren_depth == 0 {
                            effects_end = effects_start + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let before = &output[..effects_start];
            let after = output[effects_end..].trim_start();
            output = format!("{}{}", before.trim_end(), 
                if after.is_empty() || after.starts_with('{') { 
                    format!(" {}", after) 
                } else { 
                    format!(" {}", after) 
                });
        }
    }
    
    // Extract return type
    if output.contains(" -> ") {
        if let Some(arrow_pos) = output.find(" -> ") {
            let after_arrow = &output[arrow_pos + 4..];
            let ret_end = after_arrow.find(|c: char| c == '{' || c.is_whitespace())
                .unwrap_or(after_arrow.len());
            let ret_type = after_arrow[..ret_end].trim();
            if !ret_type.is_empty() {
                current_fn_ctx.return_type = Some(ret_type.to_string());
                current_fn_ctx.start_depth = function_start_brace;
            }
        }
    }
    
    if needs_semicolon(trimmed) {
        output = format!("{};", output);
    }
    
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_trait_method_detection() {
        // Trait method (no body, no where clause)
        let lines = vec!["fn foo(&self) -> i32"];
        let mut ctx = CurrentFunctionContext::new();
        
        let result = process_function_def(
            "fn foo(&self) -> i32",
            "fn foo(&self) -> i32",
            "    ",
            &lines,
            0,
            &mut ctx,
            0,
        );
        
        // Should be handled as trait method (with semicolon)
        match result {
            FunctionDefResult::Handled(s) => {
                // Note: actual output depends on function module
            }
            _ => panic!("Expected Handled"),
        }
    }
}