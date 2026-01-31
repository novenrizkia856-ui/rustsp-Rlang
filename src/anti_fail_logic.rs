//! RustS+ Anti-Fail Logic System with Full Effect Ownership
//!
//! # EFFECT OWNERSHIP MODEL
//!
//! RustS+ implements a **borrow checker for program meaning** through its Effect System.
//! Every function must honestly declare what effects it performs.
//!
//! ## Effect Types
//!
//! - `read(param)` - Function reads from a parameter
//! - `write(param)` - Function mutates a parameter (struct field, etc.)
//! - `io` - Function performs I/O operations (println!, read, write)
//! - `alloc` - Function allocates memory (Vec::new, Box::new, etc.)
//! - `panic` - Function may panic (unwrap, expect, panic!)
//!
//! ## Function Classification
//!
//! Functions are classified as either:
//! - **PURE**: No effects declared or detected (referentially transparent)
//! - **EFFECTFUL**: Has one or more effects declared
//!
//! ## Syntax
//!
//! ```text
//! fn transfer(acc Account, amount i64) effects(write acc) Account { ... }
//! fn log(msg String) effects(io) { ... }
//! fn pure_add(a i32, b i32) i32 { a + b }  // PURE - no effects
//! ```
//!
//! ## Rules
//!
//! 1. **Effect Honesty**: If a function performs an effect, it MUST declare it
//! 2. **Effect Propagation**: If A calls B, A must declare all effects of B
//! 3. **Effect Isolation**: Effects cannot leak to closures/callbacks without declaration
//! 4. **Zero Heuristics**: No guessing - explicit declaration required
//! 5. **Effect Scope**: Effects are "borrowed" by blocks, not owned

use crate::error_msg::{RsplError, ErrorCode, SourceLocation};
use std::collections::{HashMap, HashSet, BTreeSet};

//=============================================================================
// ANSI COLOR CODES
//=============================================================================

pub mod ansi {
    pub const RED: &str = "\x1b[31m";
    pub const BOLD_RED: &str = "\x1b[1;31m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BOLD_YELLOW: &str = "\x1b[1;33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const BOLD_BLUE: &str = "\x1b[1;34m";
    pub const CYAN: &str = "\x1b[36m";
    pub const BOLD_CYAN: &str = "\x1b[1;36m";
    pub const GREEN: &str = "\x1b[32m";
    pub const BOLD_GREEN: &str = "\x1b[1;32m";
    pub const WHITE: &str = "\x1b[37m";
    pub const BOLD_WHITE: &str = "\x1b[1;37m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const BOLD_MAGENTA: &str = "\x1b[1;35m";
    pub const BOLD: &str = "\x1b[1m";
    pub const RESET: &str = "\x1b[0m";
}

//=============================================================================
// EXPRESSION CONTEXT TRACKING (NEW - Fixes Enum Constructor Bug)
//=============================================================================

/// Expression context for tracking what kind of syntactic context we're in.
/// This is used to distinguish enum constructors from assignments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpressionContext {
    /// Top-level code (statements)
    TopLevel,
    /// Inside array literal `[...]`
    ArrayLiteral,
    /// Inside struct literal `{ field = value, ... }`
    StructLiteral,
    /// Inside function arguments `(arg1, arg2, ...)`
    FnArgs,
    /// Inside match arm
    MatchArm,
    /// Inside tuple literal `(a, b, c)`
    TupleLiteral,
}

/// Stack of expression contexts for nested expressions
#[derive(Debug, Clone)]
pub struct ExpressionContextStack {
    stack: Vec<ExpressionContext>,
    /// Track bracket depth `[` for array literals
    bracket_depth: usize,
    /// Track open brackets positions (line numbers for debugging)
    bracket_positions: Vec<usize>,
}

impl ExpressionContextStack {
    pub fn new() -> Self {
        ExpressionContextStack {
            stack: vec![ExpressionContext::TopLevel],
            bracket_depth: 0,
            bracket_positions: Vec::new(),
        }
    }
    
    pub fn push(&mut self, ctx: ExpressionContext) {
        self.stack.push(ctx);
    }
    
    pub fn pop(&mut self) -> Option<ExpressionContext> {
        if self.stack.len() > 1 {
            self.stack.pop()
        } else {
            None // Never pop the top-level context
        }
    }
    
    pub fn current(&self) -> ExpressionContext {
        *self.stack.last().unwrap_or(&ExpressionContext::TopLevel)
    }
    
    /// Check if we're inside any expression context where enum constructors
    /// should NOT be treated as assignments
    pub fn is_in_expression_context(&self) -> bool {
        matches!(
            self.current(),
            ExpressionContext::ArrayLiteral |
            ExpressionContext::StructLiteral |
            ExpressionContext::FnArgs |
            ExpressionContext::MatchArm |
            ExpressionContext::TupleLiteral
        )
    }
    
    /// Check if we're inside an array literal
    pub fn is_in_array(&self) -> bool {
        self.bracket_depth > 0 || 
        self.stack.iter().any(|c| *c == ExpressionContext::ArrayLiteral)
    }
    
    /// Enter array literal context
    pub fn enter_array(&mut self, line: usize) {
        self.bracket_depth += 1;
        self.bracket_positions.push(line);
        self.push(ExpressionContext::ArrayLiteral);
    }
    
    /// Exit array literal context
    pub fn exit_array(&mut self) {
        if self.bracket_depth > 0 {
            self.bracket_depth -= 1;
            self.bracket_positions.pop();
            // Pop ArrayLiteral context if it's on top
            if self.current() == ExpressionContext::ArrayLiteral {
                self.pop();
            }
        }
    }
    
    /// Update bracket depth from a line
    pub fn update_from_line(&mut self, line: &str, line_num: usize) {
        let mut in_string = false;
        let mut prev = ' ';
        
        for c in line.chars() {
            if c == '"' && prev != '\\' {
                in_string = !in_string;
            }
            
            if !in_string {
                match c {
                    '[' => self.enter_array(line_num),
                    ']' => self.exit_array(),
                    _ => {}
                }
            }
            prev = c;
        }
    }
}

impl Default for ExpressionContextStack {
    fn default() -> Self {
        Self::new()
    }
}

//=============================================================================
// ENUM CONSTRUCTOR DETECTION (NEW - Core Fix)
//=============================================================================

/// Check if a line contains an enum constructor pattern.
/// 
/// Enum constructors look like:
/// - `Enum::Variant { ... }`
/// - `Enum::Variant(...)`
/// - `Enum::Variant`
/// 
/// These are NEVER assignment targets, even if they contain `=` inside.
pub fn is_enum_constructor(line: &str) -> bool {
    let trimmed = line.trim();
    
    // Quick rejection: must contain `::`
    if !trimmed.contains("::") {
        return false;
    }
    
    // Find the `::` position
    if let Some(colon_pos) = trimmed.find("::") {
        // Get the part before `::`
        let before_colon = &trimmed[..colon_pos];
        
        // The part before `::` should be a valid type name (starts with uppercase)
        let type_name = before_colon.trim();
        if type_name.is_empty() {
            return false;
        }
        
        // Check if type_name looks like a type (starts with uppercase letter)
        let first_char = type_name.chars().next().unwrap_or('_');
        if !first_char.is_uppercase() {
            return false;
        }
        
        // Verify the type name is a valid identifier (alphanumeric + underscore)
        // But allow things like `x = Tx::Variant` by checking the last word
        let words: Vec<&str> = type_name.split_whitespace().collect();
        if let Some(last_word) = words.last() {
            let first_char_last = last_word.chars().next().unwrap_or('_');
            if first_char_last.is_uppercase() && 
               last_word.chars().all(|c| c.is_alphanumeric() || c == '_') {
                // Check if after `::` there's a variant name
                let after_colon = &trimmed[colon_pos + 2..];
                let variant_part = after_colon.trim();
                
                // Variant should start with uppercase
                let variant_first = variant_part.chars().next().unwrap_or('_');
                if variant_first.is_uppercase() {
                    return true;
                }
            }
        }
    }
    
    false
}

/// Check if a line is purely an enum constructor expression (not an assignment TO an enum).
/// This detects patterns like:
/// - `Tx::Deposit { id = 7, amount = 100 }`
/// - `Some(value)`
/// - `None`
/// 
/// But NOT:
/// - `x = Tx::Deposit { ... }` (this IS an assignment of x)
pub fn is_pure_enum_constructor_expr(line: &str) -> bool {
    let trimmed = line.trim().trim_end_matches(',');
    
    // Must start with an uppercase letter (type name) followed by ::
    let first_char = trimmed.chars().next().unwrap_or('_');
    if !first_char.is_uppercase() {
        return false;
    }
    
    // Must contain ::
    if !trimmed.contains("::") {
        return false;
    }
    
    // Get the part before ::
    if let Some(colon_pos) = trimmed.find("::") {
        let type_name = &trimmed[..colon_pos];
        
        // Type name must be a valid identifier starting with uppercase
        if type_name.chars().all(|c| c.is_alphanumeric() || c == '_') &&
           type_name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            return true;
        }
    }
    
    false
}

/// Check if a line is a macro call (not an assignment)
/// 
/// Macro calls follow the pattern: identifier!(args) or identifier![args]
/// Examples:
/// - `println!("Hello")` → true
/// - `vec![1, 2, 3]` → true  
/// - `format!("{}", x)` → true
/// - `x = vec![1, 2, 3]` → false (assignment with macro on RHS)
/// - `x = 10` → false (regular assignment)
fn is_macro_call(line: &str) -> bool {
    let trimmed = line.trim();
    
    // Find the first `!` in the line
    if let Some(excl_pos) = trimmed.find('!') {
        // Get the part before `!`
        let before = &trimmed[..excl_pos];
        
        // The part before `!` should be a valid identifier (alphanumeric + underscore)
        // and must start with a letter or underscore
        if before.is_empty() {
            return false;
        }
        
        let first_char = before.chars().next().unwrap_or('_');
        if !first_char.is_alphabetic() && first_char != '_' {
            return false;
        }
        
        if !before.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return false;
        }
        
        // Check that `!` is followed by `(` or `[`
        let after = &trimmed[excl_pos..];
        if after.starts_with("!(") || after.starts_with("![") {
            return true;
        }
    }
    
    false
}

/// Extract the actual variable name from an assignment line.
/// Returns empty string if the line is NOT a variable assignment.
/// 
/// This is the CRITICAL fix for the enum constructor bug:
/// - `x = 10` → returns "x"
/// - `Tx::Deposit { id = 7 }` → returns "" (NOT an assignment)
/// - `x = Tx::Deposit { ... }` → returns "x"
/// - `println!("x = 5")` → returns "" (macro call, NOT assignment)
pub fn extract_assignment_target(line: &str) -> String {
    let trimmed = line.trim();
    
    // Skip if starts with control flow
    if trimmed.starts_with("if ") || trimmed.starts_with("while ") ||
       trimmed.starts_with("for ") || trimmed.starts_with("match ") ||
       trimmed.starts_with("return ") || trimmed.starts_with("else") {
        return String::new();
    }
    
    // ═══════════════════════════════════════════════════════════════════════
    // BUGFIX: Skip `const` and `static` declarations
    // ═══════════════════════════════════════════════════════════════════════
    // `const MAX_SIZE usize = 100` is NOT a variable assignment!
    // It's a compile-time constant declaration. The word "const" was being
    // extracted as the variable name, causing false RSPL071 errors.
    // ═══════════════════════════════════════════════════════════════════════
    if trimmed.starts_with("const ") || trimmed.starts_with("static ") {
        return String::new();
    }
    
    // CRITICAL FIX: Skip macro calls like println!(), vec![], format!(), etc.
    // Macro calls are NEVER assignment targets - the `=` inside them is part of the macro args
    if is_macro_call(trimmed) {
        return String::new();
    }
    
    // CRITICAL: If line is a pure enum constructor (starts with Type::Variant),
    // it's NOT an assignment
    if is_pure_enum_constructor_expr(trimmed) {
        return String::new();
    }
    
    // Look for `=` that's NOT part of `==`, `!=`, `<=`, `>=`, `=>`
    let mut in_string = false;
    let mut prev_char = ' ';
    let mut eq_pos: Option<usize> = None;
    
    for (i, c) in trimmed.char_indices() {
        if c == '"' && prev_char != '\\' {
            in_string = !in_string;
        }
        
        if !in_string && c == '=' {
            // Check if it's a comparison or arrow
            let next_char = trimmed.chars().nth(i + 1).unwrap_or(' ');
            if prev_char != '=' && prev_char != '!' && prev_char != '<' && prev_char != '>' &&
               next_char != '=' && next_char != '>' {
                eq_pos = Some(i);
                break;
            }
        }
        prev_char = c;
    }
    
    let eq_pos = match eq_pos {
        Some(p) => p,
        None => return String::new(),
    };
    
    // Get left side of =
    let left_side = trimmed[..eq_pos].trim();
    
    // ═══════════════════════════════════════════════════════════════════════
    // CRITICAL FIX: Skip FIELD ACCESS like `obj.field = value`
    // ═══════════════════════════════════════════════════════════════════════
    //
    // `res.ok = false` is a FIELD MUTATION, not a variable assignment!
    // - It does NOT create a new variable `res`
    // - It does NOT shadow outer variable `res`
    // - It mutates the `ok` field of existing variable `res`
    //
    // This should be handled by mutation tracking, not assignment tracking.
    // ═══════════════════════════════════════════════════════════════════════
    if left_side.contains('.') {
        return String::new();
    }
    
    // If left side contains `::`, it's NOT an assignment target
    // (it's a struct field init like `Tx::Variant { field = value }`)
    if left_side.contains("::") {
        return String::new();
    }
    
    // Handle `mut x` case
    let var_part = if left_side.starts_with("mut ") {
        &left_side[4..]
    } else if left_side.starts_with("outer ") {
        &left_side[6..]
    } else {
        left_side
    };
    
    // Extract just the identifier (stop at space, colon, bracket)
    let var_name: String = var_part.trim()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    
    // Final validation: if the var_name looks like a Type (starts with uppercase)
    // AND the line contains `::` anywhere, it's likely an enum constructor context
    if !var_name.is_empty() {
        let first_char = var_name.chars().next().unwrap_or('_');
        if first_char.is_uppercase() && trimmed.contains("::") {
            // Double check: is this line like `x = Type::Variant`?
            // or is it `Type::Variant { field = value }`?
            // The key is: if var_name IS the type name, it's NOT an assignment
            if trimmed.starts_with(&var_name) && trimmed[var_name.len()..].trim_start().starts_with("::") {
                return String::new();
            }
        }
    }
    
    var_name
}

