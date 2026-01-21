//! Literal transformation functions for RustS+ transpiler
//! 
//! Contains functions for transforming struct/enum literal fields:
//! - Field syntax transformation: `field = value` → `field: value,`
//! - Nested struct literal handling
//! - String literal transformation to String::from

use crate::helpers::is_valid_identifier;
use crate::function::CurrentFunctionContext;

/// Transform a literal field line: `field = value` → `field: value,`
/// NO `let`, NO `;` - this is expression-only context!
pub fn transform_literal_field(line: &str) -> String {
    transform_literal_field_with_ctx(line, None)
}

/// Transform a literal field with optional function context
pub fn transform_literal_field_with_ctx(line: &str, ctx: Option<&CurrentFunctionContext>) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // Handle spread syntax
    if trimmed.starts_with("..") {
        return format!("{}{}", leading_ws, trimmed);
    }
    
    // Skip braces
    if trimmed.is_empty() || trimmed == "{" || trimmed == "}" || trimmed == "}," {
        return line.to_string();
    }
    
    // Already has colon (except ::) - but might have nested struct with = in value
    if trimmed.contains(':') && !trimmed.contains("::") {
        // Check if there's a nested struct that needs transformation
        if trimmed.contains('{') && trimmed.contains('=') {
            if let Some(colon_pos) = trimmed.find(':') {
                if !trimmed[..colon_pos].contains("::") {
                    let field = trimmed[..colon_pos].trim();
                    let value = trimmed[colon_pos + 1..].trim().trim_end_matches(',');
                    let transformed_value = transform_nested_struct_value(value);
                    return format!("{}{}: {},", leading_ws, field, transformed_value);
                }
            }
        }
        let clean = trimmed.trim_end_matches(',');
        return format!("{}{},", leading_ws, clean);
    }
    
    // Nested literal start: `header = Header {` - transform = to :
    if trimmed.contains('{') {
        if let Some(eq_pos) = find_field_eq_top_level(trimmed) {
            let field = trimmed[..eq_pos].trim();
            let value = trimmed[eq_pos + 1..].trim();
            if is_valid_field_name(field) {
                let transformed_value = transform_nested_struct_value(value);
                let tv = transformed_value.trim();
                let is_multiline_start = tv.ends_with('{') || tv.ends_with('[');
                let already_has_comma = tv.ends_with(',');
                let suffix = if !is_multiline_start && !already_has_comma { "," } else { "" };
                return format!("{}{}: {}{}", leading_ws, field, transformed_value, suffix);
            }
        }
        let transformed = transform_nested_struct_value(trimmed);
        let t = transformed.trim();
        let is_multiline_start = t.ends_with('{') || t.ends_with('[');
        let already_has_comma = t.ends_with(',') || t.ends_with("},");
        let suffix = if !is_multiline_start && !already_has_comma { "," } else { "" };
        return format!("{}{}{}", leading_ws, transformed, suffix);
    }
    
    // Simple field: `field = value`
    if let Some(eq_pos) = find_field_eq(trimmed) {
        let field = trimmed[..eq_pos].trim();
        let value = trimmed[eq_pos + 1..].trim().trim_end_matches(',');
        
        if is_valid_field_name(field) && !value.is_empty() {
            let mut transformed_value = if is_string_literal(value) {
                let inner = &value[1..value.len()-1];
                format!("String::from(\"{}\")", inner)
            } else {
                value.to_string()
            };
            
            // Add .to_vec() for slice parameters assigned to struct fields
            if let Some(ctx) = ctx {
                if is_valid_identifier(&transformed_value) && ctx.is_slice_param(&transformed_value) {
                    transformed_value = format!("{}.to_vec()", transformed_value);
                }
            }
            
            // Add .clone() for field access expressions
            if should_clone_field_value(&transformed_value) {
                transformed_value = format!("{}.clone()", transformed_value);
            }
            
            return format!("{}{}: {},", leading_ws, field, transformed_value);
        }
    }
    
    format!("{}{}", leading_ws, trimmed)
}

