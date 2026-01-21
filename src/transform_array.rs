//! Array literal transformation functions for RustS+ transpiler
//! 
//! Contains functions for transforming array literal elements:
//! - Element transformation with commas
//! - Enum struct init transformation in arrays
//! - Field transformation within array elements

use crate::transform_literal::{find_field_eq, is_valid_field_name};

/// Transform an array element line - handle enum struct variants, string literals, etc.
pub fn transform_array_element(line: &str) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // Empty line
    if trimmed.is_empty() {
        return String::new();
    }
    
    // Closing bracket
    if trimmed == "]" || trimmed == "];" {
        return format!("{}]", leading_ws);
    }
    
    // Comments pass through
    if trimmed.starts_with("//") {
        return line.to_string();
    }
    
    // Transform enum struct init: Event::C { x = 4 } -> Event::C { x: 4 }
    let transformed = transform_enum_struct_init_in_array(trimmed);
    
    // Ensure element ends with comma (unless it's just a closing bracket)
    let with_comma = if transformed.ends_with(',') || transformed.ends_with('{') 
                        || transformed.ends_with('[') || transformed == "]" {
        transformed
    } else {
        format!("{},", transformed)
    };
    
    format!("{}{}", leading_ws, with_comma)
}

/// Transform enum struct variant inside array: Event::C { x = 4 } -> Event::C { x: 4 }
pub fn transform_enum_struct_init_in_array(s: &str) -> String {
    // Handle BOTH enum struct variants (Event::C { x = 1 })
    // AND plain struct literals (BlobRef { hash = x })
    if !s.contains('{') {
        return s.to_string();
    }
    
    // Need = inside braces to transform
    if let Some(brace_start) = s.find('{') {
        let after_brace = &s[brace_start..];
        let has_assignment_eq = has_assignment_equals_in_braces(after_brace);
        if !has_assignment_eq {
            return s.to_string();
        }
    }
    
    // Find { and transform fields inside
    if let Some(brace_start) = s.find('{') {
        let before = &s[..brace_start + 1];
        let after_brace = &s[brace_start + 1..];
        
        // Find matching }
        if let Some(brace_end) = after_brace.rfind('}') {
            let fields_part = &after_brace[..brace_end];
            let after_close = &after_brace[brace_end..];
            
            // Transform fields: x = 1 -> x: 1
            let transformed_fields = transform_enum_fields_inline(fields_part);
            return format!("{}{}{}", before, transformed_fields, after_close);
        }
    }
    
    s.to_string()
}

/// Check if there's an assignment = inside braces (not == or !=)
pub fn has_assignment_equals_in_braces(s: &str) -> bool {
    let mut in_string = false;
    let mut prev_char = ' ';
    let chars: Vec<char> = s.chars().collect();
    
    for (i, &c) in chars.iter().enumerate() {
        if c == '"' && prev_char != '\\' {
            in_string = !in_string;
        }
        
        if !in_string && c == '=' {
            // Check it's not == or != or <= or >=
            let next = chars.get(i + 1).copied().unwrap_or(' ');
            if prev_char != '!' && prev_char != '=' && prev_char != '<' && prev_char != '>' 
               && next != '=' && next != '>' {
                return true;
            }
        }
        
        prev_char = c;
    }
    
    false
}

/// Transform inline enum fields: "x = 1, y = 2" → "x: 1, y: 2"
pub fn transform_enum_fields_inline(fields: &str) -> String {
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
            let transformed = transform_single_enum_field(&current);
            if !transformed.is_empty() {
                result.push(transformed);
            }
            current.clear();
        } else {
            current.push(c);
        }
    }
    
    // Last field
    let transformed = transform_single_enum_field(&current);
    if !transformed.is_empty() {
        result.push(transformed);
    }
    
    result.join(", ")
}

/// Transform a single enum field: `x = 1` → `x: 1`
/// Also handles nested struct literals recursively!
fn transform_single_enum_field(field: &str) -> String {
    let trimmed = field.trim();
    if trimmed.is_empty() { return String::new(); }
    
    // Already transformed (has : but not ::)
    if trimmed.contains(':') && !trimmed.contains("::") && !trimmed.contains('=') { 
        return trimmed.to_string(); 
    }
    
    // Find = that's not ==, !=, etc
    if let Some(eq_pos) = find_field_eq(trimmed) {
        let name = trimmed[..eq_pos].trim();
        let value = trimmed[eq_pos + 1..].trim();
        
        if is_valid_field_name(name) {
            // Recursively transform nested struct literals in value
            let transformed_value = transform_nested_struct_value_in_array(value);
            return format!("{}: {}", name, transformed_value);
        }
    }
    
    // Even if no = found at top level, might have nested struct with =
    if trimmed.contains('{') && trimmed.contains('=') {
        return transform_nested_struct_value_in_array(trimmed);
    }
    
    trimmed.to_string()
}

/// Transform nested struct literals recursively within arrays
fn transform_nested_struct_value_in_array(value: &str) -> String {
    let trimmed = value.trim();
    
    if !trimmed.contains('{') || !trimmed.contains('=') {
        return trimmed.to_string();
    }
    
    if let Some(brace_start) = trimmed.find('{') {
        let before_brace = &trimmed[..brace_start + 1];
        let after_brace = &trimmed[brace_start + 1..];
        
        // Find matching closing brace
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
        
        let transformed_fields = transform_enum_fields_inline(fields_part);
        return format!("{} {} {}", before_brace, transformed_fields, after_close);
    }
    
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_transform_array_element() {
        assert_eq!(transform_array_element("    value"), "    value,");
        assert_eq!(transform_array_element("    value,"), "    value,");
        assert_eq!(transform_array_element("    ]"), "    ]");
    }
    
    #[test]
    fn test_transform_enum_struct_init_in_array() {
        let input = "Event::C { x = 4 }";
        let output = transform_enum_struct_init_in_array(input);
        assert!(output.contains("x: 4"));
        
        // Plain struct literal
        let input = "BlobRef { hash = x }";
        let output = transform_enum_struct_init_in_array(input);
        assert!(output.contains("hash: x"));
    }
    
    #[test]
    fn test_has_assignment_equals_in_braces() {
        assert!(has_assignment_equals_in_braces("{ x = 1 }"));
        assert!(!has_assignment_equals_in_braces("{ x == 1 }"));
        assert!(!has_assignment_equals_in_braces("{ x: 1 }"));
    }
    
    #[test]
    fn test_transform_enum_fields_inline() {
        assert_eq!(transform_enum_fields_inline(" x = 1, y = 2 "), "x: 1, y: 2");
        assert_eq!(transform_enum_fields_inline(" x: 1, y: 2 "), "x: 1, y: 2");
    }
}