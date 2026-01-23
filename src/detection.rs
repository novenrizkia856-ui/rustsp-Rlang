//! Detection functions for RustS+ literal patterns
//! 
//! Contains functions to detect the start of various literal expressions:
//! - Struct literals: `x = StructName { ... }`
//! - Enum struct variants: `x = Enum::Variant { ... }`
//! - Array literals: `x = [...]`
//! - Bare literals (return expressions without assignment)
//!
//! CRITICAL FIX: All detection functions must ignore braces inside string literals!
//! Example: `anyhow::bail("header {} mismatch")` should NOT trigger literal detection
//! because the `{` is inside a string.

use crate::helpers::{is_rust_block_start, is_valid_identifier};
use crate::struct_def::StructRegistry;

//===========================================================================
// STRING-AWARE BRACE DETECTION HELPERS
// These helpers find braces OUTSIDE of string literals only
//===========================================================================

/// Find the position of the first `{` that is NOT inside a string literal
/// Returns None if no such brace exists
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

/// Check if string contains `{` OUTSIDE of string literals
fn contains_brace_outside_string(s: &str) -> bool {
    find_brace_outside_string(s).is_some()
}

/// Check if string contains `::` followed by identifier and then `{` outside strings
/// This specifically detects enum variant patterns like `Enum::Variant {`
/// but NOT macro calls like `anyhow::bail("format {}")`
fn has_enum_variant_pattern(s: &str) -> bool {
    // First check if :: exists
    if !s.contains("::") {
        return false;
    }
    
    // Find position of first { outside strings
    let brace_pos = match find_brace_outside_string(s) {
        Some(pos) => pos,
        None => return false,
    };
    
    // Check what's between :: and {
    // For valid enum variant: `Enum::Variant {` - no `(` between :: and {
    // For macro calls: `anyhow::bail("...")` - has `(` between :: and {
    let before_brace = &s[..brace_pos];
    
    // Find the last :: before the brace
    if let Some(double_colon_pos) = before_brace.rfind("::") {
        let between = &before_brace[double_colon_pos + 2..];
        let between_trimmed = between.trim();
        
        // If there's a `(` in between, it's a function/macro call, not enum variant
        if between_trimmed.contains('(') {
            return false;
        }
        
        // Check if what's between is a valid identifier (variant name)
        // It should be alphanumeric/underscore only, possibly with whitespace
        let identifier: String = between_trimmed.chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        
        if !identifier.is_empty() && identifier.chars().next().unwrap().is_uppercase() {
            return true;
        }
    }
    
    false
}

//===========================================================================
// STRUCT LITERAL DETECTION
//===========================================================================

/// Detect if line starts a struct literal: `varname = StructName {`
/// Returns (var_name, struct_name) if matched, excludes Enum::Variant
pub fn detect_struct_literal_start(line: &str, registry: &StructRegistry) -> Option<(String, String)> {
    let trimmed = line.trim();
    
    // CRITICAL FIX: EXCLUDE function definitions and other Rust blocks
    if is_rust_block_start(trimmed) {
        return None;
    }
    
    // CRITICAL FIX: Use string-aware brace detection
    if !trimmed.contains('=') || !contains_brace_outside_string(trimmed) {
        return None;
    }
    
    let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
    if parts.len() != 2 { return None; }
    
    let var_name = parts[0].trim();
    let rhs = parts[1].trim();
    
    // CRITICAL FIX: Find brace outside strings in RHS
    let brace_pos = match find_brace_outside_string(rhs) {
        Some(pos) => pos,
        None => return None,
    };
    
    // EXCLUDE enum paths (:: before {)
    let before_brace = &rhs[..brace_pos];
    if before_brace.contains("::") {
        return None;
    }
    
    let struct_name: String = rhs
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    
    // Registry check or PascalCase heuristic
    if registry.is_struct(&struct_name) || 
       (!struct_name.is_empty() && struct_name.chars().next().unwrap().is_uppercase()) {
        return Some((var_name.to_string(), struct_name));
    }
    
    None
}

