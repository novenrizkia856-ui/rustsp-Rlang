//! RustS+ Effect Intermediate Representation (EIR)
//!
//! EIR is built from HIR and performs structural effect inference.
//! This is the core of RustS+'s Effect Ownership System.
//!
//! ## Effect Inference Rules (Formal)
//!
//! The effect inference is defined structurally over the HIR:
//!
//! ```text
//! Γ ⊢ e : τ | E   means "expression e has type τ and effects E under context Γ"
//!
//! LITERALS:
//!   Γ ⊢ n : int | ∅                    (integer literals have no effects)
//!   Γ ⊢ "s" : String | alloc           (string literals allocate)
//!   Γ ⊢ true/false : bool | ∅
//!
//! VARIABLES:
//!   Γ(x) = τ, param
//!   ────────────────
//!   Γ ⊢ x : τ | read(x)                (reading a parameter has read effect)
//!
//!   Γ(x) = τ, local
//!   ────────────────
//!   Γ ⊢ x : τ | ∅                      (reading a local has no effect)
//!
//! FIELD READ:
//!   Γ ⊢ e : τ | E,  τ has field f : σ
//!   ─────────────────────────────────
//!   Γ ⊢ e.f : σ | E                    (field read inherits base effects)
//!
//! FIELD WRITE:
//!   Γ ⊢ e₁ : τ | E₁,  Γ ⊢ e₂ : σ | E₂,  root(e₁) = x, param
//!   ────────────────────────────────────────────────────────
//!   Γ ⊢ e₁.f = e₂ : () | E₁ ∪ E₂ ∪ {write(x)}
//!
//! FUNCTION CALL:
//!   Γ ⊢ eᵢ : τᵢ | Eᵢ,  f has effects Ef
//!   ────────────────────────────────────
//!   Γ ⊢ f(e₁,...,eₙ) : τ | ⋃Eᵢ ∪ Ef
//!
//! BINARY OP:
//!   Γ ⊢ e₁ : τ | E₁,  Γ ⊢ e₂ : τ | E₂
//!   ─────────────────────────────────
//!   Γ ⊢ e₁ ⊕ e₂ : τ' | E₁ ∪ E₂
//!
//! CONDITIONAL:
//!   Γ ⊢ e₁ : bool | E₁,  Γ ⊢ e₂ : τ | E₂,  Γ ⊢ e₃ : τ | E₃
//!   ────────────────────────────────────────────────────────
//!   Γ ⊢ if e₁ { e₂ } else { e₃ } : τ | E₁ ∪ E₂ ∪ E₃
//!
//! CLOSURE:
//!   Γ, params ⊢ body : τ | E_body
//!   capture = E_body ∩ outer_params
//!   ────────────────────────────────
//!   Γ ⊢ |params| body : Fn | capture
//! ```

use std::collections::{HashMap, HashSet, BTreeSet};
use crate::ast::{Span, Spanned, Ident, Literal, EffectDecl};
use crate::hir::{
    BindingId, BindingInfo, HirExpr, HirStmt, HirBlock, HirFnDef,
    HirCallTarget, Path,
};

//=============================================================================
// EFFECT TYPES
//=============================================================================

/// Effect annotation from structural inference
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Effect {
    /// Read from a binding (parameter or captured variable)
    Read(BindingId),
    /// Write to a binding
    Write(BindingId),
    /// I/O effect
    Io,
    /// Memory allocation
    Alloc,
    /// May panic
    Panic,
    /// Calls a function with given effects
    Calls { func_name: String, effects: BTreeSet<Effect> },
}

impl Effect {
    /// Check if this is a propagatable effect (bubbles up to callers)
    pub fn is_propagatable(&self) -> bool {
        matches!(self, Effect::Io | Effect::Alloc | Effect::Panic)
    }
    
    /// Convert to string for display
    pub fn display(&self, bindings: &HashMap<BindingId, BindingInfo>) -> String {
        match self {
            Effect::Read(id) => {
                let name = bindings.get(id).map(|b| b.name.as_str()).unwrap_or("?");
                format!("read({})", name)
            }
            Effect::Write(id) => {
                let name = bindings.get(id).map(|b| b.name.as_str()).unwrap_or("?");
                format!("write({})", name)
            }
            Effect::Io => "io".to_string(),
            Effect::Alloc => "alloc".to_string(),
            Effect::Panic => "panic".to_string(),
            Effect::Calls { func_name, .. } => format!("calls({})", func_name),
        }
    }
    
