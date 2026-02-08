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
use crate::lowering::lookahead_lowering::detect_arm_has_if_expr;
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

/// Look-ahead: Check if a match arm starts a multi-line struct destructuring pattern.
///
/// Detects patterns like:
/// ```text
/// DAEvent::NodeRegistered {
///     version,
///     timestamp_ms,
///     node_id,
/// } {
///     // arm body
/// }
/// ```
///
/// Returns true if the `{` on the current line opens a destructuring pattern
/// (not the arm body), and a later `} {` line closes it and opens the body.
fn is_multiline_destructure_start(lines: &[&str], current_line: usize) -> bool {
    let first_trimmed = lines[current_line].trim();
    
    // Must end with `{`
    if !first_trimmed.ends_with('{') {
        return false;
    }
    
    // Count braces on this line - must be exactly 1 unmatched `{`
    let mut depth: i32 = 0;
    for c in first_trimmed.chars() {
        if c == '{' { depth += 1; }
        if c == '}' { depth -= 1; }
    }
    if depth != 1 {
        return false;
    }
    
    // Look ahead for `} {` pattern that closes destructuring and opens body.
    // We track running brace depth starting from 1 (the unmatched `{` above).
    let mut running_depth = depth;
    let limit = lines.len().min(current_line + 50);
    
    for i in (current_line + 1)..limit {
        let t = lines[i].trim();
        
        // Count braces on this line
        let mut line_opens = 0i32;
        let mut line_closes = 0i32;
        for c in t.chars() {
            if c == '{' { line_opens += 1; }
            if c == '}' { line_closes += 1; }
        }
        
        running_depth = running_depth - line_closes + line_opens;
        
        // `} {` pattern: the line has both `}` and ends with `{`,
        // and after processing, depth is back to 1 (one new `{` opened).
        // The `}` must come before the `{` on the line.
        if running_depth == 1 && line_closes > 0 && t.ends_with('{') {
            if let (Some(close_pos), Some(open_pos)) = (t.find('}'), t.rfind('{')) {
                if close_pos < open_pos {
                    return true; // Found `} {` → multi-line destructuring confirmed
                }
            }
        }
        
        // If depth dropped to 0 or below without finding `} {`, not a destructuring
        if running_depth <= 0 {
            return false;
        }
    }
    
    false
}

/// Process a line while inside multi-line destructuring pattern.
///
/// In destructuring mode, field lines are passed through as-is.
/// When `} {` is encountered, it's transformed to `} => {` and arm body begins.
fn process_destructuring_line(
    trimmed: &str,
    leading_ws: &str,
    brace_depth: usize,
    match_mode: &mut MatchModeStack,
) -> Option<MatchModeResult> {
    if !match_mode.in_destructuring() {
        return None;
    }
    
    // Check if this line is `} {` (close destructure, open body)
    if trimmed.ends_with('{') && trimmed.contains('}') {
        if let (Some(close_pos), Some(open_pos)) = (trimmed.find('}'), trimmed.rfind('{')) {
            if close_pos < open_pos {
                // Extract the pattern part before `{` (the closing `}` and anything before it)
                let pattern_part = trimmed[..open_pos].trim();
                match_mode.exit_destructuring();
                match_mode.enter_arm_body(brace_depth, false);
                return Some(MatchModeResult::Handled(
                    format!("{}{} => {{", leading_ws, pattern_part)
                ));
            }
        }
    }
    
    // Regular destructuring field line - pass through as-is
    // These are lines like `version,`, `timestamp_ms,`, etc.
    Some(MatchModeResult::Handled(format!("{}{}", leading_ws, trimmed)))
}

/// Process match closing brace
fn process_match_close(
    clean_line: &str,
    leading_ws: &str,
    brace_depth: usize,
    match_mode: &mut MatchModeStack,
) -> Option<String> {
    // CRITICAL FIX: Check exit conditions BEFORE modifying state
    let should_exit_arm = match_mode.should_exit_arm(brace_depth);
    let should_exit_match = match_mode.should_exit_match(brace_depth);
    
    // Priority 1: Exit arm body (depth < arm_body_depth)
    if should_exit_arm {
        let uses_parens = match_mode.arm_uses_parens();
        match_mode.exit_arm_body();
        return Some(transform_arm_close_with_parens(clean_line, uses_parens));
    }
    
    // Priority 2: Exit match entirely (!in_arm_body && depth <= match_depth)
    if should_exit_match {
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
    
    // CRITICAL FIX: Detect multi-line struct destructuring pattern
    // e.g., `DAEvent::NodeRegistered {` where `{` opens destructuring, not body
    // Look ahead to see if a `} {` line closes destructuring and opens body
    if is_multiline_destructure_start(lines, line_num) {
        match_mode.enter_destructuring();
        // Pass through the line as-is (the `{` is part of the pattern, not body)
        return Some(format!("{}{}", leading_ws, trimmed));
    }
    
    // Multi-line arm pattern (regular - `{` opens body)
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
    
    // CRITICAL FIX: Handle multi-line struct destructuring pattern
    // When inside destructuring, process field lines and detect `} {` to enter body
    if let Some(result) = process_destructuring_line(trimmed, leading_ws, brace_depth, match_mode) {
        return result;
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
    
    #[test]
    fn test_multiline_destructure_detection() {
        // Multi-line destructuring: `Pattern {\n field,\n } {`
        let lines = vec![
            "    match da_event {",
            "    DAEvent::NodeRegistered {",
            "        version,",
            "        timestamp_ms,",
            "        node_id,",
            "    } {",
            "        assert_eq!(version, 1);",
            "    }",
            "    }",
        ];
        
        // Line 1 is `DAEvent::NodeRegistered {` → should detect as destructuring start
        assert!(is_multiline_destructure_start(&lines, 1));
        
        // Line 0 is `match da_event {` → NOT a destructuring start
        assert!(!is_multiline_destructure_start(&lines, 0));
    }
    
    #[test]
    fn test_non_destructure_arm() {
        // Regular arm: `Pattern {` where `{` opens body (no `} {` later)
        let lines = vec![
            "    match x {",
            "    Some(v) {",
            "        do_something(v);",
            "    }",
            "    }",
        ];
        
        // Line 1 is `Some(v) {` → NOT a destructuring (body lines don't have `} {`)
        assert!(!is_multiline_destructure_start(&lines, 1));
    }
    
    #[test]
    fn test_destructuring_processing() {
        let mut match_mode = MatchModeStack::new();
        match_mode.enter_match(1, false);
        match_mode.enter_destructuring();
        
        // Regular field line → pass through
        let result = process_destructuring_line("version,", "        ", 2, &mut match_mode);
        assert!(result.is_some());
        match result.unwrap() {
            MatchModeResult::Handled(s) => assert!(s.contains("version,")),
            _ => panic!("Expected Handled"),
        }
        
        // `} {` line → transform to `} => {` and enter arm body
        let result = process_destructuring_line("} {", "    ", 2, &mut match_mode);
        assert!(result.is_some());
        match result.unwrap() {
            MatchModeResult::Handled(s) => {
                assert!(s.contains("} => {"), "Expected '}} => {{' but got: {}", s);
            }
            _ => panic!("Expected Handled"),
        }
        assert!(match_mode.in_arm_body());
        assert!(!match_mode.in_destructuring());
    }
}