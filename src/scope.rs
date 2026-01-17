//! Scope management for RustS+ compiler
//! 
//! Handles:
//! - Scope stack (push on `{`, pop on `}`)
//! - Variable lookup (innermost to outermost)
//! - Shadowing detection (type change = new declaration)
//! - Mutation tracking (same type in inner scope = mutate parent)
//! - `outer` keyword for explicit cross-scope mutation
//!
//! ## NEW: HIR Integration
//! 
//! This module now integrates with the HIR (High-level IR) system:
//! - Convert ScopeStack to HIR ScopeResolver
//! - Map legacy scope variables to HIR BindingIds
//! - Support effect analysis through HIR

use std::collections::HashMap;

//=============================================================================
// HIR INTEGRATION TYPES (NEW)
//=============================================================================

/// Unique identifier for a variable binding (mirrors hir::BindingId)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindingId(pub u32);

impl BindingId {
    pub fn new(id: u32) -> Self {
        BindingId(id)
    }
}

/// Binding information for HIR integration
#[derive(Debug, Clone)]
pub struct HirBindingInfo {
    pub id: BindingId,
    pub name: String,
    pub var_type: Option<String>,
    pub mutable: bool,
    pub scope_depth: usize,
    pub decl_line: usize,
    pub is_outer: bool,
    pub is_param: bool,
}

//=============================================================================
// LEGACY SCOPE TYPES
//=============================================================================

/// A variable within a scope
#[derive(Debug, Clone)]
pub struct ScopedVar {
    pub name: String,
    pub var_type: Option<String>,
    pub line: usize,
    /// NEW: HIR binding ID for effect tracking
    pub binding_id: Option<BindingId>,
}

/// A single scope level
#[derive(Debug, Clone)]
pub struct Scope {
    pub level: usize,
    pub vars: HashMap<String, ScopedVar>,
    /// Is this a bare block (true) or control flow block (false)?
    /// Bare blocks use shadow semantics, control flow uses mutation
    pub is_bare_block: bool,
    /// NEW: Is this scope a closure? (for effect isolation)
    pub is_closure: bool,
}

impl Scope {
    pub fn new(level: usize) -> Self {
        Scope {
            level,
            vars: HashMap::new(),
            is_bare_block: false,
            is_closure: false,
        }
    }
    
    pub fn new_bare(level: usize) -> Self {
        Scope {
            level,
            vars: HashMap::new(),
            is_bare_block: true,
            is_closure: false,
        }
    }
    
    /// NEW: Create a closure scope
    pub fn new_closure(level: usize) -> Self {
        Scope {
            level,
            vars: HashMap::new(),
            is_bare_block: false,
            is_closure: true,
        }
    }
}

/// Manages the scope stack
#[derive(Debug)]
pub struct ScopeStack {
    /// Stack of scopes (index 0 = outermost/function level)
    pub scopes: Vec<Scope>,
    /// Variables that need mut at their declaration site
    /// Key: (var_name, declaration_line)
    pub mut_needed: HashMap<(String, usize), bool>,
    /// Count of control flow blocks we're inside
    control_flow_depth: usize,
    /// NEW: Next binding ID for HIR integration
    next_binding_id: u32,
    /// NEW: All bindings for HIR conversion
    all_bindings: HashMap<BindingId, HirBindingInfo>,
}

impl ScopeStack {
    pub fn new() -> Self {
        ScopeStack {
            scopes: vec![Scope::new(0)], // Start with root scope (not bare)
            mut_needed: HashMap::new(),
            control_flow_depth: 0,
            next_binding_id: 0,
            all_bindings: HashMap::new(),
        }
    }
    
    /// Current scope depth (0 = root)
    pub fn depth(&self) -> usize {
        self.scopes.len().saturating_sub(1)
    }
    
    /// Push a control flow scope (while, if, for, etc.) - allows mutation
    pub fn push(&mut self) {
        let new_level = self.scopes.len();
        self.scopes.push(Scope::new(new_level));
        self.control_flow_depth += 1;
    }
    
    /// Push a bare block scope `{}` - uses shadow semantics only if NOT in control flow
    pub fn push_bare(&mut self) {
        let new_level = self.scopes.len();
        let mut scope = Scope::new_bare(new_level);
        // If we're inside control flow, this "bare" block should still allow mutation
        if self.control_flow_depth > 0 {
            scope.is_bare_block = false;
        }
        self.scopes.push(scope);
    }
    
    /// NEW: Push a closure scope (effects do not propagate automatically)
    pub fn push_closure(&mut self) {
        let new_level = self.scopes.len();
        self.scopes.push(Scope::new_closure(new_level));
    }
    