    /// Convert from declared effect
    pub fn from_decl(decl: &EffectDecl, param_bindings: &HashMap<String, BindingId>) -> Option<Self> {
        match decl {
            EffectDecl::Read(name) => {
                param_bindings.get(&name.name).map(|id| Effect::Read(*id))
            }
            EffectDecl::Write(name) => {
                param_bindings.get(&name.name).map(|id| Effect::Write(*id))
            }
            EffectDecl::Io => Some(Effect::Io),
            EffectDecl::Alloc => Some(Effect::Alloc),
            EffectDecl::Panic => Some(Effect::Panic),
        }
    }
}

//=============================================================================
// EFFECT SET
//=============================================================================

/// A set of effects with convenience methods
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectSet {
    effects: BTreeSet<Effect>,
}

impl EffectSet {
    pub fn new() -> Self {
        EffectSet { effects: BTreeSet::new() }
    }
    
    pub fn empty() -> Self {
        Self::new()
    }
    
    pub fn singleton(effect: Effect) -> Self {
        let mut set = Self::new();
        set.insert(effect);
        set
    }
    
    pub fn insert(&mut self, effect: Effect) {
        self.effects.insert(effect);
    }
    
    pub fn extend(&mut self, other: &EffectSet) {
        self.effects.extend(other.effects.clone());
    }
    
    pub fn union(&self, other: &EffectSet) -> EffectSet {
        let mut result = self.clone();
        result.extend(other);
        result
    }
    
    pub fn contains(&self, effect: &Effect) -> bool {
        self.effects.contains(effect)
    }
    
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
    
    pub fn len(&self) -> usize {
        self.effects.len()
    }
    
    pub fn iter(&self) -> impl Iterator<Item = &Effect> {
        self.effects.iter()
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
    
    pub fn has_read(&self, id: BindingId) -> bool {
        self.effects.contains(&Effect::Read(id))
    }
    
    pub fn has_write(&self, id: BindingId) -> bool {
        self.effects.contains(&Effect::Write(id))
    }
    
    /// Get all propagatable effects
    pub fn propagatable(&self) -> EffectSet {
        let mut result = EffectSet::new();
        for e in &self.effects {
            if e.is_propagatable() {
                result.insert(e.clone());
            }
        }
        result
    }
    
    /// Get effects not in the other set
    pub fn difference(&self, other: &EffectSet) -> EffectSet {
        let mut result = EffectSet::new();
        for e in &self.effects {
            if !other.contains(e) {
                result.insert(e.clone());
            }
        }
        result
    }
    
    /// Check if this is a subset of other
    pub fn is_subset_of(&self, other: &EffectSet) -> bool {
        self.effects.is_subset(&other.effects)
    }
    
    pub fn into_inner(self) -> BTreeSet<Effect> {
        self.effects
    }
}

impl FromIterator<Effect> for EffectSet {
    fn from_iter<I: IntoIterator<Item = Effect>>(iter: I) -> Self {
        EffectSet {
            effects: iter.into_iter().collect(),
        }
    }
}

//=============================================================================
// EFFECT CONTEXT
//=============================================================================

/// Context for effect inference
#[derive(Debug)]
pub struct EffectContext {
    /// Mapping from binding IDs to info
    bindings: HashMap<BindingId, BindingInfo>,
    /// Function signatures (name -> declared effects)
    function_effects: HashMap<String, EffectSet>,
    /// Current function's parameters (for read/write detection)
    current_params: HashSet<BindingId>,
    /// Known IO-performing functions
    io_functions: HashSet<String>,
    /// Known allocating functions
    alloc_functions: HashSet<String>,
    /// Known panicking functions
    panic_functions: HashSet<String>,
}

impl EffectContext {
    pub fn new(bindings: HashMap<BindingId, BindingInfo>) -> Self {
        let mut io_functions = HashSet::new();
        let mut alloc_functions = HashSet::new();
        let mut panic_functions = HashSet::new();
        
        // Standard library I/O
        for name in &["println", "print", "eprintln", "eprint", "writeln", "write"] {
            io_functions.insert(name.to_string());
        }
        
        // Standard library allocations
        for name in &["Vec::new", "Vec::with_capacity", "String::new", "String::from",
                      "Box::new", "Rc::new", "Arc::new", "HashMap::new", "HashSet::new",
                      "vec", "to_string", "to_owned", "clone", "collect"] {
            alloc_functions.insert(name.to_string());
        }
        
        // Standard library panic
        for name in &["panic", "unwrap", "expect", "assert", "assert_eq", "assert_ne",
                      "unreachable", "unimplemented", "todo"] {
            panic_functions.insert(name.to_string());
        }
        
        EffectContext {
            bindings,
            function_effects: HashMap::new(),
            current_params: HashSet::new(),
            io_functions,
            alloc_functions,
            panic_functions,
        }
    }
    
