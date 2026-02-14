//! Clone injection helpers for RustS+ transpiler (L-04 Enhancement)
//! 
//! Contains functions for:
//! - Array access clone transformation
//! - Type detection from array elements
//! - Clone-related utility functions

use crate::helpers::is_valid_identifier;

/// Transform array index access to add .clone() for non-Copy types
/// 
/// L-04 RULE: Array access on non-Copy elements MUST use explicit strategy
/// We choose `.clone()` as the deterministic strategy.
/// 
/// Examples:
/// - `events[i]` → `events[i].clone()`
/// - `arr[0]` → `arr[0].clone()`
/// 
/// EXCEPTIONS (no clone needed):
/// - Already has .clone() 
/// - Is a method call on indexed element: `arr[i].len()`
/// - Is a field access: `arr[i].field`
/// - Has `as` cast (e.g., `arr[i] as u64`)
pub fn transform_array_access_clone(value: &str) -> String {
    let trimmed = value.trim();
    
    // Skip if empty or already has clone
    if trimmed.is_empty() || trimmed.ends_with(".clone()") {
        return value.to_string();
    }
    
    // Skip if not a simple array index pattern
    if !trimmed.contains('[') || !trimmed.contains(']') {
        return value.to_string();
    }
    
    // CRITICAL FIX (Bug #3): Skip RANGE access patterns
    // Patterns like arr[0..32], data[start..end], buf[..N], buf[32..], data[0..=31]
    // These return &[T] (slice reference), NOT an individual element T.
    // Adding .clone() to a slice range is semantically wrong and can cause
    // type mismatches (e.g., self.keypair_bytes[0..32].clone() clones the
    // entire Vec/array, not extracting a 32-byte slice).
    if let Some(bracket_start) = trimmed.find('[') {
        if let Some(bracket_end) = trimmed.rfind(']') {
            if bracket_start < bracket_end {
                let inside_brackets = &trimmed[bracket_start + 1..bracket_end];
                if inside_brackets.contains("..") {
                    return value.to_string();
                }
            }
        }
    }
    
    // Skip if there's an `as` cast
    if trimmed.contains(" as ") {
        return value.to_string();
    }
    
    // Skip complex expressions
    if trimmed.contains(" + ") || trimmed.contains(" - ") || 
       trimmed.contains(" * ") || trimmed.contains(" / ") ||
       trimmed.contains(" && ") || trimmed.contains(" || ") {
        return value.to_string();
    }
    
    // Skip if there's a method call or field access after ]
    if let Some(bracket_end) = trimmed.rfind(']') {
        let after_bracket = &trimmed[bracket_end + 1..];
        if after_bracket.starts_with('.') {
            return value.to_string();
        }
    }
    
    // Skip string/char literals
    if trimmed.starts_with('"') || trimmed.starts_with('\'') {
        return value.to_string();
    }
    
    // Skip number literals
    if trimmed.parse::<i64>().is_ok() || trimmed.parse::<f64>().is_ok() {
        return value.to_string();
    }
    
    // Check for simple array index pattern
    let mut in_string = false;
    let mut bracket_start = None;
    let mut bracket_end = None;
    
    for (i, c) in trimmed.char_indices() {
        if c == '"' {
            in_string = !in_string;
        }
        if !in_string {
            if c == '[' && bracket_start.is_none() {
                bracket_start = Some(i);
            } else if c == ']' {
                bracket_end = Some(i);
            }
        }
    }
    
    if let (Some(start), Some(_end)) = (bracket_start, bracket_end) {
        let before_bracket = &trimmed[..start];
        if is_valid_array_base(before_bracket) {
            return format!("{}.clone()", trimmed);
        }
    }
    
    value.to_string()
}

/// Check if the base of an array access is a valid identifier or field access
pub fn is_valid_array_base(base: &str) -> bool {
    let trimmed = base.trim();
    if trimmed.is_empty() {
        return false;
    }
    
    // Simple identifier: events, arr, data
    if is_valid_identifier(trimmed) {
        return true;
    }
    
    // Field access: self.events, obj.data
    if trimmed.contains('.') {
        let parts: Vec<&str> = trimmed.split('.').collect();
        return parts.iter().all(|p| is_valid_identifier(p.trim()));
    }
    
    false
}

/// Extract the pattern part from a match arm line: `Pattern {` → `Pattern`
pub fn extract_arm_pattern(line: &str) -> String {
    let trimmed = line.trim();
    
    if let Some(brace_pos) = trimmed.rfind('{') {
        return trimmed[..brace_pos].trim().to_string();
    }
    
    trimmed.to_string()
}

