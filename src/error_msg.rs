//! RustS+ Error Message System
//!
//! This module provides a comprehensive, human-readable error reporting system
//! for the RustS+ compiler. Errors are categorized, coded, and formatted to
//! provide maximum clarity and actionable suggestions.
//!
//! ## Design Philosophy
//!
//! 1. **Errors explain WHY, not just WHAT** - Every error includes context
//! 2. **Intention-aware** - Detects common patterns and suggests fixes
//! 3. **Stage-aware** - Distinguishes RustS+ errors from Rust backend errors
//! 4. **Consistent format** - All errors follow the same structure
//!
//! ## Error Code Format
//!
//! `RSPLxxx` where:
//! - `RSPL` = RustS+ Language
//! - `xxx` = 3-digit code grouped by category
//!
//! Code ranges:
//! - 001-019: Logic errors
//! - 020-039: Structure errors  
//! - 040-059: Expression errors
//! - 060-079: Control flow errors
//! - 080-099: Scope errors
//! - 100-119: Ownership errors
//! - 120-139: Type consistency errors
//! - 200-299: Rust backend mapping errors

use std::fmt;

//=============================================================================
// ERROR CATEGORIES
//=============================================================================

/// Error category for classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// General logic errors (RSPL001-019)
    Logic,
    /// Structural errors - syntax, definitions (RSPL020-039)
    Structure,
    /// Expression errors - value/type issues (RSPL040-059)
    Expression,
    /// Control flow errors - if/match/loop (RSPL060-079)
    ControlFlow,
    /// Scope errors - shadowing, visibility (RSPL080-099)
    Scope,
    /// Ownership errors - borrow, move, lifetime (RSPL100-119)
    Ownership,
    /// Type consistency errors (RSPL120-139)
    TypeConsistency,
    /// Mapped from Rust backend (RSPL200-299)
    RustBackend,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCategory::Logic => write!(f, "logic"),
            ErrorCategory::Structure => write!(f, "structure"),
            ErrorCategory::Expression => write!(f, "expression"),
            ErrorCategory::ControlFlow => write!(f, "control-flow"),
            ErrorCategory::Scope => write!(f, "scope"),
            ErrorCategory::Ownership => write!(f, "ownership"),
            ErrorCategory::TypeConsistency => write!(f, "type-consistency"),
            ErrorCategory::RustBackend => write!(f, "rust-backend"),
        }
    }
}

//=============================================================================
// ERROR CODES
//=============================================================================

/// Stable error codes for RustS+
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    // Logic errors (001-019)
    /// Generic logic error
    RSPL001,
    /// Unreachable code detected
    RSPL002,
    /// Infinite loop detected
    RSPL003,
    
    // Structure errors (020-039)
    /// Invalid function signature
    RSPL020,
    /// Invalid struct definition
    RSPL021,
    /// Invalid enum definition
    RSPL022,
    /// Missing function body
    RSPL023,
    /// Duplicate definition
    RSPL024,
    /// Invalid field syntax
    RSPL025,
    /// Missing type annotation where required
    RSPL026,
    
    // Expression errors (040-059)
    /// Expression used as statement (missing semicolon context)
    RSPL040,
    /// Statement used as expression
    RSPL041,
    /// Invalid assignment target
    RSPL042,
    /// Missing value in expression context
    RSPL043,
    /// Type mismatch in expression
    RSPL044,
    /// Invalid operator usage
    RSPL045,
    /// String literal where String expected
    RSPL046,
    
    // Control flow errors (060-079)
    /// If expression missing else branch (when used as value)
    RSPL060,
    /// Match expression missing arms
    RSPL061,
    /// Match arm type mismatch
    RSPL062,
    /// Unreachable match arm
    RSPL063,
    /// Non-exhaustive match
    RSPL064,
    /// Invalid guard expression
    RSPL065,
    /// Break outside loop
    RSPL066,
    /// Continue outside loop
    RSPL067,
    /// Return outside function
    RSPL068,
    
    // Logic binding errors (070-079)
    /// Same-scope reassignment without mut
    RSPL071,
    
    // Scope errors (080-099)
    /// Variable not found in scope
    RSPL080,
    /// Unintended shadowing (assignment creates new variable)
    RSPL081,
    /// Outer keyword on non-existent variable
    RSPL082,
    /// Variable used before initialization
    RSPL083,
    /// Scope leak attempt
    RSPL084,
    /// Invalid outer mutation target
    RSPL085,
    
    // Ownership errors (100-119)
    /// Move after borrow
    RSPL100,
    /// Mutable borrow while immutable exists
    RSPL101,
    /// Multiple mutable borrows
    RSPL102,
    /// Use after move
    RSPL103,
    /// Cannot mutate immutable variable
    RSPL104,
    /// Lifetime mismatch
    RSPL105,
    
    // Type consistency errors (120-139)
    /// Function return type mismatch
    RSPL120,
    /// Argument type mismatch
    RSPL121,
    /// Field type mismatch
    RSPL122,
    /// Generic constraint not satisfied
    RSPL123,
    /// Cannot infer type
    RSPL124,
    
    // Rust backend mapped errors (200-299)
    /// Generic Rust error (unmapped)
    RSPL200,
    /// Rust borrow checker error
    RSPL201,
    /// Rust type error
    RSPL202,
    /// Rust lifetime error
    RSPL203,
    /// Rust move error
    RSPL204,
}