    pub fn register_function(&mut self, name: &str, effects: EffectSet) {
        self.function_effects.insert(name.to_string(), effects);
    }
    
    pub fn enter_function(&mut self, params: &[BindingId]) {
        self.current_params = params.iter().cloned().collect();
    }
    
    pub fn exit_function(&mut self) {
        self.current_params.clear();
    }
    
    pub fn is_param(&self, id: BindingId) -> bool {
        self.current_params.contains(&id)
    }
    
    pub fn get_binding(&self, id: BindingId) -> Option<&BindingInfo> {
        self.bindings.get(&id)
    }
    
    pub fn get_function_effects(&self, name: &str) -> Option<&EffectSet> {
        self.function_effects.get(name)
    }
    
    pub fn is_io_function(&self, name: &str) -> bool {
        self.io_functions.contains(name)
    }
    
    pub fn is_alloc_function(&self, name: &str) -> bool {
        self.alloc_functions.contains(name)
    }
    
    pub fn is_panic_function(&self, name: &str) -> bool {
        self.panic_functions.contains(name)
    }
}

//=============================================================================
// EFFECT INFERENCE ENGINE
//=============================================================================

/// The main effect inference engine
#[derive(Debug)]
pub struct EffectInference<'a> {
    ctx: &'a EffectContext,
}

impl<'a> EffectInference<'a> {
    pub fn new(ctx: &'a EffectContext) -> Self {
        EffectInference { ctx }
    }
    
    /// Infer effects for an expression
    pub fn infer_expr(&self, expr: &Spanned<HirExpr>) -> EffectSet {
        match &expr.node {
            // LITERAL RULES
            HirExpr::Literal(lit) => self.infer_literal(lit),
            
            // VARIABLE RULE
            HirExpr::Var(id) => self.infer_var(*id),
            
            // BINARY OP: E₁ ∪ E₂
            HirExpr::Binary { left, right, .. } => {
                let e1 = self.infer_expr(left);
                let e2 = self.infer_expr(right);
                e1.union(&e2)
            }
            
            // UNARY OP
            HirExpr::Unary { operand, .. } => self.infer_expr(operand),
            
            // FIELD READ: inherits base effects
            HirExpr::Field { base, .. } => self.infer_expr(base),
            
            // INDEX: base ∪ index effects
            HirExpr::Index { base, index } => {
                let e1 = self.infer_expr(base);
                let e2 = self.infer_expr(index);
                e1.union(&e2)
            }
            
            // FUNCTION CALL
            HirExpr::Call { target, args } => self.infer_call(target, args),
            
            // STRUCT LITERAL: alloc + field effects
            HirExpr::Struct { fields, .. } => {
                let mut effects = EffectSet::singleton(Effect::Alloc);
                for (_, e) in fields {
                    effects.extend(&self.infer_expr(e));
                }
                effects
            }
            
            // TUPLE/ARRAY: alloc + element effects
            HirExpr::Tuple(elems) | HirExpr::Array(elems) => {
                let mut effects = EffectSet::singleton(Effect::Alloc);
                for e in elems {
                    effects.extend(&self.infer_expr(e));
                }
                effects
            }
            
            // IF: cond ∪ then ∪ else
            HirExpr::If { condition, then_branch, else_branch } => {
                let mut effects = self.infer_expr(condition);
                effects.extend(&self.infer_block(then_branch));
                if let Some(else_block) = else_branch {
                    effects.extend(&self.infer_block(else_block));
                }
                effects
            }
            
            // MATCH: scrutinee ∪ all arms
            HirExpr::Match { scrutinee, arms } => {
                let mut effects = self.infer_expr(scrutinee);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        effects.extend(&self.infer_expr(guard));
                    }
                    effects.extend(&self.infer_expr(&arm.body));
                }
                effects
            }
            
