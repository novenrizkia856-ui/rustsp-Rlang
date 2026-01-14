//! Enum definition and instantiation parsing for RustS+
//!
//! Handles:
//! - Enum definitions with unit, tuple, and struct variants
//! - Enum instantiation
//! - Pattern matching (pass-through to Rust)

use std::collections::HashSet;

/// Registry of known enum names
#[derive(Debug, Clone, Default)]
pub struct EnumRegistry {
    pub names: HashSet<String>,
}

impl EnumRegistry {
    pub fn new() -> Self {
        EnumRegistry {
            names: HashSet::new(),
        }
    }
    
    pub fn register(&mut self, name: &str) {
        self.names.insert(name.to_string());
    }
    
    pub fn is_enum(&self, name: &str) -> bool {
        self.names.contains(name)
    }
}

/// Check if a line starts an enum definition
pub fn is_enum_definition(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("enum ") || 
    (trimmed.starts_with("pub ") && trimmed.contains("enum "))
}

/// Parse enum definition header, returns enum name if found
pub fn parse_enum_header(line: &str) -> Option<String> {
    let trimmed = line.trim();
    
    let after_enum = if trimmed.starts_with("pub enum ") {
        trimmed.strip_prefix("pub enum ")?
    } else if trimmed.starts_with("enum ") {
        trimmed.strip_prefix("enum ")?
    } else {
        return None;
    };
    
    // Extract name (before { or whitespace)
    let name: String = after_enum
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Enum variant types
#[derive(Debug, Clone, PartialEq)]
pub enum VariantKind {
    /// Unit variant: `Ping`
    Unit,
    /// Tuple variant: `Text(String)` or `Point(i32, i32)`
    Tuple,
    /// Struct variant: `Move { x i32, y i32 }`
    Struct,
}

/// Detect what kind of variant this line represents
pub fn detect_variant_kind(line: &str) -> Option<VariantKind> {
    let trimmed = line.trim();
    
    // Skip empty, braces, comments
    if trimmed.is_empty() || trimmed == "{" || trimmed == "}" || trimmed.starts_with("//") {
        return None;
    }
    
    // Check for tuple variant: Name(...)
    if trimmed.contains('(') && trimmed.contains(')') {
        return Some(VariantKind::Tuple);
    }
    
    // Check for struct variant: Name { ... } or Name {
    if trimmed.contains('{') {
        return Some(VariantKind::Struct);
    }
    
    // Check for continuation of struct variant (fields inside)
    // This is handled by context tracking
    
    // Unit variant: just a name
    let name: String = trimmed
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    
    if !name.is_empty() && name.chars().next().unwrap().is_uppercase() {
        return Some(VariantKind::Unit);
    }
    
    None
}

/// Transform an enum variant line from RustS+ to Rust
pub fn transform_enum_variant(line: &str, in_struct_variant: bool) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // Skip empty, braces only
    if trimmed.is_empty() {
        return line.to_string();
    }
    
    if trimmed == "{" {
        return format!("{} {{", leading_ws.trim_end());
    }
    
    if trimmed == "}" {
        // Closing brace for struct variant
        if in_struct_variant {
            return format!("{}}},", leading_ws);
        }
        return line.to_string();
    }
    
    // Comments pass through
    if trimmed.starts_with("//") {
        return line.to_string();
    }
    
    // Already has comma at end, likely processed
    if trimmed.ends_with(',') {
        return line.to_string();
    }
    
    // Inside struct variant - transform fields
    if in_struct_variant && !trimmed.contains('{') && !trimmed.contains('}') {
        return transform_struct_variant_field(line);
    }
    
    // Tuple variant: Name(Type) or Name(T1, T2)
    if trimmed.contains('(') && trimmed.contains(')') && !trimmed.contains('{') {
        return format!("{}{},", leading_ws, trimmed);
    }
    
    // Struct variant start: Name { or Name { x i32 }
    if trimmed.contains('{') {
        return transform_struct_variant_line(line);
    }
    
    // Unit variant: just Name
    let name: String = trimmed
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    
    if !name.is_empty() {
        return format!("{}{},", leading_ws, name);
    }
    
    line.to_string()
}

/// Transform a struct variant field line
/// Input:  "        x i32"
/// Output: "        x: i32,"
fn transform_struct_variant_field(line: &str) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // Already has colon
    if trimmed.contains(':') {
        let clean = trimmed.trim_end_matches(',');
        return format!("{}{},", leading_ws, clean);
    }
    
    // Parse: field_name Type
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() >= 2 {
        let field_name = parts[0];
        // CRITICAL FIX: Strip trailing comma from field_type to avoid double comma
        let field_type = parts[1..].join(" ").trim_end_matches(',').to_string();
        return format!("{}{}: {},", leading_ws, field_name, field_type);
    }
    
    line.to_string()
}

