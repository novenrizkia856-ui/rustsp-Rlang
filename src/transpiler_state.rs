//! Transpiler State Management
//!
//! Contains the `TranspilerState` struct which holds all mutable state
//! during the transpilation process.

use crate::function::CurrentFunctionContext;
use crate::enum_def::EnumParseContext;
use crate::modes::{LiteralModeStack, ArrayModeStack, UseImportMode};
use crate::control_flow::MatchModeStack;

/// Main state container for the transpiler
/// 
/// This struct holds all mutable state that is tracked during
/// line-by-line transpilation of RustS+ code.
pub struct TranspilerState {
    // Depth tracking
    pub brace_depth: usize,
    pub bracket_depth: usize,
    
    // Function context
    pub in_function_body: bool,
    pub function_start_brace: usize,
    pub current_fn_ctx: CurrentFunctionContext,
    
    // Struct/enum definition contexts
    pub in_struct_def: bool,
    pub struct_def_depth: usize,
    pub enum_ctx: EnumParseContext,
    
    // Mode stacks
    pub literal_mode: LiteralModeStack,
    pub array_mode: ArrayModeStack,
    pub match_mode: MatchModeStack,
    pub use_import_mode: UseImportMode,
    
    // If expression assignment tracking
    pub if_expr_assignment_depth: Option<usize>,
    
    // Multi-line function signature accumulation
    pub multiline_fn_acc: Option<String>,
    pub multiline_fn_leading_ws: String,
    
    // Multi-line assignment accumulation
    pub multiline_assign_acc: Option<String>,
    pub multiline_assign_leading_ws: String,
    
    // Expression continuation tracking
    pub prev_line_was_continuation: bool,
    pub multiline_expr_depth: i32,
}

impl TranspilerState {
    /// Create a new TranspilerState with default values
    pub fn new() -> Self {
        Self {
            brace_depth: 0,
            bracket_depth: 0,
            in_function_body: false,
            function_start_brace: 0,
            current_fn_ctx: CurrentFunctionContext::new(),
            in_struct_def: false,
            struct_def_depth: 0,
            enum_ctx: EnumParseContext::new(),
            literal_mode: LiteralModeStack::new(),
            array_mode: ArrayModeStack::new(),
            match_mode: MatchModeStack::new(),
            use_import_mode: UseImportMode::new(),
            if_expr_assignment_depth: None,
            multiline_fn_acc: None,
            multiline_fn_leading_ws: String::new(),
            multiline_assign_acc: None,
            multiline_assign_leading_ws: String::new(),
            prev_line_was_continuation: false,
            multiline_expr_depth: 0,
        }
    }
    
    /// Reset continuation flag (called when line is empty)
    pub fn reset_continuation(&mut self) {
        self.prev_line_was_continuation = false;
    }
    
    /// Enter function body context
    pub fn enter_function(&mut self) {
        self.in_function_body = true;
        self.function_start_brace = self.brace_depth + 1;
    }
    
    /// Exit function body context
    pub fn exit_function(&mut self) {
        self.in_function_body = false;
        self.current_fn_ctx.exit();
    }
    
    /// Enter struct definition context
    pub fn enter_struct_def(&mut self) {
        self.in_struct_def = true;
        self.struct_def_depth = self.brace_depth;
    }
    
    /// Exit struct definition context
    pub fn exit_struct_def(&mut self) {
        self.in_struct_def = false;
    }
    
    /// Check if should exit function context
    pub fn should_exit_function(&self, trimmed: &str) -> bool {
        self.in_function_body 
            && self.brace_depth < self.function_start_brace 
            && trimmed == "}"
    }
    
    /// Check if should exit struct definition
    pub fn should_exit_struct_def(&self, trimmed: &str) -> bool {
        self.in_struct_def 
            && trimmed == "}" 
            && self.brace_depth <= self.struct_def_depth
    }
}

impl Default for TranspilerState {
    fn default() -> Self {
        Self::new()
    }
}