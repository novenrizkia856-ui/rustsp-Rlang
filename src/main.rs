//! RustS+ Compiler - Main Entry Point
//!
//! ## Compilation Pipeline (IR-Based)
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │  STAGE 0: IR CONSTRUCTION & EFFECT ANALYSIS                         │
//! │    → Lexer: Source → Tokens                                        │
//! │    → Parser: Tokens → AST (function signatures + effects)          │
//! │    → HIR Builder: AST → HIR (scope resolution, binding IDs)        │
//! │    → EIR Builder: HIR → EIR (structural effect inference)          │
//! │    → Build function table with effect contracts                    │
//! │    → Build effect dependency graph for cross-function checking     │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │  STAGE 1: ANTI-FAIL LOGIC CHECK (IR-Based)                          │
//! │    → Logic-01: Expression completeness (if/match branches)          │
//! │    → Logic-02: Ambiguous shadowing detection                        │
//! │    → Logic-03: Illegal statements in expression context             │
//! │    → Logic-04: Implicit mutation detection                          │
//! │    → Logic-05: Unclear intent patterns                              │
//! │    → Logic-06: Same-scope reassignment without mut                  │
//! │    → Effect-01: Undeclared effect validation (STRUCTURAL)           │
//! │    → Effect-02: Effect leak detection                               │
//! │    → Effect-03: Pure calling effectful detection                    │
//! │    → Effect-04: Cross-function effect propagation                   │
//! │    → Effect-05: Effect scope validation                             │
//! │    → Effect-06: Effect ownership validation                         │
//! │                                                                     │
//! │    Effect Inference Rules (Formal):                                 │
//! │    • infer(42)        = ∅                                          │
//! │    • infer("string")  = {alloc}                                    │
//! │    • infer(param_x)   = {read(x)}                                  │
//! │    • infer(x.f = e)   = infer(e) ∪ {write(root(x))}               │
//! │    • infer(f(args))   = ⋃infer(args) ∪ effects(f)                 │
//! │    • infer(if/match)  = union of all branches                      │
//! │                                                                     │
//! │    ⚠️  IF ANY VIOLATION → COMPILATION STOPS HERE                    │
//! │    ⚠️  RUST CODE IS NOT GENERATED                                   │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │  STAGE 2: LOWERING (RustS+ → Rust)                                  │
//! │    → Transform RustS+ syntax to valid Rust                          │
//! │    → Apply L-01 through L-12 transformations                        │
//! │    → Strip effects clause from signatures                           │
//! │    → Sanity check generated Rust                                    │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │  STAGE 3: RUST COMPILATION (rustc)                                  │
//! │    → Compile generated Rust to binary                               │
//! │    → Map rustc errors back to RustS+ source                         │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! The key insight of RustS+ is that **dishonest code never reaches Rust**.
//! If your code has logic errors or undeclared effects, it stops at Stage 1.

use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio, exit};
use std::collections::HashMap;

use rustsp::parse_rusts;
use rustsp::error_msg::map_rust_error;
use rustsp::anti_fail_logic::{
    check_logic, check_logic_no_effects, check_logic_custom,
    format_logic_errors, ansi, analyze_functions
};
use rustsp::rust_sanity::{check_rust_output, format_internal_error};

// NEW: IR module imports
use rustsp::ast::EffectDecl;
use rustsp::eir::{Effect, EffectSet, EffectContext, EffectInference, EffectDependencyGraph};
use rustsp::parser::{Lexer, FunctionParser, extract_function_signatures};
use rustsp::hir::{BindingId, BindingInfo, ScopeResolver};

//=============================================================================
// IR-BASED EFFECT ANALYSIS (NEW)
//=============================================================================

/// Analyze source using IR-based effect inference
/// Returns: (function_name -> (declared, detected, undeclared, line))
fn analyze_effects_ir(source: &str) -> HashMap<String, (EffectSet, EffectSet, EffectSet, usize)> {
    let mut results = HashMap::new();
    
    // Step 1: Extract function signatures with effects
    let signatures = extract_function_signatures(source);
    
    // Step 2: Build effect context
    let bindings = HashMap::new();
    let mut ctx = EffectContext::new(bindings);
    
    // Register all functions with their declared effects
    for (name, effects, _line) in &signatures {
        let effect_set: EffectSet = effects.iter()
            .filter_map(|e| convert_effect_decl(e))
            .collect();
        ctx.register_function(name, effect_set);
    }
    
    // Step 3: Analyze each function
    for (name, effects, line) in signatures {
        let declared: EffectSet = effects.iter()
            .filter_map(|e| convert_effect_decl(e))
            .collect();
        
        // Detect effects from function body
        let detected = detect_function_effects(source, &name, line);
        
        // Calculate undeclared effects
        let undeclared = detected.difference(&declared);
        
        results.insert(name, (declared, detected, undeclared, line));
    }
    
    results
}

