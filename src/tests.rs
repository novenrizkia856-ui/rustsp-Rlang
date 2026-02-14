//! Test suite for RustS+ transpiler
//!
//! Contains comprehensive tests for all transpilation features including:
//! - Auto let/mut detection
//! - Struct/enum literal transformation  
//! - Array literal handling
//! - Match expression transformation
//! - Effect annotation stripping
//! - And more

#[cfg(test)]
mod tests {
    use crate::parse_rusts;

    #[test]
    fn test_auto_let() {
        let input = "a = 10";
        let output = parse_rusts(input);
        assert!(output.contains("let a = 10;"));
    }

    #[test]
    fn test_auto_mut() {
        let input = "a = 10\na = 20";
        let output = parse_rusts(input);
        assert!(output.contains("let mut a = 10;"));
        assert!(output.contains("a = 20;"));
    }

    #[test]
    fn test_type_annotation() {
        let input = "a: i32 = 10";
        let output = parse_rusts(input);
        assert!(output.contains("let a: i32 = 10;"));
    }

    #[test]
    fn test_string_literal() {
        let input = r#"a = "hello""#;
        let output = parse_rusts(input);
        assert!(output.contains("String::from(\"hello\")"));
    }

    #[test]
    fn test_struct_literal_no_let_inside() {
        let input = r#"u = User {
    id = 1
    name = "kian"
}"#;
        let output = parse_rusts(input);
        // Should NOT have `let id` or `let name` inside
        assert!(!output.contains("let id"));
        assert!(!output.contains("let name"));
        // Should have proper field syntax
        assert!(output.contains("id: 1,"));
        assert!(output.contains("name: String::from(\"kian\"),"));
    }

    #[test]
    fn test_struct_literal_multiline() {
        let input = r#"u = User {
    id = 1
    name = "test"
}"#;
        let output = parse_rusts(input);
        assert!(output.contains("let u = User {"));
        assert!(output.contains("id: 1,"));
        assert!(output.contains("name: String::from(\"test\"),"));
        // Top-level struct literal closes with }; (statement)
        assert!(output.contains("};"));
    }
    
    #[test]
    fn test_enum_struct_variant_no_let() {
        let input = r#"m = Message::Move { x = 10, y = 20 }"#;
        let output = parse_rusts(input);
        assert!(!output.contains("let x"));
        assert!(!output.contains("let y"));
        assert!(output.contains("x: 10"));
        assert!(output.contains("y: 20"));
    }
    
    #[test]
    fn test_bare_struct_literal_in_match_arm() {
        // This is the bug case: bare struct literal as return expression in match arm
        // Should NOT have `let` inside the struct literal fields!
        let input = r#"struct Wallet {
    id u32
    balance i64
}

enum Transaction {
    Deposit { amount i64 }
}

