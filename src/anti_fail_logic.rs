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
        
        if s.starts_with("write(") && s.ends_with(')') {
            let inner = &s[6..s.len()-1];
            return Some(Effect::Write(inner.trim().to_string()));
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
        }
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
        self.scope_stack.pop();
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
}

//=============================================================================
// EFFECT ANALYZER - The core effect tracking engine
//=============================================================================

/// Analyzes a function body for effects
#[derive(Debug)]
pub struct EffectAnalyzer {
    /// Current function being analyzed
    current_function: Option<String>,
    /// Detected effects in current function
    detected_effects: BTreeSet<Effect>,
    /// Function parameters (for write detection)
    parameters: HashMap<String, String>,  // name -> type
    /// Functions called
    called_functions: Vec<(String, usize)>,  // (name, line)
    /// Effect ownership tracker
    ownership_tracker: EffectOwnershipTracker,
    /// I/O patterns
    io_patterns: Vec<&'static str>,
    /// Allocation patterns  
    alloc_patterns: Vec<&'static str>,
    /// Panic patterns
    panic_patterns: Vec<&'static str>,
}

impl EffectAnalyzer {
    pub fn new() -> Self {
        EffectAnalyzer {
            current_function: None,
            detected_effects: BTreeSet::new(),
            parameters: HashMap::new(),
            called_functions: Vec::new(),
            ownership_tracker: EffectOwnershipTracker::new(),
            io_patterns: vec![
                "println!", "print!", "eprintln!", "eprint!",
                "std::io::", "File::", "read_to_string", "write_all",
                "stdin()", "stdout()", "stderr()",
                "read_line", "BufReader", "BufWriter",
                "OpenOptions", "create(", "open(",
            ],
            alloc_patterns: vec![
                "Vec::new", "vec!", "String::new", "String::from",
                "Box::new", "Rc::new", "Arc::new",
                "HashMap::new", "HashSet::new",
                "BTreeMap::new", "BTreeSet::new",
                ".to_vec()", ".to_string()", ".to_owned()",
                ".clone()",
            ],
            panic_patterns: vec![
                "panic!", "unreachable!", "unimplemented!", "todo!",
                ".unwrap()", ".expect(", "assert!", "assert_eq!", "assert_ne!",
            ],
        }
    }
    
    pub fn enter_function(&mut self, name: &str, params: &[(String, String)], declared: &EffectSignature) {
        self.current_function = Some(name.to_string());
        self.detected_effects.clear();
        self.parameters = params.iter().cloned().collect();
        self.called_functions.clear();
        self.ownership_tracker.enter_function(name, declared);
    }
    
    pub fn exit_function(&mut self) -> (BTreeSet<Effect>, Vec<(String, usize)>) {
        self.current_function = None;
        let effects = std::mem::take(&mut self.detected_effects);
        let calls = std::mem::take(&mut self.called_functions);
        self.parameters.clear();
        let _usages = self.ownership_tracker.exit_function();
        (effects, calls)
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
    
    /// Analyze a line for effects
    pub fn analyze_line(&mut self, line: &str, line_num: usize) {
        let trimmed = line.trim();
        
        // Skip comments
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            return;
        }
        
        // I/O detection
        if let Some(io_op) = self.detect_io(trimmed) {
            let effect = Effect::Io;
            self.detected_effects.insert(effect.clone());
            self.ownership_tracker.record_effect(effect, line_num);
        }
        
        // Allocation detection
        if let Some(_alloc_op) = self.detect_alloc(trimmed) {
            let effect = Effect::Alloc;
            self.detected_effects.insert(effect.clone());
            self.ownership_tracker.record_effect(effect, line_num);
        }
        
        // Panic detection
        if let Some(_panic_op) = self.detect_panic(trimmed) {
            let effect = Effect::Panic;
            self.detected_effects.insert(effect.clone());
            self.ownership_tracker.record_effect(effect, line_num);
        }
        
        // Parameter mutation detection
        if let Some(param) = self.detect_param_mutation(trimmed) {
            let effect = Effect::Write(param.clone());
            self.detected_effects.insert(effect.clone());
            self.ownership_tracker.record_effect(effect, line_num);
        }
        
        // Parameter read detection (field access without mutation)
        if let Some(param) = self.detect_param_read(trimmed) {
            let effect = Effect::Read(param.clone());
            self.detected_effects.insert(effect.clone());
            self.ownership_tracker.record_effect(effect, line_num);
        }
        
        // Function call detection
        for func_name in self.detect_function_calls(trimmed) {
            self.called_functions.push((func_name, line_num));
        }
        
        // Closure detection
        if self.detect_closure_start(trimmed) {
            // Closures are tracked for effect leak detection
        }
    }
    
