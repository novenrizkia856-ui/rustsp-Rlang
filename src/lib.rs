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
// NEW MODULAR COMPONENTS - Lowering (analysis/preparation)
// ============================================================================
pub mod transpiler_state;
pub mod depth_tracking_lowering;
pub mod lookahead_lowering;
pub mod multiline_fn_lowering;
pub mod multiline_assign_lowering;
pub mod use_import_lowering;
pub mod array_mode_lowering;
pub mod literal_mode_lowering;
pub mod match_mode_lowering;

// ============================================================================
// NEW MODULAR COMPONENTS - Translation (RustS+ → Rust)
// ============================================================================
pub mod struct_def_translate;
pub mod enum_def_translate;
pub mod literal_start_translate;
pub mod literal_inline_translate;
pub mod function_def_translate;
pub mod const_static_translate;
pub mod native_passthrough_translate;
pub mod array_literal_translate;
pub mod assignment_translate;
pub mod expression_translate;
pub mod macro_translate;

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