//=============================================================================
// EFFECT TYPES - Core Effect Definitions
//=============================================================================

/// Represents a single effect that a function may perform
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Effect {
    /// Read from a parameter: `read(param_name)`
    Read(String),
    /// Write/mutate a parameter: `write(param_name)`
    Write(String),
    /// Perform I/O operations: `io`
    Io,
    /// Allocate memory: `alloc`
    Alloc,
    /// May panic: `panic`
    Panic,
    /// Call effectful function (internal tracking): `calls(fn_name)`
    Calls(String),
}

impl Effect {
    pub fn display(&self) -> String {
        match self {
            Effect::Read(p) => format!("read({})", p),
            Effect::Write(p) => format!("write({})", p),
            Effect::Io => "io".to_string(),
            Effect::Alloc => "alloc".to_string(),
            Effect::Panic => "panic".to_string(),
            Effect::Calls(f) => format!("calls({})", f),
        }
    }
    
    /// Parse an effect from string
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        
        if s == "io" {
            return Some(Effect::Io);
        }
        if s == "alloc" {
            return Some(Effect::Alloc);
        }
        if s == "panic" {
            return Some(Effect::Panic);
        }
        
        if s.starts_with("read(") && s.ends_with(')') {
            let inner = &s[5..s.len()-1];
            return Some(Effect::Read(inner.trim().to_string()));
        }
        
        // Also support `read param` syntax (without parentheses)
        if s.starts_with("read ") {
            let inner = &s[5..];
            if !inner.is_empty() {
                return Some(Effect::Read(inner.trim().to_string()));
            }
        }
        
        if s.starts_with("write(") && s.ends_with(')') {
            let inner = &s[6..s.len()-1];
            return Some(Effect::Write(inner.trim().to_string()));
        }
        
        // Also support `write param` syntax (without parentheses)
        if s.starts_with("write ") {
            let inner = &s[6..];
            if !inner.is_empty() {
                return Some(Effect::Write(inner.trim().to_string()));
            }
        }
        
        if s.starts_with("calls(") && s.ends_with(')') {
            let inner = &s[6..s.len()-1];
            return Some(Effect::Calls(inner.trim().to_string()));
        }
        
        None
    }
    
    /// Check if this is a propagatable effect (should bubble up to callers)
    pub fn is_propagatable(&self) -> bool {
        matches!(self, Effect::Io | Effect::Alloc | Effect::Panic)
    }
    
    /// Check if this is a parameter-bound effect
    pub fn is_parameter_bound(&self) -> bool {
        matches!(self, Effect::Read(_) | Effect::Write(_))
    }
}

//=============================================================================
// EFFECT SIGNATURE - Function's Effect Contract
//=============================================================================

/// Effect signature for a function - the "contract" of what effects it may perform
#[derive(Debug, Clone, Default)]
pub struct EffectSignature {
    /// Declared effects in function signature
    pub effects: BTreeSet<Effect>,
    /// Is this function marked as pure (no effects)?
    pub is_pure: bool,
}

impl EffectSignature {
    pub fn new() -> Self {
        EffectSignature {
            effects: BTreeSet::new(),
            is_pure: true,
        }
    }
    
    pub fn with_effects(effects: BTreeSet<Effect>) -> Self {
        EffectSignature {
            is_pure: effects.is_empty(),
            effects,
        }
    }
    
    pub fn add(&mut self, effect: Effect) {
        self.effects.insert(effect);
        self.is_pure = false;
    }
    
    pub fn has_effect(&self, effect: &Effect) -> bool {
        self.effects.contains(effect)
    }
    
    pub fn has_write(&self, param: &str) -> bool {
        self.effects.contains(&Effect::Write(param.to_string()))
    }
    
    pub fn has_read(&self, param: &str) -> bool {
        self.effects.contains(&Effect::Read(param.to_string()))
    }
    
    pub fn has_io(&self) -> bool {
        self.effects.contains(&Effect::Io)
    }
    
    pub fn has_alloc(&self) -> bool {
        self.effects.contains(&Effect::Alloc)
    }
    
    pub fn has_panic(&self) -> bool {
        self.effects.contains(&Effect::Panic)
    }
    
    /// Get all propagatable effects
    pub fn propagatable_effects(&self) -> Vec<Effect> {
        self.effects.iter()
            .filter(|e| e.is_propagatable())
            .cloned()
            .collect()
    }
    
    /// Format effects for display
    pub fn display(&self) -> String {
        if self.effects.is_empty() {
            return "pure".to_string();
        }
        self.effects.iter()
            .map(|e| e.display())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

//=============================================================================
// FUNCTION INFO - Complete Function Metadata
//=============================================================================

/// Complete function information including effects
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    pub name: String,
    pub parameters: Vec<(String, String)>,  // (name, type)
    pub return_type: Option<String>,
    pub declared_effects: EffectSignature,
    pub detected_effects: EffectSignature,
    pub line_number: usize,
    pub end_line: usize,
    pub calls: Vec<String>,  // Functions this function calls
    pub is_public: bool,
    pub body_lines: Vec<(usize, String)>,  // (line_num, content)
}

impl FunctionInfo {
    pub fn new(name: &str, line: usize) -> Self {
        FunctionInfo {
            name: name.to_string(),
            parameters: Vec::new(),
            return_type: None,
            declared_effects: EffectSignature::new(),
            detected_effects: EffectSignature::new(),
            line_number: line,
            end_line: line,
            calls: Vec::new(),
            is_public: false,
            body_lines: Vec::new(),
        }
    }
    
    /// Check if function is main (special case for I/O allowance)
    pub fn is_main(&self) -> bool {
        self.name == "main"
    }
    
    /// Get all effects that are detected but not declared
    pub fn undeclared_effects(&self) -> Vec<Effect> {
        self.detected_effects.effects.iter()
            .filter(|e| !self.declared_effects.has_effect(e))
            .cloned()
            .collect()
    }
    
    /// Check if a parameter exists
    pub fn has_parameter(&self, name: &str) -> bool {
        self.parameters.iter().any(|(n, _)| n == name)
    }
}

//=============================================================================
// LOGIC VIOLATION CATEGORIES
//=============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicViolation {
    /// Logic-01: if/match as expression missing branches
    IncompleteExpression,
    /// Logic-02: Shadowing without `outer`
    AmbiguousShadowing,
    /// Logic-03: Statement in expression context
    IllegalStatementInExpression,
    /// Logic-04: Implicit mutation
    ImplicitMutation,
    /// Logic-05: Unclear intent
    UnclearIntent,
    /// Logic-06: Same-scope reassignment without `mut`
    SameScopeReassignment,
    /// Effect-01: Undeclared effect performed
    UndeclaredEffect,
    /// Effect-02: Effect leak (effect in nested scope without propagation)
    EffectLeak,
    /// Effect-03: Pure function calling effectful function
    PureCallingEffectful,
    /// Effect-04: Missing effect propagation
    MissingEffectPropagation,
    /// Effect-05: Effect scope violation
    EffectScopeViolation,
    /// Effect-06: Concurrent effect conflict
    ConcurrentEffectConflict,
}

impl LogicViolation {
    pub fn code(&self) -> &'static str {
        match self {
            Self::IncompleteExpression => "Logic-01",
            Self::AmbiguousShadowing => "Logic-02",
            Self::IllegalStatementInExpression => "Logic-03",
            Self::ImplicitMutation => "Logic-04",
            Self::UnclearIntent => "Logic-05",
            Self::SameScopeReassignment => "Logic-06",
            Self::UndeclaredEffect => "Effect-01",
            Self::EffectLeak => "Effect-02",
            Self::PureCallingEffectful => "Effect-03",
            Self::MissingEffectPropagation => "Effect-04",
            Self::EffectScopeViolation => "Effect-05",
            Self::ConcurrentEffectConflict => "Effect-06",
        }
    }
    
    pub fn description(&self) -> &'static str {
        match self {
            Self::IncompleteExpression => "incomplete expression branches",
            Self::AmbiguousShadowing => "ambiguous variable shadowing",
            Self::IllegalStatementInExpression => "illegal statement in expression",
            Self::ImplicitMutation => "implicit mutation without declaration",
            Self::UnclearIntent => "unclear code intent",
            Self::SameScopeReassignment => "same-scope reassignment without mut",
            Self::UndeclaredEffect => "undeclared effect performed",
            Self::EffectLeak => "effect leaked to nested scope",
            Self::PureCallingEffectful => "pure function calling effectful function",
            Self::MissingEffectPropagation => "missing effect propagation",
            Self::EffectScopeViolation => "effect scope violation",
            Self::ConcurrentEffectConflict => "concurrent effect conflict",
        }
    }
}

//=============================================================================
// SCOPE TRACKING
//=============================================================================

#[derive(Debug, Clone)]
struct Scope {
    variables: HashMap<String, usize>,
    mutable_vars: HashSet<String>,
    depth: usize,
    is_expression_context: bool,
    #[allow(dead_code)]
    start_line: usize,
    /// Effects active in this scope (borrowed from function)
    active_effects: HashSet<Effect>,
    /// Is this a closure/lambda scope?
    is_closure: bool,
    /// CRITICAL FIX: Is this a control flow scope (if/while/for/match)?
    /// Control flow scopes allow mutation of outer variables without `outer` keyword
    is_control_flow: bool,
}

impl Scope {
    fn new(depth: usize, is_expression_context: bool, start_line: usize) -> Self {
        Scope {
            variables: HashMap::new(),
            mutable_vars: HashSet::new(),
            depth,
            is_expression_context,
            start_line,
            active_effects: HashSet::new(),
            is_closure: false,
            is_control_flow: false,
        }
    }
    
    fn new_control_flow(depth: usize, is_expression_context: bool, start_line: usize) -> Self {
        let mut scope = Scope::new(depth, is_expression_context, start_line);
        scope.is_control_flow = true;
        scope
    }
    
    fn new_closure(depth: usize, start_line: usize) -> Self {
        let mut scope = Scope::new(depth, false, start_line);
        scope.is_closure = true;
        scope
    }
    
    fn declare(&mut self, var: &str, line: usize) {
        self.variables.insert(var.to_string(), line);
    }
    
    fn declare_mut(&mut self, var: &str, line: usize) {
        self.variables.insert(var.to_string(), line);
        self.mutable_vars.insert(var.to_string());
    }
    
    fn has(&self, var: &str) -> bool {
        self.variables.contains_key(var)
    }
    
    fn is_mutable(&self, var: &str) -> bool {
        self.mutable_vars.contains(var)
    }
    
    #[allow(dead_code)]
    fn get_declaration_line(&self, var: &str) -> Option<usize> {
        self.variables.get(var).copied()
    }
}

//=============================================================================
// CONTROL FLOW TRACKING
//=============================================================================

#[derive(Debug, Clone)]
struct ControlFlowExpr {
    start_line: usize,
    is_value_context: bool,
    has_else: bool,
    kind: ControlFlowKind,
    assigned_to: Option<String>,
    start_depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ControlFlowKind {
    If,
    Match,
}

//=============================================================================
// EFFECT OWNERSHIP TRACKER
//=============================================================================

/// Tracks effect ownership within function bodies
#[derive(Debug)]
struct EffectOwnershipTracker {
    /// Current function owning the effects
    owner_function: Option<String>,
    /// Stack of effect scopes (for nested blocks)
    scope_stack: Vec<EffectScope>,
    /// Detected effect usages with line numbers
    effect_usages: Vec<EffectUsage>,
}

#[derive(Debug, Clone)]
struct EffectScope {
    depth: usize,
    is_closure: bool,
    borrowed_effects: HashSet<Effect>,
    line_start: usize,
}

#[derive(Debug, Clone)]
struct EffectUsage {
    effect: Effect,
    line: usize,
    in_closure: bool,
    scope_depth: usize,
}

impl EffectOwnershipTracker {
    fn new() -> Self {
        EffectOwnershipTracker {
            owner_function: None,
            scope_stack: Vec::new(),
            effect_usages: Vec::new(),
        }
    }
    
    fn enter_function(&mut self, name: &str, declared_effects: &EffectSignature) {
        self.owner_function = Some(name.to_string());
        self.scope_stack.clear();
        self.effect_usages.clear();
        
        // Function body is the root scope with all declared effects
        self.scope_stack.push(EffectScope {
            depth: 0,
            is_closure: false,
            borrowed_effects: declared_effects.effects.iter().cloned().collect(),
            line_start: 0,
        });
    }
    
    fn exit_function(&mut self) -> Vec<EffectUsage> {
        self.owner_function = None;
        self.scope_stack.clear();
        std::mem::take(&mut self.effect_usages)
    }
    
    fn enter_block(&mut self, depth: usize, line: usize) {
        // Inherit effects from parent scope
        let parent_effects = self.scope_stack.last()
            .map(|s| s.borrowed_effects.clone())
            .unwrap_or_default();
        
        self.scope_stack.push(EffectScope {
            depth,
            is_closure: false,
            borrowed_effects: parent_effects,
            line_start: line,
        });
    }
    
    fn enter_closure(&mut self, depth: usize, line: usize) {
        // Closures do NOT inherit effects by default
        self.scope_stack.push(EffectScope {
            depth,
            is_closure: true,
            borrowed_effects: HashSet::new(),
            line_start: line,
        });
    }
    
    fn exit_block(&mut self) {
        if self.scope_stack.len() > 1 {
            self.scope_stack.pop();
        }
    }
    
