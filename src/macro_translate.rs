//! Macro Translation
//!
//! Transforms function-style macro calls to correct Rust macro syntax.
//!
//! Examples:
//! - `anyhow("msg")` → `anyhow!("msg")`
//! - `unreachable()` → `unreachable!()`
//! - `vec(1, 2, 3)` → `vec!(1, 2, 3)`

/// List of common macros that users might accidentally call as functions
/// 
/// CRITICAL: Only include macros that are NEVER used as methods or attributes
/// DO NOT include: write, writeln (RwLock methods), cfg (attribute)
const MACROS_TO_TRANSFORM: &[&str] = &[
    "anyhow",
    "unreachable", 
    "unimplemented",
    "panic",
    "todo",
    "format",
    "vec",
    "println",
    "eprintln",
    "dbg",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "matches",      // CRITICAL: matches! macro for pattern matching
    "bail",         // anyhow::bail!
    "ensure",       // anyhow::ensure!
    "include_str",
    "include_bytes",
    "concat",
    "stringify",
    // NOTE: Removed 'write', 'writeln', 'cfg' - these conflict with methods/attributes
];

/// Transform function-style macro calls to correct Rust macro syntax
pub fn transform_macros_to_correct_syntax(code: &str) -> String {
    let mut result = code.to_string();
    
    for macro_name in MACROS_TO_TRANSFORM {
        result = transform_single_macro(&result, macro_name);
    }
    
    result
}

/// Transform a single macro from function-style to macro-style
fn transform_single_macro(code: &str, macro_name: &str) -> String {
    let pattern = format!("{}(", macro_name);
    
    let mut new_result = String::new();
    let chars: Vec<char> = code.chars().collect();
    let mut i = 0;
    
    while i < chars.len() {
        // Check if we're at the start of the pattern
        let remaining: String = chars[i..].iter().collect();
        if remaining.starts_with(&pattern) {
            // Check character before is not alphanumeric (word boundary)
            let prev_char = if i > 0 { chars[i - 1] } else { ' ' };
            let is_word_boundary = !prev_char.is_alphanumeric() && prev_char != '_';
            
            // Check it's not already `!(`
            let already_macro = i > 0 && chars[i - 1] == '!';
            
            // CRITICAL: Check it's NOT a method call (preceded by `.`)
            let is_method_call = prev_char == '.';
            
            // CRITICAL: Check it's NOT in an attribute context (preceded by `[` or `#[`)
            let is_attribute = prev_char == '[' || 
                (i >= 2 && chars[i - 2] == '#' && chars[i - 1] == '[');
            
            if is_word_boundary && !already_macro && !is_method_call && !is_attribute {
                // Insert `!` before `(`
                new_result.push_str(macro_name);
                new_result.push('!');
                i += macro_name.len(); // Skip past macro name, next iteration will add `(`
                continue;
            }
        }
        
        new_result.push(chars[i]);
        i += 1;
    }
    
    new_result
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_transform_anyhow() {
        assert_eq!(
            transform_macros_to_correct_syntax("anyhow(\"error\")"),
            "anyhow!(\"error\")"
        );
    }
    
    #[test]
    fn test_transform_unreachable() {
        assert_eq!(
            transform_macros_to_correct_syntax("unreachable()"),
            "unreachable!()"
        );
    }
    
    #[test]
    fn test_transform_vec() {
        assert_eq!(
            transform_macros_to_correct_syntax("let x = vec(1, 2, 3)"),
            "let x = vec!(1, 2, 3)"
        );
    }
    
    #[test]
    fn test_already_macro() {
        // Should not double-transform
        assert_eq!(
            transform_macros_to_correct_syntax("anyhow!(\"error\")"),
            "anyhow!(\"error\")"
        );
    }
    
    #[test]
    fn test_method_call_not_transformed() {
        // Method calls should NOT be transformed
        assert_eq!(
            transform_macros_to_correct_syntax("lock.write()"),
            "lock.write()"
        );
    }
    
    #[test]
    fn test_longer_identifier_not_transformed() {
        // `my_panic()` should not become `my_panic!()`
        assert_eq!(
            transform_macros_to_correct_syntax("my_panic()"),
            "my_panic()"
        );
    }
    
    #[test]
    fn test_matches_macro() {
        assert_eq!(
            transform_macros_to_correct_syntax("if matches(x, Some(_))"),
            "if matches!(x, Some(_))"
        );
    }
    
    #[test]
    fn test_format_in_expression() {
        assert_eq!(
            transform_macros_to_correct_syntax("let s = format(\"hello {}\", name)"),
            "let s = format!(\"hello {}\", name)"
        );
    }
}