impl ErrorCode {
    /// Get the numeric code as string
    pub fn code_str(&self) -> &'static str {
        match self {
            // Logic
            ErrorCode::RSPL001 => "RSPL001",
            ErrorCode::RSPL002 => "RSPL002",
            ErrorCode::RSPL003 => "RSPL003",
            // Structure
            ErrorCode::RSPL020 => "RSPL020",
            ErrorCode::RSPL021 => "RSPL021",
            ErrorCode::RSPL022 => "RSPL022",
            ErrorCode::RSPL023 => "RSPL023",
            ErrorCode::RSPL024 => "RSPL024",
            ErrorCode::RSPL025 => "RSPL025",
            ErrorCode::RSPL026 => "RSPL026",
            // Expression
            ErrorCode::RSPL040 => "RSPL040",
            ErrorCode::RSPL041 => "RSPL041",
            ErrorCode::RSPL042 => "RSPL042",
            ErrorCode::RSPL043 => "RSPL043",
            ErrorCode::RSPL044 => "RSPL044",
            ErrorCode::RSPL045 => "RSPL045",
            ErrorCode::RSPL046 => "RSPL046",
            // Control flow
            ErrorCode::RSPL060 => "RSPL060",
            ErrorCode::RSPL061 => "RSPL061",
            ErrorCode::RSPL062 => "RSPL062",
            ErrorCode::RSPL063 => "RSPL063",
            ErrorCode::RSPL064 => "RSPL064",
            ErrorCode::RSPL065 => "RSPL065",
            ErrorCode::RSPL066 => "RSPL066",
            ErrorCode::RSPL067 => "RSPL067",
            ErrorCode::RSPL068 => "RSPL068",
            // Scope
            ErrorCode::RSPL071 => "RSPL071",
            ErrorCode::RSPL080 => "RSPL080",
            ErrorCode::RSPL081 => "RSPL081",
            ErrorCode::RSPL082 => "RSPL082",
            ErrorCode::RSPL083 => "RSPL083",
            ErrorCode::RSPL084 => "RSPL084",
            ErrorCode::RSPL085 => "RSPL085",
            // Ownership
            ErrorCode::RSPL100 => "RSPL100",
            ErrorCode::RSPL101 => "RSPL101",
            ErrorCode::RSPL102 => "RSPL102",
            ErrorCode::RSPL103 => "RSPL103",
            ErrorCode::RSPL104 => "RSPL104",
            ErrorCode::RSPL105 => "RSPL105",
            // Type consistency
            ErrorCode::RSPL120 => "RSPL120",
            ErrorCode::RSPL121 => "RSPL121",
            ErrorCode::RSPL122 => "RSPL122",
            ErrorCode::RSPL123 => "RSPL123",
            ErrorCode::RSPL124 => "RSPL124",
            // Rust backend
            ErrorCode::RSPL200 => "RSPL200",
            ErrorCode::RSPL201 => "RSPL201",
            ErrorCode::RSPL202 => "RSPL202",
            ErrorCode::RSPL203 => "RSPL203",
            ErrorCode::RSPL204 => "RSPL204",
        }
    }
    
    /// Get the category for this error code
    pub fn category(&self) -> ErrorCategory {
        match self {
            ErrorCode::RSPL001 | ErrorCode::RSPL002 | ErrorCode::RSPL003 => ErrorCategory::Logic,
            ErrorCode::RSPL020 | ErrorCode::RSPL021 | ErrorCode::RSPL022 |
            ErrorCode::RSPL023 | ErrorCode::RSPL024 | ErrorCode::RSPL025 |
            ErrorCode::RSPL026 => ErrorCategory::Structure,
            ErrorCode::RSPL040 | ErrorCode::RSPL041 | ErrorCode::RSPL042 |
            ErrorCode::RSPL043 | ErrorCode::RSPL044 | ErrorCode::RSPL045 |
            ErrorCode::RSPL046 => ErrorCategory::Expression,
            ErrorCode::RSPL060 | ErrorCode::RSPL061 | ErrorCode::RSPL062 |
            ErrorCode::RSPL063 | ErrorCode::RSPL064 | ErrorCode::RSPL065 |
            ErrorCode::RSPL066 | ErrorCode::RSPL067 | ErrorCode::RSPL068 => ErrorCategory::ControlFlow,
            ErrorCode::RSPL071 |
            ErrorCode::RSPL080 | ErrorCode::RSPL081 | ErrorCode::RSPL082 |
            ErrorCode::RSPL083 | ErrorCode::RSPL084 | ErrorCode::RSPL085 => ErrorCategory::Scope,
            ErrorCode::RSPL100 | ErrorCode::RSPL101 | ErrorCode::RSPL102 |
            ErrorCode::RSPL103 | ErrorCode::RSPL104 | ErrorCode::RSPL105 => ErrorCategory::Ownership,
            ErrorCode::RSPL120 | ErrorCode::RSPL121 | ErrorCode::RSPL122 |
            ErrorCode::RSPL123 | ErrorCode::RSPL124 => ErrorCategory::TypeConsistency,
            ErrorCode::RSPL200 | ErrorCode::RSPL201 | ErrorCode::RSPL202 |
            ErrorCode::RSPL203 | ErrorCode::RSPL204 => ErrorCategory::RustBackend,
        }
    }
}

