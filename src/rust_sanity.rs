//! Rust Sanity Check Module (L-05)
//! 
//! ATURAN L-05: Jika Stage-1 Lulus ⇒ Rust Output TIDAK BOLEH INVALID
//! 
//! This module validates that the generated Rust code is syntactically valid
//! BEFORE passing it to rustc. If invalid Rust is generated, we emit an
//! INTERNAL COMPILER ERROR instead of letting rustc fail.
//!
//! Checks performed:
//! - Balanced delimiters: (), [], {}
//! - No illegal tokens: bare `mut x = ...` without `let`
//! - No unclosed strings/chars
//! - Valid expression structure

/// Result of sanity check
#[derive(Debug, Clone)]
pub struct SanityCheckResult {
    pub is_valid: bool,
    pub errors: Vec<SanityError>,
}

#[derive(Debug, Clone)]
pub struct SanityError {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub kind: SanityErrorKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SanityErrorKind {
    UnbalancedDelimiter,
    IllegalToken,
    UnclosedString,
    InvalidExpression,
    InternalLoweringError,
    /// L-05: Effect annotations leaked into Rust output
    EffectAnnotationLeakage,
}

impl SanityCheckResult {
    pub fn ok() -> Self {
        SanityCheckResult {
            is_valid: true,
            errors: Vec::new(),
        }
    }
    