    fn record_effect(&mut self, effect: Effect, line: usize) {
        let (in_closure, scope_depth) = self.scope_stack.last()
            .map(|s| (s.is_closure, s.depth))
            .unwrap_or((false, 0));
        
        self.effect_usages.push(EffectUsage {
            effect,
            line,
            in_closure,
            scope_depth,
        });
    }
    
    fn is_in_closure(&self) -> bool {
        self.scope_stack.iter().any(|s| s.is_closure)
    }
    
    #[allow(dead_code)]
    fn current_depth(&self) -> usize {
        self.scope_stack.last().map(|s| s.depth).unwrap_or(0)
    }
}

//=============================================================================
// EFFECT ANALYZER
//=============================================================================

/// Analyzes effects within a function body
#[derive(Debug)]
pub struct EffectAnalyzer {
    /// Current function name
    current_function: Option<String>,
    /// Current function parameters
    parameters: Vec<(String, String)>,
    /// Detected effects
    detected_effects: BTreeSet<Effect>,
    /// Declared effects (from signature)
    declared_effects: EffectSignature,
    /// Function calls detected
    function_calls: Vec<(String, usize)>,  // (name, line)
    /// Effect ownership tracker
    ownership_tracker: EffectOwnershipTracker,
    // NEW: IR-based effect context
    ir_context: Option<crate::eir::EffectContext>,
    ir_detected_effects: Option<crate::eir::EffectSet>,
}

impl EffectAnalyzer {
    pub fn new() -> Self {
        EffectAnalyzer {
            current_function: None,
            parameters: Vec::new(),
            detected_effects: BTreeSet::new(),
            declared_effects: EffectSignature::new(),
            function_calls: Vec::new(),
            ownership_tracker: EffectOwnershipTracker::new(),
            ir_context: None,
            ir_detected_effects: None,
        }
    }
    
    pub fn enter_function(&mut self, name: &str, params: &[(String, String)], declared: &EffectSignature) {
        self.current_function = Some(name.to_string());
        self.parameters = params.to_vec();
        self.detected_effects.clear();
        self.declared_effects = declared.clone();
        self.function_calls.clear();
        self.ownership_tracker.enter_function(name, declared);
    }
    
    pub fn exit_function(&mut self) -> (BTreeSet<Effect>, Vec<(String, usize)>) {
        self.ownership_tracker.exit_function();
        self.current_function = None;
        (
            std::mem::take(&mut self.detected_effects),
            std::mem::take(&mut self.function_calls)
        )
    }
    
    pub fn enter_block(&mut self, depth: usize, line: usize) {
        self.ownership_tracker.enter_block(depth, line);
    }
    
    pub fn enter_closure(&mut self, depth: usize, line: usize) {
        self.ownership_tracker.enter_closure(depth, line);
    }
    
    pub fn exit_block(&mut self) {
        self.ownership_tracker.exit_block();
    }
    
    pub fn analyze_line(&mut self, line: &str, line_num: usize) {
        // Detect I/O effects
        if self.detect_io_effect(line) {
            self.detected_effects.insert(Effect::Io);
            self.ownership_tracker.record_effect(Effect::Io, line_num);
        }
        
        // Detect allocation effects
        if self.detect_alloc_effect(line) {
            self.detected_effects.insert(Effect::Alloc);
            self.ownership_tracker.record_effect(Effect::Alloc, line_num);
        }
        
        // Detect panic effects
        if self.detect_panic_effect(line) {
            self.detected_effects.insert(Effect::Panic);
            self.ownership_tracker.record_effect(Effect::Panic, line_num);
        }
        
        // Detect parameter mutations (write effects)
        if let Some(param) = self.detect_param_mutation(line) {
            let effect = Effect::Write(param.clone());
            self.detected_effects.insert(effect.clone());
            self.ownership_tracker.record_effect(effect, line_num);
        }
        
        // Detect parameter reads
        if let Some(param) = self.detect_param_read(line) {
            let effect = Effect::Read(param);
            self.detected_effects.insert(effect.clone());
            self.ownership_tracker.record_effect(effect, line_num);
        }
        
        // Detect function calls
        for call in self.detect_function_calls(line) {
            self.function_calls.push((call, line_num));
        }
    }
    
    fn detect_io_effect(&self, line: &str) -> bool {
        // Use IR-based detection when available
        if let Some(effects) = self.ir_detected_effects.as_ref() {
            return effects.has_io();
        }
        
        // Fallback to pattern matching
        // IMPROVED: Added comprehensive I/O patterns for various categories
        let io_patterns = [
            // === CONSOLE I/O ===
            "println!", "print!", "eprintln!", "eprint!",
            "stdin()", "stdout()", "stderr()",
            
            // === FILE I/O ===
            "std::io", "File::", "OpenOptions::",
            // ═══════════════════════════════════════════════════════════════════════
            // BUGFIX: Removed generic ".read(" and ".write(" patterns
            // ═══════════════════════════════════════════════════════════════════════
            // These patterns were causing FALSE POSITIVES with synchronization primitives!
            // 
            // RwLock.read(), RwLock.write(), Mutex.lock(), RefCell.borrow() are NOT I/O!
            // They are memory synchronization primitives that operate in-process.
            //
            // TRUE I/O operations:
            //   - File::open().read() - reads from filesystem
            //   - TcpStream::connect().write() - writes to network
            //   - stdin().read() - reads from console
            //
            // NOT I/O (synchronization):
            //   - RwLock::new().read() - acquires read lock in memory
            //   - Mutex::new().lock() - acquires mutex in memory
            //   - RefCell::new().borrow() - borrows reference in memory
            //
            // We now use more specific patterns to avoid false positives.
            // ═══════════════════════════════════════════════════════════════════════
            ".read_exact(", ".read_to_string(", ".read_to_end(",
            ".write_all(", ".flush(",
            "Read::read", "Write::write",
            "BufRead::", "io::Read", "io::Write",
            "fs::read", "fs::write", "fs::create", "fs::open",
            "fs::remove", "fs::rename", "fs::copy",
            "fs::create_dir", "fs::remove_dir", "fs::read_dir",
            "BufReader::", "BufWriter::",
            
            // === NETWORKING I/O ===
            "TcpStream::", "TcpListener::", "UdpSocket::",
            "std::net::", "ToSocketAddrs",
            ".connect(", ".bind(", ".listen(", ".accept(",
            ".send(", ".recv(", ".send_to(", ".recv_from(",
            
            // === ENVIRONMENT I/O ===
            "std::env::var", "std::env::args", "std::env::current_dir",
            "std::env::set_var", "std::env::remove_var",
            "env::var", "env::args", "env::current_dir",
            
            // === PROCESS I/O ===
            "std::process::", "Command::", "Child::",
            ".spawn(", ".output(", ".status(",
            
            // === PATH OPERATIONS (may do filesystem checks) ===
            ".canonicalize(", ".metadata(", ".symlink_metadata(",
            ".exists()", ".is_file()", ".is_dir()",
        ];
        
        io_patterns.iter().any(|p| line.contains(p))
    }
    
    fn detect_alloc_effect(&self, line: &str) -> bool {
        // CRITICAL FIX: Removed `.clone()` and `.collect()` from alloc patterns
        //
        // Reason for removing `.clone()`:
        //   `.clone()` on Copy types (i32, u64, bool, char, etc.) does NOT
        //   allocate memory - it just copies bits on the stack. Only `.clone()`
        //   on heap-allocated types (String, Vec, Box, etc.) performs allocation.
        //   Since we can't determine the type at this stage (no type inference),
        //   including `.clone()` causes many false positives.
        //
        // Reason for removing `.collect()`:
        //   `.collect()` can produce various outputs, some that don't allocate
        //   (e.g., collecting into `()`, summing with `Sum`, etc.).
        //
        // For strict effect tracking, users can explicitly declare `effects(alloc)`
        // when they know they're cloning heap types or collecting into containers.
        let alloc_patterns = [
            // Explicit constructors - definite heap allocation
            "Vec::new", "Vec::with_capacity",
            "String::new", "String::from", "String::with_capacity",
            "Box::new", "Rc::new", "Arc::new",
            "HashMap::new", "HashMap::with_capacity",
            "HashSet::new", "HashSet::with_capacity",
            "BTreeMap::new", "BTreeSet::new",
            "VecDeque::new", "LinkedList::new", "BinaryHeap::new",
            // Macros that allocate
            "vec!", "format!",
            // Methods that definitely allocate new heap memory
            ".to_string()", ".to_owned()", ".to_vec()",
            ".into_boxed_slice()", ".into_boxed_str()",
        ];
        
        alloc_patterns.iter().any(|p| line.contains(p))
    }
    
    fn detect_panic_effect(&self, line: &str) -> bool {
        let panic_patterns = [
            "panic!", ".unwrap()", ".expect(",
            "assert!", "assert_eq!", "assert_ne!",
            "unreachable!", "unimplemented!", "todo!",
        ];
        
        panic_patterns.iter().any(|p| line.contains(p))
    }
    
    fn detect_param_mutation(&self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        
        // ═══════════════════════════════════════════════════════════════════════
        // CRITICAL FIX: Skip struct field initialization
        // ═══════════════════════════════════════════════════════════════════════
        // 
        // Pattern `from = from.address` inside struct literal is NOT mutation.
        // It's field initialization where:
        // - `from` (left side) is the FIELD NAME of the struct
        // - `from.address` (right side) is reading from parameter
        //
        // We should ONLY detect mutation when:
        // 1. `param.field = value` (direct field mutation)
        // 2. `param = value` at TOP LEVEL (not inside struct literal)
        // ═══════════════════════════════════════════════════════════════════════
        
        // Skip if line looks like struct field initialization (has comma at end or is indented field)
        // Struct field init patterns: "fieldname = value" or "fieldname = value,"
        // These are typically indented and may end with comma
        
        // Check for parameter field mutation: `param.field = value`
        for (param, _ty) in &self.parameters {
            // Pattern 1: `param.field = ` (this IS mutation)
            let field_assign_pattern = format!("{}.", param);
            if trimmed.contains(&field_assign_pattern) {
                // Check if there's assignment after the field access
                if let Some(dot_pos) = trimmed.find(&field_assign_pattern) {
                    let after_dot = &trimmed[dot_pos + field_assign_pattern.len()..];
                    // Look for pattern: fieldname = value (but not ==)
                    // This means: param.fieldname = value
                    let mut found_field = false;
                    let mut in_field_name = true;
                    let mut chars_iter = after_dot.chars().peekable();
                    
                    while let Some(c) = chars_iter.next() {
                        if in_field_name {
                            if c.is_alphanumeric() || c == '_' {
                                found_field = true;
                                continue;
                            }
                            if c == ' ' && found_field {
                                in_field_name = false;
                                continue;
                            }
                            if c == '=' && found_field {
                                // Check it's not ==
                                if chars_iter.peek() != Some(&'=') {
                                    return Some(param.clone());
                                }
                            }
                            break;
                        } else {
                            // After field name, look for =
                            if c == '=' {
                                if chars_iter.peek() != Some(&'=') {
                                    return Some(param.clone());
                                }
                            }
                            if !c.is_whitespace() && c != '=' {
                                break;
                            }
                        }
                    }
                }
            }
            
            // Pattern 2: Direct reassignment `param = value` at TOP LEVEL ONLY
            // This should NOT match struct field init like `from = from.address`
            // 
            // Key insight: In struct field init, the LEFT side of `=` is the FIELD name,
            // which may happen to have the same name as a parameter. But this is NOT mutation.
            //
            // We only consider it mutation if:
            // - Line STARTS with `param = ` (it's a standalone statement)
            // - NOT inside a struct literal context
            //
            // Heuristic: If the line doesn't start with the param name, it's probably
            // inside a struct literal as a field initializer.
            
            let direct_pattern = format!("{} =", param);
            let direct_pattern2 = format!("{}=", param);
            
            // Only match if line STARTS with the pattern (not inside struct literal)
            if trimmed.starts_with(&direct_pattern) && 
               !trimmed.contains("==") && !trimmed.contains("!=") {
                // Make sure it's not a struct field init by checking context
                // If line ends with comma or is inside braces, it's likely field init
                if !trimmed.ends_with(',') {
                    return Some(param.clone());
                }
            }
            if trimmed.starts_with(&direct_pattern2) && 
               !trimmed.contains("==") && !trimmed.contains("!=") {
                if !trimmed.ends_with(',') {
                    return Some(param.clone());
                }
            }
        }
        None
    }
    
    fn detect_param_read(&self, line: &str) -> Option<String> {
        // Check for parameter field read without mutation
        for (param, _ty) in &self.parameters {
            let field_pattern = format!("{}.", param);
            if line.contains(&field_pattern) {
                // Already detected as write, skip
                if self.detect_param_mutation(line).is_some() {
                    continue;
                }
                return Some(param.clone());
            }
            
            // Direct parameter use
            if line.contains(param) {
                // Check it's used as a value, not as assignment target
                let trimmed = line.trim();
                if !trimmed.starts_with(&format!("{} =", param)) &&
                   !trimmed.starts_with(&format!("{}=", param)) {
                    return Some(param.clone());
                }
            }
        }
        None
    }
    
    fn detect_function_calls(&self, line: &str) -> Vec<String> {
        let mut calls = Vec::new();
        let mut chars = line.chars().peekable();
        let mut current_word = String::new();
        let mut in_string = false;
        
        while let Some(c) = chars.next() {
            if c == '"' {
                in_string = !in_string;
                continue;
            }
            
            if in_string {
                continue;
            }
            
            if c.is_alphanumeric() || c == '_' {
                current_word.push(c);
            } else if c == '(' && !current_word.is_empty() {
                // Found function call
                if !self.is_keyword_or_macro(&current_word) &&
                   !self.is_type_constructor(&current_word) {
                    calls.push(current_word.clone());
                }
                current_word.clear();
            } else if c == ':' && chars.peek() == Some(&':') {
                // Method call - skip the type part
                current_word.clear();
                chars.next(); // consume second :
            } else {
                current_word.clear();
            }
        }
        calls
    }
    