            // BLOCK
            HirExpr::Block(block) => self.infer_block(block),
            
            // CLOSURE: captures effects from outer scope
            HirExpr::Closure { body, captures, .. } => {
                let body_effects = self.infer_expr(body);
                // Filter to only effects that involve captured variables
                let mut closure_effects = EffectSet::new();
                for effect in body_effects.iter() {
                    match effect {
                        Effect::Read(id) if captures.contains(id) => {
                            closure_effects.insert(effect.clone());
                        }
                        Effect::Write(id) if captures.contains(id) => {
                            closure_effects.insert(effect.clone());
                        }
                        Effect::Io | Effect::Alloc | Effect::Panic => {
                            closure_effects.insert(effect.clone());
                        }
                        _ => {}
                    }
                }
                closure_effects
            }
            
            // RETURN/BREAK: propagate inner effects
            HirExpr::Return(Some(e)) | HirExpr::Break(Some(e)) => {
                self.infer_expr(e)
            }
            HirExpr::Return(None) | HirExpr::Break(None) | HirExpr::Continue => {
                EffectSet::empty()
            }
            
            // RANGE
            HirExpr::Range { start, end, .. } => {
                let mut effects = EffectSet::new();
                if let Some(s) = start {
                    effects.extend(&self.infer_expr(s));
                }
                if let Some(e) = end {
                    effects.extend(&self.infer_expr(e));
                }
                effects
            }
            
            // REFERENCE: propagate inner effects
            HirExpr::Ref { expr, .. } => self.infer_expr(expr),
            
            // DEREFERENCE: propagate inner effects
            HirExpr::Deref(expr) => self.infer_expr(expr),
        }
    }
    
    /// Infer effects from a literal
    fn infer_literal(&self, lit: &Literal) -> EffectSet {
        match lit {
            // String literals allocate
            Literal::String(_) => EffectSet::singleton(Effect::Alloc),
            // Other literals have no effects
            _ => EffectSet::empty(),
        }
    }
    
    /// Infer effects from variable reference
    fn infer_var(&self, id: BindingId) -> EffectSet {
        // Only parameters contribute to read effects
        if self.ctx.is_param(id) {
            EffectSet::singleton(Effect::Read(id))
        } else {
            EffectSet::empty()
        }
    }
    
    /// Infer effects from function call
    fn infer_call(&self, target: &HirCallTarget, args: &[Spanned<HirExpr>]) -> EffectSet {
        let mut effects = EffectSet::new();
        
        // Add argument effects
        for arg in args {
            effects.extend(&self.infer_expr(arg));
        }
        
        // Add function effects
        match target {
            HirCallTarget::Function(path) => {
                let func_name = path.to_string();
                
                // Check for known effect-producing functions
                if self.ctx.is_io_function(&func_name) {
                    effects.insert(Effect::Io);
                }
                if self.ctx.is_alloc_function(&func_name) {
                    effects.insert(Effect::Alloc);
                }
                if self.ctx.is_panic_function(&func_name) {
                    effects.insert(Effect::Panic);
                }
                
                // Check registered functions
                if let Some(func_effects) = self.ctx.get_function_effects(&func_name) {
                    effects.extend(func_effects);
                }
            }
            HirCallTarget::Method { receiver, method } => {
                // Method call - add receiver effects plus method-specific effects
                effects.extend(&self.infer_expr(receiver));
                effects.extend(&self.infer_method_effects(method));
            }
        }
        
        effects
    }
    
    /// Infer effects from method name
    fn infer_method_effects(&self, method: &Ident) -> EffectSet {
        let mut effects = EffectSet::new();
        let method_name = &method.name;
        
        // I/O methods
        if ["read", "write", "flush", "read_line", "read_to_string"].contains(&method_name.as_str()) {
            effects.insert(Effect::Io);
        }
        
        // Allocating methods
        if ["to_string", "to_owned", "clone", "collect", "into_iter", "push", "insert"].contains(&method_name.as_str()) {
            effects.insert(Effect::Alloc);
        }
        
        // Panicking methods
        if method_name == "unwrap" || method_name == "expect" {
            effects.insert(Effect::Panic);
        }
        
        effects
    }
    
    /// Add write effect if target is a parameter
    fn add_write_effect(&self, effects: &mut EffectSet, target: &Spanned<HirExpr>) {
        if let Some(root_id) = self.find_root_binding(target) {
            if self.ctx.is_param(root_id) {
                effects.insert(Effect::Write(root_id));
            }
        }
    }
    
    /// Find the root binding of a place expression
    fn find_root_binding(&self, expr: &Spanned<HirExpr>) -> Option<BindingId> {
        match &expr.node {
            HirExpr::Var(id) => Some(*id),
            HirExpr::Field { base, .. } => self.find_root_binding(base),
            HirExpr::Index { base, .. } => self.find_root_binding(base),
            _ => None,
        }
    }
    
    /// Infer effects for a block
    pub fn infer_block(&self, block: &Spanned<HirBlock>) -> EffectSet {
        let mut effects = EffectSet::new();
        
        for stmt in &block.node.stmts {
            match &stmt.node {
                HirStmt::Let { init: Some(e), .. } => {
                    effects.extend(&self.infer_expr(e));
                }
                HirStmt::Let { init: None, .. } => {
                    // No init, no effects
                }
                HirStmt::Expr(e) => {
                    effects.extend(&self.infer_expr(e));
                }
                HirStmt::Assign { target, value } => {
                    effects.extend(&self.infer_expr(value));
                    self.add_write_effect(&mut effects, target);
                }
                HirStmt::While { condition, body } => {
                    effects.extend(&self.infer_expr(condition));
                    effects.extend(&self.infer_block(body));
                }
                HirStmt::For { iter, body, .. } => {
                    effects.extend(&self.infer_expr(iter));
                    effects.extend(&self.infer_block(body));
                }
                HirStmt::Loop { body } => {
                    effects.extend(&self.infer_block(body));
                }
            }
        }
        
        if let Some(e) = &block.node.expr {
            effects.extend(&self.infer_expr(e));
        }
        
        effects
    }
}

