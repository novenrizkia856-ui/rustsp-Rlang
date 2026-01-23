//! Type Environment for Type-Driven Effect Inference
//!
//! This module provides the infrastructure for inferring effects from types
//! rather than from string pattern matching.
//!
//! ## Design
//!
//! ```text
//! BEFORE (Pattern-Based):
//!   fn detect_alloc(line: &str) -> bool {
//!       line.contains("Vec::new") || line.contains("Box::new")  // Fragile!
//!   }
//!
//! AFTER (Type-Driven):
//!   fn infer_effects(expr: &HirExpr, type_env: &TypeEnv) -> EffectSet {
//!       match expr {
//!           HirExpr::Call { func, .. } => {
//!               let func_type = type_env.lookup(func);
//!               func_type.effect_signature()  // From type, not string!
//!           }
//!       }
//!   }
//! ```
//!
//! ## Key Types
//!
//! - `EffectSignature`: Effect signature attached to function types
//! - `FunctionType`: Function type with parameters, return type, and effects
//! - `TypeEnv`: Type environment for type and effect lookups
//! - `TypeDrivenInference`: Inference engine using types instead of patterns
//!
//! ## Inference Rules (Formal)
//!
//! ```text
//! Γ ⊢ e : τ | E   means "expression e has type τ and effects E under context Γ"
//!
//! VARIABLE (Parameter):
//!   Γ(x) = τ, is_param=true
//!   ─────────────────────────
//!   Γ ⊢ x : τ | read(x)
//!
//! FUNCTION CALL:
//!   Γ ⊢ f : (τ₁,...,τₙ) → τ | Ef
//!   Γ ⊢ eᵢ : τᵢ | Eᵢ
//!   ──────────────────────────────
//!   Γ ⊢ f(e₁,...,eₙ) : τ | Ef ∪ ⋃Eᵢ
//!
//! FIELD MUTATION:
//!   Γ ⊢ e : τ | E
//!   root(e) = x, is_param(x)=true
//!   ─────────────────────────────────
//!   Γ ⊢ e.f = v : () | E ∪ write(x)
//! ```

use std::collections::{HashMap, HashSet, BTreeSet};
use crate::ast::{Type, EffectDecl, Ident, FnDef, Literal};
use crate::hir::{
    BindingId, BindingInfo, HirExpr, HirStmt, HirBlock, HirCallTarget,
    HirMatchArm, Spanned,
};
use crate::eir::{Effect, EffectSet};

//=============================================================================
// EFFECT SIGNATURE
//=============================================================================

/// Effect on a parameter
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamEffect {
    /// Parameter is read
    Read,
    /// Parameter is written (mutated)
    Write,
    /// Parameter is both read and written
    ReadWrite,
    /// No effect on parameter
    None,
}

impl ParamEffect {
    /// Combine two param effects
    pub fn combine(&self, other: &ParamEffect) -> ParamEffect {
        match (self, other) {
            (ParamEffect::None, e) | (e, ParamEffect::None) => e.clone(),
            (ParamEffect::Read, ParamEffect::Read) => ParamEffect::Read,
            (ParamEffect::Write, ParamEffect::Write) => ParamEffect::Write,
            _ => ParamEffect::ReadWrite,
        }
    }
}

/// Effect signature for a function
///
/// This captures the declared effects of a function, including:
/// - Global effects (io, alloc, panic)
/// - Parameter effects (read/write on specific parameters)
#[derive(Debug, Clone, Default)]
pub struct EffectSignature {
    /// Global effects this function performs
    pub effects: EffectSet,
    
    /// Parameter effects: param_name -> effect
    pub param_effects: HashMap<String, ParamEffect>,
    
    /// Whether this function is pure (no effects)
    pub is_pure: bool,
}

impl EffectSignature {
    /// Create empty (pure) effect signature
    pub fn pure() -> Self {
        EffectSignature {
            effects: EffectSet::new(),
            param_effects: HashMap::new(),
            is_pure: true,
        }
    }
    
