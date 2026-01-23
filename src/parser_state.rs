//! Parser state management for RustS+ transpiler
//!
//! This module contains the ParserState struct that holds all the mode stacks
//! and tracking variables needed during the second pass of transpilation.

use crate::function::CurrentFunctionContext;
use crate::enum_def::EnumParseContext;
use crate::modes::{LiteralModeStack, ArrayModeStack, UseImportMode};
use crate::control_flow::MatchModeStack;

/// Holds all parser state during the second pass of transpilation
pub struct ParserState {
    // Depth tracking
    pub brace_depth: usize,
    pub bracket_depth: usize,
    
    // Function context
    pub in_function_body: bool,
    pub function_start_brace: usize,
    pub current_fn_ctx: CurrentFunctionContext,
    
    // Struct/enum definition contexts
    pub in_struct_def: bool,
    pub struct_def_depth: usize,
    pub enum_ctx: EnumParseContext,
    
    // Mode stacks
    pub literal_mode: LiteralModeStack,
    pub array_mode: ArrayModeStack,
    pub match_mode: MatchModeStack,
    pub use_import_mode: UseImportMode,
    
    // If expression assignment tracking
    pub if_expr_assignment_depth: Option<usize>,
    
    // Multi-line function signature accumulation
    pub multiline_fn_acc: Option<String>,
    pub multiline_fn_leading_ws: String,
    
    // Expression continuation tracking
    pub prev_line_was_continuation: bool,
    
    // Multiline expression depth tracking
    pub multiline_expr_depth: i32,
}

impl ParserState {
    /// Create a new parser state with all fields initialized
    pub fn new() -> Self {
        ParserState {
            brace_depth: 0,
            bracket_depth: 0,
            in_function_body: false,
            function_start_brace: 0,
            current_fn_ctx: CurrentFunctionContext::new(),
            in_struct_def: false,
            struct_def_depth: 0,
            enum_ctx: EnumParseContext::new(),
            literal_mode: LiteralModeStack::new(),
            array_mode: ArrayModeStack::new(),
            match_mode: MatchModeStack::new(),
            use_import_mode: UseImportMode::new(),
            if_expr_assignment_depth: None,
            multiline_fn_acc: None,
            multiline_fn_leading_ws: String::new(),
            prev_line_was_continuation: false,
            multiline_expr_depth: 0,
        }
    }
    
    /// Update multiline expression depth based on line content
    /// Returns (depth_before, inside_multiline_expr)
    pub fn update_multiline_depth(&mut self, trimmed: &str) -> (i32, bool) {
        // Save depth BEFORE processing this line
        let multiline_depth_before = self.multiline_expr_depth;
        
        // Update depth based on this line's parens/brackets
        {
            let mut in_string = false;
            let mut escape_next = false;
            for c in trimmed.chars() {
                if escape_next {
                    escape_next = false;
                    continue;
                }
                if c == '\\' && in_string {
                    escape_next = true;
                    continue;
                }
                if c == '"' {
                    in_string = !in_string;
                    continue;
                }
                if !in_string {
                    match c {
                        '(' | '[' => self.multiline_expr_depth += 1,
                        ')' | ']' => self.multiline_expr_depth -= 1,
                        _ => {}
                    }
                }
            }
            // Ensure depth doesn't go negative (defensive)
            if self.multiline_expr_depth < 0 {
                self.multiline_expr_depth = 0;
            }
        }
        
        // CRITICAL FIX: Skip semicolon ONLY if we're STILL inside a multiline expression
        // after processing this line. This ensures CLOSING lines (like `)`) get semicolons!
        // - Lines INSIDE: depth_before > 0 AND depth_after > 0 → skip semicolon
        // - CLOSING line: depth_before > 0 AND depth_after == 0 → ADD semicolon!
        let inside_multiline_expr = multiline_depth_before > 0 && self.multiline_expr_depth > 0;
        
        (multiline_depth_before, inside_multiline_expr)
    }
    