/// Detect element type from array literal element
/// Examples:
/// - `Event::Credit { id = 1 }` → Some("Event")
/// - `Node { id = 1 }` → Some("Node")
/// - `123` → None (primitive)
/// - `"hello"` → None (primitive)
pub fn detect_type_from_element(element: &str) -> Option<String> {
    let trimmed = element.trim().trim_end_matches(',');
    
    // Skip empty, primitives
    if trimmed.is_empty() {
        return None;
    }
    
    // Skip string literals
    if trimmed.starts_with('"') {
        return None;
    }
    
    // Skip numeric literals
    if trimmed.parse::<i64>().is_ok() || trimmed.parse::<f64>().is_ok() {
        return None;
    }
    
    // Skip bool literals
    if trimmed == "true" || trimmed == "false" {
        return None;
    }
    
    // Pattern: Enum::Variant or Enum::Variant { ... } or Enum::Variant(...)
    if trimmed.contains("::") {
        if let Some(pos) = trimmed.find("::") {
            let type_name = trimmed[..pos].trim();
            if !type_name.is_empty() && type_name.chars().next().unwrap().is_uppercase() {
                return Some(type_name.to_string());
            }
        }
    }
    
    // Pattern: StructName { ... }
    if trimmed.contains('{') {
        if let Some(pos) = trimmed.find('{') {
            let type_name = trimmed[..pos].trim();
            if !type_name.is_empty() && type_name.chars().next().unwrap().is_uppercase() {
                return Some(type_name.to_string());
            }
        }
    }
    
    // Pattern: TupleStruct(...)
    if trimmed.contains('(') && !trimmed.starts_with('(') {
        if let Some(pos) = trimmed.find('(') {
            let type_name = trimmed[..pos].trim();
            if !type_name.is_empty() && type_name.chars().next().unwrap().is_uppercase() {
                return Some(type_name.to_string());
            }
        }
    }
    
    None
}

/// Extract array variable name from array access expression
/// `events[i]` → Some("events")
/// `self.data[0]` → Some("self.data")
pub fn extract_array_var_from_access(expr: &str) -> Option<String> {
    let trimmed = expr.trim();
    
    if !trimmed.contains('[') {
        return None;
    }
    
    if let Some(pos) = trimmed.find('[') {
        let var_name = trimmed[..pos].trim();
        if !var_name.is_empty() {
            return Some(var_name.to_string());
        }
    }
    
    None
}

/// Check if an expression is an array access that would get .clone()
pub fn is_cloneable_array_access(expr: &str) -> bool {
    let trimmed = expr.trim();
    
    // Must have brackets
    if !trimmed.contains('[') || !trimmed.contains(']') {
        return false;
    }
    
    // Skip if already has .clone()
    if trimmed.ends_with(".clone()") {
        return false;
    }
    
    // CRITICAL FIX: Skip range access (returns slice, not element)
    if let Some(bracket_start) = trimmed.find('[') {
        if let Some(bracket_end) = trimmed.rfind(']') {
            if bracket_start < bracket_end {
                let inside = &trimmed[bracket_start + 1..bracket_end];
                if inside.contains("..") {
                    return false;
                }
            }
        }
    }
    
    // Skip if has method call after ]
    if let Some(bracket_end) = trimmed.rfind(']') {
        let after = &trimmed[bracket_end + 1..];
        if after.starts_with('.') {
            return false;
        }
    }
    
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_transform_array_access_clone() {
        assert_eq!(transform_array_access_clone("events[i]"), "events[i].clone()");
        assert_eq!(transform_array_access_clone("arr[0]"), "arr[0].clone()");
        assert_eq!(transform_array_access_clone("arr[i].clone()"), "arr[i].clone()"); // no double
        assert_eq!(transform_array_access_clone("arr[i].len()"), "arr[i].len()"); // method call
        assert_eq!(transform_array_access_clone("arr[i] as u64"), "arr[i] as u64"); // cast
    }
    
    #[test]
    fn test_transform_array_access_clone_range_skip() {
        // CRITICAL (Bug #3): Range access returns &[T], NOT element T
        // Must NOT add .clone() to range slices
        assert_eq!(
            transform_array_access_clone("self.keypair_bytes[0..32]"), 
            "self.keypair_bytes[0..32]",
            "Range access must NOT get .clone()"
        );
        assert_eq!(
            transform_array_access_clone("data[start..end]"), 
            "data[start..end]",
            "Variable range must NOT get .clone()"
        );
        assert_eq!(
            transform_array_access_clone("buf[..16]"), 
            "buf[..16]",
            "Range-to must NOT get .clone()"
        );
        assert_eq!(
            transform_array_access_clone("buf[32..]"), 
            "buf[32..]",
            "Range-from must NOT get .clone()"
        );
        assert_eq!(
            transform_array_access_clone("data[0..=31]"), 
            "data[0..=31]",
            "Inclusive range must NOT get .clone()"
        );
    }
    
    #[test]
    fn test_is_valid_array_base() {
        assert!(is_valid_array_base("events"));
        assert!(is_valid_array_base("self.data"));
        assert!(!is_valid_array_base(""));
        assert!(!is_valid_array_base("123"));
    }
    
    #[test]
    fn test_detect_type_from_element() {
        assert_eq!(detect_type_from_element("Event::Credit { id = 1 }"), Some("Event".to_string()));
        assert_eq!(detect_type_from_element("Node { id = 1 }"), Some("Node".to_string()));
        assert_eq!(detect_type_from_element("123"), None);
        assert_eq!(detect_type_from_element("\"hello\""), None);
    }
    
    #[test]
    fn test_extract_array_var_from_access() {
        assert_eq!(extract_array_var_from_access("events[i]"), Some("events".to_string()));
        assert_eq!(extract_array_var_from_access("self.data[0]"), Some("self.data".to_string()));
        assert_eq!(extract_array_var_from_access("value"), None);
    }
    
    #[test]
    fn test_is_cloneable_array_access() {
        assert!(is_cloneable_array_access("arr[i]"));
        assert!(!is_cloneable_array_access("arr[i].clone()"));
        assert!(!is_cloneable_array_access("arr[i].len()"));
    }
}