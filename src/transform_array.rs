//! Array element transformation for RustS+ transpiler
//!
//! This module handles transformation of elements inside array literals.
//! Elements can be:
//! - Simple values: `1, "hello", var`
//! - Single-line struct literals: `User { id = 1, name = "x" }`
//! - Single-line enum variants: `Event::Data { id = 1, body = b }`
//! - Multi-line literals (handled by literal_mode in main parser)

use crate::transform_literal::{find_field_eq, is_string_literal, transform_nested_struct_value};

/// Transform an array element line
/// 
/// This function handles:
/// 1. Simple values (numbers, strings, identifiers) - pass through with comma
/// 2. Single-line struct/enum literals - transform `=` to `:` in fields
/// 3. Multi-line literal START lines - pass through (caller handles literal mode)
/// 
/// CRITICAL: Must detect if line is COMPLETE (balanced braces) vs INCOMPLETE (multi-line start)
pub fn transform_array_element(line: &str) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // Skip empty lines
    if trimmed.is_empty() {
        return line.to_string();
    }
    
    // Skip comments
    if trimmed.starts_with("//") {
        return line.to_string();
    }
    
    // Skip closing bracket
    if trimmed == "]" || trimmed == "];" || trimmed == "]," {
        return line.to_string();
    }
    
    // Check for struct/enum literal pattern
    let has_brace = trimmed.contains('{');
    
    if has_brace {
        // Count braces OUTSIDE strings to check if complete
        let (opens, closes) = count_braces_outside_strings(trimmed);
        
        if opens == closes && opens > 0 {
            // COMPLETE single-line literal - transform fields
            let transformed = transform_complete_literal_element(trimmed);
            return format!("{}{}", leading_ws, transformed);
        } else if opens > closes {
            // INCOMPLETE multi-line literal START
            // Just pass through - the literal mode will handle fields
            return format!("{}{}", leading_ws, trimmed);
        }
    }
    
    // Simple element - ensure comma at end
    let result = if trimmed.ends_with(',') {
        trimmed.to_string()
    } else {
        format!("{},", trimmed)
    };
    
    format!("{}{}", leading_ws, result)
}

/// Transform a complete single-line struct/enum literal element
/// Input:  `SyncStatus::SyncingHeaders { start_height = 0, target_height = 100 },`
/// Output: `SyncStatus::SyncingHeaders { start_height: 0, target_height: 100 },`
fn transform_complete_literal_element(line: &str) -> String {
    let trimmed = line.trim();
    
    // Find the opening brace
    let brace_pos = match find_brace_outside_string(trimmed) {
        Some(pos) => pos,
        None => return ensure_comma(trimmed),
    };
    
    // Split into type part and fields part
    let type_part = &trimmed[..brace_pos].trim();
    let rest = &trimmed[brace_pos..];
    
    // Find closing brace
    let close_brace = match rest.rfind('}') {
        Some(pos) => pos,
        None => return ensure_comma(trimmed),
    };
    
    // Extract fields between { and }
    let fields_part = &rest[1..close_brace]; // Skip opening {
    let after_close = &rest[close_brace + 1..]; // Everything after }
    
    // Transform fields: `field = value` -> `field: value`
    let transformed_fields = transform_fields(fields_part);
    
    // Reconstruct with comma
    let needs_comma = !after_close.trim().ends_with(',');
    let suffix = if needs_comma { "," } else { "" };
    
    format!("{} {{ {} }}{}{}", type_part, transformed_fields, after_close.trim_end_matches(','), suffix)
}

/// Transform fields inside a struct/enum literal
/// Input:  `start_height = 0, target_height = 100, current_height = 50`
/// Output: `start_height: 0, target_height: 100, current_height: 50`
fn transform_fields(fields: &str) -> String {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut brace_depth: usize = 0;
    let mut paren_depth: usize = 0;
    
    // Split by comma, respecting nested structures and strings
    for c in fields.chars() {
        if c == '"' && !current.ends_with('\\') {
            in_string = !in_string;
        }
        
        if !in_string {
            match c {
                '{' => brace_depth += 1,
                '}' => brace_depth = brace_depth.saturating_sub(1),
                '(' => paren_depth += 1,
                ')' => paren_depth = paren_depth.saturating_sub(1),
                ',' if brace_depth == 0 && paren_depth == 0 => {
                    result.push(current.clone());
                    current.clear();
                    continue;
                }
                _ => {}
            }
        }
        
        current.push(c);
    }
    
    if !current.trim().is_empty() {
        result.push(current);
    }
    
    // Transform each field
    let transformed: Vec<String> = result.iter()
        .map(|f| transform_single_field(f))
        .collect();
    
    transformed.join(", ")
}

