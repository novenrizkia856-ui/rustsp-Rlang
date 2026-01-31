//! Lowering Module
//!
//! Contains modules for analysis and preparation phases of transpilation.
//! These modules handle state tracking, depth counting, look-ahead operations,
//! and mode management during the transpilation process.

// State management
pub mod transpiler_state;

// Depth tracking
pub mod depth_tracking_lowering;

// Look-ahead utilities
pub mod lookahead_lowering;

// Multi-line construct handling
pub mod multiline_fn_lowering;
pub mod multiline_assign_lowering;

// Mode handling
pub mod use_import_lowering;
pub mod array_mode_lowering;
pub mod literal_mode_lowering;
pub mod match_mode_lowering;

// Re-exports for convenience
pub use transpiler_state::TranspilerState;
pub use depth_tracking_lowering::{
    count_braces_outside_strings,
    count_brackets_outside_strings,
    update_multiline_depth,
};
pub use lookahead_lowering::{
    check_before_closing_brace,
    check_next_is_else,
    check_next_line_is_where,
    check_next_line_starts_with_pipe,
    check_next_line_is_method_chain,
    check_next_line_closes_expr,
    detect_arm_has_if_expr,
};
pub use multiline_fn_lowering::{is_multiline_fn_start, process_multiline_fn_signature, MultilineFnResult};
pub use multiline_assign_lowering::{
    is_multiline_assign_start,
    is_multiline_assign_complete,
    process_complete_multiline_assign,
};
pub use use_import_lowering::{process_use_import_line, UseImportResult};
pub use array_mode_lowering::{process_array_mode_line, ArrayModeResult};
pub use literal_mode_lowering::{process_literal_mode_line, LiteralModeResult};
pub use match_mode_lowering::{process_match_mode_line, MatchModeResult};