    fn is_keyword_or_macro(&self, name: &str) -> bool {
        // CRITICAL FIX: Separated keywords from type constructors
        //
        // Type constructors like String::from(), Vec::new(), etc. are VALID function
        // calls that should be tracked for effect analysis. Including them here would
        // prevent proper effect tracking.
        //
        // This list should ONLY contain:
        // 1. Rust keywords (cannot be function names)
        // 2. Macros (have ! in actual usage, but we check the base name)
        // 3. Boolean literals
        // 4. Option/Result variants (these are enum constructors, not functions)
        
        const KEYWORDS: &[&str] = &[
            // Rust keywords
            "if", "else", "match", "while", "for", "loop", "fn", "let", "mut",
            "struct", "enum", "impl", "trait", "pub", "mod", "use", "return",
            "break", "continue", "where", "async", "await", "move", "ref",
            "const", "static", "type", "unsafe", "extern", "crate", "self",
            "super", "dyn", "as", "in",
        ];
        
        const MACROS: &[&str] = &[
            // Common macros (base names without !)
            "println", "print", "eprintln", "eprint", "dbg",
            "vec", "format", "write", "writeln",
            "panic", "assert", "assert_eq", "assert_ne",
            "debug_assert", "debug_assert_eq", "debug_assert_ne",
            "todo", "unimplemented", "unreachable",
            "include_str", "include_bytes", "concat", "stringify",
            "env", "cfg", "compile_error",
        ];
        
        const SPECIAL_CONSTRUCTORS: &[&str] = &[
            // Option/Result variants - these are enum constructors, not function calls
            "Some", "None", "Ok", "Err",
            // Boolean literals
            "true", "false",
        ];
        
        // NOTE: Intentionally NOT including type names like "String", "Vec", "Box",
        // "Rc", "Arc", "HashMap", "HashSet" here because calls like String::from()
        // or Vec::new() ARE function calls that need effect tracking!
        
        KEYWORDS.contains(&name) ||
        MACROS.contains(&name) ||
        SPECIAL_CONSTRUCTORS.contains(&name)
    }
    
    fn is_type_constructor(&self, name: &str) -> bool {
        // IMPROVED: More accurate type constructor detection
        //
        // We only want to skip PURE type constructors that don't have effects,
        // NOT associated functions like String::from() or Vec::new().
        //
        // This function checks if `name` is a type name being used as a 
        // constructor call, like `Point(1, 2)` for tuple structs.
        //
        // Associated functions (Type::method) are handled separately because
        // the `::` causes the type name to be cleared before we see the method.
        //
        // For now, we're conservative and don't skip anything here.
        // The effect detection will handle type-associated methods correctly
        // through pattern matching on known allocating/IO/panic functions.
        false
    }
    
    fn detect_closure_start(&self, line: &str) -> bool {
        // Detect closure patterns: |args| or move |args|
        let trimmed = line.trim();
        (trimmed.contains("|") && trimmed.matches('|').count() >= 2) ||
        trimmed.contains("move |")
    }
    
    pub fn is_in_closure(&self) -> bool {
        self.ownership_tracker.is_in_closure()
    }

    /// Initialize IR-based effect context
    pub fn init_ir_context(&mut self, bindings: std::collections::HashMap<crate::hir::BindingId, crate::hir::BindingInfo>) {
        self.ir_context = Some(crate::eir::EffectContext::new(bindings));
    }
    
    /// Perform IR-based effect inference for current function body
    pub fn infer_effects_from_hir(&mut self, body: &crate::hir::Spanned<crate::hir::HirBlock>) {
        if let Some(ctx) = &self.ir_context {
            let inference = crate::eir::EffectInference::new(ctx);
            self.ir_detected_effects = Some(inference.infer_block(body));
        }
    }
    
    /// Get IR-detected effects
    pub fn get_ir_effects(&self) -> Option<&crate::eir::EffectSet> {
        self.ir_detected_effects.as_ref()
    }
}

impl Default for EffectAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

//=============================================================================
// EFFECT DEPENDENCY GRAPH
//=============================================================================

/// Tracks effect dependencies between functions
#[derive(Debug, Default)]
pub struct EffectDependencyGraph {
    /// Function -> Functions it calls
    call_graph: HashMap<String, Vec<String>>,
    /// Function -> Effects it requires from callees
    required_effects: HashMap<String, BTreeSet<Effect>>,
}

impl EffectDependencyGraph {
    pub fn new() -> Self {
        EffectDependencyGraph {
            call_graph: HashMap::new(),
            required_effects: HashMap::new(),
        }
    }
    
    pub fn add_function(&mut self, name: &str) {
        self.call_graph.entry(name.to_string()).or_default();
        self.required_effects.entry(name.to_string()).or_default();
    }
    
    pub fn add_call(&mut self, caller: &str, callee: &str) {
        self.call_graph.entry(caller.to_string())
            .or_default()
            .push(callee.to_string());
    }
    
    pub fn add_required_effect(&mut self, func: &str, effect: Effect) {
        self.required_effects.entry(func.to_string())
            .or_default()
            .insert(effect);
    }
    
    /// Compute transitive effect requirements
    pub fn compute_transitive_effects(&self, func: &str, function_table: &HashMap<String, FunctionInfo>) -> BTreeSet<Effect> {
        let mut visited = HashSet::new();
        let mut effects = BTreeSet::new();
        self.collect_effects_recursive(func, function_table, &mut visited, &mut effects);
        effects
    }
    
    fn collect_effects_recursive(
        &self,
        func: &str,
        function_table: &HashMap<String, FunctionInfo>,
        visited: &mut HashSet<String>,
        effects: &mut BTreeSet<Effect>,
    ) {
        if visited.contains(func) {
            return;
        }
        visited.insert(func.to_string());
        
        // Add this function's effects
        if let Some(info) = function_table.get(func) {
            for effect in &info.declared_effects.effects {
                if effect.is_propagatable() {
                    effects.insert(effect.clone());
                }
            }
        }
        
        // Recurse to callees
        if let Some(callees) = self.call_graph.get(func) {
            for callee in callees {
                self.collect_effects_recursive(callee, function_table, visited, effects);
            }
        }
    }
}

//=============================================================================
// ANTI-FAIL LOGIC CHECKER (Main Engine)
//=============================================================================

#[derive(Debug)]
pub struct AntiFailLogicChecker {
    // Scope tracking
    scopes: Vec<Scope>,
    brace_depth: usize,
    control_flow_stack: Vec<ControlFlowExpr>,
    
    // Expression context tracking (NEW - for fixing enum constructor bug)
    expression_context: ExpressionContextStack,
    
    // Struct literal tracking - tracks when we're inside a struct literal
    // This is critical for distinguishing `field = value` from `var = value`
    in_struct_literal_depth: usize,
    
    // Error collection
    errors: Vec<RsplError>,
    file_name: String,
    source_lines: Vec<String>,
    
    // Variable tracking
    function_vars: HashMap<String, usize>,
    reassigned_vars: HashSet<String>,
    in_function: bool,
    function_depth: usize,
    strict_mode: bool,
    
    // CRITICAL FIX: Track scope level when entering function
    // This is used to properly reset scopes when exiting function
    function_scope_level: usize,
    
    // CRITICAL FIX: Track if function body `{` has been seen
    // For multi-line signatures like:
    //   pub fn foo(
    //       param: Type
    //   ) Self {       <-- function body starts HERE, not at "pub fn foo("
    // We need to update function_depth when we see this `{`
    function_body_started: bool,
    
    // Effect system
    function_table: HashMap<String, FunctionInfo>,
    current_function_info: Option<FunctionInfo>,
    effect_analyzer: EffectAnalyzer,
    effect_graph: EffectDependencyGraph,
    
    // Effect checking enabled
    effect_checking_enabled: bool,
    
    // Strict effect mode (require all effects to be declared)
    strict_effect_mode: bool,
}

impl AntiFailLogicChecker {
    pub fn new(file_name: &str) -> Self {
        AntiFailLogicChecker {
            scopes: vec![Scope::new(0, false, 0)],
            brace_depth: 0,
            control_flow_stack: Vec::new(),
            expression_context: ExpressionContextStack::new(),
            in_struct_literal_depth: 0,
            errors: Vec::new(),
            file_name: file_name.to_string(),
            source_lines: Vec::new(),
            function_vars: HashMap::new(),
            reassigned_vars: HashSet::new(),
            in_function: false,
            function_depth: 0,
            strict_mode: true,
            function_scope_level: 0,
            function_body_started: false,
            function_table: HashMap::new(),
            current_function_info: None,
            effect_analyzer: EffectAnalyzer::new(),
            effect_graph: EffectDependencyGraph::new(),
            effect_checking_enabled: true,
            strict_effect_mode: true,
        }
    }
    
    /// Enable or disable effect checking
    pub fn set_effect_checking(&mut self, enabled: bool) {
        self.effect_checking_enabled = enabled;
    }
    
    /// Enable or disable strict effect mode
    pub fn set_strict_effect_mode(&mut self, strict: bool) {
        self.strict_effect_mode = strict;
    }
    
    /// Main entry point - runs all checks
    pub fn check(&mut self, source: &str) -> Result<(), Vec<RsplError>> {
        self.source_lines = source.lines().map(String::from).collect();
        
        // PASS 1: Collect function signatures with effects
        self.collect_function_signatures(source);
        
        // PASS 2: Analyze function bodies
        for (line_num, line) in source.lines().enumerate() {
            self.analyze_line(line, line_num + 1);
        }
        
        // Close any open control flows
        self.close_pending_control_flows();
        
        // PASS 3: Build effect dependency graph
        if self.effect_checking_enabled {
            self.build_effect_graph();
        }
        
        // PASS 4: Validate effect contracts
        if self.effect_checking_enabled {
            self.validate_effect_contracts();
            self.validate_effect_propagation();
            self.validate_effect_scope();
        }
        
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }
    
    //=========================================================================
    // PASS 1: COLLECT FUNCTION SIGNATURES
    //=========================================================================
    
    fn collect_function_signatures(&mut self, source: &str) {
        for (line_num, line) in source.lines().enumerate() {
            if self.is_function_start(line.trim()) {
                if let Some(func_info) = self.parse_function_with_effects(line, line_num + 1) {
                    self.effect_graph.add_function(&func_info.name);
                    self.function_table.insert(func_info.name.clone(), func_info);
                }
            }
        }
    }
    
    fn parse_function_with_effects(&self, line: &str, line_num: usize) -> Option<FunctionInfo> {
        let trimmed = line.trim();
        
        // Find function name
        let fn_start = if trimmed.starts_with("pub ") {
            trimmed.find("fn ")? + 3
        } else if trimmed.starts_with("async ") {
            trimmed.find("fn ")? + 3
        } else {
            trimmed.find("fn ")? + 3
        };
        
        let after_fn = &trimmed[fn_start..];
        let name_end = after_fn.find('(')?;
        let fn_name = after_fn[..name_end].trim();
        
        let mut func_info = FunctionInfo::new(fn_name, line_num);
        func_info.is_public = trimmed.starts_with("pub ");
        
        // Extract parameters
        let params_start = trimmed.find('(')? + 1;
        let params_end = trimmed.find(')')?;
        let params_str = &trimmed[params_start..params_end];
        
        for param in params_str.split(',') {
            let param = param.trim();
            if param.is_empty() {
                continue;
            }
            
            let parts: Vec<&str> = param.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let name = parts[0].trim().to_string();
                let ty = parts[1].trim().to_string();
                func_info.parameters.push((name, ty));
            } else if parts.len() == 1 {
                // Type annotation on separate line or just type
                let name = parts[0].trim().to_string();
                func_info.parameters.push((name.clone(), name));
            }
        }
        
        // Extract effects clause: `effects(...)`
        if let Some(effects_start) = line.find("effects(") {
            let after_effects = &line[effects_start + 8..];
            // Find matching close paren
            let mut depth = 1;
            let mut end_pos = 0;
            for (i, c) in after_effects.chars().enumerate() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end_pos = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            
            let effects_str = &after_effects[..end_pos];
            for effect_str in effects_str.split(',') {
                if let Some(effect) = Effect::parse(effect_str.trim()) {
                    func_info.declared_effects.add(effect);
                }
            }
        }
        
        Some(func_info)
    }
    
    //=========================================================================
    // PASS 2: LINE-BY-LINE ANALYSIS
    //=========================================================================
    