//=============================================================================
// EFFECT VALIDATION
//=============================================================================

/// Result of effect validation
#[derive(Debug)]
pub struct EffectValidationResult {
    pub errors: Vec<EffectError>,
}

impl EffectValidationResult {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// An effect violation error
#[derive(Debug)]
pub struct EffectError {
    pub kind: EffectErrorKind,
    pub span: Span,
    pub function_name: String,
}

/// Kind of effect error
#[derive(Debug)]
pub enum EffectErrorKind {
    /// Undeclared effect (RSPL300)
    UndeclaredEffect {
        effect: Effect,
    },
    /// Effect not propagated to caller (RSPL301)
    MissingPropagation {
        callee: String,
        effect: Effect,
    },
    /// Pure function calling effectful (RSPL302)
    PureCallingEffectful {
        callee: String,
    },
    /// Duplicate write to same binding (RSPL315)
    DuplicateWrite {
        binding_id: BindingId,
        binding_name: String,
    },
    /// Effect leak to closure
    EffectLeak {
        effect: Effect,
    },
}

/// Effect validator
#[derive(Debug)]
pub struct EffectValidator<'a> {
    ctx: &'a EffectContext,
    bindings: &'a HashMap<BindingId, BindingInfo>,
}

impl<'a> EffectValidator<'a> {
    pub fn new(ctx: &'a EffectContext, bindings: &'a HashMap<BindingId, BindingInfo>) -> Self {
        EffectValidator { ctx, bindings }
    }
    
    /// Validate a function's effects
    pub fn validate_function(
        &self,
        func: &HirFnDef,
        detected: &EffectSet,
        declared: &EffectSet,
    ) -> Vec<EffectError> {
        let mut errors = Vec::new();
        let func_name = func.name.name.clone();
        
        // RSPL300: Check for undeclared effects
        for effect in detected.iter() {
            if !self.is_effect_declared(effect, declared) {
                // Skip read effects for non-param bindings
                if let Effect::Read(id) = effect {
                    if !self.ctx.is_param(*id) {
                        continue;
                    }
                }
                
                errors.push(EffectError {
                    kind: EffectErrorKind::UndeclaredEffect {
                        effect: effect.clone(),
                    },
                    span: func.body.span,
                    function_name: func_name.clone(),
                });
            }
        }
        
        // RSPL302: Check pure function calling effectful
        if declared.is_empty() && !detected.is_empty() {
            // Function is declared pure but has effects
            // This should already be caught by RSPL300
        }
        
        errors
    }
    
