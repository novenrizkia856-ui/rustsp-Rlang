//! Struct Definition Translation
//!
//! Translates RustS+ struct definitions to Rust syntax.
//!
//! RustS+ struct field syntax:
//! ```text
//! pub struct Config {
//!     name String        // RustS+: no colon
//!     value i32
//! }
//! ```
//!
//! Rust struct field syntax:
//! ```text
//! pub struct Config {
//!     name: String,      // Rust: colon required
//!     value: i32,
//! }
//! ```

use crate::struct_def::{is_struct_definition, transform_struct_field};
use crate::helpers::{transform_struct_field_slice_to_vec, transform_generic_brackets};

/// Result of processing a struct definition line
pub enum StructDefResult {
    /// Started struct definition
    Started(String),
    /// Closing struct definition
    Closed(String),
    /// Field inside struct definition
    Field(String),
    /// Not a struct definition line
    NotStructDef,
}

/// Process a line that might be part of struct definition
pub fn process_struct_def_line(
    trimmed: &str,
    clean_line: &str,
    leading_ws: &str,
    brace_depth: usize,
    in_struct_def: &mut bool,
    struct_def_depth: &mut usize,
) -> StructDefResult {
    // Check for struct definition start
    if is_struct_definition(trimmed) && !*in_struct_def {
        *in_struct_def = true;
        *struct_def_depth = brace_depth;
        
        // CRITICAL FIX: Do NOT auto-inject Clone!
        // AtomicU64 and other types don't implement Clone.
        // Let user explicitly add #[derive(Clone)] when needed.
        
        return StructDefResult::Started(format!("{}{}", leading_ws, trimmed));
    }
    
    // Process inside struct definition
    if *in_struct_def {
        // Check for closing brace
        if trimmed == "}" && brace_depth <= *struct_def_depth {
            *in_struct_def = false;
            return StructDefResult::Closed(format!("{}}}", leading_ws));
        }
        
        // Transform field
        let transformed = transform_struct_field(clean_line);
        let transformed = transform_struct_field_slice_to_vec(&transformed);
        // CRITICAL FIX: Transform generic brackets in struct field types
        // e.g., Option[Arc[T]] -> Option<Arc<T>>
        let transformed = transform_generic_brackets(&transformed);
        
        return StructDefResult::Field(transformed);
    }
    
    StructDefResult::NotStructDef
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_struct_def_start() {
        let mut in_struct_def = false;
        let mut struct_def_depth = 0;
        
        let result = process_struct_def_line(
            "pub struct Config {",
            "pub struct Config {",
            "",
            0,
            &mut in_struct_def,
            &mut struct_def_depth,
        );
        
        assert!(matches!(result, StructDefResult::Started(_)));
        assert!(in_struct_def);
    }
    
    #[test]
    fn test_struct_def_close() {
        let mut in_struct_def = true;
        let mut struct_def_depth = 0;
        
        let result = process_struct_def_line(
            "}",
            "}",
            "",
            0,
            &mut in_struct_def,
            &mut struct_def_depth,
        );
        
        assert!(matches!(result, StructDefResult::Closed(_)));
        assert!(!in_struct_def);
    }
}