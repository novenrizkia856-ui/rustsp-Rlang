//! Post-processing functions for RustS+ transpiler
//! 
//! Contains functions for cleaning up transpiled output:
//! - Fixing bare mut declarations
//! - Stripping effect annotations
//! - Stripping outer keyword
//! - Single-line literal transformations

use crate::helpers::is_valid_identifier;
use crate::transform_literal::{find_field_eq, is_valid_field_name};

/// L-05 POST-PROCESSING: Fix bare `mut` declarations
/// Transform `mut x = 10` → `let mut x = 10;`
pub fn fix_bare_mut_declaration(line: &str) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // Check if line starts with `mut ` but NOT `let mut` or `&mut`
    if !trimmed.starts_with("mut ") {
        return line.to_string();
    }
    
    // Check the context - if it's already `let mut` or `&mut`, don't modify
    if line.contains("let mut") || line.contains("&mut") {
        return line.to_string();
    }
    
    // Parse: `mut var = value` or `mut var: Type = value`
    let rest = trimmed.strip_prefix("mut ").unwrap();
    
    // Find the = sign
    if let Some(eq_pos) = rest.find('=') {
        let var_part = rest[..eq_pos].trim();
        let val_part = rest[eq_pos + 1..].trim();
        
        // Check if var_part has a type annotation
        let (var_name, type_annotation) = if var_part.contains(':') {
            let parts: Vec<&str> = var_part.splitn(2, ':').collect();
            if parts.len() == 2 {
                (parts[0].trim(), format!(": {}", parts[1].trim()))
            } else {
                (var_part, String::new())
            }
        } else {
            (var_part, String::new())
        };
        
        // Validate var_name is a valid identifier
        if !var_name.is_empty() && is_valid_identifier(var_name) {
            // Ensure semicolon at end
            let val_clean = val_part.trim_end_matches(';');
            return format!("{}let mut {}{} = {};", leading_ws, var_name, type_annotation, val_clean);
        }
    }
    
    line.to_string()
}

/// L-05 POST-PROCESSING: Strip effect annotations from output lines
/// Effect annotations like `effects(...)` must not appear in Rust output.
pub fn strip_effects_from_line(line: &str) -> String {
    // Quick check
    if !line.contains("effects(") {
        return line.to_string();
    }
    
    // Don't modify comments
    let trimmed = line.trim();
    if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("*") {
        return line.to_string();
    }
    
    // Check if "effects(" is inside a string literal
    let mut in_string = false;
    let mut escape_next = false;
    let chars: Vec<char> = line.chars().collect();
    let mut effects_positions: Vec<usize> = Vec::new();
    
    for (i, &c) in chars.iter().enumerate() {
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
        
        // Look for "effects(" outside string
        if !in_string && i + 8 <= chars.len() {
            let slice: String = chars[i..i+8].iter().collect();
            if slice == "effects(" {
                effects_positions.push(i);
            }
        }
    }
    
    if effects_positions.is_empty() {
        return line.to_string();
    }
    
    // Strip all effect annotations found
    let mut result = line.to_string();
    for pos in effects_positions.iter().rev() {
        let start = *pos;
        let substring = &result[start..];
        
        let mut paren_depth = 0;
        let mut end = start;
        
        for (i, c) in substring.char_indices() {
            match c {
                '(' => paren_depth += 1,
                ')' => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        end = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        
        if end > start {
            let before = &result[..start];
            let after = &result[end..];
            let after_trimmed = after.trim_start();
            
            if before.ends_with(' ') && !after_trimmed.is_empty() {
                result = format!("{}{}", before, after_trimmed);
            } else if !before.ends_with(' ') && !after_trimmed.is_empty() && !after_trimmed.starts_with('{') {
                result = format!("{} {}", before.trim_end(), after_trimmed);
            } else {
                result = format!("{}{}", before.trim_end(), if after_trimmed.is_empty() { "" } else { " " }.to_string() + after_trimmed);
            }
        }
    }
    
    result
}

/// Strip `outer` keyword from a line
/// `outer self.hash = value` → `self.hash = value`
pub fn strip_outer_keyword(line: &str) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    if trimmed.starts_with("outer ") {
        let rest = trimmed.strip_prefix("outer ").unwrap();
        return format!("{}{}", leading_ws, rest);
    }
    
    line.to_string()
}

/// Transform single-line struct literal: `x = Struct { field = value }` → `let x = Struct { field: value };`
pub fn transform_single_line_struct_literal(line: &str) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // Quick checks
    if !trimmed.contains('{') || !trimmed.contains('}') || !trimmed.contains('=') {
        return line.to_string();
    }
    
    // Check if this is a single-line literal: `x = Struct { ... }`
    // where both { and } are on the same line
    let brace_open = trimmed.find('{');
    let brace_close = trimmed.rfind('}');
    
    if brace_open.is_none() || brace_close.is_none() {
        return line.to_string();
    }
    
    let open_pos = brace_open.unwrap();
    let close_pos = brace_close.unwrap();
    
    // Must have = before {
    let before_brace = &trimmed[..open_pos];
    let eq_pos = before_brace.rfind('=');
    if eq_pos.is_none() {
        return line.to_string();
    }
    
    let eq_idx = eq_pos.unwrap();
    let var_part = trimmed[..eq_idx].trim();
    let struct_name_and_brace = trimmed[eq_idx + 1..open_pos + 1].trim();
    let fields_part = &trimmed[open_pos + 1..close_pos];
    let after_close = &trimmed[close_pos + 1..];
    
    // Transform fields: `field = value` → `field: value`
    let transformed_fields = transform_fields_in_braces(fields_part);
    
    // Reconstruct
    format!("{}let {} = {}{}{}{}", 
        leading_ws, var_part, struct_name_and_brace, transformed_fields, "}", after_close)
}

