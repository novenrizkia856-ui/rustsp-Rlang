//! Depth Tracking Lowering
//!
//! Utilities for tracking brace and bracket depth during transpilation.
//! These functions are CRITICAL for correctly handling nested constructs
//! and avoiding miscounting delimiters inside string literals.

/// Count opening and closing braces OUTSIDE of string literals
/// 
/// This is CRITICAL to avoid counting format placeholders like {} in "hello {} world"
/// 
/// # Returns
/// A tuple of (opening_count, closing_count)
pub fn count_braces_outside_strings(s: &str) -> (usize, usize) {
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
/// 
/// # Returns
/// A tuple of (opening_count, closing_count)
pub fn count_brackets_outside_strings(s: &str) -> (usize, usize) {
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

/// Update multiline expression depth based on parentheses and brackets
/// 
/// This tracks whether we're inside a multi-line expression like:
/// ```text
/// some_function(
///     arg1,
///     arg2
/// )
/// ```
pub fn update_multiline_depth(depth: &mut i32, trimmed: &str) {
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
                '(' | '[' => *depth += 1,
                ')' | ']' => *depth -= 1,
                _ => {}
            }
        }
    }
    
    // Clamp to 0 (shouldn't go negative in valid code)
    if *depth < 0 {
        *depth = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_count_braces_simple() {
        assert_eq!(count_braces_outside_strings("{"), (1, 0));
        assert_eq!(count_braces_outside_strings("}"), (0, 1));
        assert_eq!(count_braces_outside_strings("{}"), (1, 1));
        assert_eq!(count_braces_outside_strings("{{}}"), (2, 2));
    }
    
    #[test]
    fn test_count_braces_in_string() {
        // Braces inside string should NOT be counted
        assert_eq!(count_braces_outside_strings("\"{}\""), (0, 0));
        assert_eq!(count_braces_outside_strings("format!(\"hello {}\")"), (0, 0));
        assert_eq!(count_braces_outside_strings("let x = \"{}\";"), (0, 0));
    }
    
    #[test]
    fn test_count_braces_mixed() {
        // Struct { field: "value {} more" }
        assert_eq!(count_braces_outside_strings("Struct { field: \"value {} more\" }"), (1, 1));
    }
    
    #[test]
    fn test_count_brackets() {
        assert_eq!(count_brackets_outside_strings("[1, 2, 3]"), (1, 1));
        assert_eq!(count_brackets_outside_strings("\"[not a bracket]\""), (0, 0));
    }
    
    #[test]
    fn test_multiline_depth() {
        let mut depth = 0;
        update_multiline_depth(&mut depth, "func(");
        assert_eq!(depth, 1);
        
        update_multiline_depth(&mut depth, "arg1,");
        assert_eq!(depth, 1);
        
        update_multiline_depth(&mut depth, ")");
        assert_eq!(depth, 0);
    }
}