fn apply_tx(w Wallet, tx Transaction) Wallet {
    match tx {
        Transaction::Deposit { amount } {
            Wallet {
                id = w.id
                balance = w.balance + amount
            }
        }
    }
}"#;
        let output = parse_rusts(input);
        // Should NOT have `let id` or `let balance` inside struct literal
        assert!(!output.contains("let id"), "Bug: 'let id' found in struct literal: {}", output);
        assert!(!output.contains("let balance"), "Bug: 'let balance' found in struct literal: {}", output);
        // Should have proper field syntax
        assert!(output.contains("id:"), "Missing 'id:' field syntax in output: {}", output);
        assert!(output.contains("balance:"), "Missing 'balance:' field syntax in output: {}", output);
    }
    
    //=========================================================================
    // ARRAY LITERAL TESTS
    //=========================================================================
    
    #[test]
    fn test_array_literal_multiline() {
        let input = r#"events = [
    1,
    2,
    3
]"#;
        let output = parse_rusts(input);
        // Should have proper array syntax
        assert!(output.contains("let events = ["));
        // Should NOT have semicolons inside array
        assert!(!output.contains("[;"));
        assert!(!output.contains("1;"));
        assert!(!output.contains("2;"));
        // Should close properly
        assert!(output.contains("];"));
    }
    
    #[test]
    fn test_array_literal_with_enum_variants() {
        let input = r#"events = [
    Event::Credit { id = 1, amount = 500 },
    Event::Debit { id = 2, amount = 200 },
    Event::Query(3)
]"#;
        let output = parse_rusts(input);
        // Should have proper array start
        assert!(output.contains("let events = ["));
        // Should transform enum struct fields
        assert!(output.contains("id: 1"));
        assert!(output.contains("amount: 500"));
        assert!(output.contains("id: 2"));
        assert!(output.contains("amount: 200"));
        // Should have tuple variant
        assert!(output.contains("Event::Query(3)"));
        // Should NOT have illegal patterns
        assert!(!output.contains("= [;"));
        assert!(!output.contains("[;"));
        // Should close properly
        assert!(output.contains("];"));
    }
    
    #[test]
    fn test_array_literal_single_line_unchanged() {
        let input = "arr = [1, 2, 3]";
        let output = parse_rusts(input);
        // Single-line arrays should be handled by normal assignment
        assert!(output.contains("let arr = [1, 2, 3];"));
    }
    
    //=========================================================================
    // BUG FIX TESTS - Critical regression tests for the three main bugs
    //=========================================================================
    
    /// BUG A: `mut x = expr` MUST become `let mut x = expr;`
    #[test]
    fn test_bug_a_mut_lowering() {
        let input = "mut i = 0\ni = i + 1";
        let output = parse_rusts(input);
        // MUST have `let mut`
        assert!(output.contains("let mut i = 0;"), 
            "BUG A: Expected 'let mut i = 0;', got: {}", output);
        // Reassignment should work
        assert!(output.contains("i = i + 1;"),
            "BUG A: Expected 'i = i + 1;', got: {}", output);
        // Should NOT have bare `mut i` without `let`
        assert!(!output.lines().any(|l| l.trim().starts_with("mut i")), 
            "BUG A: Found bare 'mut i' without 'let': {}", output);
    }
    
    /// BUG A: `mut x: Type = expr` MUST become `let mut x: Type = expr;`
    #[test]
    fn test_bug_a_mut_with_type() {
        let input = "mut counter: i32 = 0";
        let output = parse_rusts(input);
        assert!(output.contains("let mut counter: i32 = 0;"), 
            "BUG A: Expected 'let mut counter: i32 = 0;', got: {}", output);
    }
    
    /// BUG B: `x = if cond { a } else { b }` MUST be parenthesized
    #[test]
    fn test_bug_b_if_expr_parenthesization() {
        let input = r#"x = if true {
    1
} else {
    2
}"#;
        let output = parse_rusts(input);
        // MUST have opening paren after =
        assert!(output.contains("= (if"), 
            "BUG B: Expected '= (if', got: {}", output);
        // MUST have closing });
        assert!(output.contains("});"), 
            "BUG B: Expected '}});', got: {}", output);
    }
    
    /// BUG C: `match String { "str" { ... } }` MUST add `.as_str()`
    #[test]
    fn test_bug_c_match_string_as_str() {
        let input = r#"match status {
    "rich" {
        println!("ok")
    }
    _ {
        println!("not ok")
    }
}"#;
        let output = parse_rusts(input);
        // MUST have .as_str() added to match expression
        assert!(output.contains("status.as_str()"), 
            "BUG C: Expected 'status.as_str()', got: {}", output);
    }
    
    /// BUG C: Non-string patterns should NOT add .as_str()
    #[test]
    fn test_bug_c_match_without_string_patterns() {
        let input = r#"match x {
    0 {
        println!("zero")
    }
    _ {
        println!("other")
    }
}"#;
        let output = parse_rusts(input);
        // Should NOT have .as_str() for non-string patterns
        assert!(!output.contains(".as_str()"), 
            "BUG C: Should not add .as_str() for non-string patterns: {}", output);
    }
    
    //=========================================================================
    // 5 ATURAN LOWERING FINAL - COMPREHENSIVE REGRESSION TESTS
    //=========================================================================
    
    /// L-01: `mut x = expr` WAJIB menjadi `let mut x = expr;`
    #[test]
    fn test_l01_mut_lowering_basic() {
        let input = "mut x = 0";
        let output = parse_rusts(input);
        assert!(output.contains("let mut x = 0;"), 
            "L-01: 'mut x = 0' must become 'let mut x = 0;', got: {}", output);
        // MUST NOT have bare `mut` 
        assert!(!output.lines().any(|l| {
            let t = l.trim();
            t.starts_with("mut ") && !l.contains("let mut") && !l.contains("&mut")
        }), "L-01: Found bare 'mut' without 'let': {}", output);
    }
    
    /// L-01: `mut x = expr` followed by `x = x + 1` WAJIB menjadi proper mut binding
    #[test]
    fn test_l01_mut_with_reassignment() {
        let input = "mut x = 0\nx = x + 1";
        let output = parse_rusts(input);
        assert!(output.contains("let mut x = 0;"), 
            "L-01: First line must be 'let mut x = 0;', got: {}", output);
        assert!(output.contains("x = x + 1;"), 
            "L-01: Second line must be 'x = x + 1;', got: {}", output);
        // Second line must NOT have `let`
        let lines: Vec<&str> = output.lines().collect();
        let second_line = lines.iter().find(|l| l.contains("x + 1")).unwrap();
        assert!(!second_line.contains("let"), 
            "L-01: Reassignment must not have 'let': {}", second_line);
    }

    #[test]
    fn test_method_chain_atomic_for_tuple_destructuring() {
        let input = r#"(phrase, secret) = mnemonic::generate_mnemonic()
    .expect(\"must succeed\")"#;
        let output = parse_rusts(input);
        assert!(output.contains("let (phrase, secret) = mnemonic::generate_mnemonic()"));
        assert!(output.contains(".expect"), "missing expect chain: {}", output);
        assert!(!output.contains("generate_mnemonic();\n    .expect"), "method chain was split: {}", output);
    }

    #[test]
    fn test_tuple_destructure_requires_result_handler() {
        let input = "(phrase, secret) = mnemonic::generate_mnemonic()";
        let output = parse_rusts(input);
        assert!(output.contains("compile_error!(\"RustS+ error: tuple destructuring from call requires explicit Result handling"));
    }

    #[test]
    fn test_slice_range_not_auto_cloned() {
        let input = "secret = self.keypair_bytes[0..32].try_into()";
        let output = parse_rusts(input);
        assert!(!output.contains("[0..32].clone()"), "slice range must not be cloned: {}", output);
    }

    #[test]
    fn test_no_double_borrow_for_slice_param() {
        let input = r#"fn verify(msg [u8]) {
    consume(msg)
}

fn consume(data &[u8]) {}"#;
        let output = parse_rusts(input);
        assert!(!output.contains("consume(&msg)"), "must not auto-add borrow for slice param: {}", output);
    }
    
    /// L-02: Expression in match arm MUST be parenthesized
    #[test]
    fn test_l02_match_arm_if_expression() {
        let input = r#"match ev {
    A {
        if cond {
            1
        } else {
            2
        }
    }
}"#;
        let output = parse_rusts(input);
        // L-09: Match arms with if expressions use plain `=>`
        // The if expression is a valid expression context, no parens needed
        assert!(output.contains("=>") && output.contains("if cond"),
            "L-02/L-09: Match arm with if expr must use => format: {}", output);
    }
    
    /// L-03: Reassignment to same variable MUST use mut binding, not shadow
    #[test]
    fn test_l03_reassignment_uses_mut() {
        let input = "x = 10\nx = x + 1";
        let output = parse_rusts(input);
        // First line MUST have `let mut`
        assert!(output.contains("let mut x = 10;"), 
            "L-03: 'x = 10' followed by reassignment must be 'let mut x = 10;', got: {}", output);
        // Second line MUST NOT have `let` (it's reassignment, not new declaration)
        let lines: Vec<&str> = output.lines().collect();
        let second_line = lines.iter().find(|l| l.contains("x + 1")).unwrap();
        assert!(!second_line.contains("let"), 
            "L-03: Reassignment must not create new binding: {}", second_line);
    }
    
    /// L-03: Multiple reassignments must all work correctly
    #[test]
    fn test_l03_multiple_reassignments() {
        let input = "count = 0\ncount = count + 1\ncount = count + 2";
        let output = parse_rusts(input);
        assert!(output.contains("let mut count = 0;"), 
            "L-03: Initial assignment must be 'let mut', got: {}", output);
        // Count occurrences of "let" - should be exactly 1
        let let_count = output.matches("let ").count();
        assert_eq!(let_count, 1, 
            "L-03: Should have exactly 1 'let', got {}: {}", let_count, output);
    }
    
    /// L-04: Array access on non-Copy type MUST use .clone()
    #[test]
    fn test_l04_array_access_clone() {
        let input = "ev = events[i]";
        let output = parse_rusts(input);
        assert!(output.contains("events[i].clone()"), 
            "L-04: Array access must add .clone(): {}", output);
    }
    
    /// L-04: Array access that already has method call should NOT add .clone()
    #[test]
    fn test_l04_array_access_with_method() {
        let input = "len = items[0].len()";
        let output = parse_rusts(input);
        // Should NOT double-add clone
        assert!(!output.contains(".clone().clone()"), 
            "L-04: Should not double-clone: {}", output);
    }
    
    /// L-04: Simple numeric literals should NOT get .clone()
    #[test]
    fn test_l04_numeric_no_clone() {
        let input = "x = 42";
        let output = parse_rusts(input);
        assert!(!output.contains(".clone()"), 
            "L-04: Numeric literal should not get .clone(): {}", output);
    }
    
    /// L-05: Generated Rust output must have balanced delimiters
    #[test]
    fn test_l05_balanced_delimiters() {
        let input = r#"fn test() {
    x = 10
    if true {
        y = 20
    }
}"#;
        let output = parse_rusts(input);
        // Count braces
        let open_braces = output.matches('{').count();
        let close_braces = output.matches('}').count();
        assert_eq!(open_braces, close_braces, 
            "L-05: Braces must be balanced ({} open, {} close): {}", 
            open_braces, close_braces, output);
    }
    
    /// L-05: No bare `mut` tokens in output
    #[test]
    fn test_l05_no_bare_mut() {
        let input = "mut x = 10\nmut y: i32 = 20";
        let output = parse_rusts(input);
        // Check each line for bare mut
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("mut ") && !line.contains("let mut") && !line.contains("&mut") {
                panic!("L-05: Found bare 'mut' token: {}", line);
            }
        }
    }
    
    /// L-05: Effect annotations must NOT appear in Rust output
    #[test]
    fn test_l05_no_effect_leakage_rustsplus_syntax() {
        // RustS+ syntax function with effects
        let input = r#"fn apply_tx(w Wallet, tx Tx) effects(write w) Wallet {
    w
}"#;
        let output = parse_rusts(input);
        // MUST NOT have effects clause in output
        assert!(!output.contains("effects("), 
            "L-05: effects() must NOT appear in Rust output: {}", output);
        // MUST have return type
        assert!(output.contains("-> Wallet"), 
            "L-05: Return type 'Wallet' must be present: {}", output);
    }
    
    /// L-05: Effect annotations must be stripped from Rust-style params too
    #[test]
    fn test_l05_no_effect_leakage_rust_syntax() {
        // Rust-style params BUT with RustS+ effects
        let input = r#"fn log(msg: String) effects(io) {
    println!("{}", msg)
}"#;
        let output = parse_rusts(input);
        // MUST NOT have effects clause in output
        assert!(!output.contains("effects("), 
            "L-05: effects() must NOT appear in Rust output even with Rust-style params: {}", output);
    }
    
    /// L-05: Effect annotations with multiple effects must be stripped
    #[test]
    fn test_l05_no_effect_leakage_multiple_effects() {
        let input = r#"fn transfer(from Account, to Account) effects(read from, write to) Account {
    to
}"#;
        let output = parse_rusts(input);
        assert!(!output.contains("effects("), 
            "L-05: Multiple effects must be stripped: {}", output);
        assert!(output.contains("-> Account"), 
            "L-05: Return type must be preserved: {}", output);
    }
    
    /// L-01: mut with function call value must work
    #[test]
    fn test_l01_mut_with_function_call() {
        let input = "mut wallet = create_wallet(seed)";
        let output = parse_rusts(input);
        assert!(output.contains("let mut wallet = create_wallet(seed)"), 
            "L-01: 'mut wallet = create_wallet(seed)' must have 'let mut': {}", output);
        // MUST NOT have bare `mut`
        assert!(!output.lines().any(|l| {
            let t = l.trim();
            t.starts_with("mut ") && !l.contains("let mut") && !l.contains("&mut")
        }), "L-01: Found bare 'mut' without 'let': {}", output);
    }
    
    /// L-01: mut with array access must work
    #[test]
    fn test_l01_mut_with_array_access() {
        let input = "mut root = tx_hashes[0]";
        let output = parse_rusts(input);
        assert!(output.contains("let mut root"), 
            "L-01: 'mut root = tx_hashes[0]' must have 'let mut': {}", output);
        // Should also have .clone() due to L-04
        assert!(output.contains(".clone()"), 
            "L-04: Array access should have .clone(): {}", output);
    }
    
    /// Integration test: Complete function with all patterns
    #[test]
    fn test_integration_complete_function() {
        let input = r#"fn process() -> i32 {
    mut total = 0
    mut i = 0
    while i < 10 {
        total = total + 1
        i = i + 1
    }
    total
}"#;
        let output = parse_rusts(input);
        
        // L-01: mut declarations become let mut
        assert!(output.contains("let mut total"), 
            "Integration: 'mut total' must become 'let mut total': {}", output);
        assert!(output.contains("let mut i"), 
            "Integration: 'mut i' must become 'let mut i': {}", output);
        
        // L-03: Reassignments don't create new bindings
        let let_total_count = output.matches("let mut total").count() + output.matches("let total").count();
        assert_eq!(let_total_count, 1, 
            "Integration: Should have exactly 1 'let' for total: {}", output);
        
        // L-05: Balanced braces
        let open = output.matches('{').count();
        let close = output.matches('}').count();
        assert_eq!(open, close, 
            "Integration: Braces must be balanced: {}", output);
    }
    
    /// Test multi-line pub use import block transformation
    /// Items should have commas, closing brace should have semicolon
    #[test]
    fn test_multiline_use_import() {
        let input = r#"pub use quorum_da::{
    QuorumDA
    QuorumError
    ValidatorInfo
}"#;
        let output = parse_rusts(input);
        
        // Items should have commas
        assert!(output.contains("QuorumDA,"), 
            "Multi-line use: QuorumDA should have comma: {}", output);
        assert!(output.contains("QuorumError,"), 
            "Multi-line use: QuorumError should have comma: {}", output);
        assert!(output.contains("ValidatorInfo,"), 
            "Multi-line use: ValidatorInfo should have comma: {}", output);
        
        // Closing brace should have semicolon
        assert!(output.contains("};"), 
            "Multi-line use: closing brace should have semicolon: {}", output);
        
        // Opening should NOT have semicolon after {
        assert!(!output.contains("{;"), 
            "Multi-line use: opening should NOT have semicolon after brace: {}", output);
    }
    
    /// Test single-line use import is unchanged
    #[test]
    fn test_singleline_use_import() {
        let input = "use std::collections::{HashMap, HashSet}";
        let output = parse_rusts(input);
        
        // Single-line use should have semicolon at end
        assert!(output.contains("};") || output.ends_with(";"), 
            "Single-line use should have semicolon: {}", output);
    }
    
    /// Test simple use import
    #[test]
    fn test_simple_use_import() {
        let input = "use std::io::Result";
        let output = parse_rusts(input);
        
        // Simple use should have semicolon
        assert!(output.contains("Result;") || output.trim().ends_with(";"), 
            "Simple use should have semicolon: {}", output);
    }
}
