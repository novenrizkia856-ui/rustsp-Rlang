//! Inline literal transformation for RustS+ transpiler
//!
//! This module contains functions for transforming single-line struct and enum
//! literals, as well as inline field transformation.

use std::collections::HashMap;
use crate::transform_literal::{find_field_eq, is_valid_field_name, is_string_literal, should_clone_field_value, transform_nested_struct_value};

/// Transform single-line struct literal: `u = User { id = 1, name = "x" }`
pub fn transform_single_line_struct_literal(line: &str, var_name: &str) -> String {
    let trimmed = line.trim();
    
    if let Some(eq_pos) = trimmed.find('=') {
        let rhs = trimmed[eq_pos + 1..].trim();
        
        if let Some(brace_start) = rhs.find('{') {
            let struct_name = rhs[..brace_start].trim();
            let brace_end = rhs.rfind('}').unwrap_or(rhs.len());
            let fields_part = &rhs[brace_start + 1..brace_end];
            
            let transformed_fields = transform_literal_fields_inline(fields_part);
            
            return format!("let {} = {} {{ {} }};", var_name, struct_name, transformed_fields);
        }
    }
    
    format!("let {};", line)
}

/// Transform single-line enum literal: `e = Event::Data { id = 1 }`
pub fn transform_single_line_enum_literal(line: &str, var_name: &str, enum_path: &str) -> String {
    let trimmed = line.trim();
    
    if let Some(brace_start) = trimmed.find('{') {
        let brace_end = trimmed.rfind('}').unwrap_or(trimmed.len());
        let fields_part = &trimmed[brace_start + 1..brace_end];
        
        let transformed_fields = transform_literal_fields_inline(fields_part);
        
        return format!("let {} = {} {{ {} }};", var_name, enum_path, transformed_fields);
    }
    
    format!("let {};", line)
}

/// Transform BARE struct/enum literal (return expression): `Packet { header = h }`
/// NO let - this is a return expression!
pub fn transform_bare_struct_literal(line: &str) -> String {
    let trimmed = line.trim();
    
    if let Some(brace_start) = trimmed.find('{') {
        let name_part = trimmed[..brace_start].trim();
        let brace_end = trimmed.rfind('}').unwrap_or(trimmed.len());
        let fields_part = &trimmed[brace_start + 1..brace_end];
        
        let transformed_fields = transform_literal_fields_inline(fields_part);
        
        return format!("{} {{ {} }}", name_part, transformed_fields);
    }
    
    trimmed.to_string()
}

/// Transform inline literal fields: `id = 1, name = "x"` → `id: 1, name: String::from("x"),`
pub fn transform_literal_fields_inline(fields: &str) -> String {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut brace_depth: usize = 0;
    
    // First pass: collect all fields
    let mut raw_fields = Vec::new();
    for c in fields.chars() {
        if c == '"' && !current.ends_with('\\') {
            in_string = !in_string;
        }
        if !in_string {
            if c == '{' { brace_depth += 1; }
            if c == '}' { brace_depth = brace_depth.saturating_sub(1); }
        }
        
        if c == ',' && !in_string && brace_depth == 0 {
            raw_fields.push(current.clone());
            current.clear();
        } else {
            current.push(c);
        }
    }
    if !current.trim().is_empty() {
        raw_fields.push(current);
    }
    
    // CRITICAL FIX: Track field values to detect duplicates
    // Duplicate values (like from.address used twice) need .clone() on earlier uses
    let mut value_last_index: HashMap<String, usize> = HashMap::new();
    
    // First pass: find last occurrence of each value expression
    for (i, field) in raw_fields.iter().enumerate() {
        if let Some(val) = extract_field_value(field) {
            // Normalize the value for comparison
            let normalized = val.trim().to_string();
            if !normalized.is_empty() && is_moveable_expression(&normalized) {
                value_last_index.insert(normalized, i);
            }
        }
    }
    
    // Second pass: transform fields, adding .clone() for duplicate values
    for (i, field) in raw_fields.iter().enumerate() {
        let field_val = extract_field_value(field);
        let needs_clone = if let Some(ref val) = field_val {
            let normalized = val.trim().to_string();
            if let Some(&last_idx) = value_last_index.get(&normalized) {
                i < last_idx && is_moveable_expression(&normalized)
            } else {
                false
            }
        } else {
            false
        };
        
        let transformed = transform_single_literal_field_with_clone(field, needs_clone);
        if !transformed.is_empty() {
            result.push(transformed);
        }
    }
    
    result.join(", ")
}