    fn detect_io(&self, line: &str) -> Option<&'static str> {
        for pattern in &self.io_patterns {
            if line.contains(pattern) {
                return Some(pattern);
            }
        }
        None
    }
    
    fn detect_alloc(&self, line: &str) -> Option<&'static str> {
        for pattern in &self.alloc_patterns {
            if line.contains(pattern) {
                return Some(pattern);
            }
        }
        None
    }
    
    fn detect_panic(&self, line: &str) -> Option<&'static str> {
        for pattern in &self.panic_patterns {
            if line.contains(pattern) {
                return Some(pattern);
            }
        }
        None
    }
    
    fn detect_param_mutation(&self, line: &str) -> Option<String> {
        // Check for parameter field mutation: `param.field = value`
        for (param, _ty) in &self.parameters {
            // Pattern: `param.something = `
            let field_pattern = format!("{}.", param);
            if line.contains(&field_pattern) && line.contains('=') {
                // Make sure it's assignment, not comparison
                let after_param = line.split(&field_pattern).nth(1)?;
                // Check for = but not == or !=
                if after_param.contains('=') && 
                   !after_param.contains("==") && 
                   !after_param.contains("!=") &&
                   !after_param.starts_with('=') {
                    return Some(param.clone());
                }
            }
            
            // Pattern: `param = ` (direct reassignment of param)
            let direct_pattern = format!("{} =", param);
            let direct_pattern2 = format!("{}=", param);
            if (line.trim().starts_with(&direct_pattern) || line.contains(&format!(" {}", direct_pattern))) 
               && !line.contains("==") && !line.contains("!=") {
                return Some(param.clone());
            }
            if line.trim().starts_with(&direct_pattern2) && !line.contains("==") {
                return Some(param.clone());
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
        let keywords = [
            "if", "else", "match", "while", "for", "loop", "fn", "let", "mut",
            "struct", "enum", "impl", "trait", "pub", "mod", "use", "return",
            "break", "continue", "where", "async", "await", "move", "ref",
            "println", "print", "eprintln", "eprint", "vec", "format",
            "panic", "assert", "assert_eq", "assert_ne", "debug_assert",
            "Some", "None", "Ok", "Err", "true", "false",
            "String", "Vec", "Box", "Rc", "Arc", "HashMap", "HashSet",
        ];
        keywords.contains(&name)
    }
    
    fn is_type_constructor(&self, name: &str) -> bool {
        // Check if first char is uppercase (likely a type constructor)
        name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
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
            errors: Vec::new(),
            file_name: file_name.to_string(),
            source_lines: Vec::new(),
            function_vars: HashMap::new(),
            reassigned_vars: HashSet::new(),
            in_function: false,
            function_depth: 0,
            strict_mode: true,
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
    pub fn set_strict_effect_mode(&mut self, enabled: bool) {
        self.strict_effect_mode = enabled;
    }
    
    /// Run anti-fail logic check on source code
    pub fn check(&mut self, source: &str) -> Result<(), Vec<RsplError>> {
        self.source_lines = source.lines().map(String::from).collect();
        
        // PASS 1: Collect all function signatures with effects
        self.collect_function_signatures(source);
        
        // PASS 2: Analyze function bodies for effects
        for (line_num, line) in source.lines().enumerate() {
            let line_num = line_num + 1;
            self.analyze_line(line, line_num);
        }
        
        // Check for unclosed expressions
        self.check_unclosed_expressions();
        
        // PASS 3: Build effect dependency graph
        self.build_effect_graph();
        
        // PASS 4: Effect contract validation
        if self.effect_checking_enabled {
            self.validate_effect_contracts();
            self.validate_effect_propagation();
            self.validate_effect_scope();
        }
        
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }
    
    //=========================================================================
    // PASS 1: FUNCTION SIGNATURE COLLECTION
    //=========================================================================
    
    fn collect_function_signatures(&mut self, source: &str) {
        let mut current_func: Option<FunctionInfo> = None;
        let mut brace_count = 0;
        let mut func_brace_start = 0;
        
        for (line_num, line) in source.lines().enumerate() {
            let line_num = line_num + 1;
            let trimmed = line.trim();
            
            // Count braces
            let opens = trimmed.matches('{').count();
            let closes = trimmed.matches('}').count();
            
            if self.is_function_definition(trimmed) {
                if let Some(func_info) = self.parse_function_with_effects(trimmed, line_num) {
                    current_func = Some(func_info);
                    func_brace_start = brace_count + opens;
                }
            }
            
            brace_count += opens;
            brace_count = brace_count.saturating_sub(closes);
            
            // Collect function body
            if let Some(ref mut func) = current_func {
                func.body_lines.push((line_num, line.to_string()));
                
                // Check if function ended
                if brace_count < func_brace_start {
                    func.end_line = line_num;
                    self.function_table.insert(func.name.clone(), func.clone());
                    self.effect_graph.add_function(&func.name);
                    current_func = None;
                }
            }
        }
        
        // Handle unclosed function
        if let Some(func) = current_func {
            self.function_table.insert(func.name.clone(), func);
        }
    }
    
    fn is_function_definition(&self, line: &str) -> bool {
        (line.starts_with("fn ") || line.starts_with("pub fn ") ||
         line.starts_with("async fn ") || line.starts_with("pub async fn ")) 
        && line.contains('(')
    }
    
    fn parse_function_with_effects(&self, line: &str, line_num: usize) -> Option<FunctionInfo> {
        // Extract function name
        let is_public = line.starts_with("pub ");
        let after_fn = if line.starts_with("pub async fn ") {
            &line[13..]
        } else if line.starts_with("async fn ") {
            &line[9..]
        } else if line.starts_with("pub fn ") {
            &line[7..]
        } else if line.starts_with("fn ") {
            &line[3..]
        } else {
            return None;
        };
        
        let name_end = after_fn.find(|c: char| c == '(' || c == '<' || c == ' ')?;
        let name = after_fn[..name_end].trim().to_string();
        
        let mut func_info = FunctionInfo::new(&name, line_num);
        func_info.is_public = is_public;
        
        // Extract parameters with types
        if let Some(params_start) = line.find('(') {
            if let Some(params_end) = line.find(')') {
                let params_str = &line[params_start + 1..params_end];
                for param in params_str.split(',') {
                    let param = param.trim();
                    if param.is_empty() {
                        continue;
                    }
                    
                    // Parse: name Type or name: Type
                    if param.contains(':') {
                        let parts: Vec<&str> = param.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            let param_name = parts[0].trim().to_string();
                            let param_type = parts[1].trim().to_string();
                            func_info.parameters.push((param_name, param_type));
                        }
                    } else if let Some(space_pos) = param.find(' ') {
                        let param_name = param[..space_pos].trim().to_string();
                        let param_type = param[space_pos + 1..].trim().to_string();
                        func_info.parameters.push((param_name, param_type));
                    }
                }
            }
        }
        
        // Extract return type
        if let Some(arrow_pos) = line.find("->") {
            let after_arrow = &line[arrow_pos + 2..];
            let ret_end = after_arrow.find(|c: char| c == '{' || c == 'e')
                .unwrap_or(after_arrow.len());
            let ret_type = after_arrow[..ret_end].trim();
            if !ret_type.is_empty() && ret_type != "{" {
                func_info.return_type = Some(ret_type.to_string());
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
        
        let opens = self.count_open_braces(trimmed);
        let closes = self.count_close_braces(trimmed);
        
        // Function start detection
        if self.is_function_start(trimmed) {
            self.enter_function(line_num, opens, trimmed);
        } else if opens > 0 && self.in_function {
            let is_control_flow = self.check_control_flow_start(trimmed, line_num);
            let is_closure = self.detect_closure(trimmed);
            
            if is_closure {
                self.effect_analyzer.enter_closure(self.brace_depth + opens, line_num);
            } else if !is_control_flow && !self.is_definition(trimmed) {
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
        
        // Logic-02 & Logic-04: Check assignments
        self.check_assignment(trimmed, line_num);
        
        // Logic-05: Check unclear intent
        if self.strict_mode {
            self.check_unclear_intent(trimmed, line_num);
        }
        
        // Effect analysis (if in function)
        if self.in_function && self.effect_checking_enabled {
            self.effect_analyzer.analyze_line(trimmed, line_num);
        }
        
        // Update brace depth
        for _ in 0..opens {
            self.brace_depth += 1;
        }
        
        for _ in 0..closes {
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
        self.function_depth = self.brace_depth + opens;
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
            func_info.detected_effects.display()
        ))
        .help(format!(
            "add effect declaration to function signature:\n\n\
             fn {}(...) effects({}) {{ ... }}",
            func_info.name,
            effect.display()
        ));
        
        self.errors.push(error);
    }
    
    fn emit_missing_propagation_error(&mut self, func_info: &FunctionInfo, called: &str, effect: &Effect) {
        let error = RsplError::new(
            ErrorCode::RSPL301,
            format!(
                "function `{}` calls `{}` which has effect `{}`, but does not propagate it",
                func_info.name,
                called,
                effect.display()
            )
        )
        .at(self.make_location(func_info.line_number, &func_info.name))
        .note(format!(
            "{} VIOLATION: Missing Effect Propagation\n\n\
             in RustS+, effects must propagate upward through call chains.\n\
             `{}` calls `{}` which declares `{}`.\n\
             the caller must also declare this effect.\n\n\
             Effects are like capabilities - if you use a capability,\n\
             you must have permission for it.",
            LogicViolation::MissingEffectPropagation.code(),
            func_info.name,
            called,
            effect.display()
        ))
        .help(format!(
            "add effect to function signature:\n\n\
             fn {}(...) effects({}) {{ ... }}",
            func_info.name,
            effect.display()
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
             function `{}` has no effects declared (PURE),\n\
             but it calls `{}` which HAS effects.\n\n\
             PURE functions cannot perform ANY effects.\n\
             this ensures referential transparency.",
            LogicViolation::PureCallingEffectful.code(),
            func_info.name,
            called
        ))
        .help(format!(
            "either:\n\
             1. Add effects to `{}`:\n\
                fn {}(...) effects(...) {{ ... }}\n\
             2. Or remove the call to `{}`",
            func_info.name, func_info.name, called
        ));
        
        self.errors.push(error);
    }
    
    //=========================================================================
    // LOGIC CHECKS (Original L01-L06)
    //=========================================================================
    
    fn check_control_flow_start(&mut self, trimmed: &str, line_num: usize) -> bool {
        if let Some(cf_expr) = self.detect_control_flow_expr(trimmed, line_num) {
            self.control_flow_stack.push(cf_expr.clone());
            
            if cf_expr.is_value_context {
                self.enter_scope(true, line_num);
            }
            return true;
        }
        
        if trimmed.starts_with("else") || trimmed.contains("} else") {
            if let Some(cf) = self.control_flow_stack.last_mut() {
                if cf.kind == ControlFlowKind::If {
                    cf.has_else = true;
                }
            }
            return true;
        }
        
        if (trimmed.starts_with("if ") || trimmed.starts_with("while ") ||
            trimmed.starts_with("for ") || trimmed.starts_with("loop ") ||
            trimmed.starts_with("match ")) && trimmed.contains('{') {
            return true;
        }
        
        false
    }
    
    fn detect_control_flow_expr(&self, trimmed: &str, line_num: usize) -> Option<ControlFlowExpr> {
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
        
        if trimmed.contains("= match ") && trimmed.contains('{') {
            let assigned_to = self.extract_assignment_target(trimmed);
            return Some(ControlFlowExpr {
                start_line: line_num,
                is_value_context: true,
                has_else: false,
                kind: ControlFlowKind::Match,
                assigned_to,
                start_depth: self.brace_depth,
            });
        }
        
        None
    }
    
    fn check_unclosed_expressions(&mut self) {
        let unclosed: Vec<_> = self.control_flow_stack.drain(..).collect();
        
        for cf in unclosed {
            if cf.is_value_context && cf.kind == ControlFlowKind::If && !cf.has_else {
                self.emit_logic01_error(cf.start_line, cf.assigned_to.as_deref());
            }
        }
    }
    
    fn emit_logic01_error(&mut self, line_num: usize, assigned_to: Option<&str>) {
        let source_line = self.get_source_line(line_num);
        let var_info = assigned_to
            .map(|v| format!(" (assigning to `{}`)", v))
            .unwrap_or_default();
        
        let error = RsplError::new(
            ErrorCode::RSPL060,
            format!("`if` expression used as value but missing `else` branch{}", var_info)
        )
        .at(self.make_location(line_num, &source_line))
        .note(format!(
            "{} VIOLATION: Expression Completeness\n\n\
             in RustS+, when `if` is used as expression (assigned to variable),\n\
             MUST produce value in ALL branches.",
            LogicViolation::IncompleteExpression.code()
        ))
        .help("add `else` branch to provide value for all cases");
        
        self.errors.push(error);
    }
    
    fn check_illegal_statement(&mut self, trimmed: &str, line_num: usize) {
        let in_expr_context = self.scopes.last()
            .map(|s| s.is_expression_context)
            .unwrap_or(false);
        
        if !in_expr_context {
            return;
        }
        
        if trimmed.starts_with("let ") {
            self.emit_logic03_error(line_num, trimmed);
        }
    }
    
    fn emit_logic03_error(&mut self, line_num: usize, source: &str) {
        let error = RsplError::new(
            ErrorCode::RSPL041,
            "`let` statement not allowed in expression context"
        )
        .at(self.make_location(line_num, source))
        .note(format!(
            "{} VIOLATION: Illegal Statement in Expression\n\n\
             in RustS+, expression blocks (if/match used as value) cannot contain statements.",
            LogicViolation::IllegalStatementInExpression.code()
        ))
        .help("use RustS+ assignment syntax or move declaration outside expression");
        
        self.errors.push(error);
    }
    
    fn check_assignment(&mut self, trimmed: &str, line_num: usize) {
        if !self.in_function {
            return;
        }
        
        // Skip non-assignments
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
        
        // Extract variable name
        let var_name = self.extract_var_name(trimmed);
        if var_name.is_empty() {
            return;
        }
        
        let is_mut_decl = trimmed.starts_with("mut ");
        
        // Check for same-scope reassignment (Logic-06)
        if self.function_vars.contains_key(&var_name) && !is_mut_decl {
            // Check if already marked as mutable
            let is_known_mutable = self.scopes.iter().any(|s| s.is_mutable(&var_name));
            
            if !is_known_mutable && !self.reassigned_vars.contains(&var_name) {
                // First reassignment - this should have been `mut`
                self.emit_logic06_error(&var_name, line_num, trimmed);
            }
            
            self.reassigned_vars.insert(var_name.clone());
            return;
        }
        
        // Check shadowing (Logic-02)
        if self.is_defined_in_outer_scope(&var_name) && !is_mut_decl {
            self.check_shadowing(&var_name, line_num, trimmed);
            return;
        }
        
        // New declaration
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
        let original_line = self.function_vars.get(var_name).copied().unwrap_or(0);
        
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
        self.scopes.push(Scope::new(
            self.brace_depth + 1,
            is_expression_context,
            line_num,
        ));
    }
    
    fn exit_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }
    
    fn handle_close_brace(&mut self) {
        if self.brace_depth > 0 {
            self.brace_depth -= 1;
        }
        
        // Check control flow completion
        if let Some(cf) = self.control_flow_stack.last().cloned() {
            if self.brace_depth <= cf.start_depth {
                self.control_flow_stack.pop();
                
                if cf.is_value_context && cf.kind == ControlFlowKind::If && !cf.has_else {
                    self.emit_logic01_error(cf.start_line, cf.assigned_to.as_deref());
                }
                
                if cf.is_value_context {
                    self.exit_scope();
                }
            }
        }
        
        // Check scope exit
        if let Some(scope) = self.scopes.last() {
            if self.brace_depth < scope.depth {
                self.exit_scope();
                self.effect_analyzer.exit_block();
            }
        }
    }
    
    fn extract_assignment_target(&self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if let Some(eq_pos) = trimmed.find('=') {
            let before_eq = trimmed[..eq_pos].trim();
            let var_name: String = before_eq
                .chars()
                .skip_while(|c| *c == ' ' || *c == '\t')
                .skip_while(|c| *c == 'm' || *c == 'u' || *c == 't' || *c == ' ')
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            
            if !var_name.is_empty() {
                return Some(var_name);
            }
        }
        None
    }
    
    fn extract_var_name(&self, line: &str) -> String {
        let trimmed = line.trim();
        let start = if trimmed.starts_with("mut ") {
            4
        } else {
            0
        };
        
        let after_mut = &trimmed[start..];
        let var_end = after_mut.find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(after_mut.len());
        
        after_mut[..var_end].to_string()
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
    use ansi::*;
    
    let mut output = String::new();
    
    output.push_str(&format!(
        "\n{}╔═══════════════════════════════════════════════════════════════════╗{}\n",
        BOLD_RED, RESET
    ));
    output.push_str(&format!(
        "{}║   RustS+ Anti-Fail Logic Error (Stage 1 Contract Violation)       ║{}\n",
        BOLD_RED, RESET
    ));
    output.push_str(&format!(
        "{}╚═══════════════════════════════════════════════════════════════════╝{}\n\n",
        BOLD_RED, RESET
    ));
    
    // Group errors by category
    let mut effect_errors = Vec::new();
    let mut logic_errors = Vec::new();
    
    for error in errors {
        if matches!(error.code, 
            ErrorCode::RSPL300 | ErrorCode::RSPL301 | ErrorCode::RSPL302 |
            ErrorCode::RSPL303 | ErrorCode::RSPL304 | ErrorCode::RSPL305 |
            ErrorCode::RSPL306 | ErrorCode::RSPL307 | ErrorCode::RSPL308 |
            ErrorCode::RSPL309 | ErrorCode::RSPL310 | ErrorCode::RSPL311 |
            ErrorCode::RSPL312 | ErrorCode::RSPL313 | ErrorCode::RSPL314) {
            effect_errors.push(error);
        } else {
            logic_errors.push(error);
        }
    }
    
    // Display effect errors first
    if !effect_errors.is_empty() {
        output.push_str(&format!("{}─── Effect Errors ───{}\n\n", BOLD_MAGENTA, RESET));
        for error in effect_errors {
            output.push_str(&format_single_error(error));
            output.push('\n');
        }
    }
    
    // Display logic errors
    if !logic_errors.is_empty() {
        output.push_str(&format!("{}─── Logic Errors ───{}\n\n", BOLD_YELLOW, RESET));
        for error in logic_errors {
            output.push_str(&format_single_error(error));
            output.push('\n');
        }
    }
    
    output.push_str(&format!(
        "\n{}error{}: aborting due to {} error{}\n",
        BOLD_RED, RESET,
        errors.len(),
        if errors.len() == 1 { "" } else { "s" }
    ));
    
    output.push_str(&format!(
        "\n{}note{}: RustS+ detects logic and effect errors BEFORE Rust compilation.\n",
        CYAN, RESET
    ));
    output.push_str(&format!(
        "{}      fix these errors first - no Rust code will be generated.{}\n",
        CYAN, RESET
    ));
    
    output
}

fn format_single_error(error: &RsplError) -> String {
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