/// Transform nested struct literals recursively
/// `Address { value = addr_hash }` → `Address { value: addr_hash }`
pub fn transform_nested_struct_value(value: &str) -> String {
    let trimmed = value.trim();
    
    if !trimmed.contains('{') || !trimmed.contains('=') {
        return trimmed.to_string();
    }
    
    if let Some(brace_start) = trimmed.find('{') {
        let before_brace = &trimmed[..brace_start + 1];
        let after_brace = &trimmed[brace_start + 1..];
        
        let mut depth = 1;
        let mut brace_end = after_brace.len();
        for (i, c) in after_brace.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        brace_end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        
        let fields_part = &after_brace[..brace_end];
        let after_close = &after_brace[brace_end..];
        
        let transformed_fields = transform_struct_fields_recursive(fields_part);
        return format!("{}{}{}", before_brace, transformed_fields, after_close);
    }
    
    trimmed.to_string()
}

/// Transform struct fields recursively, handling nested structs
fn transform_struct_fields_recursive(fields: &str) -> String {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut brace_depth: usize = 0;
    let mut prev_char = ' ';
    
    for c in fields.chars() {
        if c == '"' && prev_char != '\\' {
            in_string = !in_string;
        }
        if !in_string {
            if c == '{' { brace_depth += 1; }
            if c == '}' { brace_depth = brace_depth.saturating_sub(1); }
        }
        
        if c == ',' && !in_string && brace_depth == 0 {
            let transformed = transform_single_struct_field_recursive(&current);
            if !transformed.is_empty() {
                result.push(transformed);
            }
            current.clear();
        } else {
            current.push(c);
        }
        prev_char = c;
    }
    
    let transformed = transform_single_struct_field_recursive(&current);
    if !transformed.is_empty() {
        result.push(transformed);
    }
    
    if result.is_empty() {
        String::new()
    } else {
        format!(" {} ", result.join(", "))
    }
}

/// Transform a single struct field, recursively handling nested structs
fn transform_single_struct_field_recursive(field: &str) -> String {
    let trimmed = field.trim();
    if trimmed.is_empty() { return String::new(); }
    
    // Find = at TOP level only
    if let Some(eq_pos) = find_field_eq_top_level(trimmed) {
        let name = trimmed[..eq_pos].trim();
        let value = trimmed[eq_pos + 1..].trim().trim_end_matches(',');
        
        if is_valid_field_name(name) {
            let transformed_value = if value.contains('{') && value.contains('=') {
                transform_nested_struct_value(value)
            } else {
                value.to_string()
            };
            return format!("{}: {}", name, transformed_value);
        }
    }
    
    // If already has : (but not ::), it's already transformed
    if trimmed.contains(':') && !trimmed.contains("::") {
        if let Some(colon_pos) = trimmed.find(':') {
            if !trimmed[..colon_pos].contains("::") {
                let name = trimmed[..colon_pos].trim();
                let value = trimmed[colon_pos + 1..].trim().trim_end_matches(',');
                if value.contains('{') && value.contains('=') {
                    let transformed_value = transform_nested_struct_value(value);
                    return format!("{}: {}", name, transformed_value);
                }
            }
        }
        return trimmed.to_string();
    }
    
    trimmed.to_string()
}

