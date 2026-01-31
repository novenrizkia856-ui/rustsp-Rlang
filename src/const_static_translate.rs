//! Const/Static Declaration Translation
//!
//! Translates RustS+ const/static declarations to Rust syntax.
//!
//! RustS+ syntax: `const NAME TYPE = VALUE` (no colon between name and type)
//! Rust syntax:   `const NAME: TYPE = VALUE;` (colon required, semicolon at end)
//!
//! Handles:
//! - `const NAME TYPE = VALUE` → `const NAME: TYPE = VALUE;`
//! - `static NAME TYPE = VALUE` → `static NAME: TYPE = VALUE;`
//! - `pub const NAME TYPE = VALUE` → `pub const NAME: TYPE = VALUE;`
//! - `pub static NAME TYPE = VALUE` → `pub static NAME: TYPE = VALUE;`
//! - `pub static mut NAME TYPE = VALUE` → `pub static mut NAME: TYPE = VALUE;`

use crate::helpers::transform_generic_brackets;

/// Transform RustS+ const/static declarations to Rust syntax.
/// 
/// Returns None if:
/// - Not a const/static declaration
/// - Already in Rust syntax (has colon before `=`)
pub fn transform_const_or_static(trimmed: &str) -> Option<String> {
    // Quick check: must contain const or static
    if !trimmed.contains("const ") && !trimmed.contains("static ") {
        return None;
    }
    
    // Must contain `=` for value assignment
    if !trimmed.contains('=') {
        return None;
    }
    
    // Parse the declaration
    // Pattern: [pub] [static [mut] | const] NAME TYPE = VALUE
    
    let mut rest = trimmed;
    let mut prefix = String::new();
    
    // Check for `pub`
    if rest.starts_with("pub ") {
        prefix.push_str("pub ");
        rest = rest.strip_prefix("pub ").unwrap().trim();
    }
    
    // Check for `const` or `static`
    let keyword = if rest.starts_with("const ") {
        rest = rest.strip_prefix("const ").unwrap().trim();
        "const"
    } else if rest.starts_with("static mut ") {
        rest = rest.strip_prefix("static mut ").unwrap().trim();
        "static mut"
    } else if rest.starts_with("static ") {
        rest = rest.strip_prefix("static ").unwrap().trim();
        "static"
    } else {
        return None;
    };
    
    // Now rest should be: `NAME TYPE = VALUE` or `NAME: TYPE = VALUE`
    
    // Find the `=` sign
    let eq_pos = rest.find('=')?;
    let before_eq = rest[..eq_pos].trim();
    let after_eq = rest[eq_pos + 1..].trim();
    
    // Check if already in Rust syntax (has colon before =)
    // This includes patterns like `NAME: TYPE` or `NAME: &'static str`
    if before_eq.contains(':') {
        // Already Rust syntax - ensure semicolon at end UNLESS multi-line
        let trimmed_input = trimmed.trim_end_matches(';');
        // CRITICAL FIX: Don't add semicolon if multi-line declaration
        let is_multiline = trimmed_input.trim().ends_with('[') || trimmed_input.trim().ends_with('{');
        let suffix = if is_multiline { "" } else { ";" };
        return Some(format!("{}{}", trimmed_input, suffix));
    }
    
    // RustS+ syntax: NAME TYPE (space-separated, no colon)
    // Find the NAME (first identifier) and TYPE (rest before =)
    let parts: Vec<&str> = before_eq.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    
    let name = parts[0];
    // Type is everything after the name
    let type_str = parts[1..].join(" ");
    
    // Validate name is a valid identifier
    let first_char = name.chars().next()?;
    if name.is_empty() || (!first_char.is_alphabetic() && first_char != '_') {
        return None;
    }
    
    // Transform type (Vec[T] → Vec<T>)
    let transformed_type = transform_generic_brackets(&type_str);
    
    // Value without trailing semicolon (we'll add our own if needed)
    let value = after_eq.trim_end_matches(';');
    
    // CRITICAL FIX: Don't add semicolon if this is a multi-line declaration
    // (value ends with `[` or `{` indicating array/struct literal continues on next line)
    let is_multiline_start = value.trim().ends_with('[') || value.trim().ends_with('{');
    let suffix = if is_multiline_start { "" } else { ";" };
    
    Some(format!("{}{} {}: {} = {}{}", prefix, keyword, name, transformed_type, value, suffix))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_const_transform() {
        assert_eq!(
            transform_const_or_static("const MAX_SIZE usize = 100"),
            Some("const MAX_SIZE: usize = 100;".to_string())
        );
    }
    
    #[test]
    fn test_pub_const_transform() {
        assert_eq!(
            transform_const_or_static("pub const NAME &str = \"test\""),
            Some("pub const NAME: &str = \"test\";".to_string())
        );
    }
    
    #[test]
    fn test_static_transform() {
        assert_eq!(
            transform_const_or_static("static COUNTER AtomicU64 = AtomicU64::new(0)"),
            Some("static COUNTER: AtomicU64 = AtomicU64::new(0);".to_string())
        );
    }
    
    #[test]
    fn test_static_mut_transform() {
        assert_eq!(
            transform_const_or_static("pub static mut CONFIG Option[Config] = None"),
            Some("pub static mut CONFIG: Option<Config> = None;".to_string())
        );
    }
    
    #[test]
    fn test_already_rust_syntax() {
        // Already has colon - just ensure semicolon
        assert_eq!(
            transform_const_or_static("const X: i32 = 1"),
            Some("const X: i32 = 1;".to_string())
        );
    }
    
    #[test]
    fn test_multiline_no_semicolon() {
        // Multi-line start should not add semicolon
        assert_eq!(
            transform_const_or_static("const ITEMS Vec[i32] = vec!["),
            Some("const ITEMS: Vec<i32> = vec![".to_string())
        );
    }
    
    #[test]
    fn test_not_const_static() {
        assert_eq!(transform_const_or_static("let x = 1"), None);
        assert_eq!(transform_const_or_static("fn foo() {}"), None);
    }
}