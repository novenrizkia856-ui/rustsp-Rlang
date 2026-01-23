//! RustS+ Intermediate Representation (IR) System
//!
//! This module provides the formal IR for RustS+:
//!
//! ```text
//! SOURCE (.rss)
//!     ↓
//! ┌─────────────────────────────────────┐
//! │  LEXER + PARSER                     │
//! │    → Tokenize source                │
//! │    → Build Abstract Syntax Tree     │
//! └─────────────────────────────────────┘
//!     ↓
//! ┌─────────────────────────────────────┐
//! │  HIR BUILDER                        │
//! │    → Resolve names to bindings      │
//! │    → Build scope information        │
//! │    → Track mutability               │
//! │    → Handle `outer` keyword         │
//! └─────────────────────────────────────┘
//!     ↓
//! ┌─────────────────────────────────────┐
//! │  EIR BUILDER + EFFECT INFERENCE     │
//! │    → Structural effect inference    │
//! │    → Effect propagation checking    │
//! │    → Effect ownership validation    │
//! └─────────────────────────────────────┘
//!     ↓
//! ┌─────────────────────────────────────┐
//! │  LOWERING                           │
//! │    → Transform to valid Rust        │
//! │    → Strip effects clause           │
//! └─────────────────────────────────────┘
//!     ↓
//! OUTPUT (.rs)
//! ```
//!
//! ## Module Structure
//!
//! - `ast`: Abstract Syntax Tree nodes
//! - `hir`: High-level IR with resolved bindings
//! - `eir`: Effect IR with inference rules
//! - `parser`: Source → AST conversion
//! - `lowering`: IR → Rust code generation
//! - `type_env`: Type environment for type-driven inference (Phase 1.1)

pub mod ast;
pub mod hir;
pub mod eir;
pub mod parser;
pub mod type_env;

// Re-export commonly used types
pub use ast::{
    Span, Spanned, Ident, Path, Type, Literal, BinOp, UnaryOp,
    Expr, Stmt, Block, Pattern, EffectDecl,
    FnDef, FnParam, StructDef, EnumDef, Item, Module,
};

pub use hir::{
    BindingId, BindingInfo, Scope, ScopeResolver,
    HirExpr, HirStmt, HirBlock, HirFnDef, HirItem, HirModule,
    MutationAnalysis,
};

pub use eir::{
    Effect, EffectSet, EffectContext, EffectInference,
    EffectValidationResult, EffectError, EffectErrorKind,
    EffectValidator, EffectDependencyGraph,
};

// Re-export type environment for type-driven inference (Phase 1.1)
pub use type_env::{
    TypeEnv, TypeEnvBuilder, TypeDrivenInference,
    FunctionType, EffectSignature, ParamEffect,
};

pub use parser::{
    Token, Lexer, FunctionParser,
    parse_module, extract_function_signatures,
};

use std::collections::{HashMap, HashSet, BTreeSet};

//=============================================================================
// INTEGRATION: IR-BASED EFFECT CHECKING
//=============================================================================

/// Result of IR-based analysis
#[derive(Debug)]
pub struct AnalysisResult {
    pub functions: HashMap<String, FunctionAnalysis>,
    pub errors: Vec<AnalysisError>,
}

/// Analysis result for a single function
#[derive(Debug)]
pub struct FunctionAnalysis {
    pub name: String,
    pub declared_effects: EffectSet,
    pub detected_effects: EffectSet,
    pub undeclared_effects: EffectSet,
    pub line_number: usize,
}

/// An error from IR analysis
#[derive(Debug)]
pub struct AnalysisError {
    pub kind: AnalysisErrorKind,
    pub line: usize,
    pub message: String,
}

#[derive(Debug)]
pub enum AnalysisErrorKind {
    UndeclaredEffect,
    MissingPropagation,
    PureCallingEffectful,
    DuplicateWrite,
    EffectLeak,
}

//=============================================================================
// HIGH-LEVEL API FOR INTEGRATION
//=============================================================================