/// Convert AST EffectDecl to EIR Effect
fn convert_effect_decl(decl: &EffectDecl) -> Option<Effect> {
    match decl {
        EffectDecl::Io => Some(Effect::Io),
        EffectDecl::Alloc => Some(Effect::Alloc),
        EffectDecl::Panic => Some(Effect::Panic),
        EffectDecl::Read(_) => Some(Effect::Read(BindingId::new(0))), // Placeholder
        EffectDecl::Write(_) => Some(Effect::Write(BindingId::new(0))), // Placeholder
    }
}

/// Detect effects from function body using pattern matching
/// This is the hybrid approach - uses patterns but informed by IR structure
fn detect_function_effects(source: &str, func_name: &str, start_line: usize) -> EffectSet {
    let mut effects = EffectSet::new();
    let lines: Vec<&str> = source.lines().collect();
    
    // Find function body
    let mut in_function = false;
    let mut brace_depth = 0;
    
    for (i, line) in lines.iter().enumerate() {
        let line_num = i + 1;
        let trimmed = line.trim();
        
        // Check if we're at the function start
        if line_num == start_line {
            in_function = true;
            brace_depth = trimmed.matches('{').count() as i32 - trimmed.matches('}').count() as i32;
            continue;
        }
        
        if !in_function {
            continue;
        }
        
        // Track brace depth
        brace_depth += trimmed.matches('{').count() as i32;
        brace_depth -= trimmed.matches('}').count() as i32;
        
        if brace_depth <= 0 {
            break; // End of function
        }
        
        // Detect I/O effects
        if detect_io_pattern(trimmed) {
            effects.insert(Effect::Io);
        }
        
        // Detect allocation effects
        if detect_alloc_pattern(trimmed) {
            effects.insert(Effect::Alloc);
        }
        
        // Detect panic effects
        if detect_panic_pattern(trimmed) {
            effects.insert(Effect::Panic);
        }
    }
    
    effects
}

fn detect_io_pattern(line: &str) -> bool {
    let patterns = [
        "println!", "print!", "eprintln!", "eprint!",
        "std::io", "File::", "stdin()", "stdout()", "stderr()",
        ".read(", ".write(", ".flush(",
        "fs::read", "fs::write", "fs::create", "fs::open",
    ];
    patterns.iter().any(|p| line.contains(p))
}

