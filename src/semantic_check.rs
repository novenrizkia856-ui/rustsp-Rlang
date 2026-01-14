//! RustS+ Semantic Checker (Stage 1 Logic Gate)
//!
//! This module validates RustS+ source code BEFORE lowering to Rust.
//! All semantic rules are enforced here as HARD ERRORS.
//!
//! ## Rules Enforced
//!
//! 1. **Expression Completeness (RSPL060)**: if/match used as value must have all branches
//! 2. **Expression-Only Block (RSPL037)**: No `let` statements in expression context  
//! 3. **Intent-Aware Shadowing (RSPL081)**: Assignment to outer variable needs `outer` keyword
//! 4. **Mutation Tracking**: Variable reassignment is tracked for auto-mut
//!
//! ## Design
//!
//! The checker performs a single pass through the source, tracking:
//! - Scope stack (for shadowing detection)
//! - Expression context (for value-context detection)
//! - Variable declarations and mutations
//! - Control flow structure (if/match branches)

use crate::error_msg::{RsplError, ErrorCode, SourceLocation};
use std::collections::{HashMap, HashSet};

//=============================================================================
// SEMANTIC CONTEXT
//=============================================================================

/// Represents a scope in the program
#[derive(Debug, Clone)]
struct Scope {
    /// Variables declared in this scope
    variables: HashSet<String>,
    /// Variables that were reassigned in this scope
    reassigned: HashSet<String>,
    /// Depth level of this scope
    depth: usize,
    /// Is this scope an expression context (if/match body)?
    is_expression_context: bool,
    /// Line where scope started
    start_line: usize,
}

impl Scope {
    fn new(depth: usize, is_expression_context: bool, start_line: usize) -> Self {
        Scope {
            variables: HashSet::new(),
            reassigned: HashSet::new(),
            depth,
            is_expression_context,
            start_line,
        }
    }
}

/// Tracks an if/match expression for completeness checking
#[derive(Debug, Clone)]
struct ControlFlowExpr {
    /// Line where the expression started
    start_line: usize,
    /// Is this used in value context (assignment, return)?
    is_value_context: bool,
    /// Does it have an else branch (for if)?
    has_else: bool,
    /// Type of expression
    kind: ControlFlowKind,
    /// The variable being assigned to (if any)
    assigned_to: Option<String>,
    /// Brace depth when started
    start_depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ControlFlowKind {
    If,
    Match,
}

/// Main semantic checker state
#[derive(Debug)]
pub struct SemanticChecker {
    /// Stack of scopes
    scopes: Vec<Scope>,
    /// All variables declared at any point (for shadowing detection)
    all_variables: HashMap<String, Vec<usize>>, // var -> [line numbers of declarations]
    /// Current brace depth
    brace_depth: usize,
    /// Stack of control flow expressions being analyzed
    control_flow_stack: Vec<ControlFlowExpr>,
    /// Collected errors
    errors: Vec<RsplError>,
    /// Source file name
    file_name: String,
    /// Source lines for error reporting
    source_lines: Vec<String>,
    /// Variables that have been assigned (for mutation tracking)
    assigned_vars: HashMap<String, usize>, // var -> first assignment line
    /// Variables that have been reassigned
    reassigned_vars: HashSet<String>,
    /// Are we inside a function?
    in_function: bool,
    /// Current function depth
    function_depth: usize,
}

impl SemanticChecker {
    pub fn new(file_name: &str) -> Self {
        SemanticChecker {
            scopes: vec![Scope::new(0, false, 0)], // Global scope
            all_variables: HashMap::new(),
            brace_depth: 0,
            control_flow_stack: Vec::new(),
            errors: Vec::new(),
            file_name: file_name.to_string(),
            source_lines: Vec::new(),
            assigned_vars: HashMap::new(),
            reassigned_vars: HashSet::new(),
            in_function: false,
            function_depth: 0,
        }
    }
    
    /// Run semantic analysis on source code
    pub fn check(&mut self, source: &str) -> Result<(), Vec<RsplError>> {
        self.source_lines = source.lines().map(String::from).collect();
        
        for (line_num, line) in source.lines().enumerate() {
            let line_num = line_num + 1; // 1-indexed
            self.analyze_line(line, line_num);
        }
        
        // Check for unclosed control flow expressions
        self.check_unclosed_expressions();
        
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }
    