/// Analyze RustS+ source code using the IR system
/// 
/// This function provides a bridge between the IR system and the existing
/// compiler infrastructure. It:
/// 1. Parses function signatures and effects
/// 2. Builds effect context
/// 3. Performs effect inference
/// 4. Returns analysis results compatible with existing error reporting
pub fn analyze_source_ir(source: &str) -> AnalysisResult {
    let mut result = AnalysisResult {
        functions: HashMap::new(),
        errors: Vec::new(),
    };
    
    // Step 1: Extract function signatures
    let signatures = extract_function_signatures(source);
    
    // Step 2: Build effect context
    let bindings = HashMap::new(); // In full impl, populated from HIR
    let mut ctx = EffectContext::new(bindings);
    
    // Register all functions
    for (name, effects, _line) in &signatures {
        let effect_set: EffectSet = effects.iter()
            .filter_map(|e| convert_decl_to_effect(e))
            .collect();
        ctx.register_function(name, effect_set);
    }
    
    // Step 3: Build analysis for each function
    for (name, effects, line) in signatures {
        let declared: EffectSet = effects.iter()
            .filter_map(|e| convert_decl_to_effect(e))
            .collect();
        
        // In full implementation, detected effects come from HIR analysis
        // For now, we return an empty analysis that can be filled in
        let analysis = FunctionAnalysis {
            name: name.clone(),
            declared_effects: declared.clone(),
            detected_effects: EffectSet::new(),
            undeclared_effects: EffectSet::new(),
            line_number: line,
        };
        
        result.functions.insert(name, analysis);
    }
    
    result
}

//=============================================================================
// TYPE-DRIVEN ANALYSIS (PHASE 1.1)
//=============================================================================

/// Analyze RustS+ source code using TYPE-DRIVEN effect inference
///
/// This is the new Phase 1.1 implementation that uses TypeEnv for inference
/// instead of pattern matching. Key differences:
///
/// 1. Function effects come from type signatures, not string patterns
/// 2. Parameter tracking uses BindingId, not string names
/// 3. Effect inference is structural, following formal rules
///
/// # Example
///
/// ```ignore
/// let source = r#"
/// fn transfer(acc Account, amount i64) effects(write acc, io) Account {
///     acc.balance -= amount
///     println!("Transferred {}", amount)
///     acc
/// }
/// "#;
///
/// let result = analyze_source_typed(source);
/// let transfer = result.functions.get("transfer").unwrap();
/// assert!(transfer.declared_effects.has_io());
/// assert!(transfer.declared_effects.has_write(/* acc's binding id */));
/// ```
pub fn analyze_source_typed(source: &str) -> AnalysisResult {
    let mut result = AnalysisResult {
        functions: HashMap::new(),
        errors: Vec::new(),
    };
    
    // Step 1: Extract function signatures
    let signatures = extract_function_signatures(source);
    
    // Step 2: Build TypeEnv (TYPE-DRIVEN approach)
    let mut builder = TypeEnvBuilder::new();
    
    // Register all functions in TypeEnv
    for (name, effects, line) in &signatures {
        // Extract parameter names from signature
        let param_names = extract_param_names_from_source(source, *line);
        builder.register_from_signature(name, effects, &param_names, *line);
    }
    
    let type_env = builder.build();
    
    // Step 3: Build analysis for each function using TypeEnv
    for (name, effects, line) in signatures {
        // Get declared effects from TypeEnv (proper BindingId resolution)
        let declared = type_env.get_function_effects(&name)
            .cloned()
            .unwrap_or_default();
        
        // For now, detected effects would come from HIR traversal
        // with TypeDrivenInference. Full implementation would:
        // 1. Build HIR for function body
        // 2. Create TypeDrivenInference with type_env
        // 3. Call infer_block on function body
        let detected = EffectSet::new();
        
        let undeclared = detected.difference(&declared);
        
        let analysis = FunctionAnalysis {
            name: name.clone(),
            declared_effects: declared,
            detected_effects: detected,
            undeclared_effects: undeclared,
            line_number: line,
        };
        
        result.functions.insert(name, analysis);
    }
    
    result
}

/// Build TypeEnv from source code
///
/// This is a convenience function for creating a TypeEnv populated
/// with all function signatures from the source.
pub fn build_type_env(source: &str) -> TypeEnv {
    let signatures = extract_function_signatures(source);
    let mut builder = TypeEnvBuilder::new();
    
    for (name, effects, line) in &signatures {
        let param_names = extract_param_names_from_source(source, *line);
        builder.register_from_signature(name, effects, &param_names, *line);
    }
    
    builder.build()
}