//=============================================================================
// SOURCE LOCATION
//=============================================================================

/// Location in source code
#[derive(Debug, Clone, Default)]
pub struct SourceLocation {
    /// File name
    pub file: String,
    /// Line number (1-indexed)
    pub line: usize,
    /// Column number (1-indexed)
    pub column: usize,
    /// The source line text
    pub source_line: String,
    /// Highlight start position in source_line
    pub highlight_start: usize,
    /// Highlight length
    pub highlight_len: usize,
}

impl SourceLocation {
    pub fn new(file: &str, line: usize, column: usize) -> Self {
        SourceLocation {
            file: file.to_string(),
            line,
            column,
            source_line: String::new(),
            highlight_start: 0,
            highlight_len: 0,
        }
    }
    
    pub fn with_source(mut self, source_line: &str, highlight_start: usize, highlight_len: usize) -> Self {
        self.source_line = source_line.to_string();
        self.highlight_start = highlight_start;
        self.highlight_len = highlight_len.max(1);
        self
    }
}

//=============================================================================
// RSPL ERROR
//=============================================================================

/// A complete RustS+ error with all required information
#[derive(Debug, Clone)]
pub struct RsplError {
    /// Error code
    pub code: ErrorCode,
    /// Short title/message
    pub title: String,
    /// Source location
    pub location: SourceLocation,
    /// Detailed explanation (note section)
    pub explanation: Option<String>,
    /// Suggested fix (help section)
    pub suggestion: Option<String>,
    /// Additional labels for multi-span errors
    pub labels: Vec<(SourceLocation, String)>,
}

impl RsplError {
    /// Create a new error
    pub fn new(code: ErrorCode, title: impl Into<String>) -> Self {
        RsplError {
            code,
            title: title.into(),
            location: SourceLocation::default(),
            explanation: None,
            suggestion: None,
            labels: Vec::new(),
        }
    }
    
    /// Set the location
    pub fn at(mut self, location: SourceLocation) -> Self {
        self.location = location;
        self
    }
    
    /// Set location from file/line/column
    pub fn at_pos(mut self, file: &str, line: usize, column: usize) -> Self {
        self.location = SourceLocation::new(file, line, column);
        self
    }
    
    /// Add source context
    pub fn with_source(mut self, source_line: &str, highlight_start: usize, highlight_len: usize) -> Self {
        self.location = self.location.with_source(source_line, highlight_start, highlight_len);
        self
    }
    
    /// Add explanation (note section)
    pub fn note(mut self, explanation: impl Into<String>) -> Self {
        self.explanation = Some(explanation.into());
        self
    }
    
