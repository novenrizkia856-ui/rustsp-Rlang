//! Literal Inline Translation
//!
//! Transforms inline struct/enum literal fields from RustS+ to Rust syntax.
//!
//! RustS+ field syntax: `field = value`
//! Rust field syntax: `field: value`

use crate::transform_literal::find_field_eq;

/// Transform inline fields: `x = 1, y = 2` -> `x: 1, y: 2`
pub fn transform_fields_inline(fields: &str) -> String {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut brace_depth: usize = 0;
    
    for c in fields.chars() {
        if c == '"' && !current.ends_with('\\') {
            in_string = !in_string;
        }
        if !in_string {
            if c == '{' { brace_depth += 1; }
            if c == '}' { brace_depth = brace_depth.saturating_sub(1); }
        }
        
        if c == ',' && !in_string && brace_depth == 0 {
            result.push(transform_single_inline_field(&current));
            current.clear();
        } else {
            current.push(c);
        }
    }
    
    if !current.trim().is_empty() {
        result.push(transform_single_inline_field(&current));
    }
    
    result.join(", ")
}

/// Transform a single field: `field = value` -> `field: value`
pub fn transform_single_inline_field(field: &str) -> String {
    let trimmed = field.trim();
    
    if trimmed.is_empty() { 
        return String::new(); 
    }
    
    // Spread syntax - pass through
    if trimmed.starts_with("..") { 
        return trimmed.to_string(); 
    }
    
    // Already has colon (not ::) - pass through
    if trimmed.contains(':') && !trimmed.contains("::") {
        return trimmed.to_string();
    }
    
    // Field assignment: `field = value`
    if let Some(eq_pos) = find_field_eq(trimmed) {
        let name = trimmed[..eq_pos].trim();
        let value = trimmed[eq_pos + 1..].trim();
        return format!("{}: {}", name, value);
    }
    
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_transform_single_field() {
        assert_eq!(transform_single_inline_field("name = \"test\""), "name: \"test\"");
        assert_eq!(transform_single_inline_field("value = 42"), "value: 42");
        assert_eq!(transform_single_inline_field("..other"), "..other");
        assert_eq!(transform_single_inline_field("field: value"), "field: value");
    }
    
    #[test]
    fn test_transform_fields_inline() {
        assert_eq!(
            transform_fields_inline("name = \"test\", value = 42"),
            "name: \"test\", value: 42"
        );
    }
    
    #[test]
    fn test_transform_fields_with_nested() {
        // Nested struct should not break parsing
        let input = "outer = Inner { x = 1 }, y = 2";
        let result = transform_fields_inline(input);
        assert!(result.contains("outer:"));
        assert!(result.contains("y:"));
    }
    
    #[test]
    fn test_transform_fields_with_string() {
        // Commas inside strings should not split fields
        let input = "msg = \"hello, world\", count = 1";
        let result = transform_fields_inline(input);
        // Should have exactly 2 fields
        assert_eq!(result.matches(':').count(), 2);
    }
}