    /// Create from declared effects
    pub fn from_decls(decls: &[EffectDecl], param_bindings: &HashMap<String, BindingId>) -> Self {
        let mut sig = EffectSignature::default();
        
        for decl in decls {
            match decl {
                EffectDecl::Io => {
                    sig.effects.insert(Effect::Io);
                    sig.is_pure = false;
                }
                EffectDecl::Alloc => {
                    sig.effects.insert(Effect::Alloc);
                    sig.is_pure = false;
                }
                EffectDecl::Panic => {
                    sig.effects.insert(Effect::Panic);
                    sig.is_pure = false;
                }
                EffectDecl::Read(param) => {
                    if let Some(&id) = param_bindings.get(&param.name) {
                        sig.effects.insert(Effect::Read(id));
                        let current = sig.param_effects.get(&param.name)
                            .cloned()
                            .unwrap_or(ParamEffect::None);
                        sig.param_effects.insert(
                            param.name.clone(),
                            current.combine(&ParamEffect::Read)
                        );
                    }
                    sig.is_pure = false;
                }
                EffectDecl::Write(param) => {
                    if let Some(&id) = param_bindings.get(&param.name) {
                        sig.effects.insert(Effect::Write(id));
                        let current = sig.param_effects.get(&param.name)
                            .cloned()
                            .unwrap_or(ParamEffect::None);
                        sig.param_effects.insert(
                            param.name.clone(),
                            current.combine(&ParamEffect::Write)
                        );
                    }
                    sig.is_pure = false;
                }
            }
        }
        
        sig.is_pure = sig.effects.is_empty() && sig.param_effects.is_empty();
        sig
    }
    
    /// Check if this signature has a specific effect
    pub fn has_effect(&self, effect: &Effect) -> bool {
        self.effects.contains(effect)
    }
    
    /// Get effect on a parameter
    pub fn param_effect(&self, param: &str) -> ParamEffect {
        self.param_effects.get(param).cloned().unwrap_or(ParamEffect::None)
    }
}

//=============================================================================
// FUNCTION TYPE
//=============================================================================

/// Function type with effect signature
///
/// This represents the complete type of a function including:
/// - Name
/// - Parameter names and types
/// - Return type
/// - Effect signature
#[derive(Debug, Clone)]
pub struct FunctionType {
    /// Function name
    pub name: String,
    
    /// Parameters: (name, type)
    pub params: Vec<(String, Type)>,
    
    /// Return type (None for unit/void)
    pub return_type: Option<Type>,
    
    /// Effect signature
    pub effect_sig: EffectSignature,
    
    /// Line number where defined (for error messages)
    pub line_number: usize,
}

impl FunctionType {
    /// Create a new function type
    pub fn new(name: String, line_number: usize) -> Self {
        FunctionType {
            name,
            params: Vec::new(),
            return_type: None,
            effect_sig: EffectSignature::pure(),
            line_number,
        }
    }
    
    /// Get the effect set for this function
    pub fn effects(&self) -> &EffectSet {
        &self.effect_sig.effects
    }
    
    /// Check if this function is pure
    pub fn is_pure(&self) -> bool {
        self.effect_sig.is_pure
    }
    
    /// Get parameter type by name
    pub fn param_type(&self, name: &str) -> Option<&Type> {
        self.params.iter()
            .find(|(n, _)| n == name)
            .map(|(_, t)| t)
    }
    
    /// Get parameter type by index
    pub fn param_type_at(&self, index: usize) -> Option<&Type> {
        self.params.get(index).map(|(_, t)| t)
    }
}

//=============================================================================
// TYPE ENVIRONMENT
//=============================================================================

/// Type environment for type-driven effect inference
///
/// This is the central data structure that stores type information
/// for all bindings and functions, enabling type-driven effect inference.
#[derive(Debug, Default)]
pub struct TypeEnv {
    /// Function signatures: name -> FunctionType
    functions: HashMap<String, FunctionType>,
    
    /// Binding types: BindingId -> Type
    binding_types: HashMap<BindingId, Type>,
    
    /// Binding info: BindingId -> BindingInfo
    binding_info: HashMap<BindingId, BindingInfo>,
    
    /// Current function's parameter bindings: name -> BindingId
    current_params: HashMap<String, BindingId>,
    
    /// Current function's parameter set (for fast lookup)
    current_param_set: HashSet<BindingId>,
    