    /// Add suggestion (help section)
    pub fn help(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
    
    /// Add additional label
    pub fn label(mut self, location: SourceLocation, message: impl Into<String>) -> Self {
        self.labels.push((location, message.into()));
        self
    }
    
    /// Format the error for display
    pub fn format(&self) -> String {
        let mut output = String::new();
        let category = self.code.category();
        
        // Header line
        output.push_str(&format!(
            "error[{}][{}]: {}\n",
            self.code.code_str(),
            category,
            self.title
        ));
        
        // Location
        if !self.location.file.is_empty() {
            output.push_str(&format!(
                "  --> {}:{}:{}\n",
                self.location.file,
                self.location.line,
                self.location.column
            ));
        }
        
        // Source line with highlight
        if !self.location.source_line.is_empty() {
            let line_num_width = self.location.line.to_string().len();
            let padding = " ".repeat(line_num_width);
            
            output.push_str(&format!("{}  |\n", padding));
            output.push_str(&format!(
                "{} |   {}\n",
                self.location.line,
                self.location.source_line
            ));
            
            // Highlight
            let highlight_padding = " ".repeat(self.location.highlight_start);
            let highlight = "^".repeat(self.location.highlight_len);
            output.push_str(&format!(
                "{}  |   {}{}\n",
                padding,
                highlight_padding,
                highlight
            ));
        }
        
        // Additional labels
        for (loc, msg) in &self.labels {
            if !loc.source_line.is_empty() {
                let line_num_width = loc.line.to_string().len();
                let padding = " ".repeat(line_num_width);
                
                output.push_str(&format!("{}  |\n", padding));
                output.push_str(&format!("{} |   {}\n", loc.line, loc.source_line));
                
                let highlight_padding = " ".repeat(loc.highlight_start);
                let highlight = "-".repeat(loc.highlight_len);
                output.push_str(&format!(
                    "{}  |   {}{} {}\n",
                    padding,
                    highlight_padding,
                    highlight,
                    msg
                ));
            }
        }
        
        // Note section
        if let Some(ref note) = self.explanation {
            output.push_str("\nnote:\n");
            for line in note.lines() {
                output.push_str(&format!("  {}\n", line));
            }
        }
        
        // Help section
        if let Some(ref help) = self.suggestion {
            output.push_str("\nhelp:\n");
            for line in help.lines() {
                output.push_str(&format!("  {}\n", line));
            }
        }
        
        output
    }
}

impl fmt::Display for RsplError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format())
    }
}

//=============================================================================
// ERROR COLLECTOR
//=============================================================================

/// Collects errors during compilation
#[derive(Debug, Default)]
pub struct ErrorCollector {
    errors: Vec<RsplError>,
    warnings: Vec<RsplError>,
    current_file: String,
    source_lines: Vec<String>,
}

impl ErrorCollector {
    pub fn new() -> Self {
        ErrorCollector {
            errors: Vec::new(),
            warnings: Vec::new(),
            current_file: String::new(),
            source_lines: Vec::new(),
        }
    }
    
    /// Set the current file being compiled
    pub fn set_file(&mut self, file: &str) {
        self.current_file = file.to_string();
    }
    
    /// Set the source code for line lookups
    pub fn set_source(&mut self, source: &str) {
        self.source_lines = source.lines().map(String::from).collect();
    }
    
    /// Get source line by number (1-indexed)
    pub fn get_source_line(&self, line: usize) -> Option<&str> {
        if line > 0 && line <= self.source_lines.len() {
            Some(&self.source_lines[line - 1])
        } else {
            None
        }
    }
    
    /// Add an error
    pub fn error(&mut self, error: RsplError) {
        self.errors.push(error);
    }
    
    /// Add a warning
    pub fn warn(&mut self, warning: RsplError) {
        self.warnings.push(warning);
    }
    
    /// Check if there are any errors
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
    
    /// Get error count
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }
    
    /// Get all errors
    pub fn errors(&self) -> &[RsplError] {
        &self.errors
    }
    
    /// Get all warnings
    pub fn warnings(&self) -> &[RsplError] {
        &self.warnings
    }
    
    /// Format all errors for display
    pub fn format_all(&self) -> String {
        let mut output = String::new();
        
        for error in &self.errors {
            output.push_str(&error.format());
            output.push('\n');
        }
        
        for warning in &self.warnings {
            // Replace "error" with "warning" in output
            let formatted = warning.format().replace("error[", "warning[");
            output.push_str(&formatted);
            output.push('\n');
        }
        
        if !self.errors.is_empty() {
            output.push_str(&format!(
                "error: aborting due to {} previous error{}\n",
                self.errors.len(),
                if self.errors.len() == 1 { "" } else { "s" }
            ));
        }
        
        output
    }
    
    /// Create location with source context
    pub fn location(&self, line: usize, column: usize, highlight_len: usize) -> SourceLocation {
        let source_line = self.get_source_line(line).unwrap_or("").to_string();
        SourceLocation {
            file: self.current_file.clone(),
            line,
            column,
            source_line,
            highlight_start: column.saturating_sub(1),
            highlight_len,
        }
    }
}