/// Detect BARE struct literal (without assignment): `StructName {`
/// Used for return expressions like: `Packet { header = ... }`
pub fn detect_bare_struct_literal(line: &str, registry: &StructRegistry) -> Option<String> {
    let trimmed = line.trim();
    
    // CRITICAL FIX: EXCLUDE function definitions and other Rust blocks
    if is_rust_block_start(trimmed) {
        return None;
    }
    
    // CRITICAL FIX: Must have { outside string literals
    let brace_pos = match find_brace_outside_string(trimmed) {
        Some(pos) => pos,
        None => return None,
    };
    
    let before_brace = &trimmed[..brace_pos];
    
    // If there's a = BEFORE {, it's an assignment, not bare literal
    if before_brace.contains('=') {
        return None;
    }
    
    // EXCLUDE enum paths (has ::)
    if before_brace.contains("::") {
        return None;
    }
    
    // EXCLUDE function/macro calls (has `(`)
    if before_brace.contains('(') {
        return None;
    }
    
    let struct_name = before_brace.trim();
    
    // Validate it's a struct name (PascalCase or in registry)
    if !struct_name.is_empty() && 
       (registry.is_struct(struct_name) || 
        struct_name.chars().next().unwrap().is_uppercase()) &&
       is_valid_identifier(struct_name) {
        return Some(struct_name.to_string());
    }
    
    None
}

//===========================================================================
// ENUM LITERAL DETECTION
//===========================================================================

/// Detect BARE enum struct variant literal (without assignment): `Enum::Variant {`
/// 
/// CRITICAL: Must NOT match macro calls like `anyhow::bail("format {}")`
pub fn detect_bare_enum_literal(line: &str) -> Option<String> {
    let trimmed = line.trim();
    
    // CRITICAL FIX: EXCLUDE function definitions and other Rust blocks
    if is_rust_block_start(trimmed) {
        return None;
    }
    
    // EXCLUDE match arms: `Event::Data { id, body } =>`
    if trimmed.contains("=>") {
        return None;
    }
    
    // Must have :: 
    if !trimmed.contains("::") {
        return None;
    }
    
    // CRITICAL FIX: Must have { OUTSIDE string literals
    let brace_pos = match find_brace_outside_string(trimmed) {
        Some(pos) => pos,
        None => return None,
    };
    
    let before_brace = &trimmed[..brace_pos];
    
    // If there's a = BEFORE {, it's an assignment
    if before_brace.contains('=') {
        return None;
    }
    
    // CRITICAL FIX: If there's a `(` before {, it's a function/macro call
    // Example: `anyhow::bail("header {}")` has `(` between `bail` and the first real `{`
    // We need to check if the `(` opens a call that contains the `{`
    if before_brace.contains('(') {
        // This could be a function/macro call, not enum variant
        // Check if it looks like: `path::name(args` where args might contain `{`
        return None;
    }
    
    let enum_path = before_brace.trim();
    if !enum_path.is_empty() && enum_path.contains("::") {
        // Additional validation: the part after :: should be a valid identifier
        // starting with uppercase (enum variant naming convention)
        if let Some(last_colon_pos) = enum_path.rfind("::") {
            let variant = &enum_path[last_colon_pos + 2..].trim();
            if !variant.is_empty() {
                let first_char = variant.chars().next().unwrap();
                // Enum variants typically start with uppercase
                // Macros like `bail`, `anyhow` start with lowercase
                if first_char.is_uppercase() {
                    return Some(enum_path.to_string());
                }
            }
        }
    }
    
    None
}

/// Detect if line starts an enum struct variant literal: `varname = Enum::Variant {`
pub fn detect_enum_literal_start(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    
    // CRITICAL FIX: EXCLUDE function definitions and other Rust blocks
    if is_rust_block_start(trimmed) {
        return None;
    }
    
    // EXCLUDE match arms: `Event::Data { id, body } =>`
    if trimmed.contains("=>") {
        return None;
    }
    
    // Must have = and ::
    if !trimmed.contains('=') || !trimmed.contains("::") {
        return None;
    }
    
    // CRITICAL FIX: Must have { OUTSIDE string literals  
    if !contains_brace_outside_string(trimmed) {
        return None;
    }
    
    let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
    if parts.len() != 2 { return None; }
    
    let var_name = parts[0].trim();
    let rhs = parts[1].trim();
    
    // CRITICAL FIX: Find brace outside strings in RHS
    let brace_pos = match find_brace_outside_string(rhs) {
        Some(pos) => pos,
        None => return None,
    };
    
    let before_brace = rhs[..brace_pos].trim();
    
    // CRITICAL FIX: If there's a `(` before {, it's likely a function call
    if before_brace.contains('(') {
        return None;
    }
    
    if before_brace.contains("::") {
        // Validate variant name starts with uppercase
        if let Some(last_colon_pos) = before_brace.rfind("::") {
            let variant = &before_brace[last_colon_pos + 2..].trim();
            if !variant.is_empty() && variant.chars().next().unwrap().is_uppercase() {
                return Some((var_name.to_string(), before_brace.to_string()));
            }
        }
    }
    
    None
}

