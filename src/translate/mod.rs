//! Translation Module
//!
//! Contains modules for translating RustS+ syntax to Rust syntax.
//! Each module handles a specific type of construct transformation.

// Definition translations
pub mod struct_def_translate;
pub mod enum_def_translate;

// Literal translations
pub mod literal_start_translate;
pub mod literal_inline_translate;

// Function and declaration translations
pub mod function_def_translate;
pub mod const_static_translate;

// Passthrough and native handling
pub mod native_passthrough_translate;

// Array handling
pub mod array_literal_translate;

// Expression translations
pub mod assignment_translate;
pub mod expression_translate;

// Macro translations
pub mod macro_translate;

// Re-exports for convenience
pub use struct_def_translate::{process_struct_def_line, StructDefResult};
pub use enum_def_translate::{process_enum_def_line, EnumDefResult};
pub use literal_start_translate::{
    process_struct_literal_start,
    process_enum_literal_start,
    process_literal_in_call,
    process_bare_struct_literal,
    process_bare_enum_literal,
    LiteralStartResult,
};
pub use literal_inline_translate::{transform_fields_inline, transform_single_inline_field};
pub use function_def_translate::{process_function_def, process_rust_passthrough_function, FunctionDefResult};
pub use const_static_translate::transform_const_or_static;
pub use native_passthrough_translate::{is_rust_native_line, process_native_line};
pub use array_literal_translate::{process_array_literal_start, ArrayLiteralResult};
pub use assignment_translate::{process_assignment, parse_var_type_annotation, handle_bare_mut_in_match};
pub use expression_translate::{process_non_assignment, process_tuple_destructuring};
pub use macro_translate::transform_macros_to_correct_syntax;