    /// Pop current scope
    pub fn pop(&mut self) {
        if self.scopes.len() > 1 {
            if let Some(scope) = self.scopes.last() {
                // Decrement control flow depth if this was a control flow scope
                if !scope.is_bare_block && !scope.is_closure && self.control_flow_depth > 0 {
                    self.control_flow_depth -= 1;
                }
            }
            self.scopes.pop();
        }
    }
    
    /// Check if current scope is a bare block
    pub fn is_current_bare(&self) -> bool {
        self.scopes.last().map(|s| s.is_bare_block).unwrap_or(false)
    }
    
    /// NEW: Check if current scope is a closure
    pub fn is_current_closure(&self) -> bool {
        self.scopes.last().map(|s| s.is_closure).unwrap_or(false)
    }
    
    /// NEW: Check if we're inside any closure scope
    pub fn in_closure(&self) -> bool {
        self.scopes.iter().any(|s| s.is_closure)
    }
    
    /// Check if we're inside any control flow
    pub fn in_control_flow(&self) -> bool {
        self.control_flow_depth > 0
    }
    
    /// Declare a variable in current scope
    pub fn declare(&mut self, name: &str, var_type: Option<String>, line: usize) {
        let binding_id = self.allocate_binding_id();
        
        let var = ScopedVar {
            name: name.to_string(),
            var_type: var_type.clone(),
            line,
            binding_id: Some(binding_id),
        };
        
        // Store HIR binding info
        let hir_info = HirBindingInfo {
            id: binding_id,
            name: name.to_string(),
            var_type,
            mutable: false,
            scope_depth: self.depth(),
            decl_line: line,
            is_outer: false,
            is_param: false,
        };
        self.all_bindings.insert(binding_id, hir_info);
        
        if let Some(scope) = self.scopes.last_mut() {
            scope.vars.insert(name.to_string(), var);
        }
    }
    
    /// NEW: Declare a function parameter
    pub fn declare_param(&mut self, name: &str, var_type: Option<String>, line: usize) {
        let binding_id = self.allocate_binding_id();
        
        let var = ScopedVar {
            name: name.to_string(),
            var_type: var_type.clone(),
            line,
            binding_id: Some(binding_id),
        };
        
        // Store HIR binding info with is_param = true
        let hir_info = HirBindingInfo {
            id: binding_id,
            name: name.to_string(),
            var_type,
            mutable: false,
            scope_depth: self.depth(),
            decl_line: line,
            is_outer: false,
            is_param: true, // This is a parameter
        };
        self.all_bindings.insert(binding_id, hir_info);
        
        if let Some(scope) = self.scopes.last_mut() {
            scope.vars.insert(name.to_string(), var);
        }
    }
    
    /// NEW: Allocate a new binding ID
    fn allocate_binding_id(&mut self) -> BindingId {
        let id = BindingId::new(self.next_binding_id);
        self.next_binding_id += 1;
        id
    }
    
    /// Look up variable from innermost to outermost scope
    /// Returns: Option<(ScopedVar, scope_level)>
    pub fn lookup(&self, name: &str) -> Option<(&ScopedVar, usize)> {
        for scope in self.scopes.iter().rev() {
            if let Some(var) = scope.vars.get(name) {
                return Some((var, scope.level));
            }
        }
        None
    }
    
    /// Look up variable ONLY in parent scopes (excludes current scope)
    /// Used for `outer` keyword - variable must exist in parent, not current
    /// Returns: Option<(ScopedVar, scope_level)>
    pub fn lookup_in_parent(&self, name: &str) -> Option<(&ScopedVar, usize)> {
        // Need at least 2 scopes (root + current) to have a parent
        if self.scopes.len() <= 1 {
            return None;
        }
        
        // Skip current scope (last), search from parent upward
        for scope in self.scopes.iter().rev().skip(1) {
            if let Some(var) = scope.vars.get(name) {
                return Some((var, scope.level));
            }
        }
        None
    }
    
    /// Check if variable exists in current (innermost) scope only
    pub fn in_current_scope(&self, name: &str) -> bool {
        if let Some(scope) = self.scopes.last() {
            scope.vars.contains_key(name)
        } else {
            false
        }
    }
    
    /// Mark a variable as needing mut at its declaration
    pub fn mark_mut(&mut self, name: &str, declaration_line: usize) {
        self.mut_needed.insert((name.to_string(), declaration_line), true);
        
        // NEW: Also update HIR binding
        for (_, info) in self.all_bindings.iter_mut() {
            if info.name == name && info.decl_line == declaration_line {
                info.mutable = true;
                break;
            }
        }
    }
    
    /// Check if a variable needs mut
    pub fn needs_mut(&self, name: &str, declaration_line: usize) -> bool {
        self.mut_needed.get(&(name.to_string(), declaration_line)).copied().unwrap_or(false)
    }
    
    //=========================================================================
    // HIR INTEGRATION METHODS (NEW)
    //=========================================================================
    