/// Transform struct variant line (handles inline or multiline start)
fn transform_struct_variant_line(line: &str) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // Check if it's all on one line: Name { x i32, y i32 }
    if trimmed.ends_with('}') {
        // Single line struct variant
        if let Some(brace_pos) = trimmed.find('{') {
            let name = trimmed[..brace_pos].trim();
            let fields_part = &trimmed[brace_pos + 1..trimmed.len() - 1];
            
            // Transform fields
            let transformed_fields = transform_inline_struct_fields(fields_part);
            return format!("{}{} {{ {} }},", leading_ws, name, transformed_fields);
        }
    }
    
    // Multiline start: Name {
    if let Some(brace_pos) = trimmed.find('{') {
        let name = trimmed[..brace_pos].trim();
        let after_brace = trimmed[brace_pos + 1..].trim();
        
        if after_brace.is_empty() {
            return format!("{}{} {{", leading_ws, name);
        } else {
            // Has content after brace on same line
            let transformed = transform_inline_struct_fields(after_brace);
            return format!("{}{} {{ {}", leading_ws, name, transformed);
        }
    }
    
    line.to_string()
}

/// Transform inline struct fields: "x i32, y i32" → "x: i32, y: i32"
fn transform_inline_struct_fields(fields: &str) -> String {
    let parts: Vec<&str> = fields.split(',').collect();
    let transformed: Vec<String> = parts.iter()
        .map(|p| {
            let trimmed = p.trim();
            if trimmed.is_empty() {
                return String::new();
            }
            if trimmed.contains(':') {
                return trimmed.to_string();
            }
            let field_parts: Vec<&str> = trimmed.split_whitespace().collect();
            if field_parts.len() >= 2 {
                format!("{}: {}", field_parts[0], field_parts[1..].join(" "))
            } else {
                trimmed.to_string()
            }
        })
        .filter(|s| !s.is_empty())
        .collect();
    
    transformed.join(", ")
}

/// Transform enum instantiation with struct syntax
/// Input:  "msg = Message::Move { x = 10, y = 20 }" 
/// Output: "let msg = Message::Move { x: 10, y: 20 };"
pub fn transform_enum_struct_init(line: &str) -> String {
    let trimmed = line.trim();
    
    // Check for struct-style enum instantiation
    if !trimmed.contains("::") || !trimmed.contains('{') {
        return line.to_string();
    }
    
    // Find the brace and transform contents
    if let Some(brace_pos) = trimmed.find('{') {
        let before_brace = &trimmed[..brace_pos + 1];
        let after_brace = &trimmed[brace_pos + 1..];
        
        // Transform field assignments inside
        let transformed_after = transform_enum_init_fields(after_brace);
        return format!("{}{}", before_brace, transformed_after);
    }
    
    line.to_string()
}

/// Transform field assignments in enum struct instantiation
fn transform_enum_init_fields(content: &str) -> String {
    let mut result = String::new();
    let mut in_string = false;
    let mut current_field = String::new();
    
    for c in content.chars() {
        if c == '"' {
            in_string = !in_string;
        }
        
        if !in_string && c == '=' {
            // Check if it's not == 
            if !current_field.ends_with('=') && !result.ends_with('=') {
                result.push_str(&current_field);
                result.push(':');
                current_field.clear();
                continue;
            }
        }
        
        current_field.push(c);
    }
    
    result.push_str(&current_field);
    result
}

/// Context for tracking enum parsing state
#[derive(Debug, Clone)]
pub struct EnumParseContext {
    /// Are we inside an enum definition?
    pub in_enum_def: bool,
    /// Are we inside a struct variant (multiline)?
    pub in_struct_variant: bool,
    /// Brace depth for tracking
    pub brace_depth: usize,
    /// Starting brace depth
    pub start_depth: usize,
}

impl EnumParseContext {
    pub fn new() -> Self {
        EnumParseContext {
            in_enum_def: false,
            in_struct_variant: false,
            brace_depth: 0,
            start_depth: 0,
        }
    }
    
    pub fn enter_enum(&mut self, depth: usize) {
        self.in_enum_def = true;
        self.start_depth = depth;
    }
    
    pub fn enter_struct_variant(&mut self) {
        self.in_struct_variant = true;
    }
    
    pub fn exit_struct_variant(&mut self) {
        self.in_struct_variant = false;
    }
    
    pub fn exit_enum(&mut self) {
        self.in_enum_def = false;
        self.in_struct_variant = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_unit_variant() {
        assert_eq!(transform_enum_variant("    Ping", false), "    Ping,");
        assert_eq!(transform_enum_variant("    Logout", false), "    Logout,");
    }
    
    #[test]
    fn test_tuple_variant() {
        assert_eq!(transform_enum_variant("    Text(String)", false), "    Text(String),");
        assert_eq!(transform_enum_variant("    Point(i32, i32)", false), "    Point(i32, i32),");
    }
    
    #[test]
    fn test_struct_variant_field() {
        assert_eq!(transform_struct_variant_field("        x i32"), "        x: i32,");
        assert_eq!(transform_struct_variant_field("        y i32"), "        y: i32,");
    }
    
    #[test]
    fn test_parse_enum_header() {
        assert_eq!(parse_enum_header("enum Message {"), Some("Message".to_string()));
        assert_eq!(parse_enum_header("pub enum Event {"), Some("Event".to_string()));
    }
    
    #[test]
    fn test_inline_struct_fields() {
        assert_eq!(transform_inline_struct_fields("x i32, y i32"), "x: i32, y: i32");
    }
}