//===========================================================================
// ARRAY LITERAL DETECTION
// Detects array literal start: `var = [` or `var = vec![` where bracket is not closed on same line
//===========================================================================

/// Detect if line starts an array literal: `varname = [` or `varname = vec![`
/// Returns (var_name, var_type, remaining_content) if matched
pub fn detect_array_literal_start(line: &str) -> Option<(String, Option<String>, String)> {
    let trimmed = line.trim();
    
    // Must have = and [
    if !trimmed.contains('=') || !trimmed.contains('[') {
        return None;
    }
    
    // Split by first =
    let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
    if parts.len() != 2 { return None; }
    
    let left = parts[0].trim();
    let rhs = parts[1].trim();
    
    // CRITICAL FIX: RHS can start with [ OR vec![
    // Determine the content after the opening bracket
    let (starts_array, after_bracket) = if rhs.starts_with('[') {
        // Direct array: `[...]`
        (true, &rhs[1..])
    } else if rhs.starts_with("vec![") {
        // Vec macro: `vec![...]`
        (true, &rhs[5..])
    } else if rhs.starts_with("Vec::from([") {
        // Vec::from: `Vec::from([...])`
        (true, &rhs[11..])
    } else {
        (false, rhs)
    };
    
    if !starts_array {
        return None;
    }
    
    // If the line ends with ] or ];, it's a single-line array - let normal handling take it
    // Count brackets in the ENTIRE rhs to check if complete
    let open_brackets = rhs.matches('[').count();
    let close_brackets = rhs.matches(']').count();
    if open_brackets == close_brackets && close_brackets > 0 {
        return None; // Complete on one line
    }
    
    // Extract var name and optional type
    let (var_name, var_type) = if left.contains(':') {
        let type_parts: Vec<&str> = left.splitn(2, ':').collect();
        if type_parts.len() == 2 {
            (type_parts[0].trim().to_string(), Some(type_parts[1].trim().to_string()))
        } else {
            (left.to_string(), None)
        }
    } else {
        (left.to_string(), None)
    };
    
    // Validate var_name
    if !is_valid_identifier(&var_name) {
        return None;
    }
    
    Some((var_name, var_type, after_bracket.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_find_brace_outside_string() {
        // Normal brace
        assert_eq!(find_brace_outside_string("Foo {"), Some(4));
        assert_eq!(find_brace_outside_string("Event::Data {"), Some(12));
        
        // Brace inside string should be ignored
        assert_eq!(find_brace_outside_string("\"hello {}\""), None);
        assert_eq!(find_brace_outside_string("anyhow::bail(\"header {}\")"), None);
        
        // Mixed - brace after string
        assert_eq!(find_brace_outside_string("println(\"x\"); Foo {"), Some(18));
        
        // Escaped quote in string
        assert_eq!(find_brace_outside_string("\"escaped \\\" quote {}\""), None);
    }
    
    #[test]
    fn test_detect_struct_literal_start() {
        let registry = StructRegistry::new();
        
        // PascalCase heuristic
        let result = detect_struct_literal_start("user = User {", &registry);
        assert!(result.is_some());
        let (var, struct_name) = result.unwrap();
        assert_eq!(var, "user");
        assert_eq!(struct_name, "User");
        
        // Should not match enum paths
        let result = detect_struct_literal_start("ev = Event::Data {", &registry);
        assert!(result.is_none());
        
        // Should not match function definitions
        let result = detect_struct_literal_start("fn foo() -> User {", &registry);
        assert!(result.is_none());
    }
    
    #[test]
    fn test_detect_bare_struct_literal() {
        let registry = StructRegistry::new();
        
        let result = detect_bare_struct_literal("User {", &registry);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "User");
        
        // Should not match assignments
        let result = detect_bare_struct_literal("u = User {", &registry);
        assert!(result.is_none());
        
        // Should not match enum paths
        let result = detect_bare_struct_literal("Event::Data {", &registry);
        assert!(result.is_none());
    }
    
    #[test]
    fn test_detect_enum_literal_start() {
        let result = detect_enum_literal_start("ev = Event::Data {");
        assert!(result.is_some());
        let (var, enum_path) = result.unwrap();
        assert_eq!(var, "ev");
        assert_eq!(enum_path, "Event::Data");
        
        // Should not match match arms
        let result = detect_enum_literal_start("Event::Data { id } =>");
        assert!(result.is_none());
    }
    
    #[test]
    fn test_detect_bare_enum_literal() {
        let result = detect_bare_enum_literal("Event::Data {");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "Event::Data");
        
        // Should not match match arms
        let result = detect_bare_enum_literal("Event::Data { id } =>");
        assert!(result.is_none());
    }
    
    #[test]
    fn test_detect_array_literal_start() {
        let result = detect_array_literal_start("arr = [");
        assert!(result.is_some());
        let (var, var_type, _) = result.unwrap();
        assert_eq!(var, "arr");
        assert!(var_type.is_none());
        
        // With type annotation
        let result = detect_array_literal_start("arr: Vec[i32] = [");
        assert!(result.is_some());
        let (var, var_type, _) = result.unwrap();
        assert_eq!(var, "arr");
        assert_eq!(var_type, Some("Vec[i32]".to_string()));
        
        // Single-line array should return None
        let result = detect_array_literal_start("arr = [1, 2, 3]");
        assert!(result.is_none());
    }
    
    //=========================================================================
    // CRITICAL BUG FIX TESTS
    // These tests verify that format strings with {} don't trigger detection
    //=========================================================================
    
    #[test]
    fn test_macro_call_not_detected_as_enum_literal() {
        // CRITICAL: anyhow::bail with format string must NOT be detected as enum literal
        let result = detect_bare_enum_literal("anyhow::bail(\"header {} mismatch\")");
        assert!(result.is_none(), "macro call should not be detected as enum literal");
        
        let result = detect_bare_enum_literal("anyhow::bail(\"header {} mismatch: expected {}, got {}\", a, b)");
        assert!(result.is_none(), "macro call with multiple format args should not be detected");
        
        // log macros
        let result = detect_bare_enum_literal("log::info(\"processing {} items\", count)");
        assert!(result.is_none(), "log::info should not be detected as enum literal");
        
        // tracing macros
        let result = detect_bare_enum_literal("tracing::debug(\"request {} completed\", id)");
        assert!(result.is_none(), "tracing macros should not be detected as enum literal");
    }
    
    #[test]
    fn test_macro_call_not_detected_as_enum_assignment() {
        // Even with assignment pattern, should not match
        let result = detect_enum_literal_start("x = anyhow::bail(\"error {}\")");
        assert!(result.is_none(), "macro call in assignment should not be detected");
    }
    
    #[test]
    fn test_real_enum_variant_still_detected() {
        // Real enum variants should still work
        let result = detect_bare_enum_literal("SyncStatus::SyncingHeaders {");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "SyncStatus::SyncingHeaders");
        
        let result = detect_enum_literal_start("status = SyncStatus::Idle {");
        // Note: SyncStatus::Idle is a unit variant, but SyncStatus::SyncingHeaders is struct variant
        // The detection should work for struct variants
    }
    
    #[test]
    fn test_format_string_with_braces() {
        // These should all return None - braces are inside strings
        let result = detect_bare_enum_literal("println(\"hello {}\")");
        assert!(result.is_none());
        
        let result = detect_bare_struct_literal("format(\"user {} logged in\")");
        assert!(result.is_none());
    }
    
    //=========================================================================
    // VEC![ MACRO DETECTION TESTS
    // Verify that vec![ is properly detected as array literal
    //=========================================================================
    
    #[test]
    fn test_detect_vec_macro_array_literal() {
        // vec![ should be detected as array literal start
        let result = detect_array_literal_start("arr = vec![");
        assert!(result.is_some(), "vec![ should be detected as array literal");
        let (var, var_type, after) = result.unwrap();
        assert_eq!(var, "arr");
        assert!(var_type.is_none());
        assert!(after.is_empty());
        
        // vec![ with initial content
        let result = detect_array_literal_start("items = vec![ SyncStatus::Idle,");
        assert!(result.is_some(), "vec![ with content should be detected");
        let (var, _, after) = result.unwrap();
        assert_eq!(var, "items");
        assert!(after.trim().starts_with("SyncStatus"));
        
        // Single-line vec![] should return None
        let result = detect_array_literal_start("arr = vec![1, 2, 3]");
        assert!(result.is_none(), "complete vec![] should not be detected");
        
        // Vec::from([ should also work
        let result = detect_array_literal_start("data = Vec::from([");
        assert!(result.is_some(), "Vec::from([ should be detected as array literal");
    }
}