    /// Standard library effect signatures (intrinsics)
    stdlib_effects: HashMap<String, EffectSet>,
    
    /// Method effect signatures: method_name -> EffectSet
    method_effects: HashMap<String, EffectSet>,
}

impl TypeEnv {
    /// Create a new type environment with stdlib registered
    pub fn new() -> Self {
        let mut env = TypeEnv::default();
        env.register_stdlib();
        env
    }
    
    /// Register standard library effect signatures
    ///
    /// This registers known effects for standard library functions.
    /// These are the "intrinsics" that form the basis of effect inference.
    fn register_stdlib(&mut self) {
        // ===== I/O Functions =====
        let io_funcs = [
            // Console I/O
            "println", "print", "eprintln", "eprint",
            "writeln", "write",
            // File I/O (function-style)
            "read", "read_exact", "read_to_string", "read_to_end",
            "write_all", "flush",
            // Stdin/Stdout
            "stdin", "stdout", "stderr",
        ];
        for name in &io_funcs {
            self.stdlib_effects.insert(
                name.to_string(),
                EffectSet::singleton(Effect::Io)
            );
        }
        
        // ===== Allocating Functions =====
        let alloc_funcs = [
            // Constructors
            "Vec::new", "Vec::with_capacity",
            "String::new", "String::from", "String::with_capacity",
            "Box::new", "Rc::new", "Arc::new",
            "HashMap::new", "HashMap::with_capacity",
            "HashSet::new", "HashSet::with_capacity",
            "BTreeMap::new", "BTreeSet::new",
            "VecDeque::new", "LinkedList::new", "BinaryHeap::new",
            // Macros (treated as functions)
            "vec", "format",
            // Conversion methods that allocate
            "to_string", "to_owned", "to_vec",
            "into_boxed_slice", "into_boxed_str",
        ];
        for name in &alloc_funcs {
            self.stdlib_effects.insert(
                name.to_string(),
                EffectSet::singleton(Effect::Alloc)
            );
        }
        
        // ===== Panicking Functions =====
        let panic_funcs = [
            "panic", "unwrap", "expect",
            "assert", "assert_eq", "assert_ne",
            "unreachable", "unimplemented", "todo",
        ];
        for name in &panic_funcs {
            self.stdlib_effects.insert(
                name.to_string(),
                EffectSet::singleton(Effect::Panic)
            );
        }
        
        // ===== Method Effects =====
        // I/O methods
        for method in &["read", "read_line", "read_to_string", "write", "write_all", "flush"] {
            self.method_effects.insert(
                method.to_string(),
                EffectSet::singleton(Effect::Io)
            );
        }
        
        // Allocating methods
        // NOTE: Removed clone() and collect() - they may not allocate
        // depending on the type (Copy types don't allocate on clone)
        for method in &["to_string", "to_owned", "to_vec", "push", "insert"] {
            self.method_effects.insert(
                method.to_string(),
                EffectSet::singleton(Effect::Alloc)
            );
        }
        
        // Panicking methods
        for method in &["unwrap", "expect"] {
            self.method_effects.insert(
                method.to_string(),
                EffectSet::singleton(Effect::Panic)
            );
        }
    }
    
    /// Register a user-defined function from parsed FnDef
    pub fn register_function(&mut self, func: &FnDef, param_bindings: &HashMap<String, BindingId>) {
        let effect_sig = EffectSignature::from_decls(&func.effects, param_bindings);
        
        let func_type = FunctionType {
            name: func.name.name.clone(),
            params: func.params.iter()
                .map(|p| (p.name.name.clone(), p.ty.clone()))
                .collect(),
            return_type: func.return_type.clone(),
            effect_sig,
            line_number: func.span.start_line,
        };
        
        self.functions.insert(func.name.name.clone(), func_type);
    }
    
    /// Register a function from signature components
    pub fn register_function_sig(
        &mut self,
        name: &str,
        effects: &[EffectDecl],
        param_bindings: &HashMap<String, BindingId>,
        line_number: usize,
    ) {
        let effect_sig = EffectSignature::from_decls(effects, param_bindings);
        
        let func_type = FunctionType {
            name: name.to_string(),
            params: Vec::new(), // Will be populated if needed
            return_type: None,
            effect_sig,
            line_number,
        };
        
        self.functions.insert(name.to_string(), func_type);
    }
    
