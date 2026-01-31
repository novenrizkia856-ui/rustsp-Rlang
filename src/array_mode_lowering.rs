//! Array Mode Lowering
//!
//! Handles multi-line array literal processing in RustS+.
//!
//! Example:
//! ```text
//! items = [
//!     Item { name = "foo" },
//!     Item { name = "bar" },
//! ]
//! ```

use crate::modes::{ArrayModeStack, LiteralModeStack, LiteralKind};
use crate::transform_array::transform_array_element;
use crate::detection::{detect_bare_struct_literal, detect_bare_enum_literal};
use crate::struct_def::StructRegistry;

/// Result of processing a line in array mode
pub enum ArrayModeResult {
    /// Line was handled by array mode
    Handled(String),
    /// Line should fall through to literal mode
    FallThroughToLiteral,
    /// Line was not for array mode
    NotHandled,
}

/// Process array closing bracket
fn process_array_close(
    clean_line: &str,
    leading_ws: &str,
    bracket_depth: usize,
    array_mode: &mut ArrayModeStack,
) -> Option<String> {
    if !array_mode.should_exit(bracket_depth) {
        return None;
    }
    
    if let Some(entry) = array_mode.exit() {
        let transformed = transform_array_element(clean_line);
        let suffix = if entry.is_assignment { ";" } else { "" };
        
        let close_line = if transformed.trim() == "]" {
            format!("{}]{}", leading_ws, suffix)
        } else {
            let without_bracket = transformed.trim().trim_end_matches(']').trim_end_matches(',');
            format!("{}    {},\n{}]{}", leading_ws, without_bracket, leading_ws, suffix)
        };
        
        return Some(close_line);
    }
    
    None
}

/// Process a line that might be part of array mode
pub fn process_array_mode_line(
    trimmed: &str,
    clean_line: &str,
    leading_ws: &str,
    bracket_depth: usize,
    opens: usize,
    closes: usize,
    prev_depth: usize,
    array_mode: &mut ArrayModeStack,
    literal_mode: &mut LiteralModeStack,
    struct_registry: &StructRegistry,
) -> ArrayModeResult {
    // Check for array closing
    if array_mode.is_active() && trimmed.contains(']') {
        if let Some(result) = process_array_close(clean_line, leading_ws, bracket_depth, array_mode) {
            return ArrayModeResult::Handled(result);
        }
    }
    
    // Process line inside array mode
    if array_mode.is_active() {
        // CRITICAL FIX: If also in literal mode, let literal mode handle it
        if literal_mode.is_active() {
            return ArrayModeResult::FallThroughToLiteral;
        }
        
        // Check if this line starts a multi-line struct/enum literal
        let starts_multiline_literal = if opens > closes {
            if trimmed.contains("::") {
                detect_bare_enum_literal(trimmed).is_some()
            } else {
                detect_bare_struct_literal(trimmed, struct_registry).is_some()
            }
        } else {
            false
        };
        
        if starts_multiline_literal {
            // Transform the start line and enter literal mode
            let transformed = transform_array_element(clean_line);
            
            // Enter literal mode for the fields
            let kind = if trimmed.contains("::") { 
                LiteralKind::EnumVariant 
            } else { 
                LiteralKind::Struct 
            };
            literal_mode.enter(kind, prev_depth + opens, false);
            
            return ArrayModeResult::Handled(transformed);
        }
        
        // Regular array element (single-line)
        let transformed = transform_array_element(clean_line);
        return ArrayModeResult::Handled(transformed);
    }
    
    ArrayModeResult::NotHandled
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_array_mode_detection() {
        let mut array_mode = ArrayModeStack::new();
        let mut literal_mode = LiteralModeStack::new();
        let struct_registry = StructRegistry::new();
        
        // Not in array mode - should return NotHandled
        let result = process_array_mode_line(
            "x",
            "x",
            "",
            0,
            0,
            0,
            0,
            &mut array_mode,
            &mut literal_mode,
            &struct_registry,
        );
        
        assert!(matches!(result, ArrayModeResult::NotHandled));
    }
}