/// Transform fields inside braces from = to : syntax
fn transform_fields_in_braces(fields: &str) -> String {
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
            result.push(transform_single_field(&current));
            current.clear();
        } else {
            current.push(c);
        }
    }
    
    // Last field
    if !current.trim().is_empty() {
        result.push(transform_single_field(&current));
    }
    
    if result.is_empty() {
        String::new()
    } else {
        format!(" {} ", result.join(", "))
    }
}

/// Transform a single field from `name = value` to `name: value`
fn transform_single_field(field: &str) -> String {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    
    // Already has colon (not ::)
    if trimmed.contains(':') && !trimmed.contains("::") && !trimmed.contains('=') {
        return trimmed.to_string();
    }
    
    // Transform = to :
    if let Some(eq_pos) = find_field_eq(trimmed) {
        let name = trimmed[..eq_pos].trim();
        let value = trimmed[eq_pos + 1..].trim();
        if is_valid_field_name(name) {
            return format!("{}: {}", name, value);
        }
    }
    
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fix_bare_mut_declaration() {
        assert_eq!(
            fix_bare_mut_declaration("mut x = 10"),
            "let mut x = 10;"
        );
        assert_eq!(
            fix_bare_mut_declaration("    mut y: i32 = 20"),
            "    let mut y: i32 = 20;"
        );
        // Should not modify existing let mut
        assert_eq!(
            fix_bare_mut_declaration("let mut x = 10;"),
            "let mut x = 10;"
        );
    }
    
    #[test]
    fn test_strip_effects_from_line() {
        assert_eq!(
            strip_effects_from_line("fn foo() effects(io) {"),
            "fn foo() {"
        );
        assert_eq!(
            strip_effects_from_line("fn bar(x: i32) -> i32 effects(read x) {"),
            "fn bar(x: i32) -> i32 {"
        );
        // Don't modify comments
        assert_eq!(
            strip_effects_from_line("// effects(io) are important"),
            "// effects(io) are important"
        );
    }
    
    #[test]
    fn test_strip_outer_keyword() {
        assert_eq!(
            strip_outer_keyword("outer self.field = value"),
            "self.field = value"
        );
        assert_eq!(
            strip_outer_keyword("    outer x = 10"),
            "    x = 10"
        );
        assert_eq!(
            strip_outer_keyword("normal line"),
            "normal line"
        );
    }
    
    #[test]
    fn test_transform_single_line_struct_literal() {
        let input = "u = User { id = 1, name = \"test\" }";
        let output = transform_single_line_struct_literal(input);
        assert!(output.contains("let u = User"));
        assert!(output.contains("id: 1"));
        assert!(output.contains("name: \"test\""));
    }
}