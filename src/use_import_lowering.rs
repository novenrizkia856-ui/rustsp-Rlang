//! Use Import Lowering
//!
//! Handles multi-line `use` import statements in RustS+.
//!
//! Example:
//! ```text
//! use std::{
//!     collections::HashMap,
//!     sync::Arc,
//! };
//! ```

use crate::modes::{UseImportMode, is_multiline_use_import_start, transform_use_import_item};

/// Result of processing a line in use import mode
pub enum UseImportResult {
    /// Line was handled by use import mode
    Handled(String),
    /// Line was not for use import mode
    NotHandled,
}

/// Process a line that might be part of use import mode
/// 
/// # Arguments
/// * `trimmed` - Trimmed line content
/// * `clean_line` - Clean line (with inline comments stripped)
/// * `leading_ws` - Leading whitespace
/// * `brace_depth` - Current brace depth
/// * `use_import_mode` - Mutable reference to use import mode state
/// 
/// # Returns
/// `UseImportResult` indicating how the line was handled
pub fn process_use_import_line(
    trimmed: &str,
    clean_line: &str,
    leading_ws: &str,
    brace_depth: usize,
    use_import_mode: &mut UseImportMode,
) -> UseImportResult {
    // Check for closing brace while in use import mode
    if use_import_mode.is_active() && trimmed == "}" {
        if use_import_mode.should_exit(brace_depth) {
            use_import_mode.exit();
            return UseImportResult::Handled(format!("{}}};", leading_ws));
        }
    }
    
    // Process line inside use import mode
    if use_import_mode.is_active() {
        let transformed = transform_use_import_item(clean_line);
        return UseImportResult::Handled(transformed);
    }
    
    // Check if this line starts a new use import block
    if let Some(is_pub) = is_multiline_use_import_start(trimmed) {
        use_import_mode.enter(brace_depth, is_pub);
        return UseImportResult::Handled(format!("{}{}", leading_ws, trimmed));
    }
    
    UseImportResult::NotHandled
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_use_import_start_detection() {
        // The actual detection is in modes module
        // This just tests integration
        let mut mode = UseImportMode::new();
        
        assert!(!mode.is_active());
        
        // Enter mode
        mode.enter(0, false);
        assert!(mode.is_active());
        
        // Exit mode
        mode.exit();
        assert!(!mode.is_active());
    }
}