    fn analyze_line(&mut self, line: &str, line_num: usize) {
        let trimmed = line.trim();
        
        if trimmed.is_empty() || trimmed.starts_with("//") {
            return;
        }
        
        // Update expression context from brackets (IMPORTANT for enum constructor fix)
        self.expression_context.update_from_line(trimmed, line_num);
        
        let opens = self.count_open_braces(trimmed);
        let closes = self.count_close_braces(trimmed);
        
        // ═══════════════════════════════════════════════════════════════════════
        // FIX: Detect struct/enum literals (single-line and multi-line)
        // ═══════════════════════════════════════════════════════════════════════
        let is_struct_literal_start = self.is_struct_literal_start(trimmed);
        let is_struct_literal_single = self.is_struct_or_enum_literal(trimmed);
        
        // Track struct literal depth for multiline struct literals
        if is_struct_literal_start && !is_struct_literal_single {
            // Multiline struct literal starting
            self.in_struct_literal_depth += opens;
        }
        
        // Function start detection
        if self.is_function_start(trimmed) {
            self.enter_function(line_num, opens, trimmed);
        } else if opens > 0 && self.in_function {
            let is_control_flow = self.check_control_flow_start(trimmed, line_num);
            let is_closure = self.detect_closure(trimmed);
            
            // ═══════════════════════════════════════════════════════════════════════
            // CRITICAL FIX: Handle multi-line function signatures
            // ═══════════════════════════════════════════════════════════════════════
            //
            // For multi-line signatures like:
            //   pub fn foo(        <-- enter_function() called, function_body_started = false
            //       param: Type
            //   ) Self {           <-- This `{` is the function body opener!
            //
            // When we see the function body `{`:
            // 1. Set function_body_started = true
            // 2. Update function_depth to include this brace
            // 3. Do NOT create an extra scope (function scope already exists)
            // ═══════════════════════════════════════════════════════════════════════
            
            if !self.function_body_started && !is_closure && !is_control_flow {
                // This `{` is the function body opener for a multi-line signature
                self.function_body_started = true;
                self.function_depth = self.brace_depth + opens;
                // Do NOT create scope - function scope was already created in enter_function()
            } else if is_closure {
                self.effect_analyzer.enter_closure(self.brace_depth + opens, line_num);
            } else if !is_control_flow && !self.is_definition(trimmed) && 
                      !is_struct_literal_single && !is_struct_literal_start &&
                      self.in_struct_literal_depth == 0 {
                // Only create scope if NOT a struct/enum literal
                for _ in 0..opens {
                    self.enter_scope(false, line_num);
                    self.effect_analyzer.enter_block(self.brace_depth + 1, line_num);
                }
            }
        } else {
            self.check_control_flow_start(trimmed, line_num);
        }
        
        // Logic-03: Check for illegal statements
        self.check_illegal_statement(trimmed, line_num);
        
        // Logic-02 & Logic-04 & Logic-06: Check assignments
        // SKIP if we're inside a struct literal
        if self.in_struct_literal_depth == 0 && !is_struct_literal_single {
            self.check_assignment(trimmed, line_num);
        }
        
        // Logic-05: Check unclear intent
        if self.strict_mode {
            self.check_unclear_intent(trimmed, line_num);
        }
        
        // Effect analysis (if in function)
        if self.in_function && self.effect_checking_enabled {
            // Skip effect analysis for struct literal field initializations
            if self.in_struct_literal_depth == 0 && !is_struct_literal_single {
                self.effect_analyzer.analyze_line(trimmed, line_num);
            }
        }
        
        // ═══════════════════════════════════════════════════════════════════════
        // FIX: Handle brace depth and scope for struct literals correctly
        // ═══════════════════════════════════════════════════════════════════════
        // 
        // For struct literals:
        // - Single-line: `x = Type { field = value }` - balanced braces, no scope change
        // - Multi-line start: `x = Type {` - enter struct literal context
        // - Multi-line end: `}` - exit struct literal context
        //
        // We track struct_literal_depth to handle multiline cases
        // ═══════════════════════════════════════════════════════════════════════
        
        // ═══════════════════════════════════════════════════════════════════════
        // CRITICAL FIX: Handle struct literal depth CORRECTLY
        // 
        // BUG WAS: Decrementing depth for ALL closing braces, including those
        //          from single-line balanced struct literals like:
        //          `blobs = vec![BlobRef { hash = x }]`
        //          This incorrectly reset depth to 0 while still inside an outer
        //          multiline struct literal!
        //
        // FIX: Only decrement depth for closing braces that are NOT part of
        //      single-line balanced struct literals.
        // ═══════════════════════════════════════════════════════════════════════
        
        // For single-line balanced struct literals, don't change depth at all
        let is_balanced_single_line = is_struct_literal_single && opens == closes && opens > 0;
        
        // Handle closes for struct literal depth tracking
        // BUT SKIP for single-line balanced struct literals - they don't affect the depth!
        if closes > 0 && self.in_struct_literal_depth > 0 && !is_balanced_single_line {
            self.in_struct_literal_depth = self.in_struct_literal_depth.saturating_sub(closes);
        }
        
        // Calculate net brace change for struct literals
        let in_struct_context = is_struct_literal_single || is_struct_literal_start || self.in_struct_literal_depth > 0;
        
        let net_opens = if is_struct_literal_single && opens == closes {
            // Balanced struct literal on one line - don't affect brace_depth or scope
            0
        } else if in_struct_context {
            // Inside or starting struct literal - don't create scopes but track depth
            0
        } else {
            opens
        };
        
        let net_closes = if is_struct_literal_single && opens == closes {
            0
        } else if in_struct_context && closes > 0 {
            // Closing struct literal - don't pop scopes
            0
        } else {
            closes
        };
        
        // Update brace depth with net values
        for _ in 0..net_opens {
            self.brace_depth += 1;
        }
        
        for _ in 0..net_closes {
            self.handle_close_brace();
        }
        
        // Check if function ended
        if self.in_function && self.brace_depth < self.function_depth {
            self.exit_function();
        }
    }
    
    fn detect_closure(&self, line: &str) -> bool {
        let trimmed = line.trim();
        // Patterns: |args| { ... } or move |args| { ... }
        (trimmed.contains("|") && trimmed.matches('|').count() >= 2 && trimmed.contains('{')) ||
        (trimmed.contains("move |") && trimmed.contains('{'))
    }
    
    fn enter_function(&mut self, line_num: usize, opens: usize, line: &str) {
        self.in_function = true;
        
        // CRITICAL FIX: Handle multi-line function signatures
        // 
        // For single-line: `pub fn foo() {`
        //   → opens = 1, function_body_started = true
        //   → function_depth = brace_depth + 1
        //
        // For multi-line: `pub fn foo(`
        //   → opens = 0, function_body_started = false
        //   → function_depth will be set later when we see `{`
        //
        self.function_body_started = opens > 0;
        
        if opens > 0 {
            // Single-line function: `{` is on the same line
            self.function_depth = self.brace_depth + opens;
        } else {
            // Multi-line function: `{` will come later
            // Set function_depth to current brace_depth for now
            // It will be updated when we see the body `{`
            self.function_depth = self.brace_depth;
        }
        
        // CRITICAL FIX: Record scope level BEFORE entering function scope
        // This is used to properly pop scopes when exiting function
        self.function_scope_level = self.scopes.len();
        
        self.enter_scope(false, line_num);
        
        // Extract function name and setup effect analyzer
        if let Some(func_info) = self.parse_function_with_effects(line, line_num) {
            let params: Vec<(String, String)> = func_info.parameters.clone();
            self.effect_analyzer.enter_function(&func_info.name, &params, &func_info.declared_effects);
            self.current_function_info = Some(func_info);
        }
    }
    
    fn exit_function(&mut self) {
        // Collect detected effects
        if let Some(mut func_info) = self.current_function_info.take() {
            let (detected_effects, calls) = self.effect_analyzer.exit_function();
            
            for effect in detected_effects {
                func_info.detected_effects.add(effect);
            }
            func_info.calls = calls.into_iter().map(|(name, _line)| name).collect();
            
            // Update function table
            self.function_table.insert(func_info.name.clone(), func_info);
        }
        
        self.in_function = false;
        self.function_depth = 0;
        self.function_vars.clear();
        self.reassigned_vars.clear();
        self.function_body_started = false;  // CRITICAL FIX: Reset for next function
        
        // CRITICAL FIX: Pop all scopes that were pushed inside this function
        // This prevents cross-function variable leakage
        while self.scopes.len() > self.function_scope_level {
            self.scopes.pop();
        }
        self.function_scope_level = 0;
        
        // CRITICAL FIX: Reset struct literal depth when exiting function
        // Any unclosed struct literals from this function should be reset
        self.in_struct_literal_depth = 0;
    }
    
    //=========================================================================
    // PASS 3: BUILD EFFECT DEPENDENCY GRAPH
    //=========================================================================
    
    fn build_effect_graph(&mut self) {
        for (name, info) in &self.function_table {
            for callee in &info.calls {
                self.effect_graph.add_call(name, callee);
            }
        }
    }
    
    //=========================================================================
    // PASS 4: EFFECT CONTRACT VALIDATION
    //=========================================================================
    
    fn validate_effect_contracts(&mut self) {
        // Clone function table to avoid borrow issues
        let functions: Vec<_> = self.function_table.values().cloned().collect();
        
        for func_info in functions {
            // Check 1: All detected effects must be declared
            self.check_undeclared_effects(&func_info);
        }
    }
    
    fn validate_effect_propagation(&mut self) {
        let functions: Vec<_> = self.function_table.values().cloned().collect();
        
        for func_info in functions {
            // Check 2: Cross-function effect propagation
            self.check_effect_propagation(&func_info);
        }
    }
    
    fn validate_effect_scope(&mut self) {
        // TODO: Implement closure effect leak detection
        // This requires more sophisticated analysis of closure bodies
    }
    
    fn check_undeclared_effects(&mut self, func_info: &FunctionInfo) {
        // Skip main function for I/O, alloc, panic (main is allowed these by default)
        let is_main = func_info.is_main();
        
        for detected in &func_info.detected_effects.effects {
            // Main is allowed implicit I/O, panic, and alloc
            if is_main && matches!(detected, Effect::Io | Effect::Panic | Effect::Alloc) {
                continue;
            }
            
            // Skip read effects - they're implicit
            if matches!(detected, Effect::Read(_)) {
                continue;
            }
            
            if !func_info.declared_effects.has_effect(detected) {
                // For write effects, check if parameter exists
                if let Effect::Write(ref param) = detected {
                    if !func_info.has_parameter(param) {
                        continue; // Not a parameter write
                    }
                }
                
                self.emit_undeclared_effect_error(func_info, detected);
            }
        }
    }
    
    fn check_effect_propagation(&mut self, func_info: &FunctionInfo) {
        // For each called function, check if its effects are propagated
        for called_name in &func_info.calls {
            if let Some(called_func) = self.function_table.get(called_name).cloned() {
                // Skip if called function is pure
                if called_func.declared_effects.is_pure && called_func.detected_effects.is_pure {
                    continue;
                }
                
                // Check if caller declares all propagatable effects of callee
                for effect in called_func.declared_effects.propagatable_effects() {
                    if !func_info.declared_effects.has_effect(&effect) {
                        // Main is exempt from propagation requirements
                        if !func_info.is_main() {
                            self.emit_missing_propagation_error(func_info, called_name, &effect);
                        }
                    }
                }
                
                // Check 3: Pure function calling effectful function
                if func_info.declared_effects.is_pure && 
                   !called_func.declared_effects.is_pure &&
                   !func_info.is_main() {
                    self.emit_pure_calling_effectful_error(func_info, called_name);
                }
            }
        }
    }
    
    fn emit_undeclared_effect_error(&mut self, func_info: &FunctionInfo, effect: &Effect) {
        let error = RsplError::new(
            ErrorCode::RSPL300,
            format!(
                "function `{}` performs effect `{}` but does not declare it",
                func_info.name,
                effect.display()
            )
        )
        .at(self.make_location(func_info.line_number, &func_info.name))
        .note(format!(
            "{} VIOLATION: Undeclared Effect\n\n\
             in RustS+, functions must HONESTLY declare their effects.\n\
             the function `{}` performs `{}` but this is not in its signature.\n\n\
             RustS+ enforces effect honesty - no hidden side effects allowed.\n\n\
             Effect Contract:\n\
             - Declared: {}\n\
             - Detected: {}",
            LogicViolation::UndeclaredEffect.code(),
            func_info.name,
            effect.display(),
            func_info.declared_effects.display(),
            effect.display()
        ))
        .help(format!(
            "add `effects({})` to the function signature:\n\n    fn {}(...) effects({}) {{ ... }}",
            effect.display(),
            func_info.name,
            if func_info.declared_effects.effects.is_empty() {
                effect.display()
            } else {
                format!("{}, {}", func_info.declared_effects.display(), effect.display())
            }
        ));
        
        self.errors.push(error);
    }
    
    fn emit_missing_propagation_error(&mut self, func_info: &FunctionInfo, called: &str, effect: &Effect) {
        let error = RsplError::new(
            ErrorCode::RSPL301,
            format!(
                "function `{}` calls `{}` which has effect `{}` but does not propagate it",
                func_info.name,
                called,
                effect.display()
            )
        )
        .at(self.make_location(func_info.line_number, &func_info.name))
        .note(format!(
            "{} VIOLATION: Missing Effect Propagation\n\n\
             function `{}` calls `{}` which performs `{}`.\n\
             effects must propagate upward - the caller must declare callee's effects.\n\n\
             This ensures no hidden effects can leak through the call chain.",
            LogicViolation::MissingEffectPropagation.code(),
            func_info.name,
            called,
            effect.display()
        ))
        .help(format!(
            "add `{}` to the effects of `{}`:\n\n    fn {}(...) effects({}) {{ ... }}",
            effect.display(),
            func_info.name,
            func_info.name,
            if func_info.declared_effects.effects.is_empty() {
                effect.display()
            } else {
                format!("{}, {}", func_info.declared_effects.display(), effect.display())
            }
        ));
        
        self.errors.push(error);
    }
    
    fn emit_pure_calling_effectful_error(&mut self, func_info: &FunctionInfo, called: &str) {
        let error = RsplError::new(
            ErrorCode::RSPL302,
            format!(
                "pure function `{}` calls effectful function `{}`",
                func_info.name,
                called
            )
        )
        .at(self.make_location(func_info.line_number, &func_info.name))
        .note(format!(
            "{} VIOLATION: Pure Calling Effectful\n\n\
             function `{}` is declared as pure (no effects),\n\
             but it calls `{}` which has effects.\n\n\
             pure functions cannot call effectful functions without\n\
             declaring that they propagate those effects.",
            LogicViolation::PureCallingEffectful.code(),
            func_info.name,
            called
        ))
        .help(format!(
            "either:\n\
             1. Add the appropriate effects to `{}`\n\
             2. Or refactor to avoid calling effectful functions",
            func_info.name
        ));
        
        self.errors.push(error);
    }
    
    //=========================================================================
    // LOGIC CHECKS
    //=========================================================================
    