    pub fn error(errors: Vec<SanityError>) -> Self {
        SanityCheckResult {
            is_valid: false,
            errors,
        }
    }
}

/// Perform comprehensive sanity check on generated Rust code
pub fn check_rust_output(rust_code: &str) -> SanityCheckResult {
    let mut errors = Vec::new();
    
    // Check 1: Balanced delimiters
    if let Some(err) = check_balanced_delimiters(rust_code) {
        errors.push(err);
    }
    
    // Check 2: Illegal tokens (bare `mut` without `let`)
    errors.extend(check_illegal_tokens(rust_code));
    
    // Check 3: Unclosed strings
    errors.extend(check_unclosed_strings(rust_code));
    
    // Check 4: Invalid patterns that indicate lowering bugs
    errors.extend(check_lowering_patterns(rust_code));
    
    // Check 5: L-05 CRITICAL - Effect annotation leakage
    // Effect annotations must NEVER appear in Rust output
    errors.extend(check_effect_annotation_leakage(rust_code));
    
    if errors.is_empty() {
        SanityCheckResult::ok()
    } else {
        SanityCheckResult::error(errors)
    }
}

/// Check for balanced delimiters: (), [], {}
fn check_balanced_delimiters(code: &str) -> Option<SanityError> {
    let mut paren_stack: Vec<(char, usize, usize)> = Vec::new();
    let mut in_string = false;
    let mut in_char = false;
    let mut escape_next = false;
    
    let lines: Vec<&str> = code.lines().collect();
    
    for (line_num, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let mut col = 0;
        
        while col < chars.len() {
            let ch = chars[col];
            
            // Handle escapes
            if escape_next {
                escape_next = false;
                col += 1;
                continue;
            }
            
            if ch == '\\' && (in_string || in_char) {
                escape_next = true;
                col += 1;
                continue;
            }
            
            // Track string state
            if ch == '"' && !in_char {
                in_string = !in_string;
                col += 1;
                continue;
            }
            
            // CRITICAL FIX: Handle `'` - distinguish char literals from lifetimes
            // Char literal: 'c' or '\n' (quote, char, optional backslash escape, quote)
            // Lifetime: 'ident (quote followed by identifier, NO closing quote)
            if ch == '\'' && !in_string && !in_char {
                // Peek ahead to determine if this is a char literal or lifetime
                if col + 1 < chars.len() {
                    let next = chars[col + 1];
                    
                    // Check for lifetime: 'ident (identifier starts with letter or _)
                    if next.is_alphabetic() || next == '_' {
                        // This is likely a lifetime like 'static, 'a, '_
                        // Skip the tick and identifier
                        col += 1; // skip the '
                        while col < chars.len() && (chars[col].is_alphanumeric() || chars[col] == '_') {
                            col += 1;
                        }
                        continue;
                    }
                    
                    // Check for char literal: 'c' or '\x'
                    // If next is backslash, it's an escape like '\n'
                    if next == '\\' {
                        // Escaped char literal: '\n', '\t', '\x00', etc.
                        // Skip: ' \ x ... '
                        col += 1; // skip '
                        col += 1; // skip \
                        // Skip escape sequence (could be \n, \x00, \u{...})
                        while col < chars.len() && chars[col] != '\'' {
                            col += 1;
                        }
                        if col < chars.len() {
                            col += 1; // skip closing '
                        }
                        continue;
                    }
                    
                    // Regular char literal: 'c' where c is a single char
                    if col + 2 < chars.len() && chars[col + 2] == '\'' {
                        col += 3; // skip 'c'
                        continue;
                    }
                }
                
                // Fallback: toggle in_char mode (legacy behavior)
                in_char = !in_char;
                col += 1;
                continue;
            }
            
            // Handle closing quote for char literals (when in_char mode from fallback)
            if ch == '\'' && in_char {
                in_char = false;
                col += 1;
                continue;
            }
            
            if in_string || in_char {
                col += 1;
                continue;
            }
            
            // Check delimiters
            match ch {
                '(' | '[' | '{' => {
                    paren_stack.push((ch, line_num + 1, col + 1));
                }
                ')' => {
                    if let Some((open, _, _)) = paren_stack.pop() {
                        if open != '(' {
                            return Some(SanityError {
                                line: line_num + 1,
                                column: col + 1,
                                message: format!("Mismatched delimiter: expected closing for '{}', found ')'", open),
                                kind: SanityErrorKind::UnbalancedDelimiter,
                            });
                        }
                    } else {
                        return Some(SanityError {
                            line: line_num + 1,
                            column: col + 1,
                            message: "Unexpected closing ')'".to_string(),
                            kind: SanityErrorKind::UnbalancedDelimiter,
                        });
                    }
                }
                ']' => {
                    if let Some((open, _, _)) = paren_stack.pop() {
                        if open != '[' {
                            return Some(SanityError {
                                line: line_num + 1,
                                column: col + 1,
                                message: format!("Mismatched delimiter: expected closing for '{}', found ']'", open),
                                kind: SanityErrorKind::UnbalancedDelimiter,
                            });
                        }
                    } else {
                        return Some(SanityError {
                            line: line_num + 1,
                            column: col + 1,
                            message: "Unexpected closing ']'".to_string(),
                            kind: SanityErrorKind::UnbalancedDelimiter,
                        });
                    }
                }
                '}' => {
                    if let Some((open, _, _)) = paren_stack.pop() {
                        if open != '{' {
                            return Some(SanityError {
                                line: line_num + 1,
                                column: col + 1,
                                message: format!("Mismatched delimiter: expected closing for '{}', found '}}'", open),
                                kind: SanityErrorKind::UnbalancedDelimiter,
                            });
                        }
                    } else {
                        return Some(SanityError {
                            line: line_num + 1,
                            column: col + 1,
                            message: "Unexpected closing '}'".to_string(),
                            kind: SanityErrorKind::UnbalancedDelimiter,
                        });
                    }
                }
                _ => {}
            }
            col += 1;
        }
    }
    
    if let Some((ch, line, col)) = paren_stack.pop() {
        return Some(SanityError {
            line,
            column: col,
            message: format!("Unclosed delimiter '{}'", ch),
            kind: SanityErrorKind::UnbalancedDelimiter,
        });
    }
    
    None
}

/// Check for illegal tokens that indicate lowering bugs
fn check_illegal_tokens(code: &str) -> Vec<SanityError> {
    let mut errors = Vec::new();
    
    for (line_num, line) in code.lines().enumerate() {
        let trimmed = line.trim();
        
        // Skip comments
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }
        
        // L-01 VIOLATION: bare `mut x = ...` without `let`
        // Pattern: line starts with `mut` but not `let mut`
        if trimmed.starts_with("mut ") && !line.contains("let mut") && !line.contains("&mut") {
            // Make sure it's not inside a function parameter or something
            // Check if there's an = sign (assignment)
            if trimmed.contains('=') && !trimmed.contains("==") {
                errors.push(SanityError {
                    line: line_num + 1,
                    column: 1,
                    message: format!("L-01 VIOLATION: bare 'mut' without 'let': {}", trimmed),
                    kind: SanityErrorKind::IllegalToken,
                });
            }
        }
        
        // Check for `= [;` which indicates broken array handling
        if trimmed.contains("= [;") || trimmed.contains("[;") && !trimmed.contains("\"") {
            errors.push(SanityError {
                line: line_num + 1,
                column: 1,
                message: "Broken array literal: contains '[;'".to_string(),
                kind: SanityErrorKind::InternalLoweringError,
            });
        }
        
        // Check for double semicolons (common lowering bug)
        if trimmed.ends_with(";;") {
            errors.push(SanityError {
                line: line_num + 1,
                column: line.len(),
                message: "Double semicolon detected".to_string(),
                kind: SanityErrorKind::InternalLoweringError,
            });
        }
    }
    