/// Extract field value from a field assignment
pub fn extract_field_value(field: &str) -> Option<String> {
    let trimmed = field.trim();
    if trimmed.is_empty() || trimmed.starts_with("..") {
        return None;
    }
    
    if let Some(eq_pos) = find_field_eq(trimmed) {
        let value = trimmed[eq_pos + 1..].trim();
        return Some(value.to_string());
    } else if trimmed.contains(':') && !trimmed.contains("::") {
        if let Some(colon_pos) = trimmed.find(':') {
            let value = trimmed[colon_pos + 1..].trim();
            return Some(value.to_string());
        }
    }
    None
}

/// Check if an expression is "moveable" (not Copy, could cause use-after-move)
pub fn is_moveable_expression(expr: &str) -> bool {
    let expr = expr.trim();
    // Skip literals and simple values that are likely Copy
    if expr.parse::<i64>().is_ok() || expr.parse::<f64>().is_ok() {
        return false;
    }
    if expr == "true" || expr == "false" {
        return false;
    }
    
    // Skip method calls (contains `()`)
    if expr.contains("()") {
        return false;
    }
    
    // Skip cast expressions (contains ` as `)
    if expr.contains(" as ") {
        return false;
    }
    
    // Skip path expressions like TxType::Stake
    if expr.contains("::") {
        return false;
    }
    
    // Field access patterns like `from.address` are likely to be moveable
    if expr.contains('.') {
        return true;
    }
    
    // Simple identifiers that are struct/enum types might be moveable
    // But we can't know for sure without type info, so be conservative
    false
}

/// Transform a single field with optional .clone()
pub fn transform_single_literal_field_with_clone(field: &str, add_clone: bool) -> String {
    let trimmed = field.trim();
    if trimmed.is_empty() { return String::new(); }
    
    // Spread syntax
    if trimmed.starts_with("..") { return trimmed.to_string(); }
    
    // Already transformed (has colon)
    if trimmed.contains(':') && !trimmed.contains("::") {
        if add_clone {
            // Find the value part and add .clone()
            if let Some(colon_pos) = trimmed.find(':') {
                let name = trimmed[..colon_pos].trim();
                let value = trimmed[colon_pos + 1..].trim();
                // Only add .clone() if value is a clonable expression
                if should_clone_field_value(value) && !value.ends_with(".clone()") {
                    return format!("{}: {}.clone()", name, value);
                }
            }
        }
        return trimmed.to_string();
    }
    
    if let Some(eq_pos) = find_field_eq(trimmed) {
        let name = trimmed[..eq_pos].trim();
        let value = trimmed[eq_pos + 1..].trim();
        
        if is_valid_field_name(name) {
            let mut transformed_value = if is_string_literal(value) {
                let inner = &value[1..value.len()-1];
                format!("String::from(\"{}\")", inner)
            } else if value.contains('{') && value.contains('=') {
                // CRITICAL FIX: Recursively transform nested struct values!
                transform_nested_struct_value(value)
            } else {
                value.to_string()
            };
            
            // Add .clone() if needed for duplicate values OR for field access expressions
            // CRITICAL FIX: Consistent with transform_literal_field_with_ctx
            let needs_clone = add_clone || should_clone_field_value(&transformed_value);
            if needs_clone && !transformed_value.ends_with(".clone()") {
                transformed_value = format!("{}.clone()", transformed_value);
            }
            
            return format!("{}: {}", name, transformed_value);
        }
    }
    
    trimmed.to_string()
}

/// Transform a single field: `id = 1` → `id: 1`
pub fn transform_single_literal_field(field: &str) -> String {
    transform_single_literal_field_with_clone(field, false)
}