    /// Get binding ID for a variable
    pub fn get_binding_id(&self, name: &str) -> Option<BindingId> {
        self.lookup(name).and_then(|(var, _)| var.binding_id)
    }
    
    /// Get all bindings (for HIR conversion)
    pub fn all_hir_bindings(&self) -> &HashMap<BindingId, HirBindingInfo> {
        &self.all_bindings
    }
    
    /// Get all parameter bindings (for effect analysis)
    pub fn get_param_bindings(&self) -> Vec<BindingId> {
        self.all_bindings.iter()
            .filter(|(_, info)| info.is_param)
            .map(|(id, _)| *id)
            .collect()
    }
    
    /// Check if a binding is a parameter
    pub fn is_param(&self, id: BindingId) -> bool {
        self.all_bindings.get(&id)
            .map(|info| info.is_param)
            .unwrap_or(false)
    }
    
    /// Get binding info
    pub fn get_binding_info(&self, id: BindingId) -> Option<&HirBindingInfo> {
        self.all_bindings.get(&id)
    }
    
    /// NEW: Export scope state to HIR-compatible format
    pub fn to_hir_bindings(&self) -> HashMap<BindingId, HirBindingInfo> {
        self.all_bindings.clone()
    }
    
    /// NEW: Check if we've crossed a closure boundary when looking up a variable
    /// This is used for effect propagation analysis
    pub fn crosses_closure_boundary(&self, name: &str) -> bool {
        let mut found_closure = false;
        
        for scope in self.scopes.iter().rev() {
            if scope.vars.contains_key(name) {
                // Found the variable - return whether we crossed a closure
                return found_closure;
            }
            if scope.is_closure {
                found_closure = true;
            }
        }
        
        // Variable not found
        false
    }
}

impl Default for ScopeStack {
    fn default() -> Self {
        Self::new()
    }
}

//=============================================================================
// ASSIGNMENT ANALYSIS
//=============================================================================

/// Result of analyzing an assignment
#[derive(Debug, Clone, PartialEq)]
pub enum AssignKind {
    /// New variable declaration (needs `let`)
    NewDecl,
    /// Shadowing - same name, different type (needs `let`)
    Shadow,
    /// Mutation of existing variable (no `let`, original needs `mut`)
    Mutation { decl_line: usize },
    /// Explicit outer mutation via `outer` keyword (no `let`, original needs `mut`)
    OuterMutation { decl_line: usize },
    /// Error - `outer` used but variable not found in parent scope
    OuterError(String),
}

/// Infer type from a value expression
pub fn infer_type(value: &str) -> Option<String> {
    let v = value.trim();
    
    // Check for expressions that reference other variables (can't infer)
    // But first check for literals
    if v.starts_with('"') && v.ends_with('"') {
        return Some("String".to_string());
    }
    if v.starts_with("String::from") || v.contains("String::from") {
        return Some("String".to_string());
    }
    if v == "true" || v == "false" {
        return Some("bool".to_string());
    }
    if v.parse::<i64>().is_ok() {
        return Some("i32".to_string());
    }
    if v.parse::<f64>().is_ok() && v.contains('.') {
        return Some("f64".to_string());
    }
    if v.starts_with('\'') && v.ends_with('\'') && v.len() == 3 {
        return Some("char".to_string());
    }
    if v.starts_with("vec![") || v.starts_with("Vec::") {
        return Some("Vec".to_string());
    }
    if v.starts_with('&') {
        return Some("ref".to_string());
    }
    
    // For expressions like `a + 1`, try to infer from context
    // If it contains arithmetic with numbers, likely same numeric type
    None
}

/// Analyze an assignment and determine what kind it is
/// 
/// RULES:
/// - In BARE BLOCK `{}`: assignment to parent variable = SHADOW (new local var)
/// - In CONTROL FLOW (`while`, `if`, `for`): same type = MUTATION, diff type = SHADOW
/// - In SAME SCOPE: same type = MUTATION, diff type = SHADOW
pub fn analyze_assignment(
    stack: &ScopeStack,
    var_name: &str,
    new_type: &Option<String>,
) -> AssignKind {
    // CASE 1: Variable exists in CURRENT scope
    if stack.in_current_scope(var_name) {
        if let Some((existing_var, _)) = stack.lookup(var_name) {
            let is_same_type = match (&existing_var.var_type, new_type) {
                (Some(et), Some(nt)) => et == nt,
                (None, None) => true,     // Both unknown = assume same
                (Some(_), None) => true,  // New type unknown = assume same
                (None, Some(_)) => true,  // Old type unknown = assume same
            };
            
            if is_same_type {
                // Same type, same scope = MUTATION
                AssignKind::Mutation {
                    decl_line: existing_var.line,
                }
            } else {
                // Different type, same scope = SHADOWING
                AssignKind::Shadow
            }
        } else {
            AssignKind::NewDecl
        }
    }
    // CASE 2: Variable exists only in PARENT scope
    else if let Some((existing_var, _)) = stack.lookup(var_name) {
        // Check if we're in a BARE block
        if stack.is_current_bare() {
            // BARE BLOCK: always shadow parent scope variables
            AssignKind::Shadow
        } else {
            // CONTROL FLOW BLOCK: check type for mutation vs shadow
            let is_same_type = match (&existing_var.var_type, new_type) {
                (Some(et), Some(nt)) => et == nt,
                (None, None) => true,
                (Some(_), None) => true,
                (None, Some(_)) => true,
            };
            
            if is_same_type {
                // Same type in control flow = MUTATION of parent
                AssignKind::Mutation {
                    decl_line: existing_var.line,
                }
            } else {
                // Different type = SHADOW
                AssignKind::Shadow
            }
        }
    }
    // CASE 3: Variable doesn't exist anywhere
    else {
        AssignKind::NewDecl
    }
}