    /// Lookup function's effect signature
    ///
    /// Returns the effect set for a function, checking:
    /// 1. User-defined functions
    /// 2. Standard library functions
    pub fn get_function_effects(&self, name: &str) -> Option<&EffectSet> {
        // First check user-defined
        if let Some(func) = self.functions.get(name) {
            return Some(&func.effect_sig.effects);
        }
        // Then check stdlib
        self.stdlib_effects.get(name)
    }
    
    /// Get method effects
    pub fn get_method_effects(&self, method: &str) -> Option<&EffectSet> {
        // Check user-defined first (methods could be in functions map)
        if let Some(func) = self.functions.get(method) {
            return Some(&func.effect_sig.effects);
        }
        // Then check known method effects
        self.method_effects.get(method)
    }
    
    /// Get full function type
    pub fn get_function_type(&self, name: &str) -> Option<&FunctionType> {
        self.functions.get(name)
    }
    
    /// Register a binding with its type
    pub fn register_binding(&mut self, id: BindingId, ty: Type, info: BindingInfo) {
        self.binding_types.insert(id, ty);
        self.binding_info.insert(id, info);
    }
    
    /// Register binding info only (when type is unknown)
    pub fn register_binding_info(&mut self, id: BindingId, info: BindingInfo) {
        self.binding_info.insert(id, info);
    }
    
    /// Get binding type
    pub fn get_binding_type(&self, id: BindingId) -> Option<&Type> {
        self.binding_types.get(&id)
    }
    
    /// Get binding info
    pub fn get_binding_info(&self, id: BindingId) -> Option<&BindingInfo> {
        self.binding_info.get(&id)
    }
    
    /// Check if binding is a parameter
    pub fn is_param(&self, id: BindingId) -> bool {
        // Fast path: check current param set
        if self.current_param_set.contains(&id) {
            return true;
        }
        // Fallback: check binding info
        self.binding_info.get(&id)
            .map(|info| info.is_param)
            .unwrap_or(false)
    }
    
    /// Enter a function scope with given parameters
    pub fn enter_function(&mut self, params: &[(String, BindingId)]) {
        self.current_params.clear();
        self.current_param_set.clear();
        for (name, id) in params {
            self.current_params.insert(name.clone(), *id);
            self.current_param_set.insert(*id);
        }
    }
    
    /// Exit function scope
    pub fn exit_function(&mut self) {
        self.current_params.clear();
        self.current_param_set.clear();
    }
    
    /// Get current param binding by name
    pub fn get_param_binding(&self, name: &str) -> Option<BindingId> {
        self.current_params.get(name).copied()
    }
    
    /// Get all current parameter bindings
    pub fn current_param_bindings(&self) -> &HashMap<String, BindingId> {
        &self.current_params
    }
    
    /// Check if a function is registered
    pub fn has_function(&self, name: &str) -> bool {
        self.functions.contains_key(name) || self.stdlib_effects.contains_key(name)
    }
    
    /// Get all registered functions
    pub fn all_functions(&self) -> impl Iterator<Item = &FunctionType> {
        self.functions.values()
    }
}

//=============================================================================
// TYPE-DRIVEN EFFECT INFERENCE
//=============================================================================

/// Type-driven effect inference engine
///
/// This replaces the pattern-based inference with type-based inference.
/// Instead of checking if a line contains "Vec::new", we look up the
/// effect signature of the function being called.
pub struct TypeDrivenInference<'a> {
    type_env: &'a TypeEnv,
}

impl<'a> TypeDrivenInference<'a> {
    /// Create a new type-driven inference engine
    pub fn new(type_env: &'a TypeEnv) -> Self {
        TypeDrivenInference { type_env }
    }
    
