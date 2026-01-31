//! Literal Mode Lowering
//!
//! Handles multi-line struct and enum literal processing in RustS+.
//!
//! Example:
//! ```text
//! config = Config {
//!     name = "test",
//!     value = 42,
//! }
//! ```

use crate::modes::{LiteralModeStack, LiteralKind, ArrayModeStack};
use crate::transform_literal::transform_literal_field_with_ctx;
use crate::function::CurrentFunctionContext;

/// Result of processing a line in literal mode
pub enum LiteralModeResult {
    /// Line was handled by literal mode
    Handled(String),
    /// Line was not for literal mode
    NotHandled,
}

/// Process literal closing brace
fn process_literal_close(
    leading_ws: &str,
    brace_depth: usize,
    literal_mode: &mut LiteralModeStack,
    array_mode: &ArrayModeStack,
) -> Option<String> {
    if !literal_mode.should_exit(brace_depth) {
        return None;
    }
    
    let was_assignment = literal_mode.current_is_assignment();
    literal_mode.exit();
    
    // CRITICAL FIX: When inside array, closing literal needs comma
    let suffix = if array_mode.is_active() {
        ","  // Inside array - element needs comma
    } else if literal_mode.is_active() { 
        ","  // Nested literal - needs comma
    } else if was_assignment {
        ";"  // Assignment - needs semicolon
    } else {
        ""   // Bare literal (return expression)
    };
    
    Some(format!("{}}}{}", leading_ws, suffix))
}

/// Process a line that might be part of literal mode
pub fn process_literal_mode_line(
    trimmed: &str,
    clean_line: &str,
    leading_ws: &str,
    brace_depth: usize,
    opens: usize,
    closes: usize,
    prev_depth: usize,
    literal_mode: &mut LiteralModeStack,
    array_mode: &ArrayModeStack,
    current_fn_ctx: Option<&CurrentFunctionContext>,
) -> LiteralModeResult {
    // Check for literal closing brace
    // Handle both "}" and "}," (user may or may not include comma)
    if literal_mode.is_active() && (trimmed == "}" || trimmed == "},") {
        if let Some(result) = process_literal_close(leading_ws, brace_depth, literal_mode, array_mode) {
            return LiteralModeResult::Handled(result);
        }
        // CRITICAL BUGFIX: If should_exit returned false but line is just "}" or "},",
        // do NOT process it as a literal field - let it fall through to normal processing.
        // This prevents incorrectly consuming closing braces that belong to outer scopes
        // (function body, impl block, etc.), which would cause "unclosed delimiter" errors.
        return LiteralModeResult::NotHandled;
    }
    
    // Process line inside literal mode (only for non-closing-brace lines)
    if literal_mode.is_active() {
        let transformed = transform_literal_field_with_ctx(clean_line, current_fn_ctx);
        
        // Check for nested literal start
        if trimmed.contains('{') && opens > closes {
            let kind = if trimmed.contains("::") { 
                LiteralKind::EnumVariant 
            } else { 
                LiteralKind::Struct 
            };
            literal_mode.enter(kind, prev_depth + opens, false);
        }
        
        return LiteralModeResult::Handled(transformed);
    }
    
    LiteralModeResult::NotHandled
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_literal_mode_close_assignment() {
        let mut literal_mode = LiteralModeStack::new();
        let array_mode = ArrayModeStack::new();
        
        // Enter literal mode as assignment
        literal_mode.enter(LiteralKind::Struct, 1, true);
        
        // Process closing brace
        let result = process_literal_mode_line(
            "}",
            "}",
            "    ",
            0,
            0,
            1,
            1,
            &mut literal_mode,
            &array_mode,
            None,
        );
        
        // Should handle and add semicolon for assignment
        match result {
            LiteralModeResult::Handled(s) => assert!(s.contains("};") || s.contains("}")),
            _ => panic!("Expected Handled result"),
        }
    }
    
    #[test]
    fn test_literal_mode_close_in_array() {
        let mut literal_mode = LiteralModeStack::new();
        let mut array_mode = ArrayModeStack::new();
        
        // Enter array mode first
        array_mode.enter(0, true, "arr".to_string(), None, true, false);
        
        // Enter literal mode (not as assignment since it's array element)
        literal_mode.enter(LiteralKind::Struct, 1, false);
        
        // Process closing brace - should add comma for array element
        let result = process_literal_mode_line(
            "}",
            "}",
            "    ",
            0,
            0,
            1,
            1,
            &mut literal_mode,
            &array_mode,
            None,
        );
        
        match result {
            LiteralModeResult::Handled(s) => assert!(s.ends_with(",") || s.contains("},")),
            _ => panic!("Expected Handled result"),
        }
    }
}