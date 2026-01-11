//! RustS+ Compiler - Main Entry Point
//!
//! Pipeline kompilasi:
//! 1. STAGE 1: Anti-Fail Logic Check (SEBELUM lowering)
//! 2. STAGE 2: Lowering RustS+ → Rust + Sanity Check (L-05)
//! 3. STAGE 3: Rust Compilation (rustc)

use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio, exit};

// NOTE: Change this import to match your crate name in Cargo.toml
// If your crate is named "rustsp", use: use rustsp::...
// If your crate is named "rusts_plus", use: use rusts_plus::...
use rustsp::parse_rusts;
use rustsp::error_msg::map_rust_error;
use rustsp::anti_fail_logic::{check_logic, format_logic_errors, ansi};
use rustsp::rust_sanity::{check_rust_output, format_internal_error};

//=============================================================================
// L-05: RUST SANITY GATE (Enhanced)
// Validates generated Rust code before calling rustc
// Uses the rust_sanity module for comprehensive validation
// Returns Some(error_message) if invalid, None if OK
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
        let line_num = line_num + 1; // 1-indexed
        
        for c in line.chars() {
            // Track string context
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
    
    // Check final balance
    if brace_depth != 0 {
        return Some(format!(
            "unbalanced braces: {} unclosed '{{'", brace_depth
        ));
    }
    if bracket_depth != 0 {
        return Some(format!(
            "unbalanced brackets: {} unclosed '['", bracket_depth
        ));
    }
    if paren_depth != 0 {
        return Some(format!(
            "unbalanced parentheses: {} unclosed '('", paren_depth
        ));
    }
    
    // Check for illegal patterns
    for (line_num, line) in rust_code.lines().enumerate() {
        let line_num = line_num + 1;
        let trimmed = line.trim();
        
        // Pattern: `= [;` - incomplete array literal
        if trimmed.contains("= [;") {
            return Some(format!(
                "incomplete array literal at line {}: found '= [;'", line_num
            ));
        }
        
        // Pattern: `= {;` - incomplete struct literal
        if trimmed.contains("= {;") {
            return Some(format!(
                "incomplete struct literal at line {}: found '= {{;'", line_num
            ));
        }
        
        // Pattern: lonely semicolon after open bracket
        if trimmed == "[;" || trimmed == "{;" {
            return Some(format!(
                "illegal semicolon after open delimiter at line {}", line_num
            ));
        }
    }
    
    None // All checks passed
}

