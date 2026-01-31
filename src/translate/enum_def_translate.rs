//! Enum Definition Translation
//!
//! Translates RustS+ enum definitions to Rust syntax.
//!
//! RustS+ enum variant syntax:
//! ```text
//! pub enum Status {
//!     Active,
//!     Pending { reason String },     // RustS+: no colon
//! }
//! ```
//!
//! Rust enum variant syntax:
//! ```text
//! pub enum Status {
//!     Active,
//!     Pending { reason: String },    // Rust: colon required
//! }
//! ```

use crate::enum_def::{EnumParseContext, is_enum_definition, transform_enum_variant};

/// Result of processing an enum definition line
pub enum EnumDefResult {
    /// Started enum definition
    Started(String),
    /// Closing struct variant inside enum
    ClosedStructVariant(String),
    /// Closing enum definition
    ClosedEnum(String),
    /// Variant inside enum definition
    Variant(String),
    /// Not an enum definition line
    NotEnumDef,
}

/// Process a line that might be part of enum definition
pub fn process_enum_def_line(
    trimmed: &str,
    clean_line: &str,
    leading_ws: &str,
    brace_depth: usize,
    opens: usize,
    closes: usize,
    enum_ctx: &mut EnumParseContext,
) -> EnumDefResult {
    // Check for enum definition start
    if is_enum_definition(trimmed) && !enum_ctx.in_enum_def {
        enum_ctx.enter_enum(brace_depth);
        
        // CRITICAL FIX: Do NOT auto-inject Clone!
        // Some enum variants may contain non-Clone types.
        // Let user explicitly add #[derive(Clone)] when needed.
        
        return EnumDefResult::Started(format!("{}{}", leading_ws, trimmed));
    }
    
    // Process inside enum definition
    if enum_ctx.in_enum_def {
        // Check for closing struct variant
        if trimmed == "}" && enum_ctx.in_struct_variant {
            enum_ctx.exit_struct_variant();
            return EnumDefResult::ClosedStructVariant(format!("{}}},", leading_ws));
        }
        
        // Check for closing enum
        if trimmed == "}" && brace_depth <= enum_ctx.start_depth {
            enum_ctx.exit_enum();
            return EnumDefResult::ClosedEnum(format!("{}}}", leading_ws));
        }
        
        // Check for struct variant start
        if trimmed.contains('{') && opens > closes {
            enum_ctx.enter_struct_variant();
        }
        
        // Transform variant
        let transformed = transform_enum_variant(clean_line, enum_ctx.in_struct_variant);
        return EnumDefResult::Variant(transformed);
    }
    
    EnumDefResult::NotEnumDef
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_enum_def_start() {
        let mut enum_ctx = EnumParseContext::new();
        
        let result = process_enum_def_line(
            "pub enum Status {",
            "pub enum Status {",
            "",
            0,
            1,
            0,
            &mut enum_ctx,
        );
        
        assert!(matches!(result, EnumDefResult::Started(_)));
        assert!(enum_ctx.in_enum_def);
    }
    
    #[test]
    fn test_enum_def_struct_variant() {
        let mut enum_ctx = EnumParseContext::new();
        enum_ctx.enter_enum(0);
        
        // Start struct variant
        let result = process_enum_def_line(
            "Pending {",
            "Pending {",
            "    ",
            1,
            1,
            0,
            &mut enum_ctx,
        );
        
        assert!(matches!(result, EnumDefResult::Variant(_)));
        assert!(enum_ctx.in_struct_variant);
    }
    
    #[test]
    fn test_enum_def_close() {
        let mut enum_ctx = EnumParseContext::new();
        enum_ctx.enter_enum(0);
        
        let result = process_enum_def_line(
            "}",
            "}",
            "",
            0,
            0,
            1,
            &mut enum_ctx,
        );
        
        assert!(matches!(result, EnumDefResult::ClosedEnum(_)));
        assert!(!enum_ctx.in_enum_def);
    }
}