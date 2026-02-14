//! Output post-processing pipeline for RustS+ transpiler
//!
//! This module contains the final post-processing steps applied to the
//! transpiled output before returning it.

use crate::helpers::transform_generic_brackets;
use crate::helpers::transform_macro_calls;
use crate::postprocess::{fix_bare_mut_declaration, strip_effects_from_line, strip_outer_keyword};

fn assert_lowering_guards(lines: &[String]) {
    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with(".expect(") {
            panic!("Invalid method chain lowering");
        }

        if line.contains(".clone()") && line.contains("[") && line.contains("]") && line.contains("..") {
            panic!("Invalid slice lowering: clone() emitted for slice expression");
        }
    }
}


/// Apply all post-processing transformations to the output lines
pub fn apply_postprocessing(output_lines: Vec<String>) -> String {
    // L-08: Transform macro calls (println -> println!, etc.)
    let transformed_lines: Vec<String> = output_lines
        .into_iter()
        .map(|line| transform_macro_calls(&line))
        .collect();
    
    //==========================================================================
    // L-01 POST-PROCESSING FIX: Catch any remaining bare `mut x = value`
    // This is a safety net for cases that slipped through the main processing.
    // Convert `mut x = value` to `let mut x = value;`
    //==========================================================================
    let fixed_lines: Vec<String> = transformed_lines
        .into_iter()
        .map(|line| fix_bare_mut_declaration(&line))
        .collect();
    
    //==========================================================================
    // L-05 POST-PROCESSING FIX: Strip any remaining effect annotations
    // This catches effect annotations that may have leaked through other paths.
    //==========================================================================
    let final_lines: Vec<String> = fixed_lines
        .into_iter()
        .map(|line| strip_effects_from_line(&line))
        .collect();
    
    //==========================================================================
    // CRITICAL POST-PROCESSING: Strip `outer` keyword from field assignments
    // `outer self.field = value` → `self.field = value`
    // This handles cases where the assignment parser didn't match because
    // `self.field` isn't a valid simple identifier
    //==========================================================================
    let outer_stripped: Vec<String> = final_lines
        .into_iter()
        .map(|line| strip_outer_keyword(&line))
        .collect();
    
    //==========================================================================
    // CRITICAL POST-PROCESSING: Transform RustS+ generic syntax to Rust
    // `Vec[String]` → `Vec<String>`, `HashMap[K, V]` → `HashMap<K, V>`
    // This handles generic type annotations throughout the code
    //==========================================================================
    let generic_transformed: Vec<String> = outer_stripped
        .into_iter()
        .map(|line| transform_generic_brackets(&line))
        .collect();
    
    assert_lowering_guards(&generic_transformed);

    generic_transformed.join("\n")
}