/// Extract parameter names from a function signature at a given line
fn extract_param_names_from_source(source: &str, line_num: usize) -> Vec<String> {
    let lines: Vec<&str> = source.lines().collect();
    if line_num == 0 || line_num > lines.len() {
        return Vec::new();
    }
    
    let line = lines[line_num - 1];
    
    // Simple extraction: find content between ( and )
    if let Some(start) = line.find('(') {
        if let Some(end) = line.find(')') {
            let params_str = &line[start + 1..end];
            
            // Split by comma and extract param names
            return params_str
                .split(',')
                .filter_map(|p| {
                    let trimmed = p.trim();
                    // Parameter format: "name Type" or "name: Type"
                    if trimmed.is_empty() {
                        None
                    } else if let Some(colon_pos) = trimmed.find(':') {
                        // Rust-style: name: Type
                        Some(trimmed[..colon_pos].trim().to_string())
                    } else if let Some(space_pos) = trimmed.find(' ') {
                        // RustS+ style: name Type
                        Some(trimmed[..space_pos].trim().to_string())
                    } else {
                        // Just a name (no type annotation)
                        Some(trimmed.to_string())
                    }
                })
                .collect();
        }
    }
    
    Vec::new()
}

/// Convert AST EffectDecl to EIR Effect
fn convert_decl_to_effect(decl: &EffectDecl) -> Option<Effect> {
    match decl {
        EffectDecl::Io => Some(Effect::Io),
        EffectDecl::Alloc => Some(Effect::Alloc),
        EffectDecl::Panic => Some(Effect::Panic),
        // Read/Write need binding resolution - placeholder for now
        EffectDecl::Read(name) => Some(Effect::Read(BindingId::new(0))), // Placeholder
        EffectDecl::Write(name) => Some(Effect::Write(BindingId::new(0))), // Placeholder
    }
}

//=============================================================================
// EFFECT DETECTION FROM SOURCE (HYBRID APPROACH)
//=============================================================================

/// Detect effects from a code block using pattern matching
/// This is a hybrid approach that works with line-based analysis
pub fn detect_effects_from_source(
    lines: &[(usize, &str)],
    params: &[(String, String)],
) -> EffectSet {
    let mut effects = EffectSet::new();
    
    for (line_num, line) in lines {
        // Detect I/O
        if detect_io_pattern(line) {
            effects.insert(Effect::Io);
        }
        
        // Detect allocation
        if detect_alloc_pattern(line) {
            effects.insert(Effect::Alloc);
        }
        
        // Detect panic
        if detect_panic_pattern(line) {
            effects.insert(Effect::Panic);
        }
        
        // Detect parameter mutations (would need HIR for precise detection)
        // Placeholder - full impl uses HIR binding analysis
    }
    
    effects
}

fn detect_io_pattern(line: &str) -> bool {
    // IMPROVED: Added comprehensive I/O patterns
    let patterns = [
        // Console I/O
        "println!", "print!", "eprintln!", "eprint!",
        "stdin()", "stdout()", "stderr()",
        
        // File I/O
        "std::io", "File::", "OpenOptions::",
        ".read(", ".read_exact(", ".read_to_string(", ".read_to_end(",
        ".write(", ".write_all(", ".flush(",
        "fs::read", "fs::write", "fs::create", "fs::open",
        "fs::remove", "fs::rename", "fs::copy",
        "fs::create_dir", "fs::remove_dir", "fs::read_dir",
        "BufReader::", "BufWriter::",
        
        // Networking I/O
        "TcpStream::", "TcpListener::", "UdpSocket::",
        "std::net::", ".connect(", ".bind(", ".listen(", ".accept(",
        ".send(", ".recv(",
        
        // Environment I/O
        "std::env::var", "std::env::args", "env::var", "env::args",
        
        // Process I/O
        "std::process::", "Command::", ".spawn(", ".output(",
    ];
    patterns.iter().any(|p| line.contains(p))
}

