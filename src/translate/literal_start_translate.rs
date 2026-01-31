//! Literal Start Translation
//!
//! Handles the start of struct and enum literal expressions in RustS+.
//!
//! Patterns handled:
//! - Assignment with struct literal: `config = Config { ... }`
//! - Assignment with enum literal: `status = Status::Active { ... }`
//! - Field assignment: `self.config = Config { ... }`
//! - Bare struct literal: `Config { ... }`
//! - Bare enum literal: `Status::Active { ... }`
//! - Literal inside function call: `Some(Config { ... })`

use crate::modes::{LiteralModeStack, LiteralKind};
use crate::detection::{
    detect_struct_literal_start, detect_enum_literal_start,
    detect_bare_struct_literal, detect_bare_enum_literal,
    detect_struct_literal_in_call, detect_enum_literal_in_call,
};
use crate::inline_literal_transform::{
    transform_single_line_struct_literal, transform_single_line_enum_literal,
    transform_bare_struct_literal,
};
use crate::helpers::{is_field_access, is_tuple_pattern};
use crate::scope::ScopeAnalyzer;
use crate::variable::VariableTracker;
use crate::struct_def::StructRegistry;

/// Result of processing a literal start
pub enum LiteralStartResult {
    /// Line was handled (literal started or single-line literal)
    Handled(String),
    /// Not a literal start
    NotLiteralStart,
}

/// Process struct literal start (assignment pattern)
pub fn process_struct_literal_start(
    trimmed: &str,
    leading_ws: &str,
    line_num: usize,
    opens: usize,
    prev_depth: usize,
    scope_analyzer: &ScopeAnalyzer,
    tracker: &VariableTracker,
    struct_registry: &StructRegistry,
    literal_mode: &mut LiteralModeStack,
) -> LiteralStartResult {
    let (var_name, struct_name) = match detect_struct_literal_start(trimmed, struct_registry) {
        Some(pair) => pair,
        None => return LiteralStartResult::NotLiteralStart,
    };
    
    // CRITICAL FIX: Check if var_name is a field access (e.g., self.field)
    // Field assignments should NOT get `let` prefix!
    let is_field = is_field_access(&var_name);
    let _is_tuple = is_tuple_pattern(&var_name);
    
    let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
    let borrowed_mut = tracker.is_mut_borrowed(&var_name);
    let mutated_via_method = tracker.is_mutated_via_method(&var_name);
    let needs_mut = scope_needs_mut || borrowed_mut || mutated_via_method;
    
    // Determine if we need `let` keyword
    let needs_let = !is_field;
    let let_keyword = if !needs_let {
        ""
    } else if needs_mut {
        "let mut "
    } else {
        "let "
    };
    
    // Single-line struct literal
    if trimmed.ends_with('}') {
        let output = if is_field {
            // Field assignment - no let, transform fields
            transform_bare_struct_literal(trimmed)
        } else {
            let transformed = transform_single_line_struct_literal(trimmed, &var_name);
            if needs_mut && needs_let {
                transformed.replacen("let ", "let mut ", 1)
            } else {
                transformed
            }
        };
        return LiteralStartResult::Handled(format!("{}{}", leading_ws, output));
    }
    
    // Multi-line struct literal - enter literal mode
    // CRITICAL FIX: Always mark as assignment (true) for semicolon handling
    literal_mode.enter(LiteralKind::Struct, prev_depth + opens, true);
    
    LiteralStartResult::Handled(format!("{}{}{} = {} {{", leading_ws, let_keyword, var_name, struct_name))
}

/// Process enum literal start (assignment pattern)
pub fn process_enum_literal_start(
    trimmed: &str,
    leading_ws: &str,
    line_num: usize,
    opens: usize,
    prev_depth: usize,
    scope_analyzer: &ScopeAnalyzer,
    tracker: &VariableTracker,
    literal_mode: &mut LiteralModeStack,
) -> LiteralStartResult {
    let (var_name, enum_path) = match detect_enum_literal_start(trimmed) {
        Some(pair) => pair,
        None => return LiteralStartResult::NotLiteralStart,
    };
    
    // CRITICAL FIX: Check if var_name is a field access
    let is_field = is_field_access(&var_name);
    let _is_tuple = is_tuple_pattern(&var_name);
    
    let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
    let borrowed_mut = tracker.is_mut_borrowed(&var_name);
    let mutated_via_method = tracker.is_mutated_via_method(&var_name);
    let needs_mut = scope_needs_mut || borrowed_mut || mutated_via_method;
    
    let needs_let = !is_field;
    let let_keyword = if !needs_let {
        ""
    } else if needs_mut {
        "let mut "
    } else {
        "let "
    };
    
    // Single-line enum literal
    if trimmed.ends_with('}') {
        let output = if is_field {
            transform_bare_struct_literal(trimmed)
        } else {
            let transformed = transform_single_line_enum_literal(trimmed, &var_name, &enum_path);
            if needs_mut && needs_let {
                transformed.replacen("let ", "let mut ", 1)
            } else {
                transformed
            }
        };
        return LiteralStartResult::Handled(format!("{}{}", leading_ws, output));
    }
    
    // Multi-line enum literal
    literal_mode.enter(LiteralKind::EnumVariant, prev_depth + opens, true);
    
    LiteralStartResult::Handled(format!("{}{}{} = {} {{", leading_ws, let_keyword, var_name, enum_path))
}