/// Transform a single field: `field = value` -> `field: value`
/// CRITICAL FIX: Also recursively transforms nested struct values!
fn transform_single_field(field: &str) -> String {
    let trimmed = field.trim();
    
    if trimmed.is_empty() {
        return String::new();
    }
    
    // Spread syntax - pass through
    if trimmed.starts_with("..") {
        return trimmed.to_string();
    }
    
    // Already has colon (Rust syntax) - but might have nested struct with = in value
    if trimmed.contains(':') && !trimmed.contains("::") {
        // Check if there's a nested struct that needs transformation
        if trimmed.contains('{') && trimmed.contains('=') {
            if let Some(colon_pos) = trimmed.find(':') {
                // Make sure it's not followed by another : (path separator)
                if colon_pos + 1 < trimmed.len() && trimmed.chars().nth(colon_pos + 1) != Some(':') {
                    let name = trimmed[..colon_pos].trim();
                    let value = trimmed[colon_pos + 1..].trim();
                    let transformed_value = transform_nested_struct_value(value);
                    return format!("{}: {}", name, transformed_value);
                }
            }
        }
        return trimmed.to_string();
    }
    
    // Find the = that separates field name from value
    if let Some(eq_pos) = find_field_eq(trimmed) {
        let name = trimmed[..eq_pos].trim();
        let value = trimmed[eq_pos + 1..].trim();
        
        // Transform string literals
        let transformed_value = if is_string_literal(value) {
            let inner = &value[1..value.len()-1];
            format!("String::from(\"{}\")", inner)
        } else if value.contains('{') && value.contains('=') {
            // CRITICAL FIX: Recursively transform nested struct values!
            transform_nested_struct_value(value)
        } else {
            value.to_string()
        };
        
        return format!("{}: {}", name, transformed_value);
    }
    
    // No transformation needed
    trimmed.to_string()
}

/// Count opening and closing braces outside of string literals
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

/// Find the position of the first `{` outside of string literals
fn find_brace_outside_string(s: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escape_next = false;
    
    for (i, c) in s.chars().enumerate() {
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
        
        if !in_string && c == '{' {
            return Some(i);
        }
    }
    
    None
}

/// Ensure line ends with comma
fn ensure_comma(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.ends_with(',') {
        trimmed.to_string()
    } else {
        format!("{},", trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_elements() {
        assert_eq!(transform_array_element("    1").trim(), "1,");
        assert_eq!(transform_array_element("    \"hello\"").trim(), "\"hello\",");
        assert_eq!(transform_array_element("    var_name,").trim(), "var_name,");
    }
    
    #[test]
    fn test_single_line_struct_literal() {
        let input = "    User { id = 1, name = \"test\" }";
        let output = transform_array_element(input);
        assert!(output.contains("id: 1"));
        assert!(output.contains("name: String::from(\"test\")"));
        assert!(output.trim().ends_with(','));
    }
    
    #[test]
    fn test_single_line_enum_variant() {
        let input = "    SyncStatus::SyncingHeaders { start_height = 0, target_height = 100, current_height = 50 },";
        let output = transform_array_element(input);
        assert!(output.contains("start_height: 0"));
        assert!(output.contains("target_height: 100"));
        assert!(output.contains("current_height: 50"));
        assert!(output.contains("SyncStatus::SyncingHeaders"));
    }
    
    #[test]
    fn test_unit_enum_variant() {
        let input = "    SyncStatus::Idle,";
        let output = transform_array_element(input);
        assert_eq!(output.trim(), "SyncStatus::Idle,");
    }
    
    #[test]
    fn test_multiline_start_not_transformed() {
        // Multi-line start - should NOT be transformed, just passed through
        let input = "    SyncStatus::SyncingHeaders {";
        let output = transform_array_element(input);
        assert_eq!(output.trim(), "SyncStatus::SyncingHeaders {");
    }
    
    #[test]
    fn test_braces_in_string() {
        // Braces inside string should not affect parsing
        let input = r#"    format!("value: {}")"#;
        let output = transform_array_element(input);
        assert!(output.contains(r#"format!("value: {}")"#));
    }
    
    #[test]
    fn test_count_braces_outside_strings() {
        assert_eq!(count_braces_outside_strings("{ hello }"), (1, 1));
        assert_eq!(count_braces_outside_strings("\"hello {}\""), (0, 0));
        assert_eq!(count_braces_outside_strings("Event { x = \"{}\" }"), (1, 1));
    }
    
    #[test]
    fn test_transform_fields() {
        let input = "start_height = 0, target_height = 100";
        let output = transform_fields(input);
        assert_eq!(output, "start_height: 0, target_height: 100");
    }
    
    #[test]
    fn test_nested_struct_field() {
        let input = "data = Inner { x = 1 }, other = 2";
        let output = transform_fields(input);
        // Inner struct should also be transformed
        assert!(output.contains("data: Inner { x: 1 }"));
        assert!(output.contains("other: 2"));
    }
}