//=============================================================================
// ERROR BUILDERS - Convenience functions for common errors
//=============================================================================

/// Scope errors
pub mod scope_errors {
    use super::*;
    
    /// Variable not found in scope
    pub fn variable_not_found(var_name: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL080,
            format!("cannot find variable `{}` in this scope", var_name)
        )
        .note(format!(
            "the variable `{}` is not declared in the current scope or any parent scope",
            var_name
        ))
        .help("check the spelling or declare the variable before using it")
    }
    
    /// Unintended shadowing
    pub fn unintended_shadow(var_name: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL081,
            format!("this assignment creates a new shadowed variable `{}`", var_name)
        )
        .note(
            "in RustS+, assignments inside a block create a new variable by default.\n\
             the outer variable remains unchanged after this block ends."
        )
        .help(format!(
            "if you intended to modify the outer variable, write:\n\
                 outer {} = ...",
            var_name
        ))
    }
    
    /// Outer on non-existent variable
    pub fn outer_not_found(var_name: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL082,
            format!("`outer` used but `{}` doesn't exist in outer scope", var_name)
        )
        .note(format!(
            "the `outer` keyword modifies a variable from an enclosing scope,\n\
             but `{}` is not declared in any outer scope",
            var_name
        ))
        .help("remove `outer` or declare the variable in an outer scope first")
    }
    
    /// Variable used before initialization
    pub fn used_before_init(var_name: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL083,
            format!("variable `{}` used before initialization", var_name)
        )
        .note("in RustS+, variables must be assigned a value before they can be used")
        .help(format!("assign a value to `{}` before this point", var_name))
    }
    
    /// Same-scope reassignment without mut (Logic-06)
    pub fn same_scope_reassignment(var_name: &str, original_line: usize) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL071,
            format!("ambiguous reassignment to `{}` in the same scope", var_name)
        )
        .note(format!(
            "Logic-06 VIOLATION: Same-Scope Reassignment Ban\n\n\
             variable `{}` was already defined at line {} in this scope.\n\
             reassigning to the same name creates a NEW binding in Rust,\n\
             which is almost certainly NOT what you intended.\n\n\
             this pattern is a common source of logic bugs and is DISALLOWED in RustS+.",
            var_name, original_line
        ))
        .help(format!(
            "if you intend to MUTATE the variable, declare it mutable:\n\n\
                 mut {} = <initial_value>\n\
                 {} = <new_value>          // OK: mutates existing binding\n\n\
             if you intend to SHADOW the variable, use an inner scope:\n\n\
                 {} = <first_value>\n\
                 {{\n\
                     {} = <second_value>   // OK: shadows in inner scope\n\
                 }}",
            var_name, var_name, var_name, var_name
        ))
    }
}

/// Control flow errors
pub mod control_flow_errors {
    use super::*;
    
    /// If expression missing else branch
    pub fn if_missing_else() -> RsplError {
        RsplError::new(
            ErrorCode::RSPL060,
            "`if` expression used as value but missing `else` branch"
        )
        .note(
            "in RustS+, when `if` is used as an expression (assigned to a variable),\n\
             it must produce a value on all branches.\n\
             an `if` without `else` produces no value when the condition is false."
        )
        .help("add an `else` branch, or don't use the `if` as a value")
    }
    
    /// Match expression missing arms
    pub fn match_no_arms() -> RsplError {
        RsplError::new(
            ErrorCode::RSPL061,
            "match expression has no arms"
        )
        .note("a `match` expression must have at least one arm")
        .help("add pattern arms to handle the matched value")
    }
    
    /// Match arm type mismatch
    pub fn match_arm_type_mismatch(expected: &str, found: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL062,
            "match arms have incompatible types"
        )
        .note(format!(
            "all match arms must produce values of the same type.\n\
             expected type: `{}`\n\
             found type: `{}`",
            expected, found
        ))
        .help("ensure all arms return the same type")
    }
    
    /// Non-exhaustive match
    pub fn match_non_exhaustive(missing: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL064,
            "match expression is not exhaustive"
        )
        .note(format!(
            "patterns not covered: {}\n\
             in RustS+, match expressions must handle all possible values",
            missing
        ))
        .help("add a `_ { ... }` arm to handle remaining cases")
    }
    
    /// Break outside loop
    pub fn break_outside_loop() -> RsplError {
        RsplError::new(
            ErrorCode::RSPL066,
            "`break` used outside of a loop"
        )
        .note("`break` can only be used inside `loop`, `while`, or `for`")
    }
    
    /// Continue outside loop
    pub fn continue_outside_loop() -> RsplError {
        RsplError::new(
            ErrorCode::RSPL067,
            "`continue` used outside of a loop"
        )
        .note("`continue` can only be used inside `loop`, `while`, or `for`")
    }
    
    /// Return outside function
    pub fn return_outside_function() -> RsplError {
        RsplError::new(
            ErrorCode::RSPL068,
            "`return` used outside of a function"
        )
        .note("`return` can only be used inside a function body")
    }
}

