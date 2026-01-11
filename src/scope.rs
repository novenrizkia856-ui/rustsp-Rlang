//! Scope management for RustS+ compiler
//! 
//! Handles:
//! - Scope stack (push on `{`, pop on `}`)
//! - Variable lookup (innermost to outermost)
//! - Shadowing detection (type change = new declaration)
//! - Mutation tracking (same type in inner scope = mutate parent)
//! - `outer` keyword for explicit cross-scope mutation

use std::collections::HashMap;

/// A variable within a scope
#[derive(Debug, Clone)]
pub struct ScopedVar {
    pub name: String,
    pub var_type: Option<String>,
    pub line: usize,
}

/// A single scope level
#[derive(Debug, Clone)]
pub struct Scope {
    pub level: usize,
    pub vars: HashMap<String, ScopedVar>,
    /// Is this a bare block (true) or control flow block (false)?
    /// Bare blocks use shadow semantics, control flow uses mutation
    pub is_bare_block: bool,
}

impl Scope {
    pub fn new(level: usize) -> Self {
        Scope {
            level,
            vars: HashMap::new(),
            is_bare_block: false,
        }
    }
    
    pub fn new_bare(level: usize) -> Self {
        Scope {
            level,
            vars: HashMap::new(),
            is_bare_block: true,
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
}

impl ScopeStack {
    pub fn new() -> Self {
        ScopeStack {
            scopes: vec![Scope::new(0)], // Start with root scope (not bare)
            mut_needed: HashMap::new(),
            control_flow_depth: 0,
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
    
    /// Pop current scope
    pub fn pop(&mut self) {
        if self.scopes.len() > 1 {
            if let Some(scope) = self.scopes.last() {
                // Decrement control flow depth if this was a control flow scope
                if !scope.is_bare_block && self.control_flow_depth > 0 {
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
    
    /// Check if we're inside any control flow
    pub fn in_control_flow(&self) -> bool {
        self.control_flow_depth > 0
    }
    
    /// Declare a variable in current scope
    pub fn declare(&mut self, name: &str, var_type: Option<String>, line: usize) {
        let var = ScopedVar {
            name: name.to_string(),
            var_type,
            line,
        };
        if let Some(scope) = self.scopes.last_mut() {
            scope.vars.insert(name.to_string(), var);
        }
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
    }
    
    /// Check if a variable needs mut
    pub fn needs_mut(&self, name: &str, declaration_line: usize) -> bool {
        self.mut_needed.get(&(name.to_string(), declaration_line)).copied().unwrap_or(false)
    }
}

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
}

impl ScopeAnalyzer {
    pub fn new() -> Self {
        ScopeAnalyzer {
            mut_vars: HashMap::new(),
            decl_lines: HashMap::new(),
            mut_lines: HashMap::new(),
            outer_lines: HashMap::new(),
        }
    }
    
    /// Analyze source and build scope information
    pub fn analyze(&mut self, source: &str) {
        let lines: Vec<&str> = source.lines().collect();
        let mut stack = ScopeStack::new();
        
        // Track if previous non-empty content was a control flow keyword
        let mut pending_control_flow = false;
        
        for (line_num, line) in lines.iter().enumerate() {
            let clean = strip_comment(line);
            let trimmed = clean.trim();
            
            // Check if this line is or contains control flow OR function definition
            // Function bodies are not "bare blocks" but are also not control flow
            let is_control_flow_line = trimmed.starts_with("if ")
                || trimmed.starts_with("} else")
                || trimmed.starts_with("else")
                || trimmed.starts_with("while ")
                || trimmed.starts_with("for ")
                || trimmed.starts_with("loop")
                || trimmed.starts_with("match ")
                || trimmed.contains("} else")  // Handle `} else {` on same line
                || trimmed.contains("else {"); // Handle `else {`
            
            // Function definitions open a normal (non-bare) scope
            let is_function_def = trimmed.starts_with("fn ") 
                || trimmed.starts_with("pub fn ");
            
            // Count braces
            let opens = trimmed.matches('{').count();
            let closes = trimmed.matches('}').count();
            
            // Pop for leading `}` BEFORE checking control flow
            // This handles `} else {` correctly
            let leading_closes = if trimmed.starts_with('}') {
                // Count consecutive leading `}`
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
            
            // Parse assignment AFTER handling leading closes
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
                        // Outer mutation: mark original decl as needing mut
                        stack.mark_mut(&var_name, decl_line);
                        self.mut_vars.insert((var_name.clone(), decl_line), true);
                        self.mut_lines.insert(line_num, (var_name, decl_line));
                        self.outer_lines.insert(line_num, true);
                    }
                    AssignKind::OuterError(msg) => {
                        // Log error - variable not found in parent scope
                        eprintln!("// COMPILE ERROR at line {}: {}", line_num + 1, msg);
                    }
                }
            }
            
            // Push for `{` - determine if bare or control flow or function
            for _ in 0..opens {
                if is_control_flow_line || pending_control_flow {
                    stack.push(); // Control flow block - allows mutation
                } else if is_function_def {
                    // Function body - push normal scope (not control flow, not bare)
                    // This allows mutation within function scope
                    let new_level = stack.scopes.len();
                    stack.scopes.push(Scope::new(new_level));
                } else {
                    stack.push_bare(); // Bare block - shadow semantics (if not in control flow)
                }
            }
            
            // Track pending control flow for lines like:
            // if cond
            // {
            if (is_control_flow_line || is_function_def) && opens == 0 {
                pending_control_flow = is_control_flow_line; // Only actual control flow carries over
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
}

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
    
    // CRITICAL: Handle `mut` keyword prefix - strip it but remember we saw it
    // `mut x = 10` should be treated as a declaration of `x`
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
        // BARE BLOCK outside control flow: shadow
        let source = "a = 10\n{\n    a = 20\n}";
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(source);
        
        assert!(analyzer.is_decl(0));  // a = 10 is declaration
        assert!(analyzer.is_decl(2));  // a = 20 is shadow in bare block
        assert!(!analyzer.needs_mut("a", 0)); // outer 'a' NOT mutated
    }
    
    #[test]
    fn test_while_loop_mutates() {
        // WHILE BLOCK: assignment to parent var with same type = mutation
        let source = "i = 0\nwhile i < 3 {\n    i = i + 1\n}";
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(source);
        
        assert!(analyzer.is_decl(0));  // i = 0 is declaration
        assert!(analyzer.is_mut(2));   // i = i + 1 is MUTATION (control flow)
        assert!(analyzer.needs_mut("i", 0)); // 'i' needs mut
    }
    
    #[test]
    fn test_same_scope_mutation() {
        // Mutation in SAME scope
        let source = "a = 10\na = 20";
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(source);
        
        assert!(analyzer.is_decl(0));  // a = 10 is declaration
        assert!(analyzer.is_mut(1));   // a = 20 is mutation (same scope)
        assert!(analyzer.needs_mut("a", 0)); // 'a' needs mut
    }
    
    #[test]
    fn test_if_block_mutates() {
        // IF BLOCK: assignment to parent var with same type = mutation
        let source = "x = 0\nif true {\n    x = 10\n}";
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(source);
        
        assert!(analyzer.is_decl(0));  // x = 0 is declaration
        assert!(analyzer.is_mut(2));   // x = 10 is MUTATION (control flow)
        assert!(analyzer.needs_mut("x", 0)); // 'x' needs mut
    }
    
    #[test]
    fn test_bare_block_inside_while_still_mutates() {
        // Bare block INSIDE control flow should still allow mutation
        let source = "total = 0\nwhile true {\n    {\n        total = total + 1\n    }\n}";
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(source);
        
        assert!(analyzer.is_decl(0));  // total = 0 is declaration
        assert!(analyzer.is_mut(3));   // total = total + 1 is MUTATION (inside control flow)
        assert!(analyzer.needs_mut("total", 0));
    }
    
    #[test]
    fn test_nested_bare_blocks_outside_control_flow() {
        // Nested bare blocks outside control flow should shadow
        let source = "a = 1\n{\n    a = 2\n    {\n        a = 3\n    }\n}";
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(source);
        
        assert!(analyzer.is_decl(0));  // outer a = 1
        assert!(analyzer.is_decl(2));  // inner a = 2 (shadows outer)
        assert!(analyzer.is_decl(4));  // innermost a = 3 (shadows inner)
        assert!(!analyzer.needs_mut("a", 0)); // No mutation
    }
    
    #[test]
    fn test_type_change_always_shadows() {
        // Different type = always shadow, even in control flow
        let source = "a = 10\nif true {\n    a = \"hello\"\n}";
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(source);
        
        assert!(analyzer.is_decl(0));  // a = 10 (i32)
        assert!(analyzer.is_decl(2));  // a = "hello" (String) - shadow
        assert!(!analyzer.needs_mut("a", 0)); // Not mutated
    }
    
    // ========== NEW TESTS FOR `outer` KEYWORD ==========
    
    #[test]
    fn test_outer_keyword_mutates_parent() {
        // `outer` keyword should mutate parent scope variable
        let source = "x = 1\n{\n    outer x = 3\n}";
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(source);
        
        assert!(analyzer.is_decl(0));  // x = 1 is declaration
        assert!(analyzer.is_mut(2));   // outer x = 3 is MUTATION (not shadow!)
        assert!(analyzer.is_outer(2)); // marked as outer
        assert!(analyzer.needs_mut("x", 0)); // 'x' needs mut
    }
    
    #[test]
    fn test_outer_vs_regular_in_bare_block() {
        // Regular assignment shadows, outer mutates
        let source = "x = 1\n{\n    x = 2\n}\n{\n    outer x = 3\n}";
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(source);
        
        assert!(analyzer.is_decl(0));  // x = 1 is declaration
        assert!(analyzer.is_decl(2));  // x = 2 is shadow (bare block, no outer)
        assert!(analyzer.is_mut(5));   // outer x = 3 is MUTATION
        assert!(analyzer.is_outer(5)); // marked as outer
        assert!(analyzer.needs_mut("x", 0)); // 'x' needs mut because of outer
    }
    
    #[test]
    fn test_outer_nested_blocks() {
        // outer should work through multiple nesting levels
        let source = "sum = 0\n{\n    outer sum = sum + 1\n    {\n        outer sum = sum + 2\n    }\n}";
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(source);
        
        assert!(analyzer.is_decl(0));  // sum = 0 is declaration
        assert!(analyzer.is_mut(2));   // outer sum = sum + 1 is mutation
        assert!(analyzer.is_outer(2)); // marked as outer
        assert!(analyzer.is_mut(4));   // outer sum = sum + 2 is mutation
        assert!(analyzer.is_outer(4)); // marked as outer
        assert!(analyzer.needs_mut("sum", 0)); // 'sum' needs mut
    }
    
    #[test]
    fn test_outer_lookup_in_parent() {
        let mut stack = ScopeStack::new();
        stack.declare("x", Some("i32".to_string()), 0);
        stack.push_bare();
        
        // x should be found in parent
        assert!(stack.lookup_in_parent("x").is_some());
        
        // y doesn't exist
        assert!(stack.lookup_in_parent("y").is_none());
    }
}