    /// Check if an effect is covered by declarations
    fn is_effect_declared(&self, effect: &Effect, declared: &EffectSet) -> bool {
        // Direct match
        if declared.contains(effect) {
            return true;
        }
        
        // Check equivalent effects
        match effect {
            Effect::Read(id) => {
                // Check if any declared read matches this binding
                for decl in declared.iter() {
                    if let Effect::Read(decl_id) = decl {
                        if decl_id == id {
                            return true;
                        }
                    }
                }
            }
            Effect::Write(id) => {
                for decl in declared.iter() {
                    if let Effect::Write(decl_id) = decl {
                        if decl_id == id {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }
        
        false
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
    /// Function -> Its detected effects
    function_effects: HashMap<String, EffectSet>,
}

impl EffectDependencyGraph {
    pub fn new() -> Self {
        EffectDependencyGraph::default()
    }
    
    pub fn add_function(&mut self, name: &str, effects: EffectSet) {
        self.call_graph.entry(name.to_string()).or_default();
        self.function_effects.insert(name.to_string(), effects);
    }
    
    pub fn add_call(&mut self, caller: &str, callee: &str) {
        self.call_graph.entry(caller.to_string())
            .or_default()
            .push(callee.to_string());
    }
    
    /// Compute transitive effects for a function
    pub fn transitive_effects(&self, func: &str) -> EffectSet {
        let mut visited = HashSet::new();
        let mut effects = EffectSet::new();
        self.collect_effects(func, &mut visited, &mut effects);
        effects
    }
    
    fn collect_effects(&self, func: &str, visited: &mut HashSet<String>, effects: &mut EffectSet) {
        if visited.contains(func) {
            return;
        }
        visited.insert(func.to_string());
        
        // Add this function's effects
        if let Some(func_effects) = self.function_effects.get(func) {
            effects.extend(func_effects);
        }
        
        // Recurse to callees
        if let Some(callees) = self.call_graph.get(func) {
            for callee in callees {
                self.collect_effects(callee, visited, effects);
            }
        }
    }
    
    /// Check for effect propagation violations (RSPL301)
    pub fn check_propagation(&self) -> Vec<(String, String, EffectSet)> {
        let mut violations = Vec::new();
        
        for (caller, callees) in &self.call_graph {
            let caller_effects = self.function_effects.get(caller)
                .cloned()
                .unwrap_or_default();
            
            for callee in callees {
                if let Some(callee_effects) = self.function_effects.get(callee) {
                    // Check if caller has all propagatable effects from callee
                    let propagatable = callee_effects.propagatable();
                    let missing = propagatable.difference(&caller_effects);
                    
                    if !missing.is_empty() {
                        violations.push((
                            caller.clone(),
                            callee.clone(),
                            missing,
                        ));
                    }
                }
            }
        }
        
        violations
    }
}

//=============================================================================
// TESTS
//=============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_effect_set() {
        let mut set = EffectSet::new();
        set.insert(Effect::Io);
        set.insert(Effect::Alloc);
        
        assert!(set.has_io());
        assert!(set.has_alloc());
        assert!(!set.has_panic());
        assert_eq!(set.len(), 2);
    }
    
    #[test]
    fn test_effect_set_union() {
        let mut set1 = EffectSet::new();
        set1.insert(Effect::Io);
        
        let mut set2 = EffectSet::new();
        set2.insert(Effect::Alloc);
        
        let union = set1.union(&set2);
        assert!(union.has_io());
        assert!(union.has_alloc());
        assert_eq!(union.len(), 2);
    }
    
    #[test]
    fn test_effect_set_difference() {
        let mut declared = EffectSet::new();
        declared.insert(Effect::Io);
        
        let mut detected = EffectSet::new();
        detected.insert(Effect::Io);
        detected.insert(Effect::Alloc);
        
        let undeclared = detected.difference(&declared);
        assert!(!undeclared.has_io());
        assert!(undeclared.has_alloc());
    }
    
    #[test]
    fn test_propagatable_effects() {
        let mut set = EffectSet::new();
        set.insert(Effect::Io);
        set.insert(Effect::Read(BindingId::new(0)));
        set.insert(Effect::Write(BindingId::new(1)));
        
        let prop = set.propagatable();
        assert!(prop.has_io());
        assert!(!prop.has_read(BindingId::new(0)));
        assert!(!prop.has_write(BindingId::new(1)));
    }
}