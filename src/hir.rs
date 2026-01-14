//! RustS+ High-level Intermediate Representation (HIR)
//!
//! HIR is the result of name resolution and scope analysis on the AST.
//! It adds:
//! - Variable binding IDs (no more name-based lookup)
//! - Scope information
//! - Mutability tracking
//! - Outer keyword resolution
//!
//! ## Design
//!
//! Each variable reference is resolved to a `BindingId` which uniquely
//! identifies the declaration site. This allows us to:
//! - Track mutations precisely
//! - Detect shadowing
//! - Build data flow graphs for effects

use std::collections::{HashMap, HashSet};
use crate::ast::{
    Span, Ident, Type, Literal, BinOp, UnaryOp, EffectDecl,
};

// Re-export Spanned publicly so other modules can use it via crate::hir::Spanned
pub use crate::ast::Spanned;

//=============================================================================
// BINDING AND SCOPE
//=============================================================================

/// Unique identifier for a variable binding
/// NOTE: PartialOrd and Ord are required for use in BTreeSet in eir.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BindingId(pub u32);

impl BindingId {
    pub fn new(id: u32) -> Self {
        BindingId(id)
    }
}

/// Information about a variable binding
#[derive(Debug, Clone)]
pub struct BindingInfo {
    pub id: BindingId,
    pub name: String,
    pub ty: Option<Type>,
    pub mutable: bool,
    pub scope_depth: usize,
    pub decl_span: Span,
    /// Is this an `outer` variable (cross-scope mutation)?
    pub is_outer: bool,
    /// Is this a function parameter?
    pub is_param: bool,
}

/// Scope information
#[derive(Debug, Clone)]
pub struct Scope {
    pub depth: usize,
    pub bindings: HashMap<String, BindingId>,
    pub parent: Option<usize>, // Index into scope stack
    pub is_closure: bool,
    pub is_control_flow: bool,
}

impl Scope {
    pub fn new(depth: usize, parent: Option<usize>) -> Self {
        Scope {
            depth,
            bindings: HashMap::new(),
            parent,
            is_closure: false,
            is_control_flow: false,
        }
    }
}

/// Scope resolver - builds scope information during HIR construction
#[derive(Debug)]
pub struct ScopeResolver {
    scopes: Vec<Scope>,
    bindings: HashMap<BindingId, BindingInfo>,
    next_binding_id: u32,
    current_scope: usize,
}

impl ScopeResolver {
    pub fn new() -> Self {
        let root_scope = Scope::new(0, None);
        ScopeResolver {
            scopes: vec![root_scope],
            bindings: HashMap::new(),
            next_binding_id: 0,
            current_scope: 0,
        }
    }
    
    /// Push a new scope
    pub fn push_scope(&mut self) {
        let depth = self.scopes.len();
        let new_scope = Scope::new(depth, Some(self.current_scope));
        self.scopes.push(new_scope);
        self.current_scope = depth;
    }
    
    /// Push a closure scope (effects don't propagate automatically)
    pub fn push_closure_scope(&mut self) {
        self.push_scope();
        if let Some(scope) = self.scopes.last_mut() {
            scope.is_closure = true;
        }
    }
    
    /// Push a control flow scope (allows mutation of outer vars)
    pub fn push_control_flow_scope(&mut self) {
        self.push_scope();
        if let Some(scope) = self.scopes.last_mut() {
            scope.is_control_flow = true;
        }
    }
    
