//! Match Mode Lowering
//!
//! Handles match expression parsing in RustS+.
//!
//! Features:
//! - Match arm pattern detection
//! - Multi-pattern arm handling (patterns joined with |)
//! - Single-line arm transformation
//! - Match arm body processing

use crate::control_flow::{
    MatchModeStack, 
    is_match_arm_pattern, is_single_line_arm, is_multi_pattern_continuation,
    transform_arm_pattern, transform_arm_close_with_parens,
    transform_single_line_arm, transform_multi_pattern_line,
};
use crate::clone_helpers::extract_arm_pattern;
use crate::lookahead_lowering::detect_arm_has_if_expr;
use crate::function::CurrentFunctionContext;

/// Result of processing a line in match mode
pub enum MatchModeResult {
    /// Line was handled by match mode
    Handled(String),
    /// Should process as match arm body
    ProcessAsArmBody,
    /// Line was not for match mode (or match just started)
    NotHandled,
}

/// Process match closing brace
fn process_match_close(
    clean_line: &str,
    leading_ws: &str,
    brace_depth: usize,
    match_mode: &mut MatchModeStack,
) -> Option<String> {
    // Check if exiting arm body
    if match_mode.should_exit_arm(brace_depth) {
        let uses_parens = match_mode.arm_uses_parens();
        match_mode.exit_arm_body();
        return Some(transform_arm_close_with_parens(clean_line, uses_parens));
    }
    
    // Check if exiting match entirely
    if match_mode.should_exit_match(brace_depth) {
        let needs_semi = match_mode.current_is_assignment();
        match_mode.exit_match();
        let suffix = if needs_semi { ";" } else { "" };
        return Some(format!("{}}}{}", leading_ws, suffix));
    }
    
    None
}

/// Process multi-pattern continuation line (starts with |)
fn process_multi_pattern_continuation(
    clean_line: &str,
    trimmed: &str,
    current_fn_ctx: &CurrentFunctionContext,
    match_mode: &MatchModeStack,
) -> Option<String> {
    if !match_mode.expecting_arm_pattern() {
        return None;
    }
    
    if !is_multi_pattern_continuation(trimmed) {
        return None;
    }
    
    let ret_type = current_fn_ctx.return_type.as_deref();
    Some(transform_multi_pattern_line(clean_line, ret_type))
}

/// Process first pattern in multi-pattern sequence
/// 
/// Detection: current line ends with `}` (struct destruct), next line starts with `|`
fn process_first_multi_pattern(
    line: &str,
    match_mode: &MatchModeStack,
    next_line_starts_with_pipe: bool,
) -> Option<String> {
    if !match_mode.expecting_arm_pattern() {
        return None;
    }
    
    if !next_line_starts_with_pipe {
        return None;
    }
    
    // This is the first pattern in a multi-pattern arm
    // DO NOT transform, pass through as-is (no => {)
    Some(line.to_string())
}

/// Process regular single-pattern match arm
fn process_regular_arm_pattern(
    clean_line: &str,
    trimmed: &str,
    leading_ws: &str,
    lines: &[&str],
    line_num: usize,
    prev_depth: usize,
    opens: usize,
    brace_depth: usize,
    current_fn_ctx: &CurrentFunctionContext,
    match_mode: &mut MatchModeStack,
) -> Option<String> {
    if !match_mode.expecting_arm_pattern() {
        return None;
    }
    
    if !is_match_arm_pattern(trimmed) {
        return None;
    }
    
    // Check for single-line arm first
    if is_single_line_arm(trimmed) {
        let ret_type = current_fn_ctx.return_type.as_deref();
        return Some(transform_single_line_arm(clean_line, ret_type));
    }
    
    // Multi-line arm pattern
    let arm_has_if_expr = detect_arm_has_if_expr(lines, line_num, prev_depth + opens);
    
    let output = if arm_has_if_expr {
        let pattern = extract_arm_pattern(trimmed);
        format!("{}{} =>", leading_ws, pattern)
    } else {
        transform_arm_pattern(clean_line)
    };
    
    match_mode.enter_arm_body(brace_depth, arm_has_if_expr);
    
    Some(output)
}

/// Process a line that might be part of match mode
pub fn process_match_mode_line(
    line: &str,
    trimmed: &str,
    clean_line: &str,
    leading_ws: &str,
    lines: &[&str],
    line_num: usize,
    brace_depth: usize,
    prev_depth: usize,
    opens: usize,
    next_line_starts_with_pipe: bool,
    current_fn_ctx: &CurrentFunctionContext,
    match_mode: &mut MatchModeStack,
) -> MatchModeResult {
    // Check for closing brace
    if match_mode.is_active() && trimmed == "}" {
        if let Some(result) = process_match_close(clean_line, leading_ws, brace_depth, match_mode) {
            return MatchModeResult::Handled(result);
        }
    }
    
    // Handle multi-pattern continuation lines (starting with |)
    if let Some(result) = process_multi_pattern_continuation(clean_line, trimmed, current_fn_ctx, match_mode) {
        return MatchModeResult::Handled(result);
    }
    
    // Handle FIRST pattern in multi-pattern sequence
    if let Some(result) = process_first_multi_pattern(line, match_mode, next_line_starts_with_pipe) {
        return MatchModeResult::Handled(result);
    }
    
    // Handle regular single-pattern arms
    if let Some(result) = process_regular_arm_pattern(
        clean_line, trimmed, leading_ws, lines, line_num,
        prev_depth, opens, brace_depth, current_fn_ctx, match_mode,
    ) {
        return MatchModeResult::Handled(result);
    }
    
    // If in arm body, signal to process as arm body
    if match_mode.in_arm_body() {
        return MatchModeResult::ProcessAsArmBody;
    }
    
    MatchModeResult::NotHandled
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_multi_pattern_continuation() {
        assert!(is_multi_pattern_continuation("| Pattern2 { x }"));
        assert!(is_multi_pattern_continuation("| Pattern3 { x } { body }"));
        assert!(!is_multi_pattern_continuation("Pattern1 { x }"));
    }
}