/// Expression errors
pub mod expression_errors {
    use super::*;
    
    /// Missing value in expression context
    pub fn missing_value() -> RsplError {
        RsplError::new(
            ErrorCode::RSPL043,
            "expected expression, found statement"
        )
        .note(
            "this position requires a value, but a statement was found.\n\
             in RustS+, assignments and declarations are statements, not expressions."
        )
    }
    
    /// Type mismatch in expression
    pub fn type_mismatch(expected: &str, found: &str, context: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL044,
            format!("type mismatch in {}", context)
        )
        .note(format!(
            "expected type: `{}`\n\
             found type: `{}`",
            expected, found
        ))
    }
    
    /// String literal where String expected
    pub fn string_literal_needs_conversion() -> RsplError {
        RsplError::new(
            ErrorCode::RSPL046,
            "string literal used where `String` is expected"
        )
        .note(
            "in RustS+, string literals (\"...\") have type `&str`.\n\
             when `String` is expected, RustS+ automatically converts them."
        )
    }
    
    /// Invalid assignment target
    pub fn invalid_assignment_target() -> RsplError {
        RsplError::new(
            ErrorCode::RSPL042,
            "invalid left-hand side of assignment"
        )
        .note("the left side of an assignment must be a variable or field")
    }
}

/// Structure errors
pub mod structure_errors {
    use super::*;
    
    /// Invalid function signature
    pub fn invalid_function_sig(detail: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL020,
            "invalid function signature"
        )
        .note(format!(
            "{}\n\
             RustS+ function syntax: `fn name(param Type, ...) ReturnType {{ ... }}`",
            detail
        ))
    }
    
    /// Invalid struct definition
    pub fn invalid_struct_def(detail: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL021,
            "invalid struct definition"
        )
        .note(format!(
            "{}\n\
             RustS+ struct syntax: `struct Name {{ field Type, ... }}`",
            detail
        ))
    }
    
    /// Invalid enum definition
    pub fn invalid_enum_def(detail: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL022,
            "invalid enum definition"
        )
        .note(format!(
            "{}\n\
             RustS+ enum syntax: `enum Name {{ Variant, Variant(Type), Variant {{ field Type }}, ... }}`",
            detail
        ))
    }
    
    /// Invalid field syntax
    pub fn invalid_field_syntax(field: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL025,
            format!("invalid field syntax: `{}`", field)
        )
        .note("RustS+ field syntax: `field_name Type` (no colon needed)")
        .help("example: `name String` or `count i32`")
    }
}

/// Ownership errors
pub mod ownership_errors {
    use super::*;
    
    /// Cannot mutate immutable variable
    pub fn cannot_mutate_immutable(var_name: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL104,
            format!("cannot mutate `{}` as it is not mutable", var_name)
        )
        .note(
            "in RustS+, variables that are reassigned are automatically made mutable.\n\
             this error indicates the variable was never reassigned in its scope,\n\
             but is being mutated (e.g., via method call or `&mut` borrow)."
        )
        .help(format!(
            "to make `{}` mutable, reassign it at least once:\n\
                 {} = {}\n\
                 {} = {}  // now it's mutable",
            var_name, var_name, var_name, var_name, var_name
        ))
    }
    
    /// Use after move
    pub fn use_after_move(var_name: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL103,
            format!("use of moved value: `{}`", var_name)
        )
        .note(format!(
            "the value of `{}` was moved to another location.\n\
             in RustS+, values are moved by default when passed to functions\n\
             or assigned to other variables.",
            var_name
        ))
        .help("consider cloning the value or restructuring to avoid the move")
    }
    
    /// Multiple mutable borrows
    pub fn multiple_mut_borrows(var_name: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL102,
            format!("cannot borrow `{}` as mutable more than once", var_name)
        )
        .note(
            "RustS+ (like Rust) enforces that only one mutable reference\n\
             can exist at a time. this prevents data races."
        )
        .help("ensure the first mutable borrow is no longer in use")
    }
}