    errors
}

/// Check for unclosed strings
fn check_unclosed_strings(code: &str) -> Vec<SanityError> {
    let mut errors = Vec::new();
    
    for (line_num, line) in code.lines().enumerate() {
        let trimmed = line.trim();
        
        // Skip comments
        if trimmed.starts_with("//") {
            continue;
        }
        
        // Count unescaped quotes
        let mut in_string = false;
        let mut escape_next = false;
        
        for ch in line.chars() {
            if escape_next {
                escape_next = false;
                continue;
            }
            
            if ch == '\\' {
                escape_next = true;
                continue;
            }
            
            if ch == '"' {
                in_string = !in_string;
            }
        }
        
        // If line ends with string open and doesn't have continuation markers
        if in_string && !line.ends_with('\\') && !trimmed.ends_with(',') {
            // Check if it might be a multiline string (raw string)
            if !line.contains("r#\"") && !line.contains("r\"") {
                errors.push(SanityError {
                    line: line_num + 1,
                    column: 1,
                    message: "Possible unclosed string literal".to_string(),
                    kind: SanityErrorKind::UnclosedString,
                });
            }
        }
    }
    
    errors
}

/// Check for patterns that indicate specific lowering bugs
fn check_lowering_patterns(code: &str) -> Vec<SanityError> {
    let mut errors = Vec::new();
    
    for (line_num, line) in code.lines().enumerate() {
        let trimmed = line.trim();
        
        // Pattern: `=> {}` without proper arm body (L-02 violation)
        // Valid: `=> { ... }` or `=> (...)` or `=> expr,`
        // Invalid: `=> {};` at end of match arm
        
        // Pattern: assignment in expression position without parens
        // e.g., `let x = if { ... }` instead of `let x = (if { ... })`
        // This is technically valid Rust, so we won't flag it
        
        // Pattern: Empty match arm body
        if trimmed == "=> {}," || trimmed == "=> { }," {
            errors.push(SanityError {
                line: line_num + 1,
                column: 1,
                message: "Empty match arm body detected".to_string(),
                kind: SanityErrorKind::InvalidExpression,
            });
        }
        
        // Pattern: `}); }` - malformed if expression close
        if trimmed.contains("}); }") {
            errors.push(SanityError {
                line: line_num + 1,
                column: 1,
                message: "Malformed expression close: '}); }'".to_string(),
                kind: SanityErrorKind::InternalLoweringError,
            });
        }
    }
    
    errors
}

//=============================================================================
// L-05 CRITICAL: EFFECT ANNOTATION LEAKAGE CHECK
// Effect annotations (effects(...)) must NEVER appear in Rust output.
// If they do, it's an INTERNAL COMPILER ERROR in the lowering layer.
//=============================================================================

