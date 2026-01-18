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

pub mod ast;
pub mod hir;
pub mod eir;
pub mod parser;

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
}