/// Process literal inside function call
pub fn process_literal_in_call(
    trimmed: &str,
    leading_ws: &str,
    opens: usize,
    closes: usize,
    prev_depth: usize,
    struct_registry: &StructRegistry,
    literal_mode: &mut LiteralModeStack,
) -> LiteralStartResult {
    if opens <= closes || !trimmed.contains('(') {
        return LiteralStartResult::NotLiteralStart;
    }
    
    // Check for struct literal inside function call
    if let Some(_struct_name) = detect_struct_literal_in_call(trimmed, struct_registry) {
        literal_mode.enter(LiteralKind::Struct, prev_depth + opens, false);
        let transformed = transform_call_with_struct_literal(trimmed);
        return LiteralStartResult::Handled(format!("{}{}", leading_ws, transformed));
    }
    
    // Check for enum literal inside function call
    if let Some(_enum_path) = detect_enum_literal_in_call(trimmed) {
        literal_mode.enter(LiteralKind::EnumVariant, prev_depth + opens, false);
        let transformed = transform_call_with_struct_literal(trimmed);
        return LiteralStartResult::Handled(format!("{}{}", leading_ws, transformed));
    }
    
    LiteralStartResult::NotLiteralStart
}

/// Process bare struct literal (no assignment, just `StructName { ... }`)
pub fn process_bare_struct_literal(
    trimmed: &str,
    leading_ws: &str,
    opens: usize,
    closes: usize,
    prev_depth: usize,
    struct_registry: &StructRegistry,
    literal_mode: &mut LiteralModeStack,
) -> LiteralStartResult {
    let struct_name = match detect_bare_struct_literal(trimmed, struct_registry) {
        Some(name) => name,
        None => return LiteralStartResult::NotLiteralStart,
    };
    
    // CRITICAL FIX: Check for COMPLETE single-line literals
    let is_complete_single_line = trimmed.ends_with('}') || 
                                  trimmed.ends_with("},") ||
                                  trimmed.ends_with("};");
    
    if is_complete_single_line && opens == closes {
        let transformed = transform_bare_struct_literal(trimmed);
        return LiteralStartResult::Handled(format!("{}{}", leading_ws, transformed));
    }
    
    // Multi-line start
    if opens > closes {
        literal_mode.enter(LiteralKind::Struct, prev_depth + opens, false);
        return LiteralStartResult::Handled(format!("{}{} {{", leading_ws, struct_name));
    }
    
    // Just transform and output
    let transformed = transform_bare_struct_literal(trimmed);
    LiteralStartResult::Handled(format!("{}{}", leading_ws, transformed))
}

/// Process bare enum literal
pub fn process_bare_enum_literal(
    trimmed: &str,
    leading_ws: &str,
    opens: usize,
    closes: usize,
    prev_depth: usize,
    literal_mode: &mut LiteralModeStack,
) -> LiteralStartResult {
    let enum_path = match detect_bare_enum_literal(trimmed) {
        Some(path) => path,
        None => return LiteralStartResult::NotLiteralStart,
    };
    
    let is_complete_single_line = trimmed.ends_with('}') || 
                                  trimmed.ends_with("},") ||
                                  trimmed.ends_with("};");
    
    if is_complete_single_line && opens == closes {
        let transformed = transform_bare_struct_literal(trimmed);
        return LiteralStartResult::Handled(format!("{}{}", leading_ws, transformed));
    }
    
    if opens > closes {
        literal_mode.enter(LiteralKind::EnumVariant, prev_depth + opens, false);
        return LiteralStartResult::Handled(format!("{}{} {{", leading_ws, enum_path));
    }
    
    let transformed = transform_bare_struct_literal(trimmed);
    LiteralStartResult::Handled(format!("{}{}", leading_ws, transformed))
}

/// Transform a line containing struct literal inside function call
fn transform_call_with_struct_literal(line: &str) -> String {
    let trimmed = line.trim();
    
    let brace_pos = match trimmed.find('{') {
        Some(pos) => pos,
        None => return trimmed.to_string(),
    };
    
    let after_brace = &trimmed[brace_pos + 1..];
    
    if after_brace.trim().is_empty() || after_brace.trim() == "{" {
        return trimmed.to_string();
    }
    
    let before_brace = &trimmed[..brace_pos + 1];
    
    if let Some(close_pos) = after_brace.rfind('}') {
        let fields_part = &after_brace[..close_pos];
        let after_close = &after_brace[close_pos..];
        
        let transformed_fields = crate::translate::literal_inline_translate::transform_fields_inline(fields_part);
        
        return format!("{} {} {}", before_brace, transformed_fields, after_close);
    }
    
    trimmed.to_string()
}