    /// Infer effects for an expression using type information
    ///
    /// This is the main inference function that recursively analyzes
    /// expressions and builds up the effect set.
    pub fn infer_expr(&self, expr: &Spanned<HirExpr>) -> EffectSet {
        match &expr.node {
            // ===== LITERAL RULES =====
            HirExpr::Literal(lit) => self.infer_literal(lit),
            
            // ===== VARIABLE RULE =====
            // Γ(x) = τ, is_param(x)
            // ────────────────────────
            // Γ ⊢ x : τ | read(x)
            HirExpr::Var(id) => {
                if self.type_env.is_param(*id) {
                    EffectSet::singleton(Effect::Read(*id))
                } else {
                    EffectSet::empty()
                }
            }
            
            // ===== BINARY OP RULE =====
            // Γ ⊢ e₁ : τ | E₁,  Γ ⊢ e₂ : τ | E₂
            // ────────────────────────────────────
            // Γ ⊢ e₁ ⊕ e₂ : τ' | E₁ ∪ E₂
            HirExpr::Binary { left, right, .. } => {
                self.infer_expr(left).union(&self.infer_expr(right))
            }
            
            // ===== UNARY OP RULE =====
            HirExpr::Unary { operand, .. } => self.infer_expr(operand),
            
            // ===== FIELD READ RULE =====
            // Γ ⊢ e : τ | E,  τ has field f : σ
            // ──────────────────────────────────
            // Γ ⊢ e.f : σ | E
            HirExpr::Field { base, .. } => self.infer_expr(base),
            
            // ===== INDEX RULE =====
            HirExpr::Index { base, index } => {
                self.infer_expr(base).union(&self.infer_expr(index))
            }
            
            // ===== FUNCTION CALL RULE (TYPE-DRIVEN!) =====
            // Γ ⊢ f : (τ₁,...,τₙ) → τ | Ef
            // Γ ⊢ eᵢ : τᵢ | Eᵢ
            // ───────────────────────────────
            // Γ ⊢ f(e₁,...,eₙ) : τ | Ef ∪ ⋃Eᵢ
            HirExpr::Call { target, args } => self.infer_call(target, args),
            
            // ===== STRUCT LITERAL =====
            // Struct construction is stack-allocated, no alloc effect
            HirExpr::Struct { fields, .. } => {
                let mut effects = EffectSet::new();
                for (_, e) in fields {
                    effects.extend(&self.infer_expr(e));
                }
                effects
            }
            
            // ===== TUPLE/ARRAY LITERAL =====
            // Stack allocated, no alloc effect
            HirExpr::Tuple(elems) | HirExpr::Array(elems) => {
                let mut effects = EffectSet::new();
                for e in elems {
                    effects.extend(&self.infer_expr(e));
                }
                effects
            }
            
            // ===== IF EXPRESSION =====
            // Γ ⊢ e₁ : bool | E₁,  Γ ⊢ e₂ : τ | E₂,  Γ ⊢ e₃ : τ | E₃
            // ──────────────────────────────────────────────────────────
            // Γ ⊢ if e₁ { e₂ } else { e₃ } : τ | E₁ ∪ E₂ ∪ E₃
            HirExpr::If { condition, then_branch, else_branch } => {
                let mut effects = self.infer_expr(condition);
                effects.extend(&self.infer_block(then_branch));
                if let Some(else_b) = else_branch {
                    effects.extend(&self.infer_block(else_b));
                }
                effects
            }
            
            // ===== MATCH EXPRESSION =====
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
            
            // ===== BLOCK EXPRESSION =====
            HirExpr::Block(block) => self.infer_block(block),
            
            // ===== CLOSURE =====
            // Γ, params ⊢ body : τ | E_body
            // capture = E_body ∩ outer_params
            // ────────────────────────────────
            // Γ ⊢ |params| body : Fn | capture
            HirExpr::Closure { body, captures, .. } => {
                let body_effects = self.infer_expr(body);
                let mut closure_effects = EffectSet::new();
                
                for effect in body_effects.iter() {
                    match effect {
                        // Only propagate effects on captured variables
                        Effect::Read(id) if captures.contains(id) => {
                            closure_effects.insert(effect.clone());
                        }
                        Effect::Write(id) if captures.contains(id) => {
                            closure_effects.insert(effect.clone());
                        }
                        // Always propagate global effects
                        Effect::Io | Effect::Alloc | Effect::Panic => {
                            closure_effects.insert(effect.clone());
                        }
                        _ => {}
                    }
                }
                closure_effects
            }
            
            // ===== RETURN/BREAK =====
            HirExpr::Return(Some(e)) | HirExpr::Break(Some(e)) => {
                self.infer_expr(e)
            }
            HirExpr::Return(None) | HirExpr::Break(None) | HirExpr::Continue => {
                EffectSet::empty()
            }
            
            // ===== RANGE =====
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
            
            // ===== REFERENCE =====
            HirExpr::Ref { expr, .. } => self.infer_expr(expr),
            
            // ===== DEREFERENCE =====
            HirExpr::Deref(expr) => self.infer_expr(expr),
        }
    }
    