/// Type consistency errors
pub mod type_errors {
    use super::*;
    
    /// Function return type mismatch
    pub fn return_type_mismatch(fn_name: &str, expected: &str, found: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL120,
            format!("function `{}` returns wrong type", fn_name)
        )
        .note(format!(
            "function declared to return `{}`\n\
             but the body returns `{}`",
            expected, found
        ))
        .help("ensure the return value matches the declared return type")
    }
    
    /// Argument type mismatch
    pub fn argument_type_mismatch(fn_name: &str, param: &str, expected: &str, found: &str) -> RsplError {
        RsplError::new(
            ErrorCode::RSPL121,
            format!("argument type mismatch in call to `{}`", fn_name)
        )
        .note(format!(
            "parameter `{}` expects type `{}`\n\
             but received type `{}`",
            param, expected, found
        ))
    }
}

//=============================================================================
// RUST ERROR MAPPING
//=============================================================================

/// Maps rustc error messages to RustS+ errors
pub fn map_rust_error(rust_error: &str, _source: &str) -> Option<RsplError> {
    // Try to extract meaningful parts from Rust error
    let rust_error_lower = rust_error.to_lowercase();
    
    // Cannot borrow as mutable
    if rust_error_lower.contains("cannot borrow") && rust_error_lower.contains("mutable") {
        if let Some(var_name) = extract_variable_name(rust_error, "cannot borrow `", "`") {
            return Some(
                ownership_errors::cannot_mutate_immutable(&var_name)
                    .note("this error was detected by the Rust backend during compilation")
            );
        }
    }
    
    // Use of moved value
    if rust_error_lower.contains("use of moved value") {
        if let Some(var_name) = extract_variable_name(rust_error, "moved value: `", "`") {
            return Some(
                ownership_errors::use_after_move(&var_name)
                    .note("this error was detected by the Rust backend during compilation")
            );
        }
    }
    
    // Cannot find value in scope
    if rust_error_lower.contains("cannot find value") && rust_error_lower.contains("in this scope") {
        if let Some(var_name) = extract_variable_name(rust_error, "cannot find value `", "`") {
            return Some(
                scope_errors::variable_not_found(&var_name)
                    .note("this error was detected by the Rust backend during compilation")
            );
        }
    }
    
    // `()` type in Display context - often means if-without-else used as value
    if rust_error_lower.contains("`()` doesn't implement") || 
       rust_error_lower.contains("`()` cannot be formatted") {
        return Some(
            control_flow_errors::if_missing_else()
                .note(
                    "this error was detected by the Rust backend during compilation.\n\
                     the `()` type suggests an `if` expression without `else` was used as a value."
                )
        );
    }
    
    // Expected unit type `()` but found something else - if expression branch mismatch
    if rust_error_lower.contains("expected `()`") && rust_error_lower.contains("found") {
        return Some(
            control_flow_errors::if_missing_else()
                .note("this error was detected by the Rust backend during compilation")
        );
    }
    
    // Mismatched types
    if rust_error_lower.contains("mismatched types") {
        let expected = extract_between(rust_error, "expected `", "`").unwrap_or("unknown");
        let found = extract_between(rust_error, "found `", "`").unwrap_or("unknown");
        return Some(
            expression_errors::type_mismatch(&expected, &found, "expression")
                .note("this error was detected by the Rust backend during compilation")
        );
    }
    
    // Expected expression, found `,`
    if rust_error_lower.contains("expected expression") {
        return Some(
            expression_errors::missing_value()
                .note("this error was detected by the Rust backend during compilation")
        );
    }
    
    // If missing else - direct detection
    if rust_error_lower.contains("if` may be missing an `else` clause") {
        return Some(
            control_flow_errors::if_missing_else()
                .note("this error was detected by the Rust backend during compilation")
        );
    }
    
    // Non-exhaustive patterns in match
    if rust_error_lower.contains("non-exhaustive patterns") {
        let missing = extract_between(rust_error, "patterns ", " not covered")
            .or_else(|| extract_between(rust_error, "pattern ", " not covered"))
            .unwrap_or("_");
        return Some(
            control_flow_errors::match_non_exhaustive(missing)
                .note("this error was detected by the Rust backend during compilation")
        );
    }
    
    // Match arms have incompatible types
    if rust_error_lower.contains("match arms have incompatible types") {
        let expected = extract_between(rust_error, "expected `", "`").unwrap_or("unknown");
        let found = extract_between(rust_error, "found `", "`").unwrap_or("unknown");
        return Some(
            control_flow_errors::match_arm_type_mismatch(&expected, &found)
                .note("this error was detected by the Rust backend during compilation")
        );
    }
    
    // Cannot borrow immutable
    if rust_error_lower.contains("cannot borrow immutable") {
        if let Some(var_name) = extract_variable_name(rust_error, "borrow immutable `", "`")
            .or_else(|| extract_variable_name(rust_error, "borrow `", "`")) {
            return Some(
                ownership_errors::cannot_mutate_immutable(&var_name)
                    .note("this error was detected by the Rust backend during compilation")
            );
        }
    }
    
    // Multiple mutable borrows
    if rust_error_lower.contains("cannot borrow") && rust_error_lower.contains("more than once") {
        if let Some(var_name) = extract_variable_name(rust_error, "cannot borrow `", "`") {
            return Some(
                ownership_errors::multiple_mut_borrows(&var_name)
                    .note("this error was detected by the Rust backend during compilation")
            );
        }
    }
    
    // Generic fallback - wrap the Rust error
    Some(
        RsplError::new(
            ErrorCode::RSPL200,
            "compilation error from Rust backend"
        )
        .note(format!(
            "the following error was reported by rustc:\n\n{}",
            rust_error.trim()
        ))
        .help("this error occurred during Rust compilation.\ncheck the Rust error message above for details.")
    )
}