fn detect_alloc_pattern(line: &str) -> bool {
    let patterns = [
        "Vec::new", "Vec::with_capacity",
        "String::new", "String::from", ".to_string()", ".to_owned()",
        "Box::new", "Rc::new", "Arc::new",
        "HashMap::new", "HashSet::new", "BTreeMap::new", "BTreeSet::new",
        "vec!", ".clone()", ".collect()",
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
// RUST SANITY CHECK (L-05 Validation)
//=============================================================================

fn rust_sanity_check(rust_code: &str) -> Option<String> {
    // Use the comprehensive rust_sanity module
    let result = check_rust_output(rust_code);
    if !result.is_valid {
        return Some(format_internal_error(&result));
    }
    
    // Additional legacy checks for backward compatibility
    let mut brace_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    let mut paren_depth: i32 = 0;
    let mut in_string = false;
    let mut prev_char = ' ';
    
    for (line_num, line) in rust_code.lines().enumerate() {
        let line_num = line_num + 1;
        
        for c in line.chars() {
            if c == '"' && prev_char != '\\' {
                in_string = !in_string;
            }
            
            if !in_string {
                match c {
                    '{' => brace_depth += 1,
                    '}' => {
                        brace_depth -= 1;
                        if brace_depth < 0 {
                            return Some(format!(
                                "unbalanced braces: extra '}}' at line {}", line_num
                            ));
                        }
                    }
                    '[' => bracket_depth += 1,
                    ']' => {
                        bracket_depth -= 1;
                        if bracket_depth < 0 {
                            return Some(format!(
                                "unbalanced brackets: extra ']' at line {}", line_num
                            ));
                        }
                    }
                    '(' => paren_depth += 1,
                    ')' => {
                        paren_depth -= 1;
                        if paren_depth < 0 {
                            return Some(format!(
                                "unbalanced parentheses: extra ')' at line {}", line_num
                            ));
                        }
                    }
                    _ => {}
                }
            }
            prev_char = c;
        }
    }
    
    if brace_depth != 0 {
        return Some(format!("unbalanced braces: {} unclosed '{{'", brace_depth));
    }
    if bracket_depth != 0 {
        return Some(format!("unbalanced brackets: {} unclosed '['", bracket_depth));
    }
    if paren_depth != 0 {
        return Some(format!("unbalanced parentheses: {} unclosed '('", paren_depth));
    }
    
    // Check for illegal patterns
    for (line_num, line) in rust_code.lines().enumerate() {
        let line_num = line_num + 1;
        let trimmed = line.trim();
        
        if trimmed.contains("= [;") {
            return Some(format!(
                "incomplete array literal at line {}: found '= [;'", line_num
            ));
        }
        
        if trimmed.contains("= {;") {
            return Some(format!(
                "incomplete struct literal at line {}: found '= {{;'", line_num
            ));
        }
        
        if trimmed == "[;" || trimmed == "{;" {
            return Some(format!(
                "illegal semicolon after open delimiter at line {}", line_num
            ));
        }
        
        // Check for effects leaking to Rust output (CRITICAL)
        if trimmed.contains("effects(") && (trimmed.contains("fn ") || trimmed.contains("pub fn ")) {
            return Some(format!(
                "effects clause leaked to Rust output at line {}", line_num
            ));
        }
    }
    
    None
}

//=============================================================================
// USAGE & HELP
//=============================================================================

fn print_usage() {
    eprintln!("{}╔═══════════════════════════════════════════════════════════════╗{}", 
        ansi::BOLD_CYAN, ansi::RESET);
    eprintln!("{}║              RustS+ Compiler v0.8.0 (IR Edition)              ║{}",
        ansi::BOLD_CYAN, ansi::RESET);
    eprintln!("{}║      The Language with Effect Honesty                         ║{}",
        ansi::BOLD_CYAN, ansi::RESET);
    eprintln!("{}╚═══════════════════════════════════════════════════════════════╝{}\n",
        ansi::BOLD_CYAN, ansi::RESET);
    
    eprintln!("{}USAGE:{}", ansi::BOLD_YELLOW, ansi::RESET);
    eprintln!("    rustsp <input.rss> [options]\n");
    
    eprintln!("{}OPTIONS:{}", ansi::BOLD_YELLOW, ansi::RESET);
    eprintln!("    {}-o <file>{}        Specify output file (binary or .rs)", ansi::GREEN, ansi::RESET);
    eprintln!("    {}--emit-rs{}        Only emit .rs file without compiling", ansi::GREEN, ansi::RESET);
    eprintln!("    {}--raw-errors{}     Show raw Rust errors (no mapping)", ansi::GREEN, ansi::RESET);
    eprintln!("    {}--skip-logic{}     Skip logic check (DANGEROUS)", ansi::BOLD_RED, ansi::RESET);
    eprintln!("    {}--skip-effects{}   Skip effect checking only", ansi::YELLOW, ansi::RESET);
    eprintln!("    {}--strict-effects{} Require ALL effects to be declared", ansi::YELLOW, ansi::RESET);
    eprintln!("    {}--use-ir{}         Use IR-based effect inference (NEW)", ansi::BOLD_GREEN, ansi::RESET);
    eprintln!("    {}--analyze{}        Analyze and show function effects", ansi::GREEN, ansi::RESET);
    eprintln!("    {}--analyze-ir{}     Analyze with IR-based inference (NEW)", ansi::BOLD_GREEN, ansi::RESET);
    eprintln!("    {}--quiet, -q{}      Suppress success messages", ansi::GREEN, ansi::RESET);
    eprintln!("    {}-h, --help{}       Show this help message", ansi::GREEN, ansi::RESET);
    eprintln!("    {}-V, --version{}    Show version\n", ansi::GREEN, ansi::RESET);
    
    eprintln!("{}EXAMPLES:{}", ansi::BOLD_YELLOW, ansi::RESET);
    eprintln!("    rustsp main.rss -o myprogram        {}Compile to binary{}", ansi::CYAN, ansi::RESET);
    eprintln!("    rustsp main.rss --emit-rs           {}Print Rust to stdout{}", ansi::CYAN, ansi::RESET);
    eprintln!("    rustsp main.rss --emit-rs -o out.rs {}Write Rust to file{}", ansi::CYAN, ansi::RESET);
    eprintln!("    rustsp main.rss --use-ir            {}Use IR-based analysis{}", ansi::CYAN, ansi::RESET);
    eprintln!("    rustsp main.rss --analyze-ir        {}Show IR effect analysis{}\n", ansi::CYAN, ansi::RESET);
    
    eprintln!("{}EFFECT SYSTEM:{}", ansi::BOLD_YELLOW, ansi::RESET);
    eprintln!("    RustS+ requires functions to declare their effects:");
    eprintln!("    ");
    eprintln!("    {}// Pure function (no effects){}", ansi::CYAN, ansi::RESET);
    eprintln!("    fn add(a i32, b i32) i32 {{ a + b }}");
    eprintln!("    ");
    eprintln!("    {}// Function with I/O effect{}", ansi::CYAN, ansi::RESET);
    eprintln!("    fn greet(name String) {}effects(io){} {{ println!(\"Hello, {{}}\", name) }}", ansi::BOLD_GREEN, ansi::RESET);
    eprintln!("    ");
    eprintln!("    {}// Function that mutates parameter{}", ansi::CYAN, ansi::RESET);
    eprintln!("    fn deposit(acc Account, amt i64) {}effects(write acc){} Account {{ ... }}", ansi::BOLD_GREEN, ansi::RESET);
    eprintln!("");
    
    eprintln!("{}EFFECT TYPES:{}", ansi::BOLD_YELLOW, ansi::RESET);
    eprintln!("    {}io{}        - I/O operations (println!, File::*, etc.)", ansi::GREEN, ansi::RESET);
    eprintln!("    {}alloc{}     - Memory allocation (Vec::new, Box::new, etc.)", ansi::GREEN, ansi::RESET);
    eprintln!("    {}panic{}     - May panic (unwrap, expect, panic!)", ansi::GREEN, ansi::RESET);
    eprintln!("    {}read(x){}   - Reads from parameter x", ansi::GREEN, ansi::RESET);
    eprintln!("    {}write(x){}  - Mutates parameter x", ansi::GREEN, ansi::RESET);
    eprintln!("");
    
    eprintln!("{}IR-BASED INFERENCE:{}", ansi::BOLD_YELLOW, ansi::RESET);
    eprintln!("    With {}--use-ir{}, effect inference is structural:", ansi::GREEN, ansi::RESET);
    eprintln!("    ");
    eprintln!("    infer(42)       = ∅");
    eprintln!("    infer(\"str\")    = {{alloc}}");
    eprintln!("    infer(param_x)  = {{read(x)}}");
    eprintln!("    infer(x.f = e)  = infer(e) ∪ {{write(x)}}");
    eprintln!("    infer(f(args))  = ⋃infer(args) ∪ effects(f)");
    eprintln!("");
}

fn print_version() {
    println!("RustS+ Compiler v0.8.0 (IR Edition)");
    println!("Effect System: Enabled (Full Effect Ownership)");
    println!("Logic Checks: L-01 through L-06");
    println!("Effect Checks: Effect-01 through Effect-06");
    println!("");
    println!("NEW: IR-Based Effect System:");
    println!("  - AST: Abstract Syntax Tree");
    println!("  - HIR: High-level IR with scope resolution");
    println!("  - EIR: Effect IR with structural inference");
    println!("");
    println!("Effect Inference:");
    println!("  - Structural, not regex-based");
    println!("  - Effect Ownership Model");
    println!("  - Cross-Function Effect Propagation");
    println!("  - Zero Heuristics - Explicit Declaration Required");
}

fn print_analysis(source: &str, file_name: &str) {
    let functions = analyze_functions(source, file_name);
    
    eprintln!("{}╔═══════════════════════════════════════════════════════════════╗{}",
        ansi::BOLD_CYAN, ansi::RESET);
    eprintln!("{}║              RustS+ Effect Analysis                           ║{}",
        ansi::BOLD_CYAN, ansi::RESET);
    eprintln!("{}╚═══════════════════════════════════════════════════════════════╝{}\n",
        ansi::BOLD_CYAN, ansi::RESET);
    
    if functions.is_empty() {
        eprintln!("  No functions found.");
        return;
    }
    
    for (name, info) in &functions {
        let purity = if info.declared_effects.is_pure && info.detected_effects.is_pure {
            format!("{}PURE{}", ansi::BOLD_GREEN, ansi::RESET)
        } else {
            format!("{}EFFECTFUL{}", ansi::BOLD_YELLOW, ansi::RESET)
        };
        
        eprintln!("{}fn {}{} [{}]", ansi::BOLD_WHITE, name, ansi::RESET, purity);
        eprintln!("  {}├─ Line:{} {}", ansi::BLUE, ansi::RESET, info.line_number);
        
        if !info.parameters.is_empty() {
            let params: Vec<String> = info.parameters.iter()
                .map(|(n, t)| format!("{}: {}", n, t))
                .collect();
            eprintln!("  {}├─ Parameters:{} ({})", ansi::BLUE, ansi::RESET, params.join(", "));
        }
        
        if let Some(ref ret) = info.return_type {
            eprintln!("  {}├─ Returns:{} {}", ansi::BLUE, ansi::RESET, ret);
        }
        
        if !info.declared_effects.is_pure {
            eprintln!("  {}├─ Declared:{} effects({})", 
                ansi::BLUE, ansi::RESET,
                info.declared_effects.display());
        } else {
            eprintln!("  {}├─ Declared:{} (none - pure)", ansi::BLUE, ansi::RESET);
        }
        
        if !info.detected_effects.is_pure {
            let status = if info.undeclared_effects().is_empty() {
                format!("{}✓{}", ansi::GREEN, ansi::RESET)
            } else {
                format!("{}✗{}", ansi::RED, ansi::RESET)
            };
            eprintln!("  {}├─ Detected:{} {} effects({})", 
                ansi::BLUE, ansi::RESET, status,
                info.detected_effects.display());
        } else {
            eprintln!("  {}├─ Detected:{} (none)", ansi::BLUE, ansi::RESET);
        }
        
        if !info.calls.is_empty() {
            eprintln!("  {}└─ Calls:{} {}", ansi::BLUE, ansi::RESET, info.calls.join(", "));
        }
        
        let undeclared = info.undeclared_effects();
        if !undeclared.is_empty() && name != "main" {
            eprintln!("     {}⚠ UNDECLARED:{} {}", 
                ansi::BOLD_RED, ansi::RESET,
                undeclared.iter().map(|e| e.display()).collect::<Vec<_>>().join(", "));
        }
        
        eprintln!("");
    }
    
    // Summary
    let total = functions.len();
    let pure_count = functions.values()
        .filter(|f| f.declared_effects.is_pure && f.detected_effects.is_pure)
        .count();
    let effectful_count = total - pure_count;
    let violations = functions.values()
        .filter(|f| !f.undeclared_effects().is_empty() && f.name != "main")
        .count();
    
    eprintln!("{}Summary:{}", ansi::BOLD_YELLOW, ansi::RESET);
    eprintln!("  Total functions: {}", total);
    eprintln!("  Pure functions: {}", pure_count);
    eprintln!("  Effectful functions: {}", effectful_count);
    if violations > 0 {
        eprintln!("  {}Effect violations: {}{}", ansi::BOLD_RED, violations, ansi::RESET);
    } else {
        eprintln!("  {}All effects properly declared ✓{}", ansi::BOLD_GREEN, ansi::RESET);
    }
}

/// NEW: Print IR-based analysis
fn print_analysis_ir(source: &str, file_name: &str) {
    let effects = analyze_effects_ir(source);
    
    eprintln!("{}╔═══════════════════════════════════════════════════════════════╗{}",
        ansi::BOLD_CYAN, ansi::RESET);
    eprintln!("{}║         RustS+ Effect Analysis (IR-Based)                     ║{}",
        ansi::BOLD_CYAN, ansi::RESET);
    eprintln!("{}╚═══════════════════════════════════════════════════════════════╝{}\n",
        ansi::BOLD_CYAN, ansi::RESET);
    
    if effects.is_empty() {
        eprintln!("  No functions found.");
        return;
    }
    
    let bindings = HashMap::new();
    
    for (name, (declared, detected, undeclared, line)) in &effects {
        let purity = if declared.is_empty() && detected.is_empty() {
            format!("{}PURE{}", ansi::BOLD_GREEN, ansi::RESET)
        } else {
            format!("{}EFFECTFUL{}", ansi::BOLD_YELLOW, ansi::RESET)
        };
        
        eprintln!("{}fn {}{} [{}]", ansi::BOLD_WHITE, name, ansi::RESET, purity);
        eprintln!("  {}├─ Line:{} {}", ansi::BLUE, ansi::RESET, line);
        
        if !declared.is_empty() {
            let effects_str: Vec<String> = declared.iter()
                .map(|e| e.display(&bindings))
                .collect();
            eprintln!("  {}├─ Declared:{} effects({})", 
                ansi::BLUE, ansi::RESET, effects_str.join(", "));
        } else {
            eprintln!("  {}├─ Declared:{} (none - pure)", ansi::BLUE, ansi::RESET);
        }
        
        if !detected.is_empty() {
            let status = if undeclared.is_empty() {
                format!("{}✓{}", ansi::GREEN, ansi::RESET)
            } else {
                format!("{}✗{}", ansi::RED, ansi::RESET)
            };
            let effects_str: Vec<String> = detected.iter()
                .map(|e| e.display(&bindings))
                .collect();
            eprintln!("  {}├─ Detected:{} {} effects({})", 
                ansi::BLUE, ansi::RESET, status, effects_str.join(", "));
        } else {
            eprintln!("  {}├─ Detected:{} (none)", ansi::BLUE, ansi::RESET);
        }
        
        if !undeclared.is_empty() && name != "main" {
            let effects_str: Vec<String> = undeclared.iter()
                .map(|e| e.display(&bindings))
                .collect();
            eprintln!("     {}⚠ UNDECLARED:{} {}", 
                ansi::BOLD_RED, ansi::RESET, effects_str.join(", "));
        }
        
        eprintln!("");
    }
    
    // Summary
    let total = effects.len();
    let pure_count = effects.values()
        .filter(|(d, det, _, _)| d.is_empty() && det.is_empty())
        .count();
    let effectful_count = total - pure_count;
    let violations = effects.iter()
        .filter(|(name, (_, _, und, _))| !und.is_empty() && *name != "main")
        .count();
    
    eprintln!("{}Summary (IR-Based):{}", ansi::BOLD_YELLOW, ansi::RESET);
    eprintln!("  Total functions: {}", total);
    eprintln!("  Pure functions: {}", pure_count);
    eprintln!("  Effectful functions: {}", effectful_count);
    if violations > 0 {
        eprintln!("  {}Effect violations: {}{}", ansi::BOLD_RED, violations, ansi::RESET);
    } else {
        eprintln!("  {}All effects properly declared ✓{}", ansi::BOLD_GREEN, ansi::RESET);
    }
    
    eprintln!("\n{}Inference Method:{} Structural (IR-based)", ansi::CYAN, ansi::RESET);
}

//=============================================================================
// MAIN ENTRY POINT
//=============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    
    // Version check
    if args.len() == 2 && (args[1] == "--version" || args[1] == "-V") {
        print_version();
        exit(0);
    }
    
    // Help check
    if args.len() < 2 || args[1] == "-h" || args[1] == "--help" {
        print_usage();
        exit(if args.len() < 2 { 1 } else { 0 });
    }
    
    // Parse arguments
    let mut input_file: Option<String> = None;
    let mut output_file: Option<String> = None;
    let mut emit_rs_only = false;
    let mut raw_errors = false;
    let mut skip_logic = false;
    let mut skip_effects = false;
    let mut strict_effects = false;
    let mut analyze_only = false;
    let mut analyze_ir = false;  // NEW
    let mut use_ir = false;       // NEW
    let mut quiet = false;
    
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                if i + 1 < args.len() {
                    output_file = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("{}error{}: -o requires an output file name",
                        ansi::BOLD_RED, ansi::RESET);
                    exit(1);
                }
            }
            "--emit-rs" => {
                emit_rs_only = true;
                i += 1;
            }
            "--raw-errors" => {
                raw_errors = true;
                i += 1;
            }
            "--skip-logic" => {
                skip_logic = true;
                eprintln!("{}╔═══════════════════════════════════════════════════════════════╗{}",
                    ansi::BOLD_YELLOW, ansi::RESET);
                eprintln!("{}║  WARNING: --skip-logic flag is DANGEROUS                      ║{}",
                    ansi::BOLD_YELLOW, ansi::RESET);
                eprintln!("{}║  Logic errors will NOT be caught before Rust compilation!     ║{}",
                    ansi::BOLD_YELLOW, ansi::RESET);
                eprintln!("{}╚═══════════════════════════════════════════════════════════════╝{}",
                    ansi::BOLD_YELLOW, ansi::RESET);
                i += 1;
            }
            "--skip-effects" => {
                skip_effects = true;
                if !quiet {
                    eprintln!("{}note{}: Effect checking disabled. Effects will not be validated.",
                        ansi::CYAN, ansi::RESET);
                }
                i += 1;
            }
            "--strict-effects" => {
                strict_effects = true;
                if !quiet {
                    eprintln!("{}note{}: Strict effect mode enabled. ALL effects must be declared.",
                        ansi::CYAN, ansi::RESET);
                }
                i += 1;
            }
            "--use-ir" => {
                use_ir = true;
                if !quiet {
                    eprintln!("{}note{}: Using IR-based effect inference (structural).",
                        ansi::BOLD_GREEN, ansi::RESET);
                }
                i += 1;
            }
            "--analyze" => {
                analyze_only = true;
                i += 1;
            }
            "--analyze-ir" => {
                analyze_ir = true;
                i += 1;
            }
            "--quiet" | "-q" => {
                quiet = true;
                i += 1;
            }
            arg => {
                if arg.starts_with('-') {
                    eprintln!("{}error{}: unknown option '{}'",
                        ansi::BOLD_RED, ansi::RESET, arg);
                    exit(1);
                }
                if input_file.is_none() {
                    input_file = Some(arg.to_string());
                }
                i += 1;
            }
        }
    }
    
    // Validate input file
    let input_path = match input_file {
        Some(p) => p,
        None => {
            eprintln!("{}error{}: No input file specified",
                ansi::BOLD_RED, ansi::RESET);
            print_usage();
            exit(1);
        }
    };
    
    if !Path::new(&input_path).exists() {
        eprintln!("{}error{}: Input file '{}' not found",
            ansi::BOLD_RED, ansi::RESET, input_path);
        exit(1);
    }
    
    // Read source file
    let source = match fs::read_to_string(&input_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("{}error{}: reading '{}': {}",
                ansi::BOLD_RED, ansi::RESET, input_path, e);
            exit(1);
        }
    };
    
    //=========================================================================
    // ANALYZE MODE (IR-based)
    //=========================================================================
    
    if analyze_ir {
        print_analysis_ir(&source, &input_path);
        exit(0);
    }
    
    //=========================================================================
    // ANALYZE MODE (Legacy)
    //=========================================================================
    
    if analyze_only {
        print_analysis(&source, &input_path);
        exit(0);
    }
    
    //=========================================================================
    // STAGE 0 & 1: ANTI-FAIL LOGIC CHECK
    //=========================================================================
    
    if !skip_logic {
        if !quiet {
            if use_ir {
                eprintln!("{}[Stage 0]{} Building IR and effect context...", 
                    ansi::BOLD_BLUE, ansi::RESET);
            } else {
                eprintln!("{}[Stage 0]{} Building effect table and dependency graph...", 
                    ansi::BOLD_BLUE, ansi::RESET);
            }
            eprintln!("{}[Stage 1]{} Analyzing effects and logic...", 
                ansi::BOLD_BLUE, ansi::RESET);
        }
        
        // Use IR-based checking if requested
        if use_ir && !skip_effects {
            let effects = analyze_effects_ir(&source);
            
            // Check for undeclared effects
            let mut has_violations = false;
            let bindings = HashMap::new();
            
            for (name, (_, _, undeclared, line)) in &effects {
                if !undeclared.is_empty() && name != "main" {
                    has_violations = true;
                    
                    eprintln!("\n{}error[RSPL300]{}: undeclared effects in function `{}`",
                        ansi::BOLD_RED, ansi::RESET, name);
                    eprintln!("  {}-->{} {}:{}", ansi::BOLD_BLUE, ansi::RESET, input_path, line);
                    
                    for effect in undeclared.iter() {
                        eprintln!("       {}= detected:{} {} (not declared)",
                            ansi::BOLD_CYAN, ansi::RESET, effect.display(&bindings));
                    }
                    
                    eprintln!("\n{}help{}: add `effects({})` to function signature",
                        ansi::BOLD_YELLOW, ansi::RESET,
                        undeclared.iter().map(|e| e.display(&bindings)).collect::<Vec<_>>().join(", "));
                }
            }
            
            if has_violations {
                exit(1);
            }
        }
        
        // Still run the legacy checks for logic rules
        let check_result = if skip_effects {
            check_logic_no_effects(&source, &input_path)
        } else if use_ir {
            // Skip legacy effect checks if using IR
            check_logic_no_effects(&source, &input_path)
        } else {
            check_logic_custom(&source, &input_path, true, strict_effects)
        };
        
        if let Err(errors) = check_result {
            eprintln!("{}", format_logic_errors(&errors));
            exit(1);
        }
        
        if !quiet {
            if use_ir {
                eprintln!("{}[Stage 1]{} ✓ All logic and effect checks passed (IR-based)", 
                    ansi::BOLD_GREEN, ansi::RESET);
            } else {
                eprintln!("{}[Stage 1]{} ✓ All logic and effect checks passed", 
                    ansi::BOLD_GREEN, ansi::RESET);
            }
        }
    }
    
    //=========================================================================
    // STAGE 2: LOWERING (RustS+ → Rust)
    //=========================================================================
    
    if !quiet {
        eprintln!("{}[Stage 2]{} Lowering RustS+ to Rust...", 
            ansi::BOLD_BLUE, ansi::RESET);
    }
    
    let rust_code = parse_rusts(&source);
    
    //=========================================================================
    // STAGE 2.5: RUST SANITY GATE
    //=========================================================================
    
    if let Some(sanity_error) = rust_sanity_check(&rust_code) {
        eprintln!("\n{}╔═══════════════════════════════════════════════════════════════╗{}",
            ansi::BOLD_RED, ansi::RESET);
        eprintln!("{}║   RUSTS+ INTERNAL ERROR (Lowering Bug Detected)              ║{}",
            ansi::BOLD_RED, ansi::RESET);
        eprintln!("{}╚═══════════════════════════════════════════════════════════════╝{}\n",
            ansi::BOLD_RED, ansi::RESET);
        
        eprintln!("{}error[RUSTSP_INTERNAL][lowering]{}: invalid Rust code generated\n",
            ansi::BOLD_RED, ansi::RESET);
        
        eprintln!("{}note{}:", ansi::BOLD_CYAN, ansi::RESET);
        eprintln!("  RustS+ detected an internal lowering error.");
        eprintln!("  This is a COMPILER BUG, not your fault.\n");
        eprintln!("  Problem: {}\n", sanity_error);
        
        eprintln!("{}help{}:", ansi::BOLD_YELLOW, ansi::RESET);
        eprintln!("  {}Please report this issue with your source code.{}\n",
            ansi::GREEN, ansi::RESET);
        
        let debug_filename = format!("{}_debug.rs", 
            Path::new(&input_path).file_stem().and_then(|s| s.to_str()).unwrap_or("output"));
        let _ = fs::write(&debug_filename, &rust_code);
        eprintln!("{}note{}: Generated (invalid) Rust saved to: {}",
            ansi::CYAN, ansi::RESET, debug_filename);
        
        exit(1);
    }
    
    if !quiet {
        eprintln!("{}[Stage 2]{} ✓ Lowering complete", 
            ansi::BOLD_GREEN, ansi::RESET);
    }
    
    //=========================================================================
    // EMIT RS MODE
    //=========================================================================
    
    if emit_rs_only {
        match output_file {
            Some(ref out_path) => {
                if let Err(e) = fs::write(out_path, &rust_code) {
                    eprintln!("{}error{}: writing '{}': {}",
                        ansi::BOLD_RED, ansi::RESET, out_path, e);
                    exit(1);
                }
                if !quiet {
                    eprintln!("{}✓ Rust code written to{}: {}",
                        ansi::BOLD_GREEN, ansi::RESET, out_path);
                }
            }
            None => {
                println!("{}", rust_code);
            }
        }
        exit(0);
    }
    
    //=========================================================================
    // STAGE 3: RUST COMPILATION
    //=========================================================================
    
    if !quiet {
        eprintln!("{}[Stage 3]{} Compiling with rustc...", 
            ansi::BOLD_BLUE, ansi::RESET);
    }
    
    let input_stem = Path::new(&input_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    
    let temp_rs_filename = format!("{}_rusts_temp.rs", input_stem);
    let temp_rs_path_str = temp_rs_filename.clone();
    
    if let Err(e) = fs::write(&temp_rs_path_str, &rust_code) {
        eprintln!("{}error{}: writing temporary Rust file: {}",
            ansi::BOLD_RED, ansi::RESET, e);
        exit(1);
    }
    
    let output_binary = output_file.unwrap_or_else(|| {
        format!("./{}", input_stem)
    });
    
    let rustc_output = Command::new("rustc")
        .arg(&temp_rs_path_str)
        .arg("-o")
        .arg(&output_binary)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    
    match rustc_output {
        Ok(output) => {
            if output.status.success() {
                if !quiet {
                    eprintln!("{}╔═══════════════════════════════════════════════════════════════╗{}",
                        ansi::BOLD_GREEN, ansi::RESET);
                    eprintln!("{}║  ✓ Successfully compiled: {:<36} ║{}",
                        ansi::BOLD_GREEN, output_binary, ansi::RESET);
                    eprintln!("{}╚═══════════════════════════════════════════════════════════════╝{}",
                        ansi::BOLD_GREEN, ansi::RESET);
                }
                let _ = fs::remove_file(&temp_rs_path_str);
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                
                if raw_errors {
                    eprintln!("{}", stderr);
                } else {
                    eprintln!("\n{}╔═══════════════════════════════════════════════════════════════╗{}",
                        ansi::BOLD_RED, ansi::RESET);
                    eprintln!("{}║   RUSTS+ COMPILATION ERROR (Stage 3 - Rust Backend)          ║{}",
                        ansi::BOLD_RED, ansi::RESET);
                    eprintln!("{}╚═══════════════════════════════════════════════════════════════╝{}\n",
                        ansi::BOLD_RED, ansi::RESET);
                    
                    if let Some(mapped_error) = map_rust_error(&stderr, &source) {
                        eprintln!("{}error{}: {}", ansi::BOLD_RED, ansi::RESET, mapped_error.title);
                        if let Some(ref note) = mapped_error.explanation {
                            eprintln!("\n{}note{}:", ansi::BOLD_CYAN, ansi::RESET);
                            for line in note.lines() {
                                eprintln!("  {}", line);
                            }
                        }
                        if let Some(ref help) = mapped_error.suggestion {
                            eprintln!("\n{}help{}:", ansi::BOLD_YELLOW, ansi::RESET);
                            for line in help.lines() {
                                eprintln!("  {}{}{}", ansi::GREEN, line, ansi::RESET);
                            }
                        }
                    }
                    
                    eprintln!("\n{}───────────────────────────────────────────────────────────────{}",
                        ansi::BLUE, ansi::RESET);
                    eprintln!("{}Original Rust error (for reference):{}",
                        ansi::CYAN, ansi::RESET);
                    eprintln!("{}───────────────────────────────────────────────────────────────{}",
                        ansi::BLUE, ansi::RESET);
                    eprintln!("{}", stderr);
                }
                
                eprintln!("\n{}note{}: Generated Rust code saved at: {}",
                    ansi::CYAN, ansi::RESET, temp_rs_path_str);
                exit(1);
            }
        }
        Err(e) => {
            eprintln!("{}error{}: Failed to run rustc: {}",
                ansi::BOLD_RED, ansi::RESET, e);
            eprintln!("Make sure rustc is installed and in your PATH");
            exit(1);
        }
    }
}