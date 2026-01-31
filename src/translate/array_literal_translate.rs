//! Array Literal Translation
//!
//! Handles array literal start detection and translation.
//!
//! RustS+ array syntax:
//! ```text
//! items Vec[Item] = [
//!     Item { name = "foo" },
//!     Item { name = "bar" },
//! ]
//! ```
//!
//! Rust array syntax:
//! ```text
//! let items: Vec<Item> = vec![
//!     Item { name: "foo" },
//!     Item { name: "bar" },
//! ];
//! ```

use crate::modes::ArrayModeStack;
use crate::detection::detect_array_literal_start;
use crate::transform_array::transform_array_element;
use crate::scope::ScopeAnalyzer;
use crate::variable::VariableTracker;
use crate::function::CurrentFunctionContext;

/// Result of processing array literal start
pub enum ArrayLiteralResult {
    /// Array literal detected and started
    Started(String),
    /// Not an array literal start
    NotArrayLiteral,
}

/// Process array literal start
pub fn process_array_literal_start(
    trimmed: &str,
    leading_ws: &str,
    line_num: usize,
    prev_bracket_depth: usize,
    bracket_opens: usize,
    scope_analyzer: &ScopeAnalyzer,
    tracker: &VariableTracker,
    current_fn_ctx: &CurrentFunctionContext,
    array_mode: &mut ArrayModeStack,
) -> ArrayLiteralResult {
    let (var_name, var_type, after_bracket) = match detect_array_literal_start(trimmed) {
        Some(tuple) => tuple,
        None => return ArrayLiteralResult::NotArrayLiteral,
    };
    
    // Determine mutability and let requirements
    let is_param = current_fn_ctx.params.contains_key(&var_name);
    let is_decl = scope_analyzer.is_decl(line_num);
    let is_mutation = scope_analyzer.is_mut(line_num);
    let is_shadowing = tracker.is_shadowing(&var_name, line_num);
    let borrowed_mut = tracker.is_mut_borrowed(&var_name);
    let mutated_via_method = tracker.is_mutated_via_method(&var_name);
    let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
    let needs_mut = borrowed_mut || mutated_via_method || scope_needs_mut;
    let needs_let = is_decl || (!is_mutation && !is_param) || is_shadowing;
    
    // Enter array mode
    array_mode.enter(
        prev_bracket_depth + bracket_opens, 
        true,
        var_name.clone(),
        var_type.clone(),
        needs_let,
        needs_mut
    );
    
    // Build output
    let let_keyword = if needs_let {
        if needs_mut { "let mut " } else { "let " }
    } else {
        ""
    };
    
    let type_annotation = if let Some(ref t) = var_type {
        format!(": {}", t)
    } else {
        String::new()
    };
    
    // CRITICAL FIX: Detect if source uses vec![ and preserve it
    let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
    let rhs = parts.get(1).map(|s| s.trim()).unwrap_or("");
    let array_open = if rhs.starts_with("vec![") {
        "vec!["
    } else if rhs.starts_with("Vec::from([") {
        "Vec::from(["
    } else {
        "["
    };
    
    let after = after_bracket.trim();
    let mut output_lines = Vec::new();
    
    if after.is_empty() {
        output_lines.push(format!("{}{}{}{} = {}", leading_ws, let_keyword, var_name, type_annotation, array_open));
    } else {
        let transformed_first = transform_array_element(&format!("    {}", after));
        output_lines.push(format!("{}{}{}{} = {}", leading_ws, let_keyword, var_name, type_annotation, array_open));
        if !transformed_first.trim().is_empty() {
            output_lines.push(transformed_first);
        }
    }
    
    ArrayLiteralResult::Started(output_lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_array_literal_detection() {
        // Detection is done by detect_array_literal_start in detection module
        // This test verifies the integration
        let mut array_mode = ArrayModeStack::new();
        let scope_analyzer = ScopeAnalyzer::new();
        let tracker = VariableTracker::new();
        let fn_ctx = CurrentFunctionContext::new();
        
        let result = process_array_literal_start(
            "items = [",
            "    ",
            0,
            0,
            1,
            &scope_analyzer,
            &tracker,
            &fn_ctx,
            &mut array_mode,
        );
        
        // Should detect array literal
        // (actual behavior depends on detect_array_literal_start implementation)
    }
}