/// Extract variable name from error message
fn extract_variable_name(text: &str, prefix: &str, suffix: &str) -> Option<String> {
    extract_between(text, prefix, suffix).map(String::from)
}

/// Extract text between two markers
fn extract_between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let start_idx = text.find(start)?;
    let after_start = &text[start_idx + start.len()..];
    let end_idx = after_start.find(end)?;
    Some(&after_start[..end_idx])
}

//=============================================================================
// VALIDATION HELPERS
//=============================================================================

/// Validates an if expression used as a value
pub fn validate_if_expression(has_else: bool, is_value_context: bool) -> Option<RsplError> {
    if is_value_context && !has_else {
        Some(control_flow_errors::if_missing_else())
    } else {
        None
    }
}

/// Validates a match expression has arms
pub fn validate_match_expression(arm_count: usize) -> Option<RsplError> {
    if arm_count == 0 {
        Some(control_flow_errors::match_no_arms())
    } else {
        None
    }
}

/// Validates outer keyword usage
pub fn validate_outer_usage(var_name: &str, exists_in_outer: bool) -> Option<RsplError> {
    if !exists_in_outer {
        Some(scope_errors::outer_not_found(var_name))
    } else {
        None
    }
}

//=============================================================================
// TESTS
//=============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_error_format() {
        let error = scope_errors::unintended_shadow("counter")
            .at_pos("test.rss", 5, 5)
            .with_source("    counter = counter + 1", 4, 7);
        
        let formatted = error.format();
        assert!(formatted.contains("RSPL081"));
        assert!(formatted.contains("scope"));
        assert!(formatted.contains("counter"));
        assert!(formatted.contains("outer"));
    }
    
    #[test]
    fn test_if_missing_else() {
        let error = control_flow_errors::if_missing_else();
        let formatted = error.format();
        assert!(formatted.contains("RSPL060"));
        assert!(formatted.contains("control-flow"));
        assert!(formatted.contains("else"));
    }
    
    #[test]
    fn test_rust_error_mapping() {
        let rust_error = "error[E0596]: cannot borrow `data` as mutable, as it is not declared as mutable";
        let mapped = map_rust_error(rust_error, "");
        assert!(mapped.is_some());
        let error = mapped.unwrap();
        assert_eq!(error.code, ErrorCode::RSPL104);
    }
    
    #[test]
    fn test_error_collector() {
        let mut collector = ErrorCollector::new();
        collector.set_file("test.rss");
        collector.set_source("line1\nline2\nline3");
        
        assert_eq!(collector.get_source_line(1), Some("line1"));
        assert_eq!(collector.get_source_line(2), Some("line2"));
        assert_eq!(collector.get_source_line(4), None);
        
        collector.error(scope_errors::variable_not_found("x"));
        assert!(collector.has_errors());
        assert_eq!(collector.error_count(), 1);
    }
    
    #[test]
    fn test_category_display() {
        assert_eq!(format!("{}", ErrorCategory::Scope), "scope");
        assert_eq!(format!("{}", ErrorCategory::ControlFlow), "control-flow");
        assert_eq!(format!("{}", ErrorCategory::Ownership), "ownership");
    }
}