    fn check_control_flow_start(&mut self, trimmed: &str, line_num: usize) -> bool {
        // Check for control flow as expression (if/match assigned to variable)
        // PATTERN 1: `if condition { ... }` (standalone if)
        if trimmed.starts_with("if ") && !trimmed.contains("if let") {
            // Check if it's an assignment
            if let Some(assigned_to) = self.detect_assignment_to_control_flow(trimmed) {
                self.control_flow_stack.push(ControlFlowExpr {
                    start_line: line_num,
                    is_value_context: true,
                    has_else: false,
                    kind: ControlFlowKind::If,
                    assigned_to: Some(assigned_to),
                    start_depth: self.brace_depth,
                });
                return true;
            }
        }
        
        // PATTERN 2: `var = if condition { ... }` (if as expression assigned to var)
        // This is the CRITICAL fix for test_logic01_if_without_else
        if !trimmed.starts_with("if ") && trimmed.contains(" = if ") && !trimmed.contains("if let") {
            if let Some(assigned_to) = self.detect_assignment_to_control_flow(trimmed) {
                self.control_flow_stack.push(ControlFlowExpr {
                    start_line: line_num,
                    is_value_context: true,
                    has_else: false,
                    kind: ControlFlowKind::If,
                    assigned_to: Some(assigned_to),
                    start_depth: self.brace_depth,
                });
                return true;
            }
        }
        
        // Also check `= if` without space before (e.g., `x=if`)
        if !trimmed.starts_with("if ") && trimmed.contains("=if ") && !trimmed.contains("if let") {
            if let Some(assigned_to) = self.detect_assignment_to_control_flow(trimmed) {
                self.control_flow_stack.push(ControlFlowExpr {
                    start_line: line_num,
                    is_value_context: true,
                    has_else: false,
                    kind: ControlFlowKind::If,
                    assigned_to: Some(assigned_to),
                    start_depth: self.brace_depth,
                });
                return true;
            }
        }
        
        if trimmed.starts_with("match ") {
            if let Some(assigned_to) = self.detect_assignment_to_control_flow(trimmed) {
                self.control_flow_stack.push(ControlFlowExpr {
                    start_line: line_num,
                    is_value_context: true,
                    has_else: true, // match always has arms
                    kind: ControlFlowKind::Match,
                    assigned_to: Some(assigned_to),
                    start_depth: self.brace_depth,
                });
                return true;
            }
        }
        
        // PATTERN 3: `var = match expr { ... }` (match as expression)
        if !trimmed.starts_with("match ") && (trimmed.contains(" = match ") || trimmed.contains("=match ")) {
            if let Some(assigned_to) = self.detect_assignment_to_control_flow(trimmed) {
                self.control_flow_stack.push(ControlFlowExpr {
                    start_line: line_num,
                    is_value_context: true,
                    has_else: true, // match always has arms
                    kind: ControlFlowKind::Match,
                    assigned_to: Some(assigned_to),
                    start_depth: self.brace_depth,
                });
                return true;
            }
        }
        
        // Detect else keyword
        if trimmed.starts_with("else") || trimmed.contains("} else") {
            if let Some(cf) = self.control_flow_stack.last_mut() {
                cf.has_else = true;
            }
        }
        
        false
    }
    