/// Check for effect annotation leakage in generated Rust code
/// 
/// L-05 RULE: Effect annotations are for the RustS+ logic checker ONLY.
/// They must be stripped during lowering. If any appear in Rust output,
/// this indicates a bug in the lowering layer.
/// 
/// Detected patterns:
/// - `effects(` anywhere in the code
/// - `effects<` (shouldn't exist but check anyway)
/// - `effects[` (shouldn't exist but check anyway)
fn check_effect_annotation_leakage(code: &str) -> Vec<SanityError> {
    let mut errors = Vec::new();
    
    for (line_num, line) in code.lines().enumerate() {
        let trimmed = line.trim();
        
        // Skip comments - effect annotations might be mentioned in documentation
        if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("*") {
            continue;
        }
        
        // Check for string literals - don't flag effects mentioned in strings
        // We need to be careful here: only check outside of string literals
        let mut in_string = false;
        let mut escape_next = false;
        let chars: Vec<char> = line.chars().collect();
        let line_len = chars.len();
        
        let mut i = 0;
        while i < line_len {
            let c = chars[i];
            
            if escape_next {
                escape_next = false;
                i += 1;
                continue;
            }
            
            if c == '\\' && in_string {
                escape_next = true;
                i += 1;
                continue;
            }
            
            if c == '"' {
                in_string = !in_string;
                i += 1;
                continue;
            }
            
            // Only check outside strings
            if !in_string {
                // Check for "effects(" pattern
                if i + 8 < line_len {
                    let slice: String = chars[i..i+8].iter().collect();
                    if slice == "effects(" {
                        // Find the column position
                        errors.push(SanityError {
                            line: line_num + 1,
                            column: i + 1,
                            message: "L-05 VIOLATION: effect annotation leaked into Rust output".to_string(),
                            kind: SanityErrorKind::EffectAnnotationLeakage,
                        });
                        // Don't report multiple times for same line
                        break;
                    }
                }
                
                // Also check for malformed patterns
                if i + 8 < line_len {
                    let slice: String = chars[i..i+8].iter().collect();
                    if slice == "effects<" || slice == "effects[" {
                        errors.push(SanityError {
                            line: line_num + 1,
                            column: i + 1,
                            message: "L-05 VIOLATION: malformed effect annotation in Rust output".to_string(),
                            kind: SanityErrorKind::EffectAnnotationLeakage,
                        });
                        break;
                    }
                }
            }
            
            i += 1;
        }
    }
    
    errors
}

