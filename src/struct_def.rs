//! Struct definition and instantiation parsing for RustS+
//!
//! Handles:
//! - Struct definitions: `struct Name { field Type }` → `struct Name { field: Type, }`
//! - Struct instantiation: `v = Name { field = value }` → `let v = Name { field: value, };`
//! - Struct update syntax: `..other`
//! - Field mutations (integrated with scope system)

use std::collections::HashSet;

/// Registry of known struct names for instantiation detection
#[derive(Debug, Clone, Default)]
pub struct StructRegistry {
    pub names: HashSet<String>,
}

impl StructRegistry {
    pub fn new() -> Self {
        StructRegistry {
            names: HashSet::new(),
        }
    }
    
    pub fn register(&mut self, name: &str) {
        self.names.insert(name.to_string());
    }
    
    pub fn is_struct(&self, name: &str) -> bool {
        self.names.contains(name)
    }
}

/// Check if a line starts a struct definition
pub fn is_struct_definition(line: &str) -> bool {
    let trimmed = line.trim();
    // Match: struct Name {   or   #[...] or pub struct Name {
    trimmed.starts_with("struct ") || 
    (trimmed.starts_with("pub ") && trimmed.contains("struct "))
}

/// Parse struct definition header, returns struct name if found
pub fn parse_struct_header(line: &str) -> Option<String> {
    let trimmed = line.trim();
    
    // Handle: struct Name { or struct Name or pub struct Name {
    let after_struct = if trimmed.starts_with("pub struct ") {
        trimmed.strip_prefix("pub struct ")?
    } else if trimmed.starts_with("struct ") {
        trimmed.strip_prefix("struct ")?
    } else {
        return None;
    };
    
    // Extract name (before { or whitespace)
    let name: String = after_struct
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Transform a struct field line from RustS+ to Rust
/// Input:  "    id u64"         -> "    id: u64,"
/// Input:  "    pub hash String" -> "    pub hash: String,"
pub fn transform_struct_field(line: &str) -> String {
    let trimmed = line.trim();
    
    // Skip empty lines, braces, comments, attributes
    if trimmed.is_empty() || 
       trimmed == "{" || 
       trimmed == "}" ||
       trimmed.starts_with("//") ||
       trimmed.starts_with("#[") {
        return line.to_string();
    }
    
    // Check if already has colon (Rust syntax) - but not ::
    if trimmed.contains(':') && !trimmed.starts_with("pub:") {
        // Check it's actually field: Type syntax, not something else
        // Avoid false positives from :: in types
        let colon_pos = trimmed.find(':');
        let double_colon_pos = trimmed.find("::");
        if let Some(cp) = colon_pos {
            // If there's :: and : is part of ::, it's not field syntax
            if double_colon_pos.is_some() && double_colon_pos.unwrap() == cp {
                // This is ::, not field: Type - continue to parse
            } else {
                // Already Rust syntax, just ensure comma
                let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                let clean = trimmed.trim_end_matches(',');
                return format!("{}{},", leading_ws, clean);
            }
        }
    }
    
    // Parse the field line
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // CRITICAL FIX: Handle visibility modifiers (pub, pub(crate), etc.)
    // Patterns:
    //   "field_name Type"           -> 2+ parts, no visibility
    //   "pub field_name Type"       -> 3+ parts, pub visibility
    //   "pub(crate) field_name Type" -> 3+ parts, pub(crate) visibility
    
    if parts.is_empty() {
        return line.to_string();
    }
    
    // Check for visibility modifier
    let (visibility, field_start_idx) = if parts[0] == "pub" {
        // Check if it's pub(something)
        if parts.len() > 1 && parts[1].starts_with('(') {
            // pub(crate) or pub(super) etc - combine them
            // Find where the visibility ends
            let mut vis_end = 1;
            let mut combined_vis = "pub".to_string();
            for (i, part) in parts.iter().enumerate().skip(1) {
                combined_vis.push(' ');
                combined_vis.push_str(part);
                if part.ends_with(')') {
                    vis_end = i + 1;
                    break;
                }
            }
            (Some(combined_vis), vis_end)
        } else {
            (Some("pub".to_string()), 1)
        }
    } else if parts[0].starts_with("pub(") {
        // pub(crate) as single token
        (Some(parts[0].to_string()), 1)
    } else {
        (None, 0)
    };
    
    // Now parse field_name and Type from remaining parts
    let remaining_parts = &parts[field_start_idx..];
    
    if remaining_parts.len() >= 2 {
        let field_name = remaining_parts[0];
        // CRITICAL FIX: Strip trailing comma from field_type to avoid double comma
        let field_type = remaining_parts[1..].join(" ").trim_end_matches(',').to_string();
        
        // Validate field name (should be lowercase identifier, not a keyword)
        if is_valid_field_name(field_name) {
            return match visibility {
                Some(vis) => format!("{}{} {}: {},", leading_ws, vis, field_name, field_type),
                None => format!("{}{}: {},", leading_ws, field_name, field_type),
            };
        }
    }
    
    // Can't parse, return as-is
    line.to_string()
}

/// Check if this is a valid field/identifier name
/// CRITICAL: Supports Rust raw identifiers (r#keyword) for reserved keywords
fn is_valid_field_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    
    // Support raw identifiers: r#type, r#match, r#async, etc.
    let identifier = if s.starts_with("r#") && s.len() > 2 {
        &s[2..]  // Strip r# prefix for validation
    } else {
        s
    };
    
    if identifier.is_empty() {
        return false;
    }
    
    let first = identifier.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    identifier.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Check if a line is a struct instantiation
/// Pattern: `name = StructName {` or `name = StructName{`
/// EXCLUDES enum struct variants like `Message::Move { x = 1 }`
pub fn is_struct_instantiation(line: &str, registry: &StructRegistry) -> bool {
    let trimmed = line.trim();
    
    // Must have = and {
    if !trimmed.contains('=') || !trimmed.contains('{') {
        return false;
    }
    
    // Split by first =
    let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
    if parts.len() != 2 {
        return false;
    }
    
    let rhs = parts[1].trim();
    
    // EXCLUDE enum paths - if there's :: before {, it's likely an enum variant
    if let Some(brace_pos) = rhs.find('{') {
        let before_brace = &rhs[..brace_pos];
        if before_brace.contains("::") {
            // This is an enum struct variant instantiation, not struct instantiation
            return false;
        }
    }
    
    // RHS should be StructName { or StructName{
    let struct_name: String = rhs
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    
    // Check registry or heuristic (starts with uppercase)
    if registry.is_struct(&struct_name) {
        return true;
    }
    
    // Heuristic: if name starts with uppercase and followed by {
    if !struct_name.is_empty() {
        let first_char = struct_name.chars().next().unwrap_or('_');
        if first_char.is_uppercase() && rhs.contains('{') {
            return true;
        }
    }
    
    false
}

/// Transform struct instantiation field
/// Input:  "    id = 1"
/// Output: "    id: 1,"
pub fn transform_struct_init_field(line: &str, is_string_literal: bool) -> String {
    let trimmed = line.trim();
    
    // Handle ..spread syntax
    if trimmed.starts_with("..") {
        let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        return format!("{}{}", leading_ws, trimmed);
    }
    
    // Skip braces, empty
    if trimmed.is_empty() || trimmed == "{" || trimmed == "}" {
        return line.to_string();
    }
    
    // Already has colon (Rust syntax)
    if trimmed.contains(':') && !trimmed.contains("::") {
        return line.to_string();
    }
    
    // Parse: field = value
    if let Some(eq_pos) = trimmed.find('=') {
        // Make sure it's not == or other operators
        let before = &trimmed[..eq_pos];
        let after_char = trimmed.chars().nth(eq_pos + 1);
        
        if !before.ends_with('!') && 
           !before.ends_with('<') && 
           !before.ends_with('>') &&
           !matches!(after_char, Some('=') | Some('>')) {
            
            let field = before.trim();
            let value = trimmed[eq_pos + 1..].trim();
            
            if is_valid_field_name(field) && !value.is_empty() {
                let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                
                // Transform string literals
                let transformed_value = if is_string_value(value) && !value.contains("String::from") {
                    let inner = &value[1..value.len()-1];
                    format!("String::from(\"{}\")", inner)
                } else {
                    value.to_string()
                };
                
                return format!("{}{}: {},", leading_ws, field, transformed_value);
            }
        }
    }
    
    line.to_string()
}

/// Check if value is a string literal
fn is_string_value(s: &str) -> bool {
    let trimmed = s.trim().trim_end_matches(',');
    trimmed.starts_with('"') && trimmed.ends_with('"')
}

/// Context for tracking struct parsing state
#[derive(Debug, Clone)]
pub struct StructParseContext {
    /// Are we inside a struct definition?
    pub in_struct_def: bool,
    /// Are we inside a struct instantiation?
    pub in_struct_init: bool,
    /// Brace depth for current context
    pub brace_depth: usize,
    /// Starting brace depth when entering struct
    pub start_depth: usize,
}

impl StructParseContext {
    pub fn new() -> Self {
        StructParseContext {
            in_struct_def: false,
            in_struct_init: false,
            brace_depth: 0,
            start_depth: 0,
        }
    }
    
    pub fn enter_struct_def(&mut self, depth: usize) {
        self.in_struct_def = true;
        self.start_depth = depth;
    }
    
    pub fn enter_struct_init(&mut self, depth: usize) {
        self.in_struct_init = true;
        self.start_depth = depth;
    }
    
    pub fn exit_struct(&mut self) {
        self.in_struct_def = false;
        self.in_struct_init = false;
    }
    
    pub fn is_inside(&self) -> bool {
        self.in_struct_def || self.in_struct_init
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_struct_field_transform() {
        assert_eq!(transform_struct_field("    id u64"), "    id: u64,");
        assert_eq!(transform_struct_field("    name String"), "    name: String,");
        assert_eq!(transform_struct_field("    active bool"), "    active: bool,");
    }
    
    #[test]
    fn test_struct_field_with_pub_visibility() {
        // CRITICAL: pub visibility modifier must be preserved correctly
        assert_eq!(transform_struct_field("    pub hash String"), "    pub hash: String,");
        assert_eq!(transform_struct_field("    pub name String"), "    pub name: String,");
        assert_eq!(transform_struct_field("    pub ok bool"), "    pub ok: bool,");
        assert_eq!(transform_struct_field("    pub errors Vec<String>"), "    pub errors: Vec<String>,");
    }
    
    #[test]
    fn test_struct_field_with_pub_crate_visibility() {
        // pub(crate) and other restricted visibility
        assert_eq!(transform_struct_field("    pub(crate) id u64"), "    pub(crate) id: u64,");
    }
    
    #[test]
    fn test_struct_field_with_trailing_comma() {
        // CRITICAL: Input with trailing comma should NOT produce double comma
        assert_eq!(transform_struct_field("    value i32,"), "    value: i32,");
        assert_eq!(transform_struct_field("    name String,"), "    name: String,");
    }
    
    #[test]
    fn test_struct_field_with_raw_identifier() {
        // CRITICAL: Raw identifiers (r#keyword) must be supported
        // These are used when field names are Rust reserved keywords
        assert_eq!(transform_struct_field("    r#type String"), "    r#type: String,");
        assert_eq!(transform_struct_field("    pub r#type String"), "    pub r#type: String,");
        assert_eq!(transform_struct_field("    r#match i32"), "    r#match: i32,");
        assert_eq!(transform_struct_field("    r#async bool"), "    r#async: bool,");
    }
    
    #[test]
    fn test_struct_init_field_with_raw_identifier() {
        // CRITICAL: Raw identifiers in struct instantiation
        assert_eq!(transform_struct_init_field("    r#type = \"wasm\".to_string()", false), "    r#type: \"wasm\".to_string(),");
        assert_eq!(transform_struct_init_field("    r#match = 42", false), "    r#match: 42,");
    }
    
    #[test]
    fn test_struct_init_field_transform() {
        assert_eq!(transform_struct_init_field("    id = 1", false), "    id: 1,");
        assert_eq!(transform_struct_init_field("    name = \"kian\"", true), "    name: String::from(\"kian\"),");
    }
    
    #[test]
    fn test_spread_syntax() {
        assert_eq!(transform_struct_init_field("    ..other", false), "    ..other");
    }
    
    #[test]
    fn test_parse_struct_header() {
        assert_eq!(parse_struct_header("struct User {"), Some("User".to_string()));
        assert_eq!(parse_struct_header("pub struct Config {"), Some("Config".to_string()));
    }
}