    /// Update brace depth tracking and return (prev_depth, opens, closes)
    /// CRITICAL: Ignores braces inside string literals to prevent false positives
    /// from format strings like "header {} mismatch"
    pub fn update_brace_depth(&mut self, trimmed: &str) -> (usize, usize, usize) {
        let prev_depth = self.brace_depth;
        let (opens, closes) = count_braces_outside_strings(trimmed);
        self.brace_depth += opens;
        self.brace_depth = self.brace_depth.saturating_sub(closes);
        (prev_depth, opens, closes)
    }
    
    /// Update bracket depth tracking and return (prev_bracket_depth, bracket_opens, bracket_closes)
    /// CRITICAL: Ignores brackets inside string literals
    pub fn update_bracket_depth(&mut self, trimmed: &str) -> (usize, usize, usize) {
        let prev_bracket_depth = self.bracket_depth;
        let (bracket_opens, bracket_closes) = count_brackets_outside_strings(trimmed);
        self.bracket_depth += bracket_opens;
        self.bracket_depth = self.bracket_depth.saturating_sub(bracket_closes);
        (prev_bracket_depth, bracket_opens, bracket_closes)
    }
    
    /// Check if we should exit function context
    pub fn check_exit_function(&mut self, trimmed: &str) {
        if self.in_function_body && self.brace_depth < self.function_start_brace && trimmed == "}" {
            self.in_function_body = false;
            self.current_fn_ctx.exit();
        }
    }
    
    /// Enter function body context
    pub fn enter_function(&mut self, sig: &crate::function::FunctionSignature) {
        self.in_function_body = true;
        self.function_start_brace = self.brace_depth + 1;
        self.current_fn_ctx.enter(sig, self.function_start_brace);
    }
}

impl Default for ParserState {
    fn default() -> Self {
        Self::new()
    }
}

/// Count opening and closing braces OUTSIDE of string literals
/// This is CRITICAL to avoid counting format placeholders like {} in "hello {} world"
fn count_braces_outside_strings(s: &str) -> (usize, usize) {
    let mut opens = 0;
    let mut closes = 0;
    let mut in_string = false;
    let mut escape_next = false;
    
    for c in s.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }
        
        if c == '\\' && in_string {
            escape_next = true;
            continue;
        }
        
        if c == '"' {
            in_string = !in_string;
            continue;
        }
        
        if !in_string {
            match c {
                '{' => opens += 1,
                '}' => closes += 1,
                _ => {}
            }
        }
    }
    
    (opens, closes)
}

/// Count opening and closing brackets OUTSIDE of string literals
fn count_brackets_outside_strings(s: &str) -> (usize, usize) {
    let mut opens = 0;
    let mut closes = 0;
    let mut in_string = false;
    let mut escape_next = false;
    
    for c in s.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }
        
        if c == '\\' && in_string {
            escape_next = true;
            continue;
        }
        
        if c == '"' {
            in_string = !in_string;
            continue;
        }
        
        if !in_string {
            match c {
                '[' => opens += 1,
                ']' => closes += 1,
                _ => {}
            }
        }
    }
    
    (opens, closes)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_count_braces_outside_strings() {
        // Normal braces
        assert_eq!(count_braces_outside_strings("{ hello }"), (1, 1));
        assert_eq!(count_braces_outside_strings("{{ nested }}"), (2, 2));
        
        // Braces inside strings should be IGNORED
        assert_eq!(count_braces_outside_strings("\"hello {} world\""), (0, 0));
        assert_eq!(count_braces_outside_strings("anyhow::bail(\"header {} mismatch\")"), (0, 0));
        
        // Mixed - real brace + format placeholder inside string
        assert_eq!(count_braces_outside_strings("if x { \"fmt {}\" }"), (1, 1));
        
        // Escaped quotes in string
        assert_eq!(count_braces_outside_strings("\"escaped \\\" quote {}\""), (0, 0));
    }
    
    #[test]
    fn test_count_brackets_outside_strings() {
        assert_eq!(count_brackets_outside_strings("[1, 2, 3]"), (1, 1));
        assert_eq!(count_brackets_outside_strings("\"array [0]\""), (0, 0));
    }
}