/// Format internal compiler error for display
pub fn format_internal_error(result: &SanityCheckResult) -> String {
    let mut output = String::new();
    
    output.push_str("\n");
    output.push_str("╔══════════════════════════════════════════════════════════════════╗\n");
    output.push_str("║  error[RUSTSP_INTERNAL][lowering]: invalid Rust code generated   ║\n");
    output.push_str("╚══════════════════════════════════════════════════════════════════╝\n");
    output.push_str("\n");
    output.push_str("note:\n");
    output.push_str("  This is a compiler bug, not your fault.\n");
    output.push_str("\n");
    
    for error in &result.errors {
        output.push_str(&format!("  --> line {}:{}\n", error.line, error.column));
        output.push_str(&format!("      {}\n", error.message));
        output.push_str(&format!("      kind: {:?}\n", error.kind));
        output.push_str("\n");
    }
    
    output.push_str("help:\n");
    output.push_str("  Please report this issue to the RustS+ developers.\n");
    output.push_str("  Include your RustS+ source code for debugging.\n");
    
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_balanced_delimiters_ok() {
        let code = r#"
fn main() {
    let x = [1, 2, 3];
    if (x > 0) {
        println!("ok");
    }
}
"#;
        let result = check_rust_output(code);
        assert!(result.is_valid, "Expected valid code: {:?}", result.errors);
    }
    
    #[test]
    fn test_unbalanced_braces() {
        let code = r#"
fn main() {
    let x = 1;
"#;
        let result = check_rust_output(code);
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.kind == SanityErrorKind::UnbalancedDelimiter));
    }
    
    #[test]
    fn test_bare_mut_detected() {
        let code = r#"
fn main() {
    mut x = 10;
}
"#;
        let result = check_rust_output(code);
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.kind == SanityErrorKind::IllegalToken));
    }
    
    #[test]
    fn test_valid_let_mut() {
        let code = r#"
fn main() {
    let mut x = 10;
    x = x + 1;
}
"#;
        let result = check_rust_output(code);
        assert!(result.is_valid, "Expected valid code: {:?}", result.errors);
    }
    
    #[test]
    fn test_double_semicolon() {
        let code = r#"
fn main() {
    let x = 10;;
}
"#;
        let result = check_rust_output(code);
        assert!(!result.is_valid);
    }
    
    #[test]
    fn test_broken_array() {
        let code = r#"
fn main() {
    let arr = [;
        1,
    ];
}
"#;
        let result = check_rust_output(code);
        assert!(!result.is_valid);
    }
    
    //=========================================================================
    // L-05: EFFECT ANNOTATION LEAKAGE TESTS
    // Effect annotations must NEVER appear in Rust output
    //=========================================================================
    
    #[test]
    fn test_l05_effect_leakage_detected() {
        // This is INVALID - effects() should never appear in Rust output
        let code = r#"
fn apply_tx(w: Wallet, tx: Tx) -> effects(write w) Wallet {
    w
}
"#;
        let result = check_rust_output(code);
        assert!(!result.is_valid, "Should detect effect annotation leakage");
        assert!(result.errors.iter().any(|e| e.kind == SanityErrorKind::EffectAnnotationLeakage),
            "Error kind should be EffectAnnotationLeakage");
    }
    
    #[test]
    fn test_l05_valid_rust_no_effects() {
        // This is VALID - no effects clause, proper Rust syntax
        let code = r#"
fn apply_tx(w: Wallet, tx: Tx) -> Wallet {
    w
}
"#;
        let result = check_rust_output(code);
        assert!(result.is_valid, "Valid Rust without effects should pass: {:?}", result.errors);
    }
    
    #[test]
    fn test_l05_effect_in_function_signature() {
        // Various effect annotation patterns that should be detected
        let code = r#"
fn log(msg: String) effects(io) {
    println!("{}", msg);
}
"#;
        let result = check_rust_output(code);
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.kind == SanityErrorKind::EffectAnnotationLeakage));
    }
    
    #[test]
    fn test_l05_effect_in_string_literal_ok() {
        // Effect mentioned in string literal is OK (documentation, etc.)
        let code = r#"
fn main() {
    let msg = "This function has effects(write x)";
    println!("{}", msg);
}
"#;
        let result = check_rust_output(code);
        assert!(result.is_valid, "Effects in string literals should be OK: {:?}", result.errors);
    }
    
    #[test]
    fn test_l05_effect_in_comment_ok() {
        // Effect mentioned in comment is OK
        let code = r#"
// This function has effects(write x)
fn update(x: i32) -> i32 {
    x + 1
}
"#;
        let result = check_rust_output(code);
        assert!(result.is_valid, "Effects in comments should be OK: {:?}", result.errors);
    }
    
    #[test]
    fn test_l05_multiple_effects_detected() {
        let code = r#"
fn transfer(a: Account, b: Account) -> effects(read a, write b) Account {
    b
}
"#;
        let result = check_rust_output(code);
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.kind == SanityErrorKind::EffectAnnotationLeakage));
    }
    
    #[test]
    fn test_l05_regression_wallet_example() {
        // The exact bug that was reported
        let code = r#"
fn apply_tx(w: Wallet, tx: Tx) -> effects(write w) Wallet {
    match tx {
        Tx::Deposit { id, amount } => {
            Wallet { balance: w.balance + amount }
        }
    }
}
"#;
        let result = check_rust_output(code);
        assert!(!result.is_valid, "The reported bug case should be detected");
        assert!(result.errors.iter().any(|e| {
            e.kind == SanityErrorKind::EffectAnnotationLeakage && 
            e.message.contains("L-05 VIOLATION")
        }), "Should have L-05 violation error");
    }
    
    // =========================================================================
    // CRITICAL: Lifetime annotation tests
    // =========================================================================
    
    #[test]
    fn test_lifetime_static_in_type() {
        // 'static lifetime should NOT be treated as a char literal
        let code = r#"
struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    original_values: Vec<(&'static str, Option<String>)>,
}
"#;
        let result = check_rust_output(code);
        assert!(result.is_valid, "Lifetimes should not confuse delimiter tracking: {:?}", result.errors);
    }
    
    #[test]
    fn test_lifetime_anonymous_in_fn() {
        // '_ anonymous lifetime should NOT be treated as a char literal
        let code = r#"
fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    Ok(())
}
"#;
        let result = check_rust_output(code);
        assert!(result.is_valid, "Anonymous lifetimes should not confuse delimiter tracking: {:?}", result.errors);
    }
    
    #[test]
    fn test_lifetime_in_async_trait() {
        // Complex lifetime in async trait return type
        let code = r#"
fn post_blob(&self, data: &[u8]) -> Pin<Box<dyn Future<Output = Result<BlobRef, DAError>> + Send + '_>>;
"#;
        let result = check_rust_output(code);
        assert!(result.is_valid, "Lifetimes in async traits should work: {:?}", result.errors);
    }
    
    #[test]
    fn test_char_literal_still_works() {
        // Regular char literals should still be handled correctly
        let code = r#"
fn main() {
    let c = 'a';
    let newline = '\n';
    let tab = '\t';
}
"#;
        let result = check_rust_output(code);
        assert!(result.is_valid, "Char literals should still work: {:?}", result.errors);
    }
}