    /// Pop current scope
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            if let Some(scope) = self.scopes.get(self.current_scope) {
                if let Some(parent) = scope.parent {
                    self.current_scope = parent;
                }
            }
        }
    }
    
    /// Declare a new binding
    pub fn declare(&mut self, name: &str, ty: Option<Type>, mutable: bool, span: Span) -> BindingId {
        let id = BindingId::new(self.next_binding_id);
        self.next_binding_id += 1;
        
        let info = BindingInfo {
            id,
            name: name.to_string(),
            ty,
            mutable,
            scope_depth: self.current_scope,
            decl_span: span,
            is_outer: false,
            is_param: false,
        };
        
        self.bindings.insert(id, info);
        if let Some(scope) = self.scopes.get_mut(self.current_scope) {
            scope.bindings.insert(name.to_string(), id);
        }
        
        id
    }
    
    /// Declare a function parameter
    pub fn declare_param(&mut self, name: &str, ty: Option<Type>, span: Span) -> BindingId {
        let id = self.declare(name, ty, false, span);
        if let Some(info) = self.bindings.get_mut(&id) {
            info.is_param = true;
        }
        id
    }
    
    /// Look up a binding by name
    pub fn lookup(&self, name: &str) -> Option<BindingId> {
        let mut scope_idx = self.current_scope;
        loop {
            if let Some(scope) = self.scopes.get(scope_idx) {
                if let Some(&id) = scope.bindings.get(name) {
                    return Some(id);
                }
                if let Some(parent) = scope.parent {
                    scope_idx = parent;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        None
    }
    
    /// Look up a binding only in outer scopes (not current)
    pub fn lookup_in_outer(&self, name: &str) -> Option<BindingId> {
        if let Some(scope) = self.scopes.get(self.current_scope) {
            if let Some(parent_idx) = scope.parent {
                let mut scope_idx = parent_idx;
                loop {
                    if let Some(scope) = self.scopes.get(scope_idx) {
                        if let Some(&id) = scope.bindings.get(name) {
                            return Some(id);
                        }
                        if let Some(parent) = scope.parent {
                            scope_idx = parent;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }
        None
    }
    
    /// Get binding info
    pub fn get_binding(&self, id: BindingId) -> Option<&BindingInfo> {
        self.bindings.get(&id)
    }
    
    /// Get all bindings
    pub fn all_bindings(&self) -> &HashMap<BindingId, BindingInfo> {
        &self.bindings
    }
    
    /// Mark a binding as mutable
    pub fn mark_mutable(&mut self, id: BindingId) {
        if let Some(info) = self.bindings.get_mut(&id) {
            info.mutable = true;
        }
    }
    
    /// Check if we're inside a closure
    pub fn in_closure(&self) -> bool {
        self.scopes.iter().any(|s| s.is_closure)
    }
    
    /// Check if binding crosses a closure boundary
    pub fn crosses_closure(&self, id: BindingId) -> bool {
        if let Some(info) = self.bindings.get(&id) {
            let decl_depth = info.scope_depth;
            // Check if there's a closure scope between current and declaration
            for i in decl_depth..self.current_scope {
                if let Some(scope) = self.scopes.get(i) {
                    if scope.is_closure {
                        return true;
                    }
                }
            }
        }
        false
    }
    
    /// Get current scope depth
    pub fn current_depth(&self) -> usize {
        self.current_scope
    }
    
    /// Get all parameter bindings
    pub fn get_params(&self) -> Vec<BindingId> {
        self.bindings.iter()
            .filter(|(_, info)| info.is_param)
            .map(|(id, _)| *id)
            .collect()
    }
}

impl Default for ScopeResolver {
    fn default() -> Self {
        Self::new()
    }
}

//=============================================================================
// HIR EXPRESSIONS
//=============================================================================

/// Path for referencing items (e.g., `std::io::Write`)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Path {
    pub segments: Vec<Ident>,
}

impl Path {
    pub fn simple(name: &str) -> Self {
        Path {
            segments: vec![Ident { name: name.to_string() }],
        }
    }
}

impl std::fmt::Display for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.segments.iter().map(|s| s.name.as_str()).collect();
        write!(f, "{}", names.join("::"))
    }
}

/// HIR expression with resolved bindings
#[derive(Debug, Clone)]
pub enum HirExpr {
    /// Literal value
    Literal(Literal),
    
    /// Variable reference (resolved to binding)
    Var(BindingId),
    
    /// Field access: expr.field
    Field {
        base: Box<Spanned<HirExpr>>,
        field: Ident,
    },
    
    /// Index access: expr[index]
    Index {
        base: Box<Spanned<HirExpr>>,
        index: Box<Spanned<HirExpr>>,
    },
    
    /// Binary operation
    Binary {
        op: BinOp,
        left: Box<Spanned<HirExpr>>,
        right: Box<Spanned<HirExpr>>,
    },
    
    /// Unary operation
    Unary {
        op: UnaryOp,
        operand: Box<Spanned<HirExpr>>,
    },
    
    /// Function/method call
    Call {
        target: HirCallTarget,
        args: Vec<Spanned<HirExpr>>,
    },
    
    /// If expression
    If {
        condition: Box<Spanned<HirExpr>>,
        then_branch: Box<Spanned<HirBlock>>,
        else_branch: Option<Box<Spanned<HirBlock>>>,
    },
    
    /// Match expression
    Match {
        scrutinee: Box<Spanned<HirExpr>>,
        arms: Vec<HirMatchArm>,
    },
    
    /// Block expression
    Block(Box<Spanned<HirBlock>>),
    
    /// Closure
    Closure {
        params: Vec<(Ident, Option<Type>)>,
        body: Box<Spanned<HirExpr>>,
        captures: Vec<BindingId>,
    },
    
    /// Struct construction
    Struct {
        name: Path,
        fields: Vec<(Ident, Spanned<HirExpr>)>,
    },
    
    /// Array/Vec literal
    Array(Vec<Spanned<HirExpr>>),
    
    /// Tuple
    Tuple(Vec<Spanned<HirExpr>>),
    
    /// Reference: &expr or &mut expr
    Ref {
        mutable: bool,
        expr: Box<Spanned<HirExpr>>,
    },
    
    /// Dereference: *expr
    Deref(Box<Spanned<HirExpr>>),
    
    /// Range: start..end or start..=end
    Range {
        start: Option<Box<Spanned<HirExpr>>>,
        end: Option<Box<Spanned<HirExpr>>>,
        inclusive: bool,
    },
    
    /// Return (in expression position)
    Return(Option<Box<Spanned<HirExpr>>>),
    
    /// Break (in expression position)
    Break(Option<Box<Spanned<HirExpr>>>),
    
    /// Continue
    Continue,
}

/// Call target in HIR
#[derive(Debug, Clone)]
pub enum HirCallTarget {
    /// Direct function call
    Function(Path),
    /// Method call on expression
    Method {
        receiver: Box<Spanned<HirExpr>>,
        method: Ident,
    },
}

/// Match arm in HIR
#[derive(Debug, Clone)]
pub struct HirMatchArm {
    pub pattern: HirPattern,
    pub guard: Option<Spanned<HirExpr>>,
    pub body: Spanned<HirExpr>,
}

/// Pattern in HIR (simplified for now)
#[derive(Debug, Clone)]
pub enum HirPattern {
    Wildcard,
    Literal(Literal),
    Binding(BindingId),
    Tuple(Vec<HirPattern>),
    Struct {
        path: Path,
        fields: Vec<(Ident, HirPattern)>,
    },
}

//=============================================================================
// HIR STATEMENTS
//=============================================================================

/// HIR statement
#[derive(Debug, Clone)]
pub enum HirStmt {
    /// Let binding: let x = expr
    Let {
        binding: BindingId,
        ty: Option<Type>,
        init: Option<Spanned<HirExpr>>,
    },
    
    /// Expression statement
    Expr(Spanned<HirExpr>),
    
    /// Assignment: lhs = rhs
    Assign {
        target: Spanned<HirExpr>,
        value: Spanned<HirExpr>,
    },
    
    /// While loop
    While {
        condition: Spanned<HirExpr>,
        body: Spanned<HirBlock>,
    },
    
    /// For loop
    For {
        binding: BindingId,
        iter: Spanned<HirExpr>,
        body: Spanned<HirBlock>,
    },
    
    /// Loop
    Loop {
        body: Spanned<HirBlock>,
    },
}

/// HIR block - sequence of statements with optional trailing expression
#[derive(Debug, Clone)]
pub struct HirBlock {
    pub stmts: Vec<Spanned<HirStmt>>,
    pub expr: Option<Spanned<HirExpr>>,
}

//=============================================================================
// HIR ITEMS (Top-level declarations)
//=============================================================================

/// HIR function definition
#[derive(Debug, Clone)]
pub struct HirFnDef {
    pub name: Ident,
    pub params: Vec<(BindingId, Ident, Type)>,
    pub return_type: Option<Type>,
    pub effects: Vec<EffectDecl>,
    pub body: Spanned<HirBlock>,
    pub local_bindings: HashMap<BindingId, BindingInfo>,
}

/// HIR module - collection of items
#[derive(Debug, Clone)]
pub struct HirModule {
    pub functions: Vec<HirFnDef>,
}

//=============================================================================
// MUTATION ANALYSIS
//=============================================================================

/// Result of analyzing mutations in a function
#[derive(Debug, Clone)]
pub struct MutationAnalysis {
    /// Bindings that are directly mutated
    pub direct_mutations: HashSet<BindingId>,
    /// Bindings whose fields are mutated
    pub field_mutations: HashSet<BindingId>,
    /// Bindings taken as &mut
    pub ref_mut_uses: HashSet<BindingId>,
}

impl MutationAnalysis {
    pub fn new() -> Self {
        MutationAnalysis {
            direct_mutations: HashSet::new(),
            field_mutations: HashSet::new(),
            ref_mut_uses: HashSet::new(),
        }
    }
    
    /// Check if a binding is mutated in any way
    pub fn is_mutated(&self, id: BindingId) -> bool {
        self.direct_mutations.contains(&id) ||
        self.field_mutations.contains(&id) ||
        self.ref_mut_uses.contains(&id)
    }
    
    /// Merge another analysis into this one
    pub fn merge(&mut self, other: &MutationAnalysis) {
        self.direct_mutations.extend(&other.direct_mutations);
        self.field_mutations.extend(&other.field_mutations);
        self.ref_mut_uses.extend(&other.ref_mut_uses);
    }
}

impl Default for MutationAnalysis {
    fn default() -> Self {
        Self::new()
    }
}

//=============================================================================
// TESTS
//=============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_scope_resolver_basic() {
        let mut resolver = ScopeResolver::new();
        let span = Span { start: 0, end: 0, line: 1, col: 1 };
        
        let x = resolver.declare("x", None, false, span);
        assert!(resolver.lookup("x").is_some());
        assert_eq!(resolver.lookup("x"), Some(x));
        
        // y doesn't exist
        assert!(resolver.lookup("y").is_none());
    }
    
    #[test]
    fn test_scope_resolver_nested() {
        let mut resolver = ScopeResolver::new();
        let span = Span { start: 0, end: 0, line: 1, col: 1 };
        
        let x = resolver.declare("x", None, false, span);
        
        resolver.push_scope();
        let y = resolver.declare("y", None, false, span);
        
        // Both visible
        assert_eq!(resolver.lookup("x"), Some(x));
        assert_eq!(resolver.lookup("y"), Some(y));
        
        resolver.pop_scope();
        
        // x still visible, y is gone
        assert_eq!(resolver.lookup("x"), Some(x));
        assert!(resolver.lookup("y").is_none());
    }
    
    #[test]
    fn test_scope_resolver_shadowing() {
        let mut resolver = ScopeResolver::new();
        let span = Span { start: 0, end: 0, line: 1, col: 1 };
        
        let x1 = resolver.declare("x", None, false, span);
        
        resolver.push_scope();
        let x2 = resolver.declare("x", None, false, span);
        
        // Inner x shadows outer
        assert_eq!(resolver.lookup("x"), Some(x2));
        assert_ne!(x1, x2);
        
        // But outer exists in outer scope lookup
        assert_eq!(resolver.lookup_in_outer("x"), Some(x1));
        
        resolver.pop_scope();
        
        // Back to outer x
        assert_eq!(resolver.lookup("x"), Some(x1));
    }
    
    #[test]
    fn test_binding_id_ord() {
        let a = BindingId::new(1);
        let b = BindingId::new(2);
        let c = BindingId::new(1);
        
        assert!(a < b);
        assert!(b > a);
        assert!(a == c);
        assert!(a <= c);
        assert!(a >= c);
    }
}