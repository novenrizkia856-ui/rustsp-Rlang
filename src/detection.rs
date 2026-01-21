//! Detection functions for RustS+ literal patterns
//! 
//! Contains functions to detect the start of various literal expressions:
//! - Struct literals: `x = StructName { ... }`
//! - Enum struct variants: `x = Enum::Variant { ... }`
//! - Array literals: `x = [...]`
//! - Bare literals (return expressions without assignment)

use crate::helpers::{is_rust_block_start, is_valid_identifier};
use crate::struct_def::StructRegistry;

/// Detect if line starts a struct literal: `varname = StructName {`
/// Returns (var_name, struct_name) if matched, excludes Enum::Variant
pub fn detect_struct_literal_start(line: &str, registry: &StructRegistry) -> Option<(String, String)> {
    let trimmed = line.trim();
    
    // CRITICAL FIX: EXCLUDE function definitions and other Rust blocks
    if is_rust_block_start(trimmed) {
        return None;
    }
    
    if !trimmed.contains('=') || !trimmed.contains('{') {
        return None;
    }
    
    let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
    if parts.len() != 2 { return None; }
    
    let var_name = parts[0].trim();
    let rhs = parts[1].trim();
    
    // EXCLUDE enum paths (:: before {)
    if let Some(brace_pos) = rhs.find('{') {
        let before_brace = &rhs[..brace_pos];
        if before_brace.contains("::") {
            return None;
        }
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
    
    // Must have { but NOT have = before it
    if !trimmed.contains('{') {
        return None;
    }
    
    // If there's a = BEFORE {, it's an assignment, not bare literal
    if let Some(brace_pos) = trimmed.find('{') {
        let before_brace = &trimmed[..brace_pos];
        if before_brace.contains('=') {
            return None;
        }
        
        // EXCLUDE enum paths (has ::)
        if before_brace.contains("::") {
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
    }
    
    None
}

/// Detect BARE enum struct variant literal (without assignment): `Enum::Variant {`
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
    
    // Must have :: and {
    if !trimmed.contains("::") || !trimmed.contains('{') {
        return None;
    }
    
    // If there's a = BEFORE {, it's an assignment
    if let Some(brace_pos) = trimmed.find('{') {
        let before_brace = &trimmed[..brace_pos];
        if before_brace.contains('=') {
            return None;
        }
        
        let enum_path = before_brace.trim();
        if !enum_path.is_empty() && enum_path.contains("::") {
            return Some(enum_path.to_string());
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
    
    if !trimmed.contains('=') || !trimmed.contains("::") || !trimmed.contains('{') {
        return None;
    }
    
    let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
    if parts.len() != 2 { return None; }
    
    let var_name = parts[0].trim();
    let rhs = parts[1].trim();
    
    // Must have :: before {
    if let Some(brace_pos) = rhs.find('{') {
        let before_brace = rhs[..brace_pos].trim();
        if before_brace.contains("::") {
            return Some((var_name.to_string(), before_brace.to_string()));
        }
    }
    
    None
}

//===========================================================================
// ARRAY LITERAL DETECTION
// Detects array literal start: `var = [` where bracket is not closed on same line
//===========================================================================

/// Detect if line starts an array literal: `varname = [` or `varname = [\n`
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
    
    // RHS must start with [ (after trimming)
    if !rhs.starts_with('[') {
        return None;
    }
    
    // If the line ends with ], it's a single-line array - let normal handling take it
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
    
    // Content after [ (may be empty or have first element)
    let after_bracket = &rhs[1..];
    
    Some((var_name, var_type, after_bracket.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    
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
}