/// Analyze an `outer` assignment - explicit mutation of parent scope variable
/// 
/// RULES:
/// - Variable MUST exist in a parent scope (not current scope)
/// - Always results in mutation (never shadow, never new decl)
/// - If variable not found in any parent scope → error
pub fn analyze_outer_assignment(
    stack: &ScopeStack,
    var_name: &str,
) -> AssignKind {
    // Look up ONLY in parent scopes (not current)
    if let Some((existing_var, _level)) = stack.lookup_in_parent(var_name) {
        // Found in parent scope → explicit outer mutation
        AssignKind::OuterMutation {
            decl_line: existing_var.line,
        }
    } else {
        // Not found in parent scope → error
        AssignKind::OuterError(format!(
            "Variable '{}' not found in outer scope. The 'outer' keyword requires the variable to exist in a parent scope.",
            var_name
        ))
    }
}

//=============================================================================
// SCOPE ANALYZER
//=============================================================================

/// Two-pass scope analyzer for a source file
#[derive(Debug)]
pub struct ScopeAnalyzer {
    /// Variables that need mut: (var_name, declaration_line) -> true
    pub mut_vars: HashMap<(String, usize), bool>,
    /// Lines that are declarations: line -> (var_name, is_shadowing)
    pub decl_lines: HashMap<usize, (String, bool)>,
    /// Lines that are mutations: line -> (var_name, decl_line)
    pub mut_lines: HashMap<usize, (String, usize)>,
    /// Lines that have `outer` keyword: line -> true
    pub outer_lines: HashMap<usize, bool>,
    /// NEW: HIR bindings collected during analysis
    pub hir_bindings: HashMap<BindingId, HirBindingInfo>,
    /// NEW: Parameter bindings for effect analysis
    pub param_bindings: Vec<BindingId>,
}

impl ScopeAnalyzer {
    pub fn new() -> Self {
        ScopeAnalyzer {
            mut_vars: HashMap::new(),
            decl_lines: HashMap::new(),
            mut_lines: HashMap::new(),
            outer_lines: HashMap::new(),
            hir_bindings: HashMap::new(),
            param_bindings: Vec::new(),
        }
    }
    