    fn detect_assignment_to_control_flow(&self, line: &str) -> Option<String> {
        // Pattern: `var = if ...` or `var = match ...`
        if let Some(eq_pos) = line.find('=') {
            let before_eq = line[..eq_pos].trim();
            let after_eq = line[eq_pos + 1..].trim();
            
            // Make sure it's not ==
            if !line.contains("==") && (after_eq.starts_with("if ") || after_eq.starts_with("match ")) {
                // Extract variable name
                let var_name = before_eq.trim_start_matches("mut ").trim();
                if !var_name.is_empty() && var_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    return Some(var_name.to_string());
                }
            }
        }
        None
    }
    
    fn close_pending_control_flows(&mut self) {
        // Check any pending control flows that weren't closed
        while let Some(cf) = self.control_flow_stack.pop() {
            if cf.is_value_context && !cf.has_else && cf.kind == ControlFlowKind::If {
                self.emit_logic01_error(&cf);
            }
        }
    }
    
    fn emit_logic01_error(&mut self, cf: &ControlFlowExpr) {
        let error = RsplError::new(
            ErrorCode::RSPL060,
            format!(
                "`if` expression used as value but missing `else` branch"
            )
        )
        .at(self.make_location(cf.start_line, "if"))
        .note(format!(
            "{} VIOLATION: Incomplete Expression\n\n\
             `if` used as value expression MUST have an `else` branch.\n\
             Without `else`, what value should `{}` have when condition is false?\n\n\
             In RustS+, expressions must always produce a value.",
            LogicViolation::IncompleteExpression.code(),
            cf.assigned_to.as_deref().unwrap_or("_")
        ))
        .help(format!(
            "add an `else` branch:\n\n    {} = if condition {{\n        value_if_true\n    }} else {{\n        value_if_false\n    }}",
            cf.assigned_to.as_deref().unwrap_or("x")
        ));
        
        self.errors.push(error);
    }
    
    fn check_illegal_statement(&mut self, trimmed: &str, line_num: usize) {
        // Check if we're in an expression context but have a statement
        if let Some(scope) = self.scopes.last() {
            if scope.is_expression_context {
                // Statements not allowed: return, break, continue as standalone
                if (trimmed.starts_with("return ") || trimmed == "return") &&
                   !trimmed.ends_with('}') {
                    let error = RsplError::new(
                        ErrorCode::RSPL041,
                        "statement used in expression context"
                    )
                    .at(self.make_location(line_num, trimmed))
                    .note(format!(
                        "{} VIOLATION: Illegal Statement in Expression\n\n\
                         `return` is a statement, not an expression.\n\
                         in expression context, every line must produce a value.",
                        LogicViolation::IllegalStatementInExpression.code()
                    ))
                    .help("remove `return` - the last expression is automatically returned");
                    
                    self.errors.push(error);
                }
            }
        }
    }
    
    /// Check assignments with EXPRESSION CONTEXT AWARENESS
    /// This is the CORE FIX for the enum constructor bug.
    fn check_assignment(&mut self, trimmed: &str, line_num: usize) {
        if !self.in_function {
            return;
        }
        
        // Skip non-assignments (no =)
        if !trimmed.contains('=') || trimmed.contains("==") || trimmed.contains("!=") {
            return;
        }
        
        // Skip if/match/for/etc
        if trimmed.starts_with("if ") || trimmed.starts_with("while ") ||
           trimmed.starts_with("for ") || trimmed.starts_with("match ") {
            return;
        }
        
        // Skip comparisons
        if trimmed.contains("<=") || trimmed.contains(">=") || trimmed.contains("=>") {
            return;
        }
        
        // Handle `outer` keyword
        if trimmed.starts_with("outer ") {
            return;
        }
        
        // ═══════════════════════════════════════════════════════════════════════
        // BUGFIX: Skip `const` and `static` declarations
        // ═══════════════════════════════════════════════════════════════════════
        // These are compile-time constants, NOT variable assignments.
        // `const MAX_SIZE usize = 100` should not trigger RSPL071.
        // ═══════════════════════════════════════════════════════════════════════
        if trimmed.starts_with("const ") || trimmed.starts_with("static ") {
            return;
        }
        
        // ═══════════════════════════════════════════════════════════════════════
        // CRITICAL FIX: Skip struct field initialization
        // ═══════════════════════════════════════════════════════════════════════
        // 
        // Pattern: `fieldname = value,` or `fieldname = value` inside struct literal
        //
        // Heuristics to detect struct field init:
        // 1. Line ends with `,` (struct fields are comma-separated)
        // 2. Line has `=` but not `==` or `!=`
        // 3. NOT a regular assignment (no `outer`, `mut`, `let` prefix)
        // 4. The left side of `=` is a simple identifier (field name)
        // 5. Context: We're likely in a struct literal if indented and follows pattern
        //
        // Examples that should be SKIPPED:
        //   `merkle_root = header.merkle_root,`  (struct field init)
        //   `version = 1,`                        (struct field init)
        //   `nonce = nonce`                       (struct field init - no comma but same pattern)
        //
        // Examples that should NOT be skipped:
        //   `x = 10`                              (variable assignment)
        //   `mut x = 10`                          (mutable declaration)
        // ═══════════════════════════════════════════════════════════════════════
        if self.looks_like_struct_field_init(trimmed) {
            return;
        }
        
        // ═══════════════════════════════════════════════════════════════════════
        // CRITICAL FIX: Skip macro calls
        // ═══════════════════════════════════════════════════════════════════════
        // Macro calls like `println!("x = 5")` contain `=` but are NOT assignments.
        // The `=` is inside the macro arguments, not an assignment operator.
        if is_macro_call(trimmed) {
            return;
        }
        
        // ═══════════════════════════════════════════════════════════════════════
        // CRITICAL FIX: Skip enum constructors
        // ═══════════════════════════════════════════════════════════════════════
        // 
        // If we're in an array literal context `[...]`, enum constructors like
        // `Tx::Deposit { id = 7, amount = 100 }` should NOT be treated as
        // variable assignments.
        //
        // The key insight: `Tx::Variant { ... }` is NEVER an assignment target.
        // It's always an expression that creates a value.
        // ═══════════════════════════════════════════════════════════════════════
        
        // Check if this line is a pure enum constructor (NOT an assignment)
        if is_pure_enum_constructor_expr(trimmed) {
            // This is `Tx::Variant { ... }`, NOT `x = something`
            // Do NOT treat as assignment
            return;
        }
        
        // Also check if we're inside array literal context
        if self.expression_context.is_in_array() {
            // Inside array, `Tx::Variant { field = value }` is NOT reassignment of Tx
            if is_enum_constructor(trimmed) {
                return;
            }
        }
        
        // ═══════════════════════════════════════════════════════════════════════
        // Use the improved extract_assignment_target which handles enum constructors
        // ═══════════════════════════════════════════════════════════════════════
        let var_name = extract_assignment_target(trimmed);
        if var_name.is_empty() {
            return;
        }
        
        let is_mut_decl = trimmed.starts_with("mut ");
        
        // ═══════════════════════════════════════════════════════════════════════
        // FIX: Proper scope-aware checking for reassignment vs shadowing
        // ═══════════════════════════════════════════════════════════════════════
        // 
        // Logic:
        // 1. If variable exists in CURRENT scope → same-scope reassignment (RSPL071)
        // 2. If variable exists in OUTER scope → shadowing (RSPL081)
        // 3. Otherwise → new declaration
        // ═══════════════════════════════════════════════════════════════════════
        
        // First, check if variable exists in CURRENT scope (top of scope stack)
        let in_current_scope = self.scopes.last()
            .map(|s| s.has(&var_name))
            .unwrap_or(false);
        
        // Check if variable exists in any outer scope (not current)
        let in_outer_scope = self.scopes.iter().rev().skip(1)
            .any(|s| s.has(&var_name));
        
        // Also check function_vars for first-level declarations
        let in_function_vars = self.function_vars.contains_key(&var_name);
        
        // Check if already marked as mutable in ANY scope
        let is_known_mutable = self.scopes.iter().any(|s| s.is_mutable(&var_name));
        
        if in_current_scope && !is_mut_decl {
            // CASE 1: Same-scope reassignment (RSPL071)
            if !is_known_mutable && !self.reassigned_vars.contains(&var_name) {
                self.emit_logic06_error(&var_name, line_num, trimmed);
            }
            self.reassigned_vars.insert(var_name.clone());
            return;
        }
        
        if (in_outer_scope || (in_function_vars && !in_current_scope)) && !is_mut_decl {
            // CASE 2: Shadowing from outer scope (RSPL081)
            // Variable exists in outer scope but not current scope
            self.emit_logic02_error(&var_name, line_num, trimmed);
            return;
        }
        
        // CASE 3: New declaration
        if is_mut_decl {
            self.function_vars.insert(var_name.clone(), line_num);
            if let Some(scope) = self.scopes.last_mut() {
                scope.declare_mut(&var_name, line_num);
            }
        } else {
            self.function_vars.insert(var_name.clone(), line_num);
            if let Some(scope) = self.scopes.last_mut() {
                scope.declare(&var_name, line_num);
            }
        }
    }
    
    fn check_shadowing(&mut self, var_name: &str, line_num: usize, trimmed: &str) {
        if !self.in_function || self.scopes.len() <= 2 {
            return;
        }
        
        if self.is_defined_in_outer_scope(var_name) {
            self.emit_logic02_error(var_name, line_num, trimmed);
        }
    }
    
    fn is_defined_in_outer_scope(&self, var_name: &str) -> bool {
        for scope in self.scopes.iter().rev().skip(1) {
            if scope.has(var_name) {
                return true;
            }
        }
        self.function_vars.contains_key(var_name)
    }
    
    fn emit_logic02_error(&mut self, var_name: &str, line_num: usize, source: &str) {
        let error = RsplError::new(
            ErrorCode::RSPL081,
            format!("ambiguous shadowing of outer variable `{}`", var_name)
        )
        .at(self.make_location(line_num, source))
        .note(format!(
            "{} VIOLATION: Ambiguous Shadowing\n\n\
             in RustS+, assignment in inner block creates NEW variable by default.\n\
             outer `{}` will NOT change after this block.\n\
             use `outer {}` to modify the outer variable.",
            LogicViolation::AmbiguousShadowing.code(),
            var_name,
            var_name
        ))
        .help(format!("use `outer {} = ...` to modify outer variable", var_name));
        
        self.errors.push(error);
    }
    
    fn emit_logic06_error(&mut self, var_name: &str, line_num: usize, source: &str) {
        // ═══════════════════════════════════════════════════════════════════════
        // BUGFIX: Get original line from multiple sources
        // ═══════════════════════════════════════════════════════════════════════
        // Previously: `function_vars.get().unwrap_or(0)` caused "line 0" errors
        // when variable was tracked in scope but not in function_vars.
        //
        // Fix: Try function_vars first, then search through scopes, then use
        // a sensible fallback (line before current) instead of 0.
        // ═══════════════════════════════════════════════════════════════════════
        let original_line = self.function_vars.get(var_name).copied()
            .or_else(|| {
                // Try to find declaration line in any scope (reverse order = innermost first)
                self.scopes.iter().rev()
                    .find_map(|s| s.variables.get(var_name).copied())
            })
            .unwrap_or_else(|| {
                // Fallback: if we can't find it anywhere, use line before current
                // This is better than "line 0" which is clearly a bug indicator
                line_num.saturating_sub(1).max(1)
            });
        
        let error = RsplError::new(
            ErrorCode::RSPL071,
            format!("reassignment to `{}` without `mut` declaration", var_name)
        )
        .at(self.make_location(line_num, source))
        .note(format!(
            "{} VIOLATION: Same-Scope Reassignment\n\n\
             variable `{}` was first assigned on line {}.\n\
             reassigning without `mut` is not allowed in RustS+.",
            LogicViolation::SameScopeReassignment.code(),
            var_name,
            original_line
        ))
        .help(format!(
            "change original declaration to:\n\n    mut {} = ...",
            var_name
        ));
        
        self.errors.push(error);
    }
    
    fn check_unclear_intent(&mut self, trimmed: &str, line_num: usize) {
        // Empty block
        if trimmed == "{}" {
            let error = RsplError::new(
                ErrorCode::RSPL001,
                "empty block has unclear intent"
            )
            .at(self.make_location(line_num, trimmed))
            .note(format!(
                "{} VIOLATION: Unclear Intent\n\n\
                 empty blocks `{{}}` are usually unintentional.",
                LogicViolation::UnclearIntent.code()
            ))
            .help("add a comment or `()` to indicate intentional empty block");
            
            self.errors.push(error);
        }
    }
    
    //=========================================================================
    // HELPER FUNCTIONS
    //=========================================================================
    
    /// Detect if line STARTS a struct or enum literal (multiline case)
    /// 
    /// Pattern: `x = Type {` without closing brace on same line
    fn is_struct_literal_start(&self, line: &str) -> bool {
        let trimmed = line.trim();
        
        // ═══════════════════════════════════════════════════════════════════════
        // CRITICAL FIX: Exclude function body openers from struct literal detection
        // ═══════════════════════════════════════════════════════════════════════
        //
        // Lines like `) Self {` or `) Result[T] {` are function return type + body,
        // NOT struct literals! "Self" is uppercase but this is a function body opener.
        //
        // Patterns to EXCLUDE:
        // - `) Self {`           - multi-line fn signature ending
        // - `) ReturnType {`     - multi-line fn signature ending  
        // - `) -> ReturnType {`  - Rust-style return type
        // - `fn name(...) {`     - single-line function
        // - `pub fn name(...) {` - single-line public function
        // - `impl Block {`       - impl block
        // - `struct Foo {`       - struct definition
        // - `enum Foo {`         - enum definition
        // - `trait Foo {`        - trait definition
        // ═══════════════════════════════════════════════════════════════════════
        
        // Lines starting with `)` are function body openers, not struct literals
        if trimmed.starts_with(')') {
            return false;
        }
        
        // Lines containing `fn ` are function definitions
        if trimmed.contains("fn ") {
            return false;
        }
        
        // CRITICAL FIX: Exclude impl/struct/enum/trait/mod definitions
        // These have PascalCase names but are NOT struct literals!
        if trimmed.starts_with("impl ") || trimmed.starts_with("impl<") {
            return false;
        }
        if trimmed.starts_with("struct ") || trimmed.starts_with("pub struct ") {
            return false;
        }
        if trimmed.starts_with("enum ") || trimmed.starts_with("pub enum ") {
            return false;
        }
        if trimmed.starts_with("trait ") || trimmed.starts_with("pub trait ") {
            return false;
        }
        if trimmed.starts_with("mod ") || trimmed.starts_with("pub mod ") {
            return false;
        }
        if trimmed.starts_with("union ") || trimmed.starts_with("pub union ") {
            return false;
        }
        
        // Lines with `->` before `{` are function return types (Rust syntax)
        if let Some(brace_pos) = trimmed.find('{') {
            let before_brace = &trimmed[..brace_pos];
            if before_brace.contains("->") {
                return false;
            }
        }
        
        // Must contain `{` but NOT `}` (multiline start)
        let has_open = trimmed.contains('{');
        let has_close = trimmed.contains('}');
        
        if !has_open || has_close {
            return false;
        }
        
        // Find position of first `{`
        let brace_pos = match trimmed.find('{') {
            Some(p) => p,
            None => return false,
        };
        
        // If `{` is at the very start, it's likely a block, not a literal
        if brace_pos == 0 {
            return false;
        }
        
        let before_brace = trimmed[..brace_pos].trim();
        
        // ═══════════════════════════════════════════════════════════════════════
        // CRITICAL FIX: Check for assignment pattern BEFORE struct name
        // ═══════════════════════════════════════════════════════════════════════
        //
        // For `header = BlockHeader {`, we need to identify that:
        // 1. There's an assignment (`header = `)
        // 2. The struct name is `BlockHeader`, not `header`
        //
        // We should look for `= TypeName {` pattern
        // ═══════════════════════════════════════════════════════════════════════
        
        // Check if there's a type name before `{`
        if let Some(last_word) = before_brace.split_whitespace().last() {
            let first_char = last_word.chars().next().unwrap_or('_');
            
            // If last word before `{` starts with uppercase, likely struct/enum
            if first_char.is_uppercase() {
                return true;
            }
            
            // Also check for Type::Variant pattern
            if last_word.contains("::") {
                let parts: Vec<&str> = last_word.split("::").collect();
                if parts.len() >= 2 {
                    let type_part = parts[0];
                    let variant_part = parts.last().unwrap_or(&"");
                    if type_part.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) &&
                       variant_part.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                        return true;
                    }
                }
            }
        }
        
        false
    }
    
    /// Detect if line contains a struct or enum literal (not a block scope)
    /// 
    /// Struct/enum literals have patterns like:
    /// - `x = Type { field = value }` (assignment with struct literal)
    /// - `Type::Variant { field = value }` (enum constructor)
    /// - `func(Type { field = value })` (struct as function arg)
    /// - `[Type { field = value }, ...]` (struct in array)
    ///
    /// These should NOT create new scope for variable tracking.
    /// 
    /// CRITICAL: This function detects SINGLE-LINE struct literals only!
    /// For multiline struct literals like:
    ///   header = BlockHeader {
    ///       field = value
    ///   }
    /// Use is_struct_literal_start() instead, which returns TRUE for the opening line.
    fn is_struct_or_enum_literal(&self, line: &str) -> bool {
        let trimmed = line.trim();
        
        // ═══════════════════════════════════════════════════════════════════════
        // CRITICAL FIX: Exclude function body openers (same as is_struct_literal_start)
        // ═══════════════════════════════════════════════════════════════════════
        if trimmed.starts_with(')') {
            return false;
        }
        if trimmed.contains("fn ") {
            return false;
        }
        
        // CRITICAL FIX: Exclude impl/struct/enum/trait/mod definitions
        if trimmed.starts_with("impl ") || trimmed.starts_with("impl<") {
            return false;
        }
        if trimmed.starts_with("struct ") || trimmed.starts_with("pub struct ") {
            return false;
        }
        if trimmed.starts_with("enum ") || trimmed.starts_with("pub enum ") {
            return false;
        }
        if trimmed.starts_with("trait ") || trimmed.starts_with("pub trait ") {
            return false;
        }
        if trimmed.starts_with("mod ") || trimmed.starts_with("pub mod ") {
            return false;
        }
        if trimmed.starts_with("union ") || trimmed.starts_with("pub union ") {
            return false;
        }
        
        if let Some(brace_pos) = trimmed.find('{') {
            let before_brace = &trimmed[..brace_pos];
            if before_brace.contains("->") {
                return false;
            }
        }
        
        // ═══════════════════════════════════════════════════════════════════════
        // CRITICAL FIX: Must contain BOTH `{` AND `}` to be a single-line literal!
        // ═══════════════════════════════════════════════════════════════════════
        // 
        // This is the key distinction between:
        //   - Single-line: `x = Type { field = value }` (has both { and })
        //   - Multiline start: `x = Type {` (has { but NOT })
        //
        // For multiline starts, is_struct_literal_start() handles detection,
        // and in_struct_literal_depth tracks the nested field lines.
        // ═══════════════════════════════════════════════════════════════════════
        let has_open = trimmed.contains('{');
        let has_close = trimmed.contains('}');
        
        // Must have BOTH open AND close braces to be a single-line literal
        if !has_open || !has_close {
            return false;
        }
        
        // Find position of first `{`
        let brace_pos = match trimmed.find('{') {
            Some(p) => p,
            None => return false,
        };
        
        // If `{` is at the very start, it's likely a block, not a literal
        if brace_pos == 0 {
            return false;
        }
        
        let before_brace = &trimmed[..brace_pos].trim();
        
        // Check brace balance - must be balanced for single-line literal
        let open_count = trimmed.chars().filter(|c| *c == '{').count();
        let close_count = trimmed.chars().filter(|c| *c == '}').count();
        
        if open_count != close_count {
            // Unbalanced - this is NOT a complete single-line literal
            return false;
        }
        
        // Pattern 1: `= Type {` or `= SomeType {` (assignment with struct literal)
        // The part before `{` should end with a type name (uppercase)
        if let Some(last_word) = before_brace.split_whitespace().last() {
            let first_char = last_word.chars().next().unwrap_or('_');
            
            // If last word before `{` starts with uppercase, likely struct/enum
            if first_char.is_uppercase() {
                return true;
            }
            
            // Also check for Type::Variant pattern
            if last_word.contains("::") {
                let parts: Vec<&str> = last_word.split("::").collect();
                if parts.len() >= 2 {
                    let type_part = parts[0];
                    let variant_part = parts.last().unwrap_or(&"");
                    if type_part.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) &&
                       variant_part.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                        return true;
                    }
                }
            }
        }
        
        // Pattern 2: Complete literal on one line with matching braces
        // Already checked balance above, so just verify there's content before `{`
        if !before_brace.is_empty() {
            return true;
        }
        
        false
    }
    
    /// Detect if line looks like struct field initialization
    /// 
    /// Pattern: `fieldname = value,` or `fieldname = value` inside struct literal
    /// 
    /// Key heuristics:
    /// - Line ends with `,` (strong indicator of struct field)
    /// - Contains `=` but not `==` or `!=`
    /// - NOT a regular assignment prefix (`outer`, `mut`, `let`)
    /// - Left side of `=` is simple identifier (not complex expression)
    fn looks_like_struct_field_init(&self, line: &str) -> bool {
        let trimmed = line.trim();
        
        // Skip if it's a regular assignment pattern
        if trimmed.starts_with("outer ") || 
           trimmed.starts_with("mut ") || 
           trimmed.starts_with("let ") {
            return false;
        }
        
        // Must contain `=` but not comparison operators
        if !trimmed.contains('=') || trimmed.contains("==") || trimmed.contains("!=") {
            return false;
        }
        
        // Strong indicator: ends with comma (struct fields are comma-separated)
        let ends_with_comma = trimmed.ends_with(',');
        
        // Find the `=` position
        let eq_pos = match trimmed.find('=') {
            Some(p) => p,
            None => return false,
        };
        
        // Make sure it's not `<=` or `>=`
        if eq_pos > 0 {
            let before_eq = trimmed.chars().nth(eq_pos - 1);
            if before_eq == Some('<') || before_eq == Some('>') || before_eq == Some('!') {
                return false;
            }
        }
        
        let left_side = trimmed[..eq_pos].trim();
        let right_side = trimmed[eq_pos + 1..].trim().trim_end_matches(',');
        
        // Left side should be simple identifier (struct field name)
        // - Single word
        // - No dots (not field access like `obj.field`)
        // - No colons (not type annotation)
        // - No parentheses (not function call)
        if left_side.contains('.') || 
           left_side.contains(':') || 
           left_side.contains('(') ||
           left_side.contains(' ') {
            return false;
        }
        
        // Check if left side is valid identifier
        let is_valid_identifier = !left_side.is_empty() && 
            left_side.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false) &&
            left_side.chars().all(|c| c.is_alphanumeric() || c == '_');
        
        if !is_valid_identifier {
            return false;
        }
        
        // If ends with comma, very likely struct field init
        if ends_with_comma {
            return true;
        }
        
        // Additional heuristic: If right side references same field pattern (like `field = other.field`)
        // and we're likely inside a struct literal (based on depth tracking)
        if self.in_struct_literal_depth > 0 {
            return true;
        }
        
        // Heuristic: If right side looks like field access or method call, probably struct init
        // e.g., `version = header.version` or `nonce = nonce`
        if right_side.contains('.') || left_side == right_side {
            // Could be struct field init like `nonce = nonce` (shorthand would be just `nonce,`)
            // Being conservative here - only if we have other evidence
            return false;
        }
        
        false
    }
    
    fn is_function_start(&self, trimmed: &str) -> bool {
        (trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") ||
         trimmed.starts_with("async fn ") || trimmed.starts_with("pub async fn "))
            && trimmed.contains('(')
    }
    
    fn is_definition(&self, trimmed: &str) -> bool {
        trimmed.starts_with("struct ") || trimmed.starts_with("enum ") ||
        trimmed.starts_with("pub struct ") || trimmed.starts_with("pub enum ") ||
        trimmed.starts_with("impl ") || trimmed.starts_with("trait ")
    }
    
    fn count_open_braces(&self, s: &str) -> usize {
        let mut count = 0;
        let mut in_string = false;
        let mut prev = ' ';
        for c in s.chars() {
            if c == '"' && prev != '\\' {
                in_string = !in_string;
            }
            if !in_string && c == '{' {
                count += 1;
            }
            prev = c;
        }
        count
    }
    
    fn count_close_braces(&self, s: &str) -> usize {
        let mut count = 0;
        let mut in_string = false;
        let mut prev = ' ';
        for c in s.chars() {
            if c == '"' && prev != '\\' {
                in_string = !in_string;
            }
            if !in_string && c == '}' {
                count += 1;
            }
            prev = c;
        }
        count
    }
    
    fn enter_scope(&mut self, is_expression_context: bool, line_num: usize) {
        let new_depth = self.scopes.len();
        self.scopes.push(Scope::new(new_depth, is_expression_context, line_num));
    }
    
    /// Enter a control flow scope (if/while/for/match)
    /// Control flow scopes allow mutation of outer variables without `outer` keyword
    fn enter_control_flow_scope(&mut self, is_expression_context: bool, line_num: usize) {
        let new_depth = self.scopes.len();
        self.scopes.push(Scope::new_control_flow(new_depth, is_expression_context, line_num));
    }
    
    fn handle_close_brace(&mut self) {
        // Check if closing a control flow expression
        for i in (0..self.control_flow_stack.len()).rev() {
            if self.control_flow_stack[i].start_depth == self.brace_depth {
                let cf = self.control_flow_stack.remove(i);
                
                // Logic-01: If expression used as value must have else
                if cf.is_value_context && !cf.has_else && cf.kind == ControlFlowKind::If {
                    self.emit_logic01_error(&cf);
                }
                break;
            }
        }
        
        if self.brace_depth > 0 {
            self.brace_depth -= 1;
        }
        
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
        
        self.effect_analyzer.exit_block();
    }
    
    /// Legacy extract_var_name - NOW DEPRECATED, use extract_assignment_target instead
    #[allow(dead_code)]
    fn extract_var_name(&self, line: &str) -> String {
        extract_assignment_target(line)
    }
    
    fn get_source_line(&self, line_num: usize) -> String {
        self.source_lines
            .get(line_num.saturating_sub(1))
            .cloned()
            .unwrap_or_default()
    }
    
    fn make_location(&self, line_num: usize, highlight: &str) -> SourceLocation {
        let source_line = self.get_source_line(line_num);
        let highlight_start = source_line.find(highlight.trim()).unwrap_or(0);
        let highlight_len = highlight.trim().len().min(40);
        
        SourceLocation {
            file: self.file_name.clone(),
            line: line_num,
            column: highlight_start + 1,
            source_line,
            highlight_start,
            highlight_len,
        }
    }
    
    /// Get the function table for external analysis
    pub fn get_function_table(&self) -> &HashMap<String, FunctionInfo> {
        &self.function_table
    }
    
    /// Get the effect dependency graph
    pub fn get_effect_graph(&self) -> &EffectDependencyGraph {
        &self.effect_graph
    }
}