    /// Analyze a single line
    fn analyze_line(&mut self, line: &str, line_num: usize) {
        let trimmed = line.trim();
        
        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with("//") {
            return;
        }
        
        // Track brace depth changes
        let opens = trimmed.matches('{').count();
        let closes = trimmed.matches('}').count();
        
        // Check for function definition
        if self.is_function_start(trimmed) {
            self.in_function = true;
            self.function_depth = self.brace_depth + opens;
            // Enter function scope
            self.enter_scope(false, line_num);
        } else if opens > 0 && self.in_function {
            // Check for control flow expressions BEFORE entering scope
            let is_control_flow = self.check_control_flow_start(trimmed, line_num);
            
            // For standalone blocks (not control flow, not function start),
            // we need to enter a new scope for EACH opening brace
            if !is_control_flow && !self.is_struct_or_enum_def(trimmed) {
                for _ in 0..opens {
                    self.enter_scope(false, line_num);
                }
            }
        } else {
            // Check for control flow expressions
            self.check_control_flow_start(trimmed, line_num);
        }
        
        // Check for let in expression context (RULE 2)
        self.check_let_in_expression_context(trimmed, line_num);
        
        // Check for assignment (variable tracking and shadowing)
        self.check_assignment(trimmed, line_num);
        
        // Update brace depth
        for _ in 0..opens {
            self.brace_depth += 1;
        }
        
        for _ in 0..closes {
            self.handle_close_brace(line_num);
        }
        
        // Check if function ended
        if self.in_function && self.brace_depth < self.function_depth {
            self.in_function = false;
            self.function_depth = 0;
            // Clear function-local tracking
            self.assigned_vars.clear();
            self.reassigned_vars.clear();
        }
    }
    
    /// Check if line is a struct or enum definition
    fn is_struct_or_enum_def(&self, trimmed: &str) -> bool {
        trimmed.starts_with("struct ") || trimmed.starts_with("pub struct ") ||
        trimmed.starts_with("enum ") || trimmed.starts_with("pub enum ")
    }
    
