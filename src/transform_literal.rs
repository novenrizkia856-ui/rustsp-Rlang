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
    
    // CRITICAL FIX: Handle array closing bracket inside struct literal
    // `]` or `],` inside a struct literal needs trailing comma for valid Rust
    if trimmed == "]" || trimmed == "]," {
        return format!("{}],", leading_ws);
    }
    
    // CRITICAL FIX: Check for `field: value` syntax PROPERLY
    // The old code used `trimmed.contains(':')` which matched `:` inside URLs like "http://..."
    // New logic: Check if there's a `:` BEFORE any `=` and OUTSIDE string literals
    if let Some(colon_pos) = find_field_colon_position(trimmed) {
        // This line has `field: value` syntax (colon is outside strings and before any =)
        // Check if there's a nested struct that needs transformation
        if trimmed.contains('{') && trimmed.contains('=') {
            let field = trimmed[..colon_pos].trim();
            let value = trimmed[colon_pos + 1..].trim().trim_end_matches(',');
            let transformed_value = transform_nested_struct_value(value);
            return format!("{}{}: {},", leading_ws, field, transformed_value);
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
            
            // CRITICAL FIX: Don't add trailing comma when value ends with `[`
            // This means it's a multi-line array start: `public_key = [`
            // The comma would produce invalid `public_key: [,`
            let tv = transformed_value.trim_end();
            let is_multiline_start = tv.ends_with('[') || tv.ends_with("vec![") || tv.ends_with("Vec::from([");
            let suffix = if is_multiline_start { "" } else { "," };
            return format!("{}{}: {}{}", leading_ws, field, transformed_value, suffix);
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

/// CRITICAL FIX: Find the position of `:` in `field: value` syntax
/// Returns Some(position) if there's a valid field colon, None otherwise
/// 
/// This function checks:
/// 1. The `:` is OUTSIDE string literals
/// 2. The `:` is NOT part of `::` (path separator)
/// 3. The `:` appears BEFORE any `=` (so it's not `field = "url:port"`)
///
/// Examples:
/// - `field: value` → Some(5)
/// - `field = "http://localhost"` → None (: is inside string)
/// - `std::fmt::Display` → None (: is part of ::)
/// - `field: Struct { x = 1 }` → Some(5)
pub fn find_field_colon_position(s: &str) -> Option<usize> {
    let chars: Vec<char> = s.chars().collect();
    let mut in_string = false;
    let mut first_eq_pos: Option<usize> = None;
    
    // First pass: find the first `=` (assignment) position outside strings
    for i in 0..chars.len() {
        if chars[i] == '"' {
            in_string = !in_string;
            continue;
        }
        if !in_string && chars[i] == '=' {
            let prev = if i > 0 { chars[i-1] } else { ' ' };
            let next = if i + 1 < chars.len() { chars[i+1] } else { ' ' };
            // Check it's a simple = not ==, !=, <=, >=, =>
            if prev != '!' && prev != '<' && prev != '>' && prev != '=' && next != '=' && next != '>' {
                first_eq_pos = Some(i);
                break;
            }
        }
    }
    
    // Second pass: find `:` that is BEFORE the first `=` and OUTSIDE strings
    in_string = false;
    for i in 0..chars.len() {
        // If we've reached the first `=`, stop searching
        if let Some(eq_pos) = first_eq_pos {
            if i >= eq_pos {
                return None; // No valid field colon found before =
            }
        }
        
        if chars[i] == '"' {
            in_string = !in_string;
            continue;
        }
        
        if !in_string && chars[i] == ':' {
            // Check it's not part of ::
            let prev = if i > 0 { chars[i-1] } else { ' ' };
            let next = if i + 1 < chars.len() { chars[i+1] } else { ' ' };
            
            if prev != ':' && next != ':' {
                // Valid field colon found!
                return Some(i);
            }
        }
    }
    
    // If no `=` was found, check if there's a standalone `:` (already Rust syntax)
    if first_eq_pos.is_none() {
        in_string = false;
        for i in 0..chars.len() {
            if chars[i] == '"' {
                in_string = !in_string;
                continue;
            }
            if !in_string && chars[i] == ':' {
                let prev = if i > 0 { chars[i-1] } else { ' ' };
                let next = if i + 1 < chars.len() { chars[i+1] } else { ' ' };
                if prev != ':' && next != ':' {
                    return Some(i);
                }
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
    
    // =========================================================================
    // CRITICAL BUG FIX TESTS
    // =========================================================================
    
    /// CRITICAL: URL colons inside strings must NOT confuse field detection
    /// Bug: `rpc_url = "http://localhost:26658".to_string()` was NOT transformed
    /// because the `:` in the URL triggered the "already has colon" check
    #[test]
    fn test_url_colon_in_value_string() {
        // The `:` in "http://localhost:26658" should NOT be treated as field: syntax
        let input = "    rpc_url = \"http://localhost:26658\".to_string()";
        let output = transform_literal_field(input);
        assert_eq!(output, "    rpc_url: \"http://localhost:26658\".to_string(),");
    }
    
    #[test]
    fn test_url_colon_various_formats() {
        // IP:port format
        assert_eq!(
            transform_literal_field("    url = \"http://127.0.0.1:8080\""),
            "    url: String::from(\"http://127.0.0.1:8080\"),"
        );
        
        // Hostname:port format
        assert_eq!(
            transform_literal_field("    endpoint = \"ws://celestia:26658\""),
            "    endpoint: String::from(\"ws://celestia:26658\"),"
        );
    }
    
    #[test]
    fn test_find_field_colon_position() {
        // Already has field: syntax - should find the colon
        assert_eq!(find_field_colon_position("field: value"), Some(5));
        assert_eq!(find_field_colon_position("name: \"test\""), Some(4));
        
        // Has `=` with colon in string - should return None (no field colon)
        assert_eq!(find_field_colon_position("url = \"http://localhost\""), None);
        assert_eq!(find_field_colon_position("rpc = \"ws://host:8080\""), None);
        
        // Path separator :: should NOT be detected
        assert_eq!(find_field_colon_position("std::fmt::Display"), None);
        assert_eq!(find_field_colon_position("value = Type::Variant"), None);
    }
    
    #[test]
    fn test_mixed_colon_scenarios() {
        // Already Rust syntax with method call containing colon
        assert_eq!(
            transform_literal_field("    id: std::process::id()"),
            "    id: std::process::id(),"
        );
        
        // Field: followed by URL (already transformed)
        assert_eq!(
            transform_literal_field("    endpoint: \"http://localhost:8080\""),
            "    endpoint: \"http://localhost:8080\","
        );
    }
    
    // =========================================================================
    // CRITICAL BUG FIX: Array literal inside struct literal
    // =========================================================================
    
    /// CRITICAL: When a struct field value is `[` (multi-line array start),
    /// the trailing comma must NOT be added, otherwise we get `field: [,`
    /// which is invalid Rust syntax.
    #[test]
    fn test_array_literal_start_in_struct_field() {
        // Multi-line array start - NO comma after [
        assert_eq!(
            transform_literal_field("    public_key = ["),
            "    public_key: ["
        );
    }
    
    /// CRITICAL: `]` inside struct literal should get trailing comma
    #[test]
    fn test_array_literal_close_in_struct_field() {
        assert_eq!(
            transform_literal_field("    ]"),
            "    ],"
        );
        assert_eq!(
            transform_literal_field("    ],"),
            "    ],"
        );
    }
}