//=============================================================================
// PUBLIC API
//=============================================================================

/// Run anti-fail logic check on RustS+ source code
pub fn check_logic(source: &str, file_name: &str) -> Result<(), Vec<RsplError>> {
    let mut checker = AntiFailLogicChecker::new(file_name);
    checker.check(source)
}

/// Run logic check without effect checking (for backward compatibility)
pub fn check_logic_no_effects(source: &str, file_name: &str) -> Result<(), Vec<RsplError>> {
    let mut checker = AntiFailLogicChecker::new(file_name);
    checker.set_effect_checking(false);
    checker.check(source)
}

/// Run logic check with custom settings
pub fn check_logic_custom(
    source: &str, 
    file_name: &str, 
    effect_checking: bool,
    strict_effects: bool,
) -> Result<(), Vec<RsplError>> {
    let mut checker = AntiFailLogicChecker::new(file_name);
    checker.set_effect_checking(effect_checking);
    checker.set_strict_effect_mode(strict_effects);
    checker.check(source)
}

/// Get function info for a source file
pub fn analyze_functions(source: &str, file_name: &str) -> HashMap<String, FunctionInfo> {
    let mut checker = AntiFailLogicChecker::new(file_name);
    let _ = checker.check(source);
    checker.function_table
}

/// Format logic errors for display
pub fn format_logic_errors(errors: &[RsplError]) -> String {
    let mut output = String::new();
    for error in errors {
        output.push_str(&format_error(error));
        output.push('\n');
    }
    output
}

/// Format a single error with colors
fn format_error(error: &RsplError) -> String {
    use ansi::*;
    
    let mut output = String::new();
    
    // Error header
    output.push_str(&format!(
        "{}error[{}][{}]{}: {}\n",
        BOLD_RED,
        error.code.code_str(),
        error.category(),
        RESET,
        error.title
    ));
    
    // Location
    if !error.location.file.is_empty() {
        output.push_str(&format!(
            "  {}--> {}:{}:{}{}\n",
            BLUE,
            error.location.file,
            error.location.line,
            error.location.column,
            RESET
        ));
    }
    
    // Source line
    if !error.location.source_line.is_empty() {
        let line_num_width = error.location.line.to_string().len();
        let padding = " ".repeat(line_num_width);
        
        output.push_str(&format!("{}{}  |{}\n", BLUE, padding, RESET));
        output.push_str(&format!(
            "{}{} |{}   {}\n",
            BLUE,
            error.location.line,
            RESET,
            error.location.source_line
        ));
        
        let highlight_padding = " ".repeat(error.location.highlight_start);
        let highlight = "^".repeat(error.location.highlight_len.max(1));
        output.push_str(&format!(
            "{}{}  |{}   {}{}{}{}\n",
            BLUE, padding, RESET,
            highlight_padding, BOLD_RED, highlight, RESET
        ));
    }
    
    // Note
    if let Some(ref note) = error.explanation {
        output.push_str(&format!("\n{}note{}:\n", BOLD_CYAN, RESET));
        for line in note.lines() {
            output.push_str(&format!("  {}\n", line));
        }
    }
    
    // Help
    if let Some(ref help) = error.suggestion {
        output.push_str(&format!("\n{}help{}:\n", BOLD_YELLOW, RESET));
        for line in help.lines() {
            output.push_str(&format!("  {}{}{}\n", GREEN, line, RESET));
        }
    }
    
    output
}

//=============================================================================
// TESTS
//=============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_logic01_if_without_else() {
        let source = r#"
fn main() {
    x = if true {
        10
    }
}
"#;
        let result = check_logic_no_effects(source, "test.rss");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, ErrorCode::RSPL060);
    }
    
    #[test]
    fn test_logic01_if_with_else_ok() {
        let source = r#"
fn main() {
    x = if true {
        10
    } else {
        20
    }
}
"#;
        let result = check_logic_no_effects(source, "test.rss");
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_logic02_shadowing() {
        let source = r#"
fn main() {
    counter = 0
    {
        counter = counter + 1
    }
}
"#;
        let result = check_logic_no_effects(source, "test.rss");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors[0].code, ErrorCode::RSPL081);
    }
    
    #[test]
    fn test_logic06_same_scope_reassignment_error() {
        let source = r#"
fn main() {
    x = 10
    x = x + 1
}
"#;
        let result = check_logic_no_effects(source, "test.rss");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors[0].code, ErrorCode::RSPL071);
    }
    
    #[test]
    fn test_logic06_mut_ok() {
        let source = r#"
fn main() {
    mut x = 10
    x = x + 1
}
"#;
        let result = check_logic_no_effects(source, "test.rss");
        assert!(result.is_ok());
    }
    
    //=========================================================================
    // ENUM CONSTRUCTOR BUG FIX TESTS (NEW)
    //=========================================================================
    
    /// REGRESSION TEST: Array of enum constructors MUST NOT trigger RSPL-071
    /// This was the original bug report.
    #[test]
    fn test_array_enum_constructors_no_reassignment_error() {
        let source = r#"
fn main() {
    txs = [
        Tx::Deposit { id = 7, amount = 100 },
        Tx::Withdraw { id = 7, amount = 50 }
    ]
}
"#;
        let result = check_logic_no_effects(source, "test.rss");
        // MUST NOT have any errors
        assert!(result.is_ok(), 
            "Array of enum constructors should NOT trigger RSPL-071. Got errors: {:?}", 
            result.unwrap_err());
    }
    
    /// REGRESSION TEST: Enum constructors with tuple syntax
    #[test]
    fn test_array_enum_tuple_constructors() {
        let source = r#"
fn main() {
    options = [
        Some(1),
        Some(2),
        None
    ]
}
"#;
        let result = check_logic_no_effects(source, "test.rss");
        assert!(result.is_ok(), 
            "Array of enum tuple constructors should NOT trigger errors. Got: {:?}",
            result.unwrap_err());
    }
    
    /// Test that actual variable reassignment still triggers RSPL-071
    #[test]
    fn test_actual_reassignment_still_errors() {
        let source = r#"
fn main() {
    x = 10
    x = 20
}
"#;
        let result = check_logic_no_effects(source, "test.rss");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors[0].code, ErrorCode::RSPL071);
    }
    
    /// Test that `x = Tx::Variant { ... }` correctly identifies x as the variable
    #[test]
    fn test_assignment_to_variable_with_enum_value() {
        let source = r#"
fn main() {
    x = Tx::Deposit { id = 7, amount = 100 }
    x = Tx::Withdraw { id = 7, amount = 50 }
}
"#;
        let result = check_logic_no_effects(source, "test.rss");
        // This SHOULD error because x is being reassigned without mut
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors[0].code, ErrorCode::RSPL071);
    }
    
    /// Test that `mut x` allows reassignment with enum values
    #[test]
    fn test_mut_assignment_with_enum_value() {
        let source = r#"
fn main() {
    mut x = Tx::Deposit { id = 7, amount = 100 }
    x = Tx::Withdraw { id = 7, amount = 50 }
}
"#;
        let result = check_logic_no_effects(source, "test.rss");
        assert!(result.is_ok());
    }
    
    /// Test enum constructor detection function
    #[test]
    fn test_is_enum_constructor() {
        // Should be enum constructors
        assert!(is_enum_constructor("Tx::Deposit { id = 7 }"));
        assert!(is_enum_constructor("Tx::Withdraw { amount = 50 }"));
        assert!(is_enum_constructor("Option::Some(value)"));
        assert!(is_enum_constructor("Result::Ok(x)"));
        
        // Should NOT be enum constructors
        assert!(!is_enum_constructor("x = 10"));
        assert!(!is_enum_constructor("std::io::stdin()"));  // lowercase module
        assert!(!is_enum_constructor("let x = 10"));
    }
    
    /// Test pure enum constructor expression detection
    #[test]
    fn test_is_pure_enum_constructor_expr() {
        // Pure enum constructors (start with Type::Variant)
        assert!(is_pure_enum_constructor_expr("Tx::Deposit { id = 7 }"));
        assert!(is_pure_enum_constructor_expr("Tx::Deposit { id = 7 },"));
        assert!(is_pure_enum_constructor_expr("Option::Some(1)"));
        
        // NOT pure (has variable assignment on left)
        assert!(!is_pure_enum_constructor_expr("x = Tx::Deposit { id = 7 }"));
        assert!(!is_pure_enum_constructor_expr("mut x = Option::Some(1)"));
    }
    
    /// Test extract_assignment_target with enum constructors
    #[test]
    fn test_extract_assignment_target() {
        // Should return variable name
        assert_eq!(extract_assignment_target("x = 10"), "x");
        assert_eq!(extract_assignment_target("mut y = 20"), "y");
        assert_eq!(extract_assignment_target("count = count + 1"), "count");
        assert_eq!(extract_assignment_target("x = Tx::Deposit { id = 7 }"), "x");
        
        // Should return empty (NOT assignments)
        assert_eq!(extract_assignment_target("Tx::Deposit { id = 7 }"), "");
        assert_eq!(extract_assignment_target("Tx::Withdraw { amount = 50 }"), "");
        assert_eq!(extract_assignment_target("if x == 10 {"), "");
        assert_eq!(extract_assignment_target("while i <= n {"), "");
    }
    
    /// Test nested array with multiple enum variants
    #[test]
    fn test_nested_enum_array() {
        let source = r#"
fn main() {
    events = [
        Event::Click { x = 10, y = 20 },
        Event::KeyPress { key = 'a' },
        Event::Scroll { delta = 5 },
        Event::Click { x = 30, y = 40 }
    ]
}
"#;
        let result = check_logic_no_effects(source, "test.rss");
        assert!(result.is_ok(), 
            "Nested enum array should compile without RSPL-071. Got: {:?}",
            result.unwrap_err());
    }
    
    //=========================================================================
    // Effect System Tests
    //=========================================================================
    
    #[test]
    fn test_effect_parse() {
        assert_eq!(Effect::parse("io"), Some(Effect::Io));
        assert_eq!(Effect::parse("alloc"), Some(Effect::Alloc));
        assert_eq!(Effect::parse("panic"), Some(Effect::Panic));
        assert_eq!(Effect::parse("read(x)"), Some(Effect::Read("x".to_string())));
        assert_eq!(Effect::parse("write(acc)"), Some(Effect::Write("acc".to_string())));
    }
    
    #[test]
    fn test_effect_io_detection() {
        let source = r#"
fn greet(name String) effects(io) {
    println!("Hello, {}", name)
}

fn main() {
    greet("World")
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_effect_undeclared_io_error() {
        let source = r#"
fn greet(name String) {
    println!("Hello, {}", name)
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.code == ErrorCode::RSPL300));
    }
    
    #[test]
    fn test_effect_write_declaration() {
        let source = r#"
struct Account {
    balance i64
}

fn deposit(acc Account, amount i64) effects(write acc) Account {
    acc.balance = acc.balance + amount
    acc
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_main_allowed_io() {
        let source = r#"
fn main() {
    println!("Hello, World!")
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_ok(), "main should be allowed implicit I/O");
    }
    
    #[test]
    fn test_effect_propagation() {
        let source = r#"
fn inner() effects(io) {
    println!("inner")
}

fn outer() effects(io) {
    inner()
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_effect_propagation_missing() {
        let source = r#"
fn inner() effects(io) {
    println!("inner")
}

fn outer() {
    inner()
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        // Should have missing propagation error
        assert!(errors.iter().any(|e| e.code == ErrorCode::RSPL301 || e.code == ErrorCode::RSPL302));
    }
    
    #[test]
    fn test_pure_function() {
        let source = r#"
fn add(a i32, b i32) i32 {
    a + b
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_effect_signature_display() {
        let mut sig = EffectSignature::new();
        assert_eq!(sig.display(), "pure");
        
        sig.add(Effect::Io);
        assert_eq!(sig.display(), "io");
        
        sig.add(Effect::Write("acc".to_string()));
        assert!(sig.display().contains("io"));
        assert!(sig.display().contains("write(acc)"));
    }
}