fn print_usage() {
    eprintln!("{}RustS+ Compiler{}", ansi::BOLD_CYAN, ansi::RESET);
    eprintln!("Usage: rustsp <input.rss> [options]");
    eprintln!("");
    eprintln!("Options:");
    eprintln!("  -o <file>        Specify output file (binary or .rs with --emit-rs)");
    eprintln!("  --emit-rs        Only emit .rs file without compiling");
    eprintln!("  --raw-errors     Show raw Rust errors (no mapping)");
    eprintln!("  --skip-logic     Skip anti-fail logic check (DANGEROUS)");
    eprintln!("  --quiet          Suppress success messages (for tooling)");
    eprintln!("  -h, --help       Show this help message");
    eprintln!("  -V, --version    Show version");
    eprintln!("");
    eprintln!("Examples:");
    eprintln!("  rustsp main.rss -o myprogram        Compile to binary");
    eprintln!("  rustsp main.rss --emit-rs           Print Rust to stdout");
    eprintln!("  rustsp main.rss --emit-rs -o out.rs Write Rust to file");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() == 2 && (args[1] == "--version" || args[1] == "-V") {
        println!("RustS+ Compiler v0.3.0");
        exit(0);
    }

    if args.len() < 2 {
        print_usage();
        exit(1);
    }
    
    if args[1] == "-h" || args[1] == "--help" {
        print_usage();
        exit(0);
    }
    
    let mut input_file: Option<String> = None;
    let mut output_file: Option<String> = None;
    let mut emit_rs_only = false;
    let mut raw_errors = false;
    let mut skip_logic = false;
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
                eprintln!("{}WARNING{}: --skip-logic flag is DANGEROUS. Logic errors will NOT be caught.",
                    ansi::BOLD_YELLOW, ansi::RESET);
                i += 1;
            }
            "--quiet" | "-q" => {
                quiet = true;
                i += 1;
            }
            arg => {
                if arg.starts_with("-") {
                    eprintln!("{}error{}: Unknown option '{}'",
                        ansi::BOLD_RED, ansi::RESET, arg);
                    exit(1);
                }
                if arg.ends_with(".rss") || arg.ends_with(".rs") {
                    input_file = Some(arg.to_string());
                } else if input_file.is_none() {
                    input_file = Some(arg.to_string());
                }
                i += 1;
            }
        }
    }
    
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
    
    let source = match fs::read_to_string(&input_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("{}error{}: reading '{}': {}",
                ansi::BOLD_RED, ansi::RESET, input_path, e);
            exit(1);
        }
    };
    
    //=========================================================================
    // STAGE 1: ANTI-FAIL LOGIC CHECK
    //=========================================================================
    // Ini adalah GATE PERTAMA. Jika ada logic error, kompilasi BERHENTI DI SINI.
    // Kode yang tidak jujur TIDAK akan diteruskan ke Rust.
    
    if !skip_logic {
        if let Err(errors) = check_logic(&source, &input_path) {
            // ╔═══════════════════════════════════════════════════════════════╗
            // ║  LOGIC ERRORS FOUND - STOP COMPILATION                        ║
            // ║  Do NOT forward dishonest code to rustc                       ║
            // ╚═══════════════════════════════════════════════════════════════╝
            eprintln!("{}", format_logic_errors(&errors));
            exit(1);
        }
    }
    
    //=========================================================================
    // STAGE 2: LOWERING (RustS+ → Rust)
    //=========================================================================
    // Hanya tercapai jika Stage 1 lolos
    
    let rust_code = parse_rusts(&source);
    
    //=========================================================================
    // STAGE 2.5: RUST SANITY GATE (Internal Validation)
    //=========================================================================
    // Validates that generated Rust is syntactically sound before calling rustc
    // If this fails, it's a COMPILER BUG, not a user error
    
    if let Some(sanity_error) = rust_sanity_check(&rust_code) {
        eprintln!("\n{}══════════════════════════════════════════════════════════════════{}",
            ansi::BOLD_RED, ansi::RESET);
        eprintln!("{}█   RUSTS+ INTERNAL ERROR (Lowering Bug Detected)   █{}",
            ansi::BOLD_RED, ansi::RESET);
        eprintln!("{}══════════════════════════════════════════════════════════════════{}\n",
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
        
        // Save the broken Rust code for debugging
        let debug_filename = format!("{}_debug.rs", 
            Path::new(&input_path).file_stem().and_then(|s| s.to_str()).unwrap_or("output"));
        let _ = fs::write(&debug_filename, &rust_code);
        eprintln!("{}note{}: Generated (invalid) Rust saved to: {}",
            ansi::CYAN, ansi::RESET, debug_filename);
        
        exit(1);
    }
    
    //=========================================================================
    // EMIT RS MODE: Write Rust code to file or stdout
    //=========================================================================
    
    if emit_rs_only {
        match output_file {
            Some(ref out_path) => {
                // Write to specified file
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
                // Print to stdout
                println!("{}", rust_code);
            }
        }
        exit(0);
    }
    
    //=========================================================================
    // BINARY COMPILATION MODE
    //=========================================================================
    
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
    
    //=========================================================================
    // STAGE 3: RUST COMPILATION (rustc)
    //=========================================================================
    // Hanya tercapai jika Stage 1 dan Stage 2 lolos
    
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
                    println!("{}✓ Successfully compiled{}: {}",
                        ansi::BOLD_GREEN, ansi::RESET, output_binary);
                }
                let _ = fs::remove_file(&temp_rs_path_str);
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                
                if raw_errors {
                    // Show raw Rust errors
                    eprintln!("{}", stderr);
                } else {
                    // Map Rust errors to RustS+ errors dengan warna
                    eprintln!("\n{}══════════════════════════════════════════════════════════════════{}",
                        ansi::BOLD_RED, ansi::RESET);
                    eprintln!("{}█   RUSTS+ COMPILATION ERROR (Stage 2 - Rust Backend)   █{}",
                        ansi::BOLD_RED, ansi::RESET);
                    eprintln!("{}══════════════════════════════════════════════════════════════════{}\n",
                        ansi::BOLD_RED, ansi::RESET);
                    
                    if let Some(mapped_error) = map_rust_error(&stderr, &source) {
                        // Format error dengan warna
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
                    
                    eprintln!("\n{}───────────────────────────────────────────────────────────────────{}",
                        ansi::BLUE, ansi::RESET);
                    eprintln!("{}Original Rust error (for reference):{}",
                        ansi::CYAN, ansi::RESET);
                    eprintln!("{}───────────────────────────────────────────────────────────────────{}",
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