    /// Check if line starts a function
    fn is_function_start(&self, trimmed: &str) -> bool {
        (trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ")) 
            && trimmed.contains('(')
    }
    
    /// Check for control flow expression start
    /// Returns true if this line starts a control flow construct (scope already handled)
    fn check_control_flow_start(&mut self, trimmed: &str, line_num: usize) -> bool {
        // Check for if expression in value context
        // Pattern: `x = if cond {` or just assignment context
        if let Some(cf_expr) = self.detect_control_flow_expr(trimmed, line_num) {
            self.control_flow_stack.push(cf_expr.clone());
            
            // Enter expression context scope
            if cf_expr.is_value_context {
                self.enter_scope(true, line_num);
            }
            return true;
        }
        
        // Check for else branch
        if trimmed.starts_with("else") || trimmed.contains("} else") {
            if let Some(cf) = self.control_flow_stack.last_mut() {
                if cf.kind == ControlFlowKind::If {
                    cf.has_else = true;
                }
            }
            return true;
        }
        
        // Check for standalone if/while/for/loop (not in value context but still control flow)
        if (trimmed.starts_with("if ") || trimmed.starts_with("while ") || 
            trimmed.starts_with("for ") || trimmed.starts_with("loop ") ||
            trimmed.starts_with("match ")) && trimmed.contains('{') {
            return true;
        }
        
        false
    }
    
    /// Detect if line starts a control flow expression
    fn detect_control_flow_expr(&self, trimmed: &str, line_num: usize) -> Option<ControlFlowExpr> {
        // Check for `x = if cond {`
        if trimmed.contains("= if ") && trimmed.contains('{') {
            let assigned_to = self.extract_assignment_target(trimmed);
            return Some(ControlFlowExpr {
                start_line: line_num,
                is_value_context: true,
                has_else: false,
                kind: ControlFlowKind::If,
                assigned_to,
                start_depth: self.brace_depth,
            });
        }
        
        // Check for `x = match expr {`
        if trimmed.contains("= match ") && trimmed.contains('{') {
            let assigned_to = self.extract_assignment_target(trimmed);
            return Some(ControlFlowExpr {
                start_line: line_num,
                is_value_context: true,
                has_else: false, // Match doesn't need else
                kind: ControlFlowKind::Match,
                assigned_to,
                start_depth: self.brace_depth,
            });
        }
        
        // Check for standalone if that might be a tail expression
        if trimmed.starts_with("if ") && trimmed.contains('{') && !trimmed.contains("else") {
            // Could be value context if it's a tail expression
            // For now, we'll be lenient and only check explicit assignments
        }
        
        None
    }
    
    /// Extract assignment target from line
    fn extract_assignment_target(&self, trimmed: &str) -> Option<String> {
        if let Some(eq_pos) = trimmed.find('=') {
            let before = &trimmed[..eq_pos];
            // Check it's not == 
            if eq_pos > 0 {
                let chars: Vec<char> = trimmed.chars().collect();
                if eq_pos + 1 < chars.len() && chars[eq_pos + 1] == '=' {
                    return None;
                }
            }
            let target = before.trim().trim_start_matches("outer ");
            if !target.is_empty() && self.is_valid_identifier(target) {
                return Some(target.to_string());
            }
        }
        None
    }
    
    /// Check if string is a valid identifier
    fn is_valid_identifier(&self, s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        let first = s.chars().next().unwrap();
        if !first.is_alphabetic() && first != '_' {
            return false;
        }
        s.chars().all(|c| c.is_alphanumeric() || c == '_')
    }
    
    /// Check if a line is a macro call (not an assignment)
    /// Macro calls follow the pattern: identifier!(args) or identifier![args]
    fn is_macro_call(&self, line: &str) -> bool {
        let trimmed = line.trim();
        
        // Find the first `!` in the line
        if let Some(excl_pos) = trimmed.find('!') {
            // Get the part before `!`
            let before = &trimmed[..excl_pos];
            
            // The part before `!` should be a valid identifier
            if !before.is_empty() && self.is_valid_identifier(before) {
                // Check that `!` is followed by `(` or `[`
                let after = &trimmed[excl_pos..];
                if after.starts_with("!(") || after.starts_with("![") {
                    return true;
                }
            }
        }
        
        false
    }
    
    /// Check for `let` in expression context (RULE 2)
    fn check_let_in_expression_context(&mut self, trimmed: &str, line_num: usize) {
        // Check if we're in an expression context
        let in_expr_context = self.scopes.last()
            .map(|s| s.is_expression_context)
            .unwrap_or(false);
        
        if in_expr_context && trimmed.starts_with("let ") {
            let error = RsplError::new(
                ErrorCode::RSPL041, // Using RSPL041 for expression-only block
                "`let` statement not allowed in expression context"
            )
            .at(self.make_location(line_num, trimmed))
            .note(
                "in RustS+, when `if` or `match` is used as an expression (assigned to a variable),\n\
                 the body can only contain expressions, not statements like `let`.\n\
                 \n\
                 expression blocks must directly produce a value."
            )
            .help(
                "move the `let` declaration outside the expression block, or\n\
                 use a regular block statement instead of an expression"
            );
            
            self.errors.push(error);
        }
    }
    
    /// Check assignment for shadowing and mutation rules
    fn check_assignment(&mut self, trimmed: &str, line_num: usize) {
        // Skip if not an assignment
        if !trimmed.contains('=') {
            return;
        }
        
        // Skip comparisons, function definitions, struct/enum definitions
        if trimmed.contains("==") || trimmed.contains("!=") || 
           trimmed.contains("<=") || trimmed.contains(">=") ||
           trimmed.contains("=>") ||
           trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") ||
           trimmed.starts_with("struct ") || trimmed.starts_with("enum ") ||
           trimmed.starts_with("if ") || trimmed.starts_with("while ") ||
           trimmed.starts_with("match ") || trimmed.starts_with("for ") ||
           trimmed.contains("= if ") || trimmed.contains("= match ") {
            return;
        }
        
        // Skip macro calls - they may contain `=` in their arguments
        // but are not variable assignments
        if self.is_macro_call(trimmed) {
            return;
        }
        
        // Extract variable name
        let is_outer = trimmed.starts_with("outer ");
        let clean = if is_outer {
            trimmed.strip_prefix("outer ").unwrap_or(trimmed)
        } else {
            trimmed
        };
        
        if let Some(eq_pos) = clean.find('=') {
            // Make sure it's not part of a comparison
            let before = clean[..eq_pos].trim();
            
            // Check for type annotation
            let var_name = if before.contains(' ') {
                // Has type annotation: `x i32 = 10`
                before.split_whitespace().next().unwrap_or(before)
            } else {
                before
            };
            
            if !self.is_valid_identifier(var_name) {
                return;
            }
            
            // Check for shadowing without outer (RULE 3)
            if !is_outer && self.is_defined_in_outer_scope(var_name) && self.in_function {
                // Check if we're in a deeper scope
                if self.scopes.len() > 2 { // More than global + function scope
                    let error = RsplError::new(
                        ErrorCode::RSPL081,
                        format!("this assignment shadows outer variable `{}`", var_name)
                    )
                    .at(self.make_location(line_num, trimmed))
                    .note(
                        format!(
                            "in RustS+, assignments inside a block create a new variable by default.\n\
                             the outer `{}` will remain unchanged after this block ends.\n\
                             \n\
                             this is likely not what you intended.",
                            var_name
                        )
                    )
                    .help(
                        format!(
                            "if you intended to modify the outer variable, write:\n\
                                 outer {} = ...\n\
                             \n\
                             if you intended to shadow, add a comment to suppress this warning",
                            var_name
                        )
                    );
                    
                    self.errors.push(error);
                }
            }
            
            // Track variable assignment
            if let Some(scope) = self.scopes.last_mut() {
                if !is_outer {
                    scope.variables.insert(var_name.to_string());
                }
            }
            
            // Track for mutation detection
            if self.assigned_vars.contains_key(var_name) && !is_outer {
                self.reassigned_vars.insert(var_name.to_string());
            } else if !is_outer {
                self.assigned_vars.insert(var_name.to_string(), line_num);
            }
            
            // Update all_variables map
            self.all_variables
                .entry(var_name.to_string())
                .or_insert_with(Vec::new)
                .push(line_num);
        }
    }
    
    /// Check if variable is defined in an outer scope
    fn is_defined_in_outer_scope(&self, var_name: &str) -> bool {
        // Skip the current scope, check outer scopes
        for scope in self.scopes.iter().rev().skip(1) {
            if scope.variables.contains(var_name) {
                return true;
            }
        }
        false
    }
    
    /// Handle closing brace
    fn handle_close_brace(&mut self, line_num: usize) {
        if self.brace_depth == 0 {
            return;
        }
        
        self.brace_depth -= 1;
        
        // Check if control flow expression ended
        if let Some(cf) = self.control_flow_stack.last() {
            if cf.start_depth == self.brace_depth && cf.kind == ControlFlowKind::If {
                // If expression closing - check for else
                if cf.is_value_context && !cf.has_else {
                    // Peek at next significant content
                    // For now, we'll check when the expression fully closes
                }
            }
        }
        
        // Check if we should pop control flow
        // This is complex because we need to wait for the complete expression
        self.maybe_close_control_flow(line_num);
        
        // Exit scope(s) when their depth is greater than current brace depth
        while self.scopes.len() > 1 {
            if let Some(scope) = self.scopes.last() {
                if scope.depth > self.brace_depth {
                    self.exit_scope();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }
    
    /// Check if control flow expression should be closed
    fn maybe_close_control_flow(&mut self, _line_num: usize) {
        // This is called after brace_depth is decremented
        while let Some(cf) = self.control_flow_stack.last() {
            if cf.start_depth >= self.brace_depth {
                let cf = self.control_flow_stack.pop().unwrap();
                
                // RULE 1: Check if expression is complete
                if cf.is_value_context && cf.kind == ControlFlowKind::If && !cf.has_else {
                    let error = RsplError::new(
                        ErrorCode::RSPL060,
                        "`if` expression used as value but missing `else` branch"
                    )
                    .at(self.make_location_for_line(cf.start_line))
                    .note(
                        "in RustS+, when `if` is used as an expression (assigned to a variable),\n\
                         it must produce a value on ALL branches.\n\
                         \n\
                         an `if` without `else` produces no value when the condition is false,\n\
                         which is invalid in value context."
                    )
                    .help(
                        "add an `else` branch to provide a value when condition is false:\n\
                         \n\
                             x = if cond {\n\
                                 value_if_true\n\
                             } else {\n\
                                 value_if_false\n\
                             }\n\
                         \n\
                         or, don't use the `if` as an expression"
                    );
                    
                    self.errors.push(error);
                }
            } else {
                break;
            }
        }
    }
    
    /// Check for unclosed expressions at end of file
    fn check_unclosed_expressions(&mut self) {
        // Collect errors first to avoid borrow issues
        let unclosed: Vec<_> = self.control_flow_stack.drain(..).collect();
        
        for cf in unclosed {
            if cf.is_value_context && cf.kind == ControlFlowKind::If && !cf.has_else {
                let location = self.make_location_for_line(cf.start_line);
                let error = RsplError::new(
                    ErrorCode::RSPL060,
                    "`if` expression used as value but missing `else` branch"
                )
                .at(location)
                .note(
                    "in RustS+, when `if` is used as an expression,\n\
                     it must produce a value on ALL branches."
                )
                .help("add an `else` branch or don't use the `if` as a value");
                
                self.errors.push(error);
            }
        }
    }
    
    /// Enter a new scope
    fn enter_scope(&mut self, is_expression_context: bool, line_num: usize) {
        self.scopes.push(Scope::new(
            self.brace_depth + 1,
            is_expression_context,
            line_num,
        ));
    }
    
    /// Exit current scope
    fn exit_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }
    
    /// Create source location for error
    fn make_location(&self, line_num: usize, source_fragment: &str) -> SourceLocation {
        let source_line = self.source_lines.get(line_num - 1)
            .map(|s| s.to_string())
            .unwrap_or_default();
        
        let highlight_start = source_line.find(source_fragment.trim())
            .unwrap_or(0);
        let highlight_len = source_fragment.trim().len().min(20);
        
        SourceLocation {
            file: self.file_name.clone(),
            line: line_num,
            column: highlight_start + 1,
            source_line,
            highlight_start,
            highlight_len,
        }
    }
    
    /// Create source location for a line number
    fn make_location_for_line(&self, line_num: usize) -> SourceLocation {
        let source_line = self.source_lines.get(line_num - 1)
            .map(|s| s.to_string())
            .unwrap_or_default();
        
        let trimmed = source_line.trim();
        let highlight_start = source_line.find(trimmed).unwrap_or(0);
        let highlight_len = trimmed.len().min(40);
        
        SourceLocation {
            file: self.file_name.clone(),
            line: line_num,
            column: highlight_start + 1,
            source_line,
            highlight_start,
            highlight_len,
        }
    }
    
    /// Get all errors
    pub fn errors(&self) -> &[RsplError] {
        &self.errors
    }
}

//=============================================================================
// PUBLIC API
//=============================================================================

/// Run semantic check on RustS+ source code
/// Returns Ok(()) if valid, Err with errors if violations found
pub fn check_semantics(source: &str, file_name: &str) -> Result<(), Vec<RsplError>> {
    let mut checker = SemanticChecker::new(file_name);
    checker.check(source)
}

/// Format semantic errors for display
pub fn format_semantic_errors(errors: &[RsplError]) -> String {
    let mut output = String::new();
    
    output.push_str(&format!("\n{}\n", "=".repeat(60)));
    output.push_str("RustS+ Semantic Error (Stage 1)\n");
    output.push_str(&format!("{}\n\n", "=".repeat(60)));
    
    for error in errors {
        output.push_str(&error.format());
        output.push('\n');
    }
    
    output.push_str(&format!(
        "\nerror: aborting due to {} semantic error{}\n",
        errors.len(),
        if errors.len() == 1 { "" } else { "s" }
    ));
    
    output.push_str("\nnote: semantic errors are detected BEFORE Rust compilation.\n");
    output.push_str("      fix these RustS+ errors first.\n");
    
    output
}

//=============================================================================
// TESTS
//=============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_if_without_else_in_value_context() {
        let source = r#"
fn main() {
    x = if true {
        10
    }
}
"#;
        let result = check_semantics(source, "test.rss");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, ErrorCode::RSPL060);
    }
    