fn detect_alloc_pattern(line: &str) -> bool {
    // CRITICAL FIX: Removed `.clone()` and `.collect()` from patterns
    //
    // Reason: `.clone()` on Copy types does NOT allocate.
    //         `.collect()` may not allocate depending on output type.
    //
    // For strict tracking, users should declare `effects(alloc)` explicitly.
    let patterns = [
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
    patterns.iter().any(|p| line.contains(p))
}

fn detect_panic_pattern(line: &str) -> bool {
    let patterns = [
        "panic!", ".unwrap()", ".expect(",
        "assert!", "assert_eq!", "assert_ne!",
        "unreachable!", "unimplemented!", "todo!",
    ];
    patterns.iter().any(|p| line.contains(p))
}

//=============================================================================
// TESTS
//=============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_analyze_source_basic() {
        let source = r#"
fn pure_add(a i32, b i32) i32 {
    a + b
}

fn log_message(msg String) effects(io) {
    println!("{}", msg)
}
"#;
        
        let result = analyze_source_ir(source);
        assert!(result.errors.is_empty());
        assert_eq!(result.functions.len(), 2);
        
        let pure_add = result.functions.get("pure_add").unwrap();
        assert!(pure_add.declared_effects.is_empty());
        
        let log_message = result.functions.get("log_message").unwrap();
        assert!(log_message.declared_effects.has_io());
    }
    
    #[test]
    fn test_effect_detection() {
        let lines = vec![
            (1, "let x = Vec::new();"),
            (2, "println!(\"hello\");"),
            (3, "let y = x.unwrap();"),
        ];
        
        let effects = detect_effects_from_source(&lines, &[]);
        assert!(effects.has_io());
        assert!(effects.has_alloc());
        assert!(effects.has_panic());
    }
    
    // =========================================================================
    // TYPE-DRIVEN ANALYSIS TESTS (Phase 1.1)
    // =========================================================================
    
    #[test]
    fn test_analyze_source_typed_basic() {
        let source = r#"
fn pure_add(a i32, b i32) i32 {
    a + b
}

fn log_message(msg String) effects(io) {
    println!("{}", msg)
}
"#;
        
        let result = analyze_source_typed(source);
        assert!(result.errors.is_empty());
        assert_eq!(result.functions.len(), 2);
        
        let pure_add = result.functions.get("pure_add").unwrap();
        assert!(pure_add.declared_effects.is_empty());
        
        let log_message = result.functions.get("log_message").unwrap();
        assert!(log_message.declared_effects.has_io());
    }
    
    #[test]
    fn test_analyze_source_typed_with_write() {
        let source = r#"
fn transfer(acc Account, amount i64) effects(write(acc), io) Account {
    acc.balance -= amount
    println!("Transferred {}", amount)
    acc
}
"#;
        
        let result = analyze_source_typed(source);
        assert_eq!(result.functions.len(), 1);
        
        let transfer = result.functions.get("transfer").unwrap();
        assert!(transfer.declared_effects.has_io());
        // Write effect should be present (binding ID assigned by builder)
        assert!(transfer.declared_effects.iter().any(|e| matches!(e, Effect::Write(_))));
    }
    
    #[test]
    fn test_build_type_env() {
        let source = r#"
fn allocate_vec() effects(alloc) Vec[i32] {
    Vec::new()
}

fn dangerous_op() effects(panic) i32 {
    let x = some_option.unwrap()
    x
}
"#;
        
        let env = build_type_env(source);
        
        // Check allocate_vec
        let alloc_effects = env.get_function_effects("allocate_vec").unwrap();
        assert!(alloc_effects.has_alloc());
        
        // Check dangerous_op
        let panic_effects = env.get_function_effects("dangerous_op").unwrap();
        assert!(panic_effects.has_panic());
    }
    
    #[test]
    fn test_extract_param_names() {
        let source = "fn transfer(acc Account, amount i64) effects(write(acc)) Account {";
        let params = extract_param_names_from_source(source, 1);
        assert_eq!(params, vec!["acc", "amount"]);
    }
    
    #[test]
    fn test_extract_param_names_rust_style() {
        let source = "fn transfer(acc: Account, amount: i64) -> Account {";
        let params = extract_param_names_from_source(source, 1);
        assert_eq!(params, vec!["acc", "amount"]);
    }
    
    #[test]
    fn test_type_env_stdlib_effects() {
        let env = TypeEnv::new();
        
        // IO functions should have IO effect
        assert!(env.get_function_effects("println").unwrap().has_io());
        assert!(env.get_function_effects("print").unwrap().has_io());
        
        // Alloc functions should have alloc effect  
        assert!(env.get_function_effects("Vec::new").unwrap().has_alloc());
        assert!(env.get_function_effects("String::from").unwrap().has_alloc());
        
        // Panic functions should have panic effect
        assert!(env.get_function_effects("unwrap").unwrap().has_panic());
        assert!(env.get_function_effects("panic").unwrap().has_panic());
    }
    
    #[test]
    fn test_type_env_method_effects() {
        let env = TypeEnv::new();
        
        // IO methods
        assert!(env.get_method_effects("read").unwrap().has_io());
        assert!(env.get_method_effects("write").unwrap().has_io());
        
        // Alloc methods
        assert!(env.get_method_effects("to_string").unwrap().has_alloc());
        assert!(env.get_method_effects("push").unwrap().has_alloc());
        
        // Panic methods
        assert!(env.get_method_effects("unwrap").unwrap().has_panic());
    }
}