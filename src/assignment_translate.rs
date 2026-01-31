//! Assignment Translation
//!
//! Handles RustS+ assignment translation to Rust.
//!
//! RustS+ assignment syntax:
//! ```text
//! x = 1                    // Simple assignment
//! mut y i32 = 2           // Mutable with type
//! config Config = Config { ... }  // Struct literal
//! ```
//!
//! Rust assignment syntax:
//! ```text
//! let x = 1;
//! let mut y: i32 = 2;
//! let config = Config { ... };
//! ```

use crate::variable::{expand_value, parse_rusts_assignment_ext};
use crate::scope::ScopeAnalyzer;
use crate::variable::VariableTracker;
use crate::function::{
    CurrentFunctionContext, FunctionRegistry,
    transform_string_concat, transform_call_args,
};
use crate::control_flow::transform_enum_struct_init;
use crate::clone_helpers::transform_array_access_clone;
use crate::helpers::ends_with_continuation_operator;

/// Process a RustS+ assignment line
pub fn process_assignment(
    var_name: &str,
    var_type: Option<&str>,
    value: &str,
    is_outer: bool,
    is_explicit_mut: bool,
    line_num: usize,
    leading_ws: &str,
    scope_analyzer: &ScopeAnalyzer,
    tracker: &VariableTracker,
    current_fn_ctx: &CurrentFunctionContext,
    fn_registry: &FunctionRegistry,
    inside_multiline_expr: bool,
    next_line_is_method_chain: bool,
    next_line_closes_expr: bool,
    prev_line_was_continuation: &mut bool,
) -> String {
    let is_decl = scope_analyzer.is_decl(line_num);
    let is_mutation = scope_analyzer.is_mut(line_num);
    let borrowed_mut = tracker.is_mut_borrowed(var_name);
    let mutated_via_method = tracker.is_mutated_via_method(var_name);
    let scope_needs_mut = scope_analyzer.needs_mut(var_name, line_num);
    let needs_mut = is_explicit_mut || borrowed_mut || mutated_via_method || scope_needs_mut;
    
    // Expand and transform value
    let mut expanded_value = expand_value(value, var_type);
    expanded_value = transform_array_access_clone(&expanded_value);
    
    if current_fn_ctx.is_inside() {
        expanded_value = transform_string_concat(&expanded_value, current_fn_ctx);
    }
    expanded_value = transform_call_args(&expanded_value, fn_registry);
    expanded_value = transform_enum_struct_init(&expanded_value);
    
    let is_param = current_fn_ctx.params.contains_key(var_name);
    let is_shadowing = tracker.is_shadowing(var_name, line_num);
    let should_have_let = is_decl || (!is_mutation && !is_param) || is_shadowing;
    
    // CRITICAL FIX: Semicolon logic
    // 1. If value ends with continuation → no semicolon (expression continues)
    // 2. If next line is method chain → no semicolon (chained call)
    // 3. If inside multiline expr AND next line closes it → no semicolon (we're last arg)
    // 4. Otherwise → add semicolon
    let suppress_semi = ends_with_continuation_operator(&expanded_value)
        || next_line_is_method_chain
        || (inside_multiline_expr && next_line_closes_expr);
    let semi = if suppress_semi { "" } else { ";" };
    *prev_line_was_continuation = ends_with_continuation_operator(&expanded_value);
    
    let type_annotation = var_type.map(|t| format!(": {}", t)).unwrap_or_default();
    
    if is_outer {
        format!("{}{} = {}{}", leading_ws, var_name, expanded_value, semi)
    } else if is_explicit_mut {
        format!("{}let mut {}{} = {}{}", leading_ws, var_name, type_annotation, expanded_value, semi)
    } else if should_have_let {
        let let_keyword = if needs_mut { "let mut" } else { "let" };
        format!("{}{} {}{} = {}{}", leading_ws, let_keyword, var_name, type_annotation, expanded_value, semi)
    } else if is_mutation && is_param {
        format!("{}{} = {}{}", leading_ws, var_name, expanded_value, semi)
    } else {
        let let_keyword = if needs_mut { "let mut" } else { "let" };
        format!("{}{} {}{} = {}{}", leading_ws, let_keyword, var_name, type_annotation, expanded_value, semi)
    }
}

/// Parse variable name with optional type annotation
/// 
/// `var_part` could be "sender &Address" which needs to become ("sender", ": &Address")
pub fn parse_var_type_annotation(var_part: &str) -> (&str, String) {
    if var_part.contains(' ') {
        let space_pos = var_part.find(' ').unwrap();
        let vname = var_part[..space_pos].trim();
        let vtype = var_part[space_pos + 1..].trim();
        
        let vname_valid = !vname.is_empty() 
            && vname.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
            && vname.chars().all(|c| c.is_alphanumeric() || c == '_');
        
        let vtype_valid = !vtype.is_empty() && (
            vtype.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
            || vtype.starts_with("Vec[") || vtype.starts_with("Vec<")
            || vtype.starts_with("Option[") || vtype.starts_with("Option<")
            || vtype.starts_with("Result[") || vtype.starts_with("Result<")
            || vtype.starts_with("HashMap[") || vtype.starts_with("HashMap<")
            || vtype.starts_with("HashSet[") || vtype.starts_with("HashSet<")
            || vtype.starts_with('&')
        );
        
        if vname_valid && vtype_valid {
            return (vname, format!(": {}", vtype));
        }
    } else if var_part.contains(':') {
        let parts: Vec<&str> = var_part.splitn(2, ':').collect();
        if parts.len() == 2 {
            return (parts[0].trim(), format!(": {}", parts[1].trim()));
        }
    }
    
    (var_part, String::new())
}

/// Handle bare `mut` in match arm body
pub fn handle_bare_mut_in_match(
    _clean_line: &str,
    trimmed: &str,
    leading_ws: &str,
    current_fn_ctx: &CurrentFunctionContext,
    fn_registry: &FunctionRegistry,
) -> Option<String> {
    if !trimmed.starts_with("mut ") || !trimmed.contains('=') || trimmed.contains("==") {
        return None;
    }
    
    let rest = trimmed.strip_prefix("mut ").unwrap().trim();
    let eq_pos = rest.find('=')?;
    
    let var_part = rest[..eq_pos].trim();
    let val_part = rest[eq_pos + 1..].trim().trim_end_matches(';');
    
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
    
    let mut expanded_value = expand_value(val_part, None);
    expanded_value = transform_array_access_clone(&expanded_value);
    if current_fn_ctx.is_inside() {
        expanded_value = transform_string_concat(&expanded_value, current_fn_ctx);
    }
    expanded_value = transform_call_args(&expanded_value, fn_registry);
    
    Some(format!("{}let mut {}{} = {};", leading_ws, var_name, type_annotation, expanded_value))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_var_type_annotation_with_space() {
        let (name, ann) = parse_var_type_annotation("sender &Address");
        assert_eq!(name, "sender");
        assert_eq!(ann, ": &Address");
    }
    
    #[test]
    fn test_parse_var_type_annotation_with_colon() {
        let (name, ann) = parse_var_type_annotation("x: i32");
        assert_eq!(name, "x");
        assert_eq!(ann, ": i32");
    }
    
    #[test]
    fn test_parse_var_type_annotation_simple() {
        let (name, ann) = parse_var_type_annotation("x");
        assert_eq!(name, "x");
        assert_eq!(ann, "");
    }
}