    #[test]
    fn test_if_with_else_is_ok() {
        let source = r#"
fn main() {
    x = if true {
        10
    } else {
        20
    }
}
"#;
        let result = check_semantics(source, "test.rss");
        // This should pass semantic check
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_shadowing_detection() {
        let source = r#"
fn main() {
    counter = 0
    {
        counter = counter + 1
    }
}
"#;
        let result = check_semantics(source, "test.rss");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, ErrorCode::RSPL081);
    }
    
    #[test]
    fn test_outer_keyword_allows_mutation() {
        let source = r#"
fn main() {
    counter = 0
    {
        outer counter = counter + 1
    }
}
"#;
        let result = check_semantics(source, "test.rss");
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_let_in_expression_context() {
        let source = r#"
fn main() {
    x = if true {
        let a = 10
        a
    } else {
        0
    }
}
"#;
        let result = check_semantics(source, "test.rss");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, ErrorCode::RSPL041);
    }
    
    #[test]
    fn test_normal_code_passes() {
        let source = r#"
fn classify(n i32) String {
    match n {
        0 { "zero" }
        x if x > 0 { "positive" }
        _ { "negative" }
    }
}

fn main() {
    println!("{}", classify(10))
}
"#;
        let result = check_semantics(source, "test.rss");
        assert!(result.is_ok());
    }
}