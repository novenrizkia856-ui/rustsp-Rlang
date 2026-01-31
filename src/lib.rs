//! RustS+ Transpiler Library
//!
//! This is the main entry point for the RustS+ to Rust transpiler.
//! The transpilation is organized into multiple phases:
//! 1. First pass: Register types and track clone requirements
//! 2. Second pass: Line-by-line transformation
//! 3. Post-processing: Final cleanup and validation

// ============================================================================
// CORE MODULES (external)
// ============================================================================
pub mod variable;
pub mod scope;
pub mod function;
pub mod struct_def;
pub mod enum_def;
pub mod control_flow;
pub mod error_msg;
pub mod semantic_check;
pub mod anti_fail_logic;
pub mod rust_sanity;
pub mod hex_normalizer;

// ============================================================================
// IR-BASED MODULES
// ============================================================================
pub mod ast;
pub mod hir;
pub mod eir;
pub mod parser;
pub mod type_env;
pub mod source_map;

// ============================================================================
// EXISTING MODULAR COMPONENTS
// ============================================================================
pub mod helpers;
pub mod modes;
pub mod detection;
pub mod transform_literal;
pub mod transform_array;
pub mod clone_helpers;
pub mod postprocess;
pub mod first_pass;
pub mod parser_state;
pub mod inline_literal_transform;
pub mod postprocess_output;
pub mod tests;

// ============================================================================
// NEW MODULAR COMPONENTS
// ============================================================================

/// Lowering modules - analysis and preparation phases
/// 
/// Contains:
/// - `transpiler_state` - Parser state management
/// - `depth_tracking` - Brace/bracket depth tracking
/// - `lookahead` - Look-ahead utilities
/// - `multiline_fn` - Multi-line function signature handling
/// - `multiline_assign` - Multi-line assignment handling
/// - `use_import_mode` - Use import mode handling
/// - `array_mode` - Array mode handling
/// - `literal_mode` - Literal mode handling
/// - `match_mode` - Match mode handling
pub mod lowering;

/// Translation modules - RustS+ → Rust syntax transformation
/// 
/// Contains:
/// - `struct_def` - Struct definition translation
/// - `enum_def` - Enum definition translation
/// - `literal_start` - Struct/enum literal start translation
/// - `literal_inline` - Inline literal field transformation
/// - `function_def` - Function definition translation
/// - `const_static` - Const/static declaration translation
/// - `native_passthrough` - Rust native line detection
/// - `array_literal` - Array literal translation
/// - `assignment` - Assignment processing
/// - `expression` - Non-assignment expression processing
/// - `macro_transform` - Macro transformation
pub mod translate;

// ============================================================================
// MAIN TRANSPILATION
// ============================================================================
pub mod transpile_main;

// ============================================================================
// RE-EXPORTS
// ============================================================================
pub use ast::{Span, Spanned, EffectDecl};
pub use hir::{BindingId, BindingInfo, ScopeResolver, HirModule};
pub use eir::{Effect, EffectSet, EffectContext, EffectInference};
pub use parser::{Lexer, FunctionParser, extract_function_signatures};

pub use type_env::{
    TypeEnv, TypeEnvBuilder, TypeDrivenInference,
    FunctionType, EffectSignature, ParamEffect,
};

// ============================================================================
// MAIN ENTRY POINT
// ============================================================================

/// Main entry point for RustS+ to Rust transpilation
pub fn parse_rusts(source: &str) -> String {
    transpile_main::parse_rusts(source)
}