/// Find field = at TOP LEVEL only (not inside nested braces)
pub fn find_field_eq_top_level(s: &str) -> Option<usize> {
    let chars: Vec<char> = s.chars().collect();
    let mut brace_depth: usize = 0;
    
    for i in 0..chars.len() {
        match chars[i] {
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '=' if brace_depth == 0 => {
                let prev = if i > 0 { chars[i-1] } else { ' ' };
                let next = if i + 1 < chars.len() { chars[i+1] } else { ' ' };
                
                if prev != '!' && prev != '<' && prev != '>' && prev != '=' && next != '=' && next != '>' {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Find the `=` that's a field assignment (not ==, !=, <=, >=, =>)
pub fn find_field_eq(s: &str) -> Option<usize> {
    let chars: Vec<char> = s.chars().collect();
    for i in 0..chars.len() {
        if chars[i] == '=' {
            let prev = if i > 0 { chars[i-1] } else { ' ' };
            let next = if i + 1 < chars.len() { chars[i+1] } else { ' ' };
            
            if prev != '!' && prev != '<' && prev != '>' && prev != '=' && next != '=' && next != '>' {
                return Some(i);
            }
        }
    }
    None
}

/// Check if a field name is valid (supports raw identifiers like r#type)
pub fn is_valid_field_name(s: &str) -> bool {
    if s.is_empty() { return false; }
    
    // Support Rust raw identifiers (r#keyword)
    let identifier = if s.starts_with("r#") && s.len() > 2 {
        &s[2..]
    } else {
        s
    };
    
    if identifier.is_empty() { return false; }
    let first = identifier.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' { return false; }
    identifier.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Check if a value is a string literal
pub fn is_string_literal(s: &str) -> bool {
    let t = s.trim();
    t.starts_with('"') && t.ends_with('"') && !t.contains("String::from")
}

/// Check if a value expression should have .clone() added
pub fn should_clone_field_value(value: &str) -> bool {
    let v = value.trim();
    
    if v.ends_with(".clone()") || v.ends_with(".to_vec()") {
        return false;
    }
    
    if v.contains("()") {
        return false;
    }
    
    if v.contains(" as ") {
        return false;
    }
    
    // Skip arithmetic operators
    if v.contains(" + ") || v.contains(" - ") || v.contains(" * ") || 
       v.contains(" / ") || v.contains(" % ") {
        return false;
    }
    
    // Skip comparison operators
    if v.contains(" == ") || v.contains(" != ") || v.contains(" < ") || 
       v.contains(" > ") || v.contains(" <= ") || v.contains(" >= ") {
        return false;
    }
    
    // Skip logical operators
    if v.contains(" && ") || v.contains(" || ") {
        return false;
    }
    
    // Skip bitwise operators
    if v.contains(" & ") || v.contains(" | ") || v.contains(" ^ ") || 
       v.contains(" << ") || v.contains(" >> ") {
        return false;
    }
    
    // Skip literals
    if v.starts_with('"') || v.parse::<i64>().is_ok() || v.parse::<f64>().is_ok() {
        return false;
    }
    if v == "true" || v == "false" {
        return false;
    }
    
    // Skip simple identifiers
    if !v.contains('.') {
        return false;
    }
    
    // Skip path expressions like TxType::Stake
    if v.contains("::") {
        return false;
    }
    
    // Skip array indexing
    if v.contains('[') {
        return false;
    }
    
    // This is a field access pattern like `from.address`
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_transform_literal_field_simple() {
        assert_eq!(transform_literal_field("    id = 1"), "    id: 1,");
        assert_eq!(transform_literal_field("    name = \"test\""), "    name: String::from(\"test\"),");
    }
    
    #[test]
    fn test_transform_literal_field_already_colon() {
        assert_eq!(transform_literal_field("    id: 1"), "    id: 1,");
        assert_eq!(transform_literal_field("    id: 1,"), "    id: 1,");
    }
    
    #[test]
    fn test_transform_literal_field_nested() {
        let input = "    header = Header { id = 1 }";
        let output = transform_literal_field(input);
        assert!(output.contains("header:"));
        assert!(output.contains("id: 1"));
    }
    
    #[test]
    fn test_find_field_eq() {
        assert_eq!(find_field_eq("x = 1"), Some(2));
        assert_eq!(find_field_eq("x == 1"), None);
        assert_eq!(find_field_eq("x != 1"), None);
        assert_eq!(find_field_eq("x <= 1"), None);
    }
    
    #[test]
    fn test_is_valid_field_name() {
        assert!(is_valid_field_name("id"));
        assert!(is_valid_field_name("_private"));
        assert!(is_valid_field_name("r#type")); // raw identifier
        assert!(!is_valid_field_name("123"));
        assert!(!is_valid_field_name(""));
    }
    
    #[test]
    fn test_is_string_literal() {
        assert!(is_string_literal("\"hello\""));
        assert!(!is_string_literal("String::from(\"hello\")"));
        assert!(!is_string_literal("123"));
    }
    
    #[test]
    fn test_transform_nested_struct_value() {
        let input = "Address { value = hash }";
        let output = transform_nested_struct_value(input);
        assert!(output.contains("value: hash"));
    }
}