    /// Infer effects from a literal
    fn infer_literal(&self, lit: &Literal) -> EffectSet {
        match lit {
            // String literals allocate heap memory
            Literal::String(_) => EffectSet::singleton(Effect::Alloc),
            // Other literals have no effects
            _ => EffectSet::empty(),
        }
    }
    
    /// Infer effects from a function call (TYPE-DRIVEN)
    ///
    /// This is the key difference from pattern-based inference:
    /// we look up the function's effect signature in the type environment.
    fn infer_call(&self, target: &HirCallTarget, args: &[Spanned<HirExpr>]) -> EffectSet {
        let mut effects = EffectSet::new();
        
        // Add argument effects
        for arg in args {
            effects.extend(&self.infer_expr(arg));
        }
        
        // Get function effects FROM TYPE SIGNATURE
        match target {
            HirCallTarget::Function(path) => {
                let func_name = path.to_string();
                
                // Look up effect signature in type environment
                if let Some(func_effects) = self.type_env.get_function_effects(&func_name) {
                    effects.extend(func_effects);
                }
                
                // Also check for short name (e.g., "println" vs "std::io::println")
                if let Some(last_segment) = path.segments.last() {
                    if let Some(func_effects) = self.type_env.get_function_effects(&last_segment.name) {
                        effects.extend(func_effects);
                    }
                }
            }
            HirCallTarget::Method { receiver, method } => {
                // Add receiver effects
                effects.extend(&self.infer_expr(receiver));
                
                // Look up method effects from type environment
                if let Some(method_effects) = self.type_env.get_method_effects(&method.name) {
                    effects.extend(method_effects);
                }
            }
        }
        
        effects
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
                // ===== ASSIGNMENT (FIELD MUTATION) =====
                // Γ ⊢ e₁ : τ | E₁,  Γ ⊢ e₂ : σ | E₂,  root(e₁) = x, is_param(x)
                // ────────────────────────────────────────────────────────────────
                // Γ ⊢ e₁ = e₂ : () | E₁ ∪ E₂ ∪ write(x)
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
        
        // Trailing expression
        if let Some(e) = &block.node.expr {
            effects.extend(&self.infer_expr(e));
        }
        
        effects
    }
    
    /// Add write effect if target is a parameter
    fn add_write_effect(&self, effects: &mut EffectSet, target: &Spanned<HirExpr>) {
        if let Some(root_id) = self.find_root_binding(target) {
            if self.type_env.is_param(root_id) {
                effects.insert(Effect::Write(root_id));
            }
        }
    }
    
    /// Find the root binding of a place expression
    ///
    /// For `a.b.c`, returns the binding for `a`.
    /// For `arr[i].field`, returns the binding for `arr`.
    fn find_root_binding(&self, expr: &Spanned<HirExpr>) -> Option<BindingId> {
        match &expr.node {
            HirExpr::Var(id) => Some(*id),
            HirExpr::Field { base, .. } => self.find_root_binding(base),
            HirExpr::Index { base, .. } => self.find_root_binding(base),
            HirExpr::Deref(inner) => self.find_root_binding(inner),
            _ => None,
        }
    }
}

//=============================================================================
// TYPE ENV BUILDER
//=============================================================================

/// Builder for TypeEnv from parsed source
pub struct TypeEnvBuilder {
    env: TypeEnv,
    next_binding_id: u32,
}

impl TypeEnvBuilder {
    pub fn new() -> Self {
        TypeEnvBuilder {
            env: TypeEnv::new(),
            next_binding_id: 0,
        }
    }
    
    /// Generate next binding ID
    fn next_id(&mut self) -> BindingId {
        let id = BindingId::new(self.next_binding_id);
        self.next_binding_id += 1;
        id
    }
    