    /// Analyze source and build scope information
    pub fn analyze(&mut self, source: &str) {
        let lines: Vec<&str> = source.lines().collect();
        let mut stack = ScopeStack::new();
        
        // Track if previous non-empty content was a control flow keyword
        let mut pending_control_flow = false;
        // NEW: Track if we're in a function definition (for parameter detection)
        let mut in_function_signature = false;
        let mut current_function_line: Option<usize> = None;
        
        //=====================================================================
        // CRITICAL FIX: Track struct literal mode
        // When inside a struct literal, `field = value` is NOT a variable assignment!
        // We track depth of struct literals separately from code blocks.
        //
        // struct_literal_depth > 0 means we're inside a struct literal and should
        // NOT treat `field = value` as variable assignment.
        //=====================================================================
        let mut struct_literal_depth: usize = 0;
        
        for (line_num, line) in lines.iter().enumerate() {
            let clean = strip_comment(line);
            let trimmed = clean.trim();
            
            // Check if this line is or contains control flow OR function definition
            let is_control_flow_line = trimmed.starts_with("if ")
                || trimmed.starts_with("} else")
                || trimmed.starts_with("else")
                || trimmed.starts_with("while ")
                || trimmed.starts_with("for ")
                || trimmed.starts_with("loop")
                || trimmed.starts_with("match ")
                || trimmed.contains("} else")
                || trimmed.contains("else {");
            
            // Function definitions open a normal (non-bare) scope
            let is_function_def = trimmed.starts_with("fn ") 
                || trimmed.starts_with("pub fn ");
            
            // NEW: Detect closure
            let is_closure = trimmed.contains("|") && 
                (trimmed.contains("||") || (trimmed.matches('|').count() >= 2));
            
            //=================================================================
            // CRITICAL FIX: Detect struct literal start
            // Patterns:
            //   - `var = StructName {` (assignment)
            //   - `Some(StructName {` (function call containing struct)
            //   - `StructName {` (bare return expression)
            //   - `vec![StructName {` (macro call)
            //   - Match arm: `Pattern { ... } {` followed by struct
            //
            // A struct literal starts when:
            //   1. Line contains `{`
            //   2. NOT a control flow statement
            //   3. NOT a function/impl/trait/mod/enum/struct definition
            //   4. Has a PascalCase identifier before `{`
            //=================================================================
            let is_struct_literal_start = detect_struct_literal_start(trimmed) 
                && !is_control_flow_line 
                && !is_function_def
                && !is_closure
                && !trimmed.starts_with("impl ")
                && !trimmed.starts_with("trait ")
                && !trimmed.starts_with("mod ")
                && !trimmed.starts_with("pub mod ")
                && !trimmed.starts_with("struct ")
                && !trimmed.starts_with("pub struct ")
                && !trimmed.starts_with("enum ")
                && !trimmed.starts_with("pub enum ");
            
            // NEW: Extract function parameters
            if is_function_def {
                in_function_signature = true;
                current_function_line = Some(line_num);
                
                // Parse parameters from function signature
                if let Some(params) = extract_function_params(trimmed) {
                    for (param_name, param_type) in params {
                        stack.declare_param(&param_name, param_type, line_num);
                    }
                }
            }
            
            // Count braces
            let opens = trimmed.matches('{').count();
            let closes = trimmed.matches('}').count();
            
            //=================================================================
            // CRITICAL FIX: Update struct_literal_depth for closing braces
            // Decrement depth for each `}` that closes a struct literal
            //=================================================================
            if struct_literal_depth > 0 {
                // Each close potentially closes a struct literal
                // But we need to be careful: some closes might be from match arms
                for _ in 0..closes {
                    if struct_literal_depth > 0 {
                        struct_literal_depth -= 1;
                    }
                }
            }
            
            // Pop for leading `}` BEFORE checking control flow
            let leading_closes = if trimmed.starts_with('}') {
                let mut count = 0;
                for c in trimmed.chars() {
                    if c == '}' { count += 1; }
                    else { break; }
                }
                count
            } else {
                0
            };
            
            for _ in 0..leading_closes {
                stack.pop();
            }
            
            //=================================================================
            // CRITICAL FIX: SKIP parse_assignment when inside struct literal
            // Inside struct literals, `field = value` is a field initialization,
            // NOT a variable assignment!
            //=================================================================
            let should_parse_assignment = struct_literal_depth == 0;
            
            // Parse assignment AFTER handling leading closes
            if should_parse_assignment {
                if let Some((var_name, var_type, value, is_outer)) = parse_assignment(trimmed) {
                    let inferred = var_type.clone().or_else(|| infer_type(&value));
                    
                    // Use different analysis for outer vs regular assignment
                    let kind = if is_outer {
                        analyze_outer_assignment(&stack, &var_name)
                    } else {
                        analyze_assignment(&stack, &var_name, &inferred)
                    };
                    
                    match kind {
                        AssignKind::NewDecl => {
                            stack.declare(&var_name, inferred, line_num);
                            self.decl_lines.insert(line_num, (var_name, false));
                        }
                        AssignKind::Shadow => {
                            stack.declare(&var_name, inferred, line_num);
                            self.decl_lines.insert(line_num, (var_name, true));
                        }
                        AssignKind::Mutation { decl_line } => {
                            stack.mark_mut(&var_name, decl_line);
                            self.mut_vars.insert((var_name.clone(), decl_line), true);
                            self.mut_lines.insert(line_num, (var_name, decl_line));
                        }
                        AssignKind::OuterMutation { decl_line } => {
                            stack.mark_mut(&var_name, decl_line);
                            self.mut_vars.insert((var_name.clone(), decl_line), true);
                            self.mut_lines.insert(line_num, (var_name, decl_line));
                            self.outer_lines.insert(line_num, true);
                        }
                        AssignKind::OuterError(msg) => {
                            eprintln!("// COMPILE ERROR at line {}: {}", line_num + 1, msg);
                        }
                    }
                }
            }
            
            //=================================================================
            // CRITICAL FIX: Update struct_literal_depth for opening braces
            // Increment depth for each `{` that starts a struct literal
            //=================================================================
            if is_struct_literal_start {
                // This line starts a struct literal
                struct_literal_depth += opens;
            } else if struct_literal_depth > 0 && opens > 0 {
                // Already inside struct literal, nested struct opens more
                // Check if this line also contains a nested struct start
                if detect_struct_literal_start(trimmed) {
                    struct_literal_depth += opens;
                }
            }
            
            // Push for `{` - determine if bare or control flow or function or closure
            for _ in 0..opens {
                if is_closure {
                    stack.push_closure(); // NEW: Closure scope
                } else if is_control_flow_line || pending_control_flow {
                    stack.push(); // Control flow block - allows mutation
                } else if is_function_def {
                    // Function body
                    let new_level = stack.scopes.len();
                    stack.scopes.push(Scope::new(new_level));
                    in_function_signature = false;
                } else {
                    stack.push_bare(); // Bare block
                }
            }
            
            // Track pending control flow
            if (is_control_flow_line || is_function_def) && opens == 0 {
                pending_control_flow = is_control_flow_line;
            } else if opens > 0 {
                pending_control_flow = false;
            }
            
            // Pop for trailing `}` (not leading)
            let trailing_closes = closes.saturating_sub(leading_closes);
            for _ in 0..trailing_closes {
                stack.pop();
            }
        }
        
        // Copy all mut requirements
        for (key, val) in &stack.mut_needed {
            self.mut_vars.insert(key.clone(), *val);
        }
        
        // NEW: Store HIR bindings
        self.hir_bindings = stack.to_hir_bindings();
        self.param_bindings = stack.get_param_bindings();
    }
    