    /// Register a function from extracted signature
    pub fn register_from_signature(
        &mut self,
        name: &str,
        effects: &[EffectDecl],
        param_names: &[String],
        line_number: usize,
    ) {
        // Create binding IDs for parameters
        let mut param_bindings = HashMap::new();
        for pname in param_names {
            let id = self.next_id();
            param_bindings.insert(pname.clone(), id);
            
            // Register binding info
            let info = BindingInfo {
                id,
                name: pname.clone(),
                ty: None,
                mutable: false,
                scope_depth: 0,
                decl_span: crate::ast::Span::default(),
                is_outer: false,
                is_param: true,
            };
            self.env.register_binding_info(id, info);
        }
        
        self.env.register_function_sig(name, effects, &param_bindings, line_number);
    }
    
    /// Build and return the TypeEnv
    pub fn build(self) -> TypeEnv {
        self.env
    }
    
    /// Get mutable reference to env for additional registration
    pub fn env_mut(&mut self) -> &mut TypeEnv {
        &mut self.env
    }
}

impl Default for TypeEnvBuilder {
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
    use crate::ast::{Span, BinOp};
    use crate::hir::{HirBlock, Path as HirPath};
    
    fn make_span() -> Span {
        Span::default()
    }
    
    fn make_spanned<T>(node: T) -> Spanned<T> {
        Spanned::new(node, make_span())
    }
    
    #[test]
    fn test_effect_signature_pure() {
        let sig = EffectSignature::pure();
        assert!(sig.is_pure);
        assert!(sig.effects.is_empty());
    }
    
    #[test]
    fn test_effect_signature_from_decls() {
        let mut param_bindings = HashMap::new();
        param_bindings.insert("acc".to_string(), BindingId::new(0));
        
        let decls = vec![
            EffectDecl::Write(Ident::new("acc")),
            EffectDecl::Io,
        ];
        
        let sig = EffectSignature::from_decls(&decls, &param_bindings);
        
        assert!(!sig.is_pure);
        assert!(sig.effects.has_io());
        assert!(sig.effects.has_write(BindingId::new(0)));
        assert_eq!(sig.param_effect("acc"), ParamEffect::Write);
    }
    
    #[test]
    fn test_type_env_stdlib() {
        let env = TypeEnv::new();
        
        // Check IO functions
        assert!(env.get_function_effects("println").is_some());
        assert!(env.get_function_effects("println").unwrap().has_io());
        
        // Check alloc functions
        assert!(env.get_function_effects("Vec::new").is_some());
        assert!(env.get_function_effects("Vec::new").unwrap().has_alloc());
        
        // Check panic functions
        assert!(env.get_function_effects("unwrap").is_some());
        assert!(env.get_function_effects("unwrap").unwrap().has_panic());
    }
    
    #[test]
    fn test_type_env_user_function() {
        let mut env = TypeEnv::new();
        
        let mut param_bindings = HashMap::new();
        param_bindings.insert("x".to_string(), BindingId::new(0));
        
        env.register_function_sig(
            "my_func",
            &[EffectDecl::Io, EffectDecl::Write(Ident::new("x"))],
            &param_bindings,
            1,
        );
        
        let effects = env.get_function_effects("my_func").unwrap();
        assert!(effects.has_io());
        assert!(effects.has_write(BindingId::new(0)));
    }
    
    #[test]
    fn test_type_env_params() {
        let mut env = TypeEnv::new();
        
        let params = vec![
            ("a".to_string(), BindingId::new(0)),
            ("b".to_string(), BindingId::new(1)),
        ];
        
        env.enter_function(&params);
        
        assert!(env.is_param(BindingId::new(0)));
        assert!(env.is_param(BindingId::new(1)));
        assert!(!env.is_param(BindingId::new(2)));
        
        assert_eq!(env.get_param_binding("a"), Some(BindingId::new(0)));
        assert_eq!(env.get_param_binding("b"), Some(BindingId::new(1)));
        
        env.exit_function();
        
        // After exit, params are cleared from current set
        // but binding_info still knows they were params
    }
    
    #[test]
    fn test_type_driven_inference_literal() {
        let env = TypeEnv::new();
        let inference = TypeDrivenInference::new(&env);
        
        // Integer literal - no effect
        let int_expr = make_spanned(HirExpr::Literal(Literal::Int(42)));
        let effects = inference.infer_expr(&int_expr);
        assert!(effects.is_empty());
        
        // String literal - alloc effect
        let str_expr = make_spanned(HirExpr::Literal(Literal::String("hello".to_string())));
        let effects = inference.infer_expr(&str_expr);
        assert!(effects.has_alloc());
    }
    
    #[test]
    fn test_type_driven_inference_var() {
        let mut env = TypeEnv::new();
        
        // Register param
        let param_info = BindingInfo {
            id: BindingId::new(0),
            name: "x".to_string(),
            ty: None,
            mutable: false,
            scope_depth: 0,
            decl_span: Span::default(),
            is_outer: false,
            is_param: true,
        };
        env.register_binding_info(BindingId::new(0), param_info);
        env.enter_function(&[("x".to_string(), BindingId::new(0))]);
        
        let inference = TypeDrivenInference::new(&env);
        
        // Param read - has read effect
        let var_expr = make_spanned(HirExpr::Var(BindingId::new(0)));
        let effects = inference.infer_expr(&var_expr);
        assert!(effects.has_read(BindingId::new(0)));
        
        // Non-param read - no effect
        let local_expr = make_spanned(HirExpr::Var(BindingId::new(1)));
        let effects = inference.infer_expr(&local_expr);
        assert!(effects.is_empty());
    }
    
    #[test]
    fn test_type_driven_inference_call() {
        let env = TypeEnv::new();
        let inference = TypeDrivenInference::new(&env);
        
        // Call to println - io effect
        let call_expr = make_spanned(HirExpr::Call {
            target: HirCallTarget::Function(HirPath::simple("println")),
            args: vec![],
        });
        let effects = inference.infer_expr(&call_expr);
        assert!(effects.has_io());
        
        // Call to Vec::new - alloc effect
        let alloc_call = make_spanned(HirExpr::Call {
            target: HirCallTarget::Function(HirPath::simple("Vec::new")),
            args: vec![],
        });
        let effects = inference.infer_expr(&alloc_call);
        assert!(effects.has_alloc());
    }
    
    #[test]
    fn test_type_driven_inference_binary() {
        let mut env = TypeEnv::new();
        env.enter_function(&[("a".to_string(), BindingId::new(0))]);
        
        // Register param
        let param_info = BindingInfo {
            id: BindingId::new(0),
            name: "a".to_string(),
            ty: None,
            mutable: false,
            scope_depth: 0,
            decl_span: Span::default(),
            is_outer: false,
            is_param: true,
        };
        env.register_binding_info(BindingId::new(0), param_info);
        
        let inference = TypeDrivenInference::new(&env);
        
        // a + 1 - has read(a) effect
        let binary_expr = make_spanned(HirExpr::Binary {
            op: BinOp::Add,
            left: Box::new(make_spanned(HirExpr::Var(BindingId::new(0)))),
            right: Box::new(make_spanned(HirExpr::Literal(Literal::Int(1)))),
        });
        let effects = inference.infer_expr(&binary_expr);
        assert!(effects.has_read(BindingId::new(0)));
    }
    
    #[test]
    fn test_type_env_builder() {
        let mut builder = TypeEnvBuilder::new();
        
        builder.register_from_signature(
            "transfer",
            &[EffectDecl::Write(Ident::new("acc")), EffectDecl::Io],
            &["acc".to_string(), "amount".to_string()],
            10,
        );
        
        let env = builder.build();
        
        let func = env.get_function_type("transfer").unwrap();
        assert_eq!(func.name, "transfer");
        assert!(func.effect_sig.effects.has_io());
        assert_eq!(func.line_number, 10);
    }
    
    #[test]
    fn test_param_effect_combine() {
        assert_eq!(ParamEffect::None.combine(&ParamEffect::Read), ParamEffect::Read);
        assert_eq!(ParamEffect::Read.combine(&ParamEffect::Write), ParamEffect::ReadWrite);
        assert_eq!(ParamEffect::Write.combine(&ParamEffect::Write), ParamEffect::Write);
    }
}