    /// Is this line a declaration?
    pub fn is_decl(&self, line: usize) -> bool {
        self.decl_lines.contains_key(&line)
    }
    
    /// Is this line a mutation?
    pub fn is_mut(&self, line: usize) -> bool {
        self.mut_lines.contains_key(&line)
    }
    
    /// Is this line an outer mutation?
    pub fn is_outer(&self, line: usize) -> bool {
        self.outer_lines.contains_key(&line)
    }
    
    /// Does this variable need mut at given line?
    pub fn needs_mut(&self, var_name: &str, line: usize) -> bool {
        self.mut_vars.get(&(var_name.to_string(), line)).copied().unwrap_or(false)
    }
    
    /// NEW: Get all HIR bindings
    pub fn get_hir_bindings(&self) -> &HashMap<BindingId, HirBindingInfo> {
        &self.hir_bindings
    }
    
    /// NEW: Get parameter bindings for effect analysis
    pub fn get_params(&self) -> &[BindingId] {
        &self.param_bindings
    }
}

impl Default for ScopeAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

//=============================================================================
// HELPER FUNCTIONS
//=============================================================================

/// Strip inline comments
fn strip_comment(line: &str) -> String {
    let mut result = String::new();
    let mut in_string = false;
    let mut prev = ' ';
    
    for (i, c) in line.char_indices() {
        if c == '"' && prev != '\\' {
            in_string = !in_string;
        }
        if !in_string && c == '/' && line[i..].starts_with("//") {
            break;
        }
        result.push(c);
        prev = c;
    }
    
    result
}

/// Simple assignment parser
/// Returns: (var_name, var_type, value, is_outer)
fn parse_assignment(line: &str) -> Option<(String, Option<String>, String, bool)> {
    let trimmed = line.trim();
    
    // Check for `outer` keyword prefix
    let (is_outer, remaining) = if trimmed.starts_with("outer ") {
        (true, trimmed.strip_prefix("outer ").unwrap().trim())
    } else {
        (false, trimmed)
    };
    
    // Handle `mut` keyword prefix
    let remaining = if remaining.starts_with("mut ") {
        remaining.strip_prefix("mut ").unwrap().trim()
    } else {
        remaining
    };
    
    // Skip Rust keywords
    let skip_prefixes = [
        "let ", "const ", "static ", "fn ", "pub ", "use ", "mod ",
        "struct ", "enum ", "impl ", "trait ", "type ", "//", "/*",
        "#", "if ", "else", "while ", "for ", "loop", "match ",
        "return ", "break", "continue",
    ];
    for prefix in &skip_prefixes {
        if remaining.starts_with(prefix) {
            return None;
        }
    }
    
    if remaining.is_empty() || remaining == "{" || remaining == "}" {
        return None;
    }
    
    if !remaining.contains('=') {
        return None;
    }
    
    // Skip compound operators
    for op in &["==", "!=", "<=", ">=", "+=", "-=", "*=", "/=", "=>"] {
        if remaining.contains(op) {
            if let Some(pos) = remaining.find('=') {
                let after = remaining.chars().nth(pos + 1);
                let before = &remaining[..pos];
                if matches!(after, Some('=') | Some('>')) {
                    return None;
                }
                if before.ends_with('!') || before.ends_with('<')
                   || before.ends_with('>') || before.ends_with('+')
                   || before.ends_with('-') || before.ends_with('*')
                   || before.ends_with('/') {
                    return None;
                }
            }
        }
    }
    
    let parts: Vec<&str> = remaining.splitn(2, '=').collect();
    if parts.len() != 2 {
        return None;
    }
    
    let left = parts[0].trim();
    let right = parts[1].trim().trim_end_matches(';');
    
    if left.is_empty() || right.is_empty() {
        return None;
    }
    
    if left.contains('(') || left.contains('[') || left.contains('{') {
        return None;
    }
    
    // Type annotation
    if left.contains(':') {
        let tp: Vec<&str> = left.splitn(2, ':').collect();
        if tp.len() == 2 {
            let var = tp[0].trim();
            let typ = tp[1].trim();
            if is_valid_ident(var) {
                return Some((var.to_string(), Some(typ.to_string()), right.to_string(), is_outer));
            }
        }
        return None;
    }
    
    if is_valid_ident(left) {
        Some((left.to_string(), None, right.to_string(), is_outer))
    } else {
        None
    }
}

fn is_valid_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = s.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

//=============================================================================
// STRUCT LITERAL DETECTION
// Detect when a line starts a struct literal expression.
// This is CRITICAL for correctly handling nested struct literals where
// `field = value` should NOT be treated as variable assignment.
//=============================================================================

/// Detect if a line starts a struct literal.
/// 
/// Returns true for patterns like:
///   - `var = StructName {` (assignment to struct literal)
///   - `Some(StructName {` (function call containing struct)
///   - `StructName {` (bare struct literal, e.g., return expression)
///   - `vec![StructName {` (macro call containing struct)
///   - `Enum::Variant {` (enum variant with struct fields)
///
/// Returns false for:
///   - `if condition {` (control flow)
///   - `fn name() {` (function definition)
///   - `impl Trait {` (impl block)
///   - `struct Name {` (struct definition)
///   - etc.
fn detect_struct_literal_start(line: &str) -> bool {
    let trimmed = line.trim();
    
    // Must contain `{` to potentially start a struct literal
    if !trimmed.contains('{') {
        return false;
    }
    
    // Find the position of `{`
    let brace_pos = match trimmed.find('{') {
        Some(pos) => pos,
        None => return false,
    };
    
    // Get the part before `{`
    let before_brace = trimmed[..brace_pos].trim();
    
    // Empty before brace (just `{`) - this is a bare block, not struct literal
    if before_brace.is_empty() {
        return false;
    }
    
    // Check for match arm pattern: `Pattern { destructure } {`
    // This is tricky - the `{` after destructure is a code block, not struct literal
    // But `Pattern {` might be followed by struct field initialization
    
    // Find the identifier before `{` - it might be:
    //   1. `StructName` in `var = StructName {`
    //   2. `StructName` in `Some(StructName {`
    //   3. `StructName` in bare `StructName {`
    //   4. `Variant` in `Enum::Variant {`
    
    // Extract the name/path right before `{`
    let name_before_brace = extract_name_before_brace(before_brace);
    
    if let Some(name) = name_before_brace {
        // Check if it looks like a struct name (PascalCase) or enum path
        if is_pascal_case(&name) || name.contains("::") {
            return true;
        }
    }
    
    false
}

/// Extract the identifier/path immediately before `{`
/// 
/// Examples:
///   - `var = StructName` -> Some("StructName")
///   - `Some(StructName` -> Some("StructName")
///   - `vec![StructName` -> Some("StructName")
///   - `Enum::Variant` -> Some("Enum::Variant")
///   - `if condition` -> Some("condition") (but not PascalCase)
fn extract_name_before_brace(before_brace: &str) -> Option<String> {
    let trimmed = before_brace.trim();
    
    if trimmed.is_empty() {
        return None;
    }
    
    // Check for assignment pattern: `var = StructName`
    if let Some(eq_pos) = trimmed.rfind('=') {
        // Make sure it's not == or !=
        let before_eq = &trimmed[..eq_pos];
        if !before_eq.ends_with('!') && !before_eq.ends_with('=') {
            let after_eq = trimmed[eq_pos + 1..].trim();
            if !after_eq.is_empty() {
                return Some(after_eq.to_string());
            }
        }
    }
    
    // Check for function call pattern: `Some(StructName` or `vec![StructName`
    // Find the last `(` or `[` and get what's after it
    let last_open = trimmed.rfind(|c| c == '(' || c == '[');
    if let Some(pos) = last_open {
        let after_open = trimmed[pos + 1..].trim();
        if !after_open.is_empty() {
            return Some(after_open.to_string());
        }
    }
    
    // Bare struct literal: just `StructName` or `Enum::Variant`
    // Get the last word/path
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if let Some(last) = words.last() {
        return Some(last.to_string());
    }
    
    None
}

/// Check if a string is PascalCase (starts with uppercase)
fn is_pascal_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    
    // Handle enum paths like Enum::Variant
    let name = if s.contains("::") {
        s.split("::").last().unwrap_or(s)
    } else {
        s
    };
    
    // CRITICAL FIX: Check if name is empty after split
    // This can happen with edge cases like "Foo::" where last() returns ""
    if name.is_empty() {
        return false;
    }
    
    // Must start with uppercase letter
    let first = match name.chars().next() {
        Some(c) => c,
        None => return false,
    };
    
    if !first.is_uppercase() {
        return false;
    }
    
    // Rest should be alphanumeric or underscore
    name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// NEW: Extract function parameters from signature
fn extract_function_params(line: &str) -> Option<Vec<(String, Option<String>)>> {
    // Find the parentheses
    let paren_start = line.find('(')?;
    let paren_end = line.find(')')?;
    
    if paren_end <= paren_start {
        return None;
    }
    
    let params_str = &line[paren_start + 1..paren_end];
    if params_str.trim().is_empty() {
        return Some(Vec::new());
    }
    
    let mut params = Vec::new();
    
    for param in params_str.split(',') {
        let param = param.trim();
        if param.is_empty() {
            continue;
        }
        
        // Parse "name Type" or "name: Type"
        let parts: Vec<&str> = if param.contains(':') {
            param.splitn(2, ':').collect()
        } else {
            param.split_whitespace().collect()
        };
        
        if parts.len() >= 2 {
            let name = parts[0].trim();
            let ty = parts[1..].join(" ").trim().to_string();
            params.push((name.to_string(), Some(ty)));
        } else if parts.len() == 1 {
            params.push((parts[0].to_string(), None));
        }
    }
    
    Some(params)
}

//=============================================================================
// TESTS
//=============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_scope_push_pop() {
        let mut stack = ScopeStack::new();
        assert_eq!(stack.depth(), 0);
        stack.push();
        assert_eq!(stack.depth(), 1);
        stack.pop();
        assert_eq!(stack.depth(), 0);
    }
    
    #[test]
    fn test_bare_block_shadows_outside_control_flow() {
        let source = "a = 10\n{\n    a = 20\n}";
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(source);
        
        assert!(analyzer.is_decl(0));
        assert!(analyzer.is_decl(2));
        assert!(!analyzer.needs_mut("a", 0));
    }
    
    #[test]
    fn test_while_loop_mutates() {
        let source = "i = 0\nwhile i < 3 {\n    i = i + 1\n}";
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(source);
        
        assert!(analyzer.is_decl(0));
        assert!(analyzer.is_mut(2));
        assert!(analyzer.needs_mut("i", 0));
    }
    
    #[test]
    fn test_same_scope_mutation() {
        let source = "a = 10\na = 20";
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(source);
        
        assert!(analyzer.is_decl(0));
        assert!(analyzer.is_mut(1));
        assert!(analyzer.needs_mut("a", 0));
    }
    
    #[test]
    fn test_outer_keyword_mutates_parent() {
        let source = "x = 1\n{\n    outer x = 3\n}";
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(source);
        
        assert!(analyzer.is_decl(0));
        assert!(analyzer.is_mut(2));
        assert!(analyzer.is_outer(2));
        assert!(analyzer.needs_mut("x", 0));
    }
    
    // NEW: Test HIR integration
    #[test]
    fn test_hir_binding_ids() {
        let mut stack = ScopeStack::new();
        stack.declare("x", Some("i32".to_string()), 0);
        stack.declare("y", Some("String".to_string()), 1);
        
        let x_id = stack.get_binding_id("x");
        let y_id = stack.get_binding_id("y");
        
        assert!(x_id.is_some());
        assert!(y_id.is_some());
        assert_ne!(x_id, y_id);
    }
    
    #[test]
    fn test_param_bindings() {
        let mut stack = ScopeStack::new();
        stack.declare_param("acc", Some("Account".to_string()), 0);
        stack.declare("x", Some("i32".to_string()), 1);
        
        let params = stack.get_param_bindings();
        assert_eq!(params.len(), 1);
        
        let acc_id = stack.get_binding_id("acc").unwrap();
        assert!(stack.is_param(acc_id));
        
        let x_id = stack.get_binding_id("x").unwrap();
        assert!(!stack.is_param(x_id));
    }
    
    #[test]
    fn test_closure_scope() {
        let mut stack = ScopeStack::new();
        stack.declare("x", Some("i32".to_string()), 0);
        
        assert!(!stack.in_closure());
        
        stack.push_closure();
        assert!(stack.in_closure());
        assert!(stack.is_current_closure());
        
        // Variable lookup should work across closure boundary
        assert!(stack.lookup("x").is_some());
        assert!(stack.crosses_closure_boundary("x"));
        
        stack.pop();
        assert!(!stack.in_closure());
    }
    
    #[test]
    fn test_extract_function_params() {
        let line = "fn transfer(acc Account, amount i64) effects(write acc) Account {";
        let params = extract_function_params(line).unwrap();
        
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].0, "acc");
        assert_eq!(params[1].0, "amount");
    }
}