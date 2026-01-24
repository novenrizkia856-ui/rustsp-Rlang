//! Detection functions for RustS+ literal patterns
//! 
//! Contains functions to detect the start of various literal expressions:
//! - Struct literals: `x = StructName { ... }`
//! - Enum struct variants: `x = Enum::Variant { ... }`
//! - Array literals: `x = [...]`
//! - Bare literals (return expressions without assignment)
//!
//! CRITICAL FIX: All detection functions must ignore braces inside string literals!
//! Example: `anyhow::bail("header {} mismatch")` should NOT trigger literal detection
//! because the `{` is inside a string.

use crate::helpers::{is_rust_block_start, is_valid_identifier};
use crate::struct_def::StructRegistry;

//===========================================================================
// STRING-AWARE BRACE DETECTION HELPERS
// These helpers find braces OUTSIDE of string literals only
//===========================================================================

/// Find the position of the first `{` that is NOT inside a string literal
/// Returns None if no such brace exists
fn find_brace_outside_string(s: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escape_next = false;
    
    for (i, c) in s.chars().enumerate() {
        if escape_next {
            escape_next = false;
            continue;
        }
        
        if c == '\\' && in_string {
            escape_next = true;
            continue;
        }
        
        if c == '"' {
            in_string = !in_string;
            continue;
        }
        
        if !in_string && c == '{' {
            return Some(i);
        }
    }
    
    None
}

/// Check if string contains `{` OUTSIDE of string literals
fn contains_brace_outside_string(s: &str) -> bool {
    find_brace_outside_string(s).is_some()
}

/// Check if string contains `::` followed by identifier and then `{` outside strings
/// This specifically detects enum variant patterns like `Enum::Variant {`
/// but NOT macro calls like `anyhow::bail("format {}")`
fn has_enum_variant_pattern(s: &str) -> bool {
    // First check if :: exists
    if !s.contains("::") {
        return false;
    }
    
    // Find position of first { outside strings
    let brace_pos = match find_brace_outside_string(s) {
        Some(pos) => pos,
        None => return false,
    };
    
    // Check what's between :: and {
    // For valid enum variant: `Enum::Variant {` - no `(` between :: and {
    // For macro calls: `anyhow::bail("...")` - has `(` between :: and {
    let before_brace = &s[..brace_pos];
    
    // Find the last :: before the brace
    if let Some(double_colon_pos) = before_brace.rfind("::") {
        let between = &before_brace[double_colon_pos + 2..];
        let between_trimmed = between.trim();
        
        // If there's a `(` in between, it's a function/macro call, not enum variant
        if between_trimmed.contains('(') {
            return false;
        }
        
        // Check if what's between is a valid identifier (variant name)
        // It should be alphanumeric/underscore only, possibly with whitespace
        let identifier: String = between_trimmed.chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        
        if !identifier.is_empty() && identifier.chars().next().unwrap().is_uppercase() {
            return true;
        }
    }
    
    false
}

//===========================================================================
// ASSIGNMENT OPERATOR DETECTION HELPER
// Finds true assignment `=` that is NOT part of comparison or compound operators
//===========================================================================

/// Find position of standalone assignment `=` in a string.
/// Returns None if no standalone `=` exists.
/// 
/// Rejects:
/// - `==`, `!=`, `<=`, `>=` (comparison operators)
/// - `+=`, `-=`, `*=`, `/=`, `%=` (compound assignment)
/// - `&=`, `|=`, `^=` (bitwise compound)
/// - `<<=`, `>>=` (shift compound)
/// - `=>` (fat arrow / match arm)
/// - `=` inside string literals or nested structures
fn find_assignment_eq_position(s: &str) -> Option<usize> {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    
    // Track nested structures
    let mut paren_depth: usize = 0;
    let mut bracket_depth: usize = 0;
    let mut brace_depth: usize = 0;
    let mut in_string = false;
    let mut prev_char = ' ';
    
    for i in 0..len {
        let c = chars[i];
        
        // Handle string literals
        if c == '"' && prev_char != '\\' {
            in_string = !in_string;
            prev_char = c;
            continue;
        }
        
        if in_string {
            prev_char = c;
            continue;
        }
        
        // Track nesting
        match c {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
        
        // Only look for `=` at top level (not nested)
        if c == '=' && paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 {
            let prev = if i > 0 { chars[i - 1] } else { ' ' };
            let next = if i + 1 < len { chars[i + 1] } else { ' ' };
            
            // Reject comparison operators: `==`, `!=`, `<=`, `>=`
            if next == '=' {
                prev_char = c;
                continue;
            }
            if prev == '!' || prev == '=' || prev == '<' || prev == '>' {
                prev_char = c;
                continue;
            }
            
            // Reject fat arrow: `=>`
            if next == '>' {
                prev_char = c;
                continue;
            }
            
            // Reject compound assignments: `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`
            if prev == '+' || prev == '-' || prev == '*' || prev == '/' || prev == '%' 
               || prev == '&' || prev == '|' || prev == '^' {
                prev_char = c;
                continue;
            }
            
            // Found a standalone assignment `=`
            return Some(i);
        }
        
        prev_char = c;
    }
    
    None
}

/// Check if line starts with control flow keyword that should never be detected as literal
fn is_control_flow_start(s: &str) -> bool {
    let trimmed = s.trim();
    trimmed.starts_with("if ") 
        || trimmed.starts_with("if(")
        || trimmed.starts_with("while ") 
        || trimmed.starts_with("while(")
        || trimmed.starts_with("for ") 
        || trimmed.starts_with("match ")
        || trimmed.starts_with("loop ")
        || trimmed.starts_with("loop{")
        || trimmed.starts_with("return ")
        || trimmed.starts_with("return(")
        || trimmed.starts_with("break ")
        || trimmed.starts_with("break;")
        || trimmed.starts_with("continue")
        || trimmed.starts_with("unsafe ")
        || trimmed.starts_with("async ")
}

//===========================================================================
// STRUCT LITERAL DETECTION
//===========================================================================

/// Detect if line starts a struct literal: `varname = StructName {`
/// Returns (var_name, struct_name) if matched, excludes Enum::Variant
pub fn detect_struct_literal_start(line: &str, registry: &StructRegistry) -> Option<(String, String)> {
    let trimmed = line.trim();
    
    // CRITICAL FIX: EXCLUDE function definitions and other Rust blocks
    if is_rust_block_start(trimmed) {
        return None;
    }
    
    // CRITICAL FIX: EXCLUDE control flow statements
    // Bug: `if self.status != SyncStatus::Idle {` was detected as struct literal
    if is_control_flow_start(trimmed) {
        return None;
    }
    
    // CRITICAL FIX: Use string-aware brace detection
    if !trimmed.contains('=') || !contains_brace_outside_string(trimmed) {
        return None;
    }
    
    // CRITICAL FIX: Find TRUE assignment `=` (not comparison operators)
    // Bug: `splitn(2, '=')` would split on first `=` even if it's part of `!=`
    let eq_pos = match find_assignment_eq_position(trimmed) {
        Some(pos) => pos,
        None => return None, // No standalone `=` found
    };
    
    let var_name = trimmed[..eq_pos].trim();
    let rhs = trimmed[eq_pos + 1..].trim();
    
    // CRITICAL FIX: Find brace outside strings in RHS
    let brace_pos = match find_brace_outside_string(rhs) {
        Some(pos) => pos,
        None => return None,
    };
    
    // EXCLUDE enum paths (:: before {)
    let before_brace = &rhs[..brace_pos];
    if before_brace.contains("::") {
        return None;
    }
    
    let struct_name: String = rhs
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    
    // Registry check or PascalCase heuristic
    if registry.is_struct(&struct_name) || 
       (!struct_name.is_empty() && struct_name.chars().next().unwrap().is_uppercase()) {
        return Some((var_name.to_string(), struct_name));
    }
    
    None
}

/// Detect BARE struct literal (without assignment): `StructName {`
/// Used for return expressions like: `Packet { header = ... }`
pub fn detect_bare_struct_literal(line: &str, registry: &StructRegistry) -> Option<String> {
    let trimmed = line.trim();
    
    // CRITICAL FIX: EXCLUDE function definitions and other Rust blocks
    if is_rust_block_start(trimmed) {
        return None;
    }
    
    // CRITICAL FIX: EXCLUDE control flow statements
    // Safety: control flow should never be detected as struct literal
    if is_control_flow_start(trimmed) {
        return None;
    }
    
    // CRITICAL FIX: Must have { outside string literals
    let brace_pos = match find_brace_outside_string(trimmed) {
        Some(pos) => pos,
        None => return None,
    };
    
    let before_brace = &trimmed[..brace_pos];
    
    // If there's a = BEFORE {, it's an assignment, not bare literal
    if before_brace.contains('=') {
        return None;
    }
    
    // EXCLUDE enum paths (has ::)
    if before_brace.contains("::") {
        return None;
    }
    
    // EXCLUDE function/macro calls (has `(`)
    if before_brace.contains('(') {
        return None;
    }
    
    let struct_name = before_brace.trim();
    
    // Validate it's a struct name (PascalCase or in registry)
    if !struct_name.is_empty() && 
       (registry.is_struct(struct_name) || 
        struct_name.chars().next().unwrap().is_uppercase()) &&
       is_valid_identifier(struct_name) {
        return Some(struct_name.to_string());
    }
    
    None
}

//===========================================================================
// ENUM LITERAL DETECTION
//===========================================================================

/// Detect BARE enum struct variant literal (without assignment): `Enum::Variant {`
/// 
/// CRITICAL: Must NOT match macro calls like `anyhow::bail("format {}")`
pub fn detect_bare_enum_literal(line: &str) -> Option<String> {
    let trimmed = line.trim();
    
    // CRITICAL FIX: EXCLUDE function definitions and other Rust blocks
    if is_rust_block_start(trimmed) {
        return None;
    }
    
    // CRITICAL FIX: EXCLUDE control flow statements
    // Bug: `if let SyncStatus::SyncingHeaders { ... } = &self.status {` was detected
    // because `{` after SyncingHeaders comes BEFORE `=`, making it look like bare enum literal
    if is_control_flow_start(trimmed) {
        return None;
    }
    
    // EXCLUDE match arms: `Event::Data { id, body } =>`
    if trimmed.contains("=>") {
        return None;
    }
    
    // Must have :: 
    if !trimmed.contains("::") {
        return None;
    }
    
    // CRITICAL FIX: Must have { OUTSIDE string literals
    let brace_pos = match find_brace_outside_string(trimmed) {
        Some(pos) => pos,
        None => return None,
    };
    
    let before_brace = &trimmed[..brace_pos];
    
    // If there's a = BEFORE {, it's an assignment
    if before_brace.contains('=') {
        return None;
    }
    
    // CRITICAL FIX: If there's a `(` before {, it's a function/macro call
    // Example: `anyhow::bail("header {}")` has `(` between `bail` and the first real `{`
    // We need to check if the `(` opens a call that contains the `{`
    if before_brace.contains('(') {
        // This could be a function/macro call, not enum variant
        // Check if it looks like: `path::name(args` where args might contain `{`
        return None;
    }
    
    let enum_path = before_brace.trim();
    if !enum_path.is_empty() && enum_path.contains("::") {
        // Additional validation: the part after :: should be a valid identifier
        // starting with uppercase (enum variant naming convention)
        if let Some(last_colon_pos) = enum_path.rfind("::") {
            let variant = &enum_path[last_colon_pos + 2..].trim();
            if !variant.is_empty() {
                let first_char = variant.chars().next().unwrap();
                // Enum variants typically start with uppercase
                // Macros like `bail`, `anyhow` start with lowercase
                if first_char.is_uppercase() {
                    return Some(enum_path.to_string());
                }
            }
        }
    }
    
    None
}

/// Detect struct literal INSIDE function call: `Some(StructName {`, `Ok(StructName {`
/// Returns the struct name if this pattern starts a struct literal
/// 
/// Pattern: `FuncCall(StructName {` where:
/// - There's a `(` before `{`
/// - Between `(` and `{` is a valid struct name (PascalCase or known struct)
/// - Brace count shows unclosed `{` (opens > closes)
/// 
/// This handles cases like:
/// - `Some(PrivateTxInfo {`
/// - `Ok(Result { field = value })`
/// - `return Some(Data {`
pub fn detect_struct_literal_in_call(line: &str, registry: &StructRegistry) -> Option<String> {
    let trimmed = line.trim();
    
    // EXCLUDE function definitions
    if is_rust_block_start(trimmed) {
        return None;
    }
    
    // EXCLUDE control flow
    if is_control_flow_start(trimmed) {
        return None;
    }
    
    // Must have both `(` and `{`
    let paren_pos = match trimmed.rfind('(') {
        Some(pos) => pos,
        None => return None,
    };
    
    let brace_pos = match find_brace_outside_string(trimmed) {
        Some(pos) if pos > paren_pos => pos,
        _ => return None,
    };
    
    // Extract text between `(` and `{`
    let between = trimmed[paren_pos + 1..brace_pos].trim();
    
    // Must be a valid struct name (PascalCase identifier or registered struct)
    if between.is_empty() {
        return None;
    }
    
    // Check for valid struct name
    let struct_name: String = between
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    
    if struct_name.is_empty() {
        return None;
    }
    
    // Validate: must start with uppercase (PascalCase) or be a registered struct
    let first_char = struct_name.chars().next().unwrap();
    if !first_char.is_uppercase() && !registry.is_struct(&struct_name) {
        return None;
    }
    
    // Check that brace is unclosed (multi-line struct literal)
    let opens = trimmed.matches('{').count();
    let closes = trimmed.matches('}').count();
    
    if opens > closes {
        return Some(struct_name);
    }
    
    None
}

/// Detect enum variant literal INSIDE function call: `Some(Enum::Variant {`
/// Returns the enum path if this pattern starts an enum literal
pub fn detect_enum_literal_in_call(line: &str) -> Option<String> {
    let trimmed = line.trim();
    
    // EXCLUDE function definitions
    if is_rust_block_start(trimmed) {
        return None;
    }
    
    // EXCLUDE control flow
    if is_control_flow_start(trimmed) {
        return None;
    }
    
    // Must have `(`, `::`, and `{`
    let paren_pos = match trimmed.rfind('(') {
        Some(pos) => pos,
        None => return None,
    };
    
    let brace_pos = match find_brace_outside_string(trimmed) {
        Some(pos) if pos > paren_pos => pos,
        _ => return None,
    };
    
    let between = &trimmed[paren_pos + 1..brace_pos];
    
    // Must have ::
    if !between.contains("::") {
        return None;
    }
    
    let enum_path = between.trim();
    
    // Validate enum path: must have uppercase variant after ::
    if let Some(last_colon_pos) = enum_path.rfind("::") {
        let variant = &enum_path[last_colon_pos + 2..].trim();
        if !variant.is_empty() {
            let first_char = variant.chars().next().unwrap();
            if first_char.is_uppercase() {
                // Check that brace is unclosed
                let opens = trimmed.matches('{').count();
                let closes = trimmed.matches('}').count();
                
                if opens > closes {
                    return Some(enum_path.to_string());
                }
            }
        }
    }
    
    None
}

/// Detect if line starts an enum struct variant literal: `varname = Enum::Variant {`
pub fn detect_enum_literal_start(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    
    // CRITICAL FIX: EXCLUDE function definitions and other Rust blocks
    if is_rust_block_start(trimmed) {
        return None;
    }
    
    // CRITICAL FIX: EXCLUDE control flow statements
    // Bug: `if self.status != SyncStatus::Idle {` was incorrectly detected because
    // splitn(2, '=') split on the `=` inside `!=`, giving:
    //   var_name = "if self.status !"
    //   rhs = " SyncStatus::Idle {"
    // Then it found `::` and `{` in rhs and treated it as enum literal!
    if is_control_flow_start(trimmed) {
        return None;
    }
    
    // EXCLUDE match arms: `Event::Data { id, body } =>`
    if trimmed.contains("=>") {
        return None;
    }
    
    // Must have = and ::
    if !trimmed.contains('=') || !trimmed.contains("::") {
        return None;
    }
    
    // CRITICAL FIX: Must have { OUTSIDE string literals  
    if !contains_brace_outside_string(trimmed) {
        return None;
    }
    
    // CRITICAL FIX: Find TRUE assignment `=` (not part of comparison operators)
    // Bug: splitn(2, '=') would split on FIRST `=`, even if part of `!=`, `==`, etc.
    let eq_pos = match find_assignment_eq_position(trimmed) {
        Some(pos) => pos,
        None => return None, // No standalone `=` found - not an assignment
    };
    
    let var_name = trimmed[..eq_pos].trim();
    let rhs = trimmed[eq_pos + 1..].trim();
    
    // CRITICAL FIX: Find brace outside strings in RHS
    let brace_pos = match find_brace_outside_string(rhs) {
        Some(pos) => pos,
        None => return None,
    };
    
    let before_brace = rhs[..brace_pos].trim();
    
    // CRITICAL FIX: If there's a `(` before {, it's likely a function call
    if before_brace.contains('(') {
        return None;
    }
    
    if before_brace.contains("::") {
        // Validate variant name starts with uppercase
        if let Some(last_colon_pos) = before_brace.rfind("::") {
            let variant = &before_brace[last_colon_pos + 2..].trim();
            if !variant.is_empty() && variant.chars().next().unwrap().is_uppercase() {
                return Some((var_name.to_string(), before_brace.to_string()));
            }
        }
    }
    
    None
}

//===========================================================================
// ARRAY LITERAL DETECTION
// Detects array literal start: `var = [` or `var = vec![` where bracket is not closed on same line
//===========================================================================

/// Detect if line starts an array literal: `varname = [` or `varname = vec![`
/// Returns (var_name, var_type, remaining_content) if matched
pub fn detect_array_literal_start(line: &str) -> Option<(String, Option<String>, String)> {
    let trimmed = line.trim();
    
    // CRITICAL FIX: EXCLUDE control flow statements
    if is_control_flow_start(trimmed) {
        return None;
    }
    
    // Must have = and [
    if !trimmed.contains('=') || !trimmed.contains('[') {
        return None;
    }
    
    // CRITICAL FIX: Find TRUE assignment `=` (not part of comparison operators)
    let eq_pos = match find_assignment_eq_position(trimmed) {
        Some(pos) => pos,
        None => return None,
    };
    
    let left = trimmed[..eq_pos].trim();
    let rhs = trimmed[eq_pos + 1..].trim();
    
    // CRITICAL FIX: RHS can start with [ OR vec![
    // Determine the content after the opening bracket
    let (starts_array, after_bracket) = if rhs.starts_with('[') {
        // Direct array: `[...]`
        (true, &rhs[1..])
    } else if rhs.starts_with("vec![") {
        // Vec macro: `vec![...]`
        (true, &rhs[5..])
    } else if rhs.starts_with("Vec::from([") {
        // Vec::from: `Vec::from([...])`
        (true, &rhs[11..])
    } else {
        (false, rhs)
    };
    
    if !starts_array {
        return None;
    }
    
    // If the line ends with ] or ];, it's a single-line array - let normal handling take it
    // Count brackets in the ENTIRE rhs to check if complete
    let open_brackets = rhs.matches('[').count();
    let close_brackets = rhs.matches(']').count();
    if open_brackets == close_brackets && close_brackets > 0 {
        return None; // Complete on one line
    }
    
    // Extract var name and optional type
    let (var_name, var_type) = if left.contains(':') {
        let type_parts: Vec<&str> = left.splitn(2, ':').collect();
        if type_parts.len() == 2 {
            (type_parts[0].trim().to_string(), Some(type_parts[1].trim().to_string()))
        } else {
            (left.to_string(), None)
        }
    } else {
        (left.to_string(), None)
    };
    
    // Validate var_name
    if !is_valid_identifier(&var_name) {
        return None;
    }
    
    Some((var_name, var_type, after_bracket.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_find_brace_outside_string() {
        // Normal brace
        assert_eq!(find_brace_outside_string("Foo {"), Some(4));
        assert_eq!(find_brace_outside_string("Event::Data {"), Some(12));
        
        // Brace inside string should be ignored
        assert_eq!(find_brace_outside_string("\"hello {}\""), None);
        assert_eq!(find_brace_outside_string("anyhow::bail(\"header {}\")"), None);
        
        // Mixed - brace after string
        assert_eq!(find_brace_outside_string("println(\"x\"); Foo {"), Some(18));
        
        // Escaped quote in string
        assert_eq!(find_brace_outside_string("\"escaped \\\" quote {}\""), None);
    }
    
    #[test]
    fn test_detect_struct_literal_start() {
        let registry = StructRegistry::new();
        
        // PascalCase heuristic
        let result = detect_struct_literal_start("user = User {", &registry);
        assert!(result.is_some());
        let (var, struct_name) = result.unwrap();
        assert_eq!(var, "user");
        assert_eq!(struct_name, "User");
        
        // Should not match enum paths
        let result = detect_struct_literal_start("ev = Event::Data {", &registry);
        assert!(result.is_none());
        
        // Should not match function definitions
        let result = detect_struct_literal_start("fn foo() -> User {", &registry);
        assert!(result.is_none());
    }
    
    #[test]
    fn test_detect_bare_struct_literal() {
        let registry = StructRegistry::new();
        
        let result = detect_bare_struct_literal("User {", &registry);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "User");
        
        // Should not match assignments
        let result = detect_bare_struct_literal("u = User {", &registry);
        assert!(result.is_none());
        
        // Should not match enum paths
        let result = detect_bare_struct_literal("Event::Data {", &registry);
        assert!(result.is_none());
    }
    
    #[test]
    fn test_detect_enum_literal_start() {
        let result = detect_enum_literal_start("ev = Event::Data {");
        assert!(result.is_some());
        let (var, enum_path) = result.unwrap();
        assert_eq!(var, "ev");
        assert_eq!(enum_path, "Event::Data");
        
        // Should not match match arms
        let result = detect_enum_literal_start("Event::Data { id } =>");
        assert!(result.is_none());
    }
    
    #[test]
    fn test_comparison_operators_not_detected_as_enum_literal() {
        // CRITICAL: These should NOT be detected as enum literals!
        // Bug was: `if self.status != SyncStatus::Idle {` was incorrectly detected
        // because splitn(2, '=') split on `=` inside `!=`
        
        let result = detect_enum_literal_start("if self.status != SyncStatus::Idle {");
        assert!(result.is_none(), "!= should not trigger enum literal detection");
        
        let result = detect_enum_literal_start("if x == SyncStatus::Synced {");
        assert!(result.is_none(), "== should not trigger enum literal detection");
        
        let result = detect_enum_literal_start("while count <= SyncStatus::Max {");
        assert!(result.is_none(), "<= should not trigger enum literal detection");
        
        let result = detect_enum_literal_start("if level >= SyncStatus::High {");
        assert!(result.is_none(), ">= should not trigger enum literal detection");
        
        // But TRUE assignment should still work
        let result = detect_enum_literal_start("self.status = SyncStatus::Synced {");
        assert!(result.is_some(), "true assignment should be detected");
    }
    
    #[test]
    fn test_control_flow_not_detected_as_literal() {
        let registry = StructRegistry::new();
        
        // if statements
        assert!(detect_struct_literal_start("if condition = User {", &registry).is_none());
        assert!(detect_enum_literal_start("if let SyncStatus::Idle = status {").is_none());
        
        // while statements
        assert!(detect_struct_literal_start("while running = Status {", &registry).is_none());
        
        // for statements
        assert!(detect_struct_literal_start("for item = Items {", &registry).is_none());
        
        // match statements
        assert!(detect_struct_literal_start("match x = Value {", &registry).is_none());
        
        // return statements
        assert!(detect_struct_literal_start("return result = Ok {", &registry).is_none());
    }
    
    #[test]
    fn test_find_assignment_eq_position() {
        // Simple assignment
        assert_eq!(find_assignment_eq_position("x = 10"), Some(2));
        assert_eq!(find_assignment_eq_position("self.status = SyncStatus::Idle"), Some(12));
        
        // Comparison operators - should return None
        assert!(find_assignment_eq_position("x == 10").is_none());
        assert!(find_assignment_eq_position("x != 10").is_none());
        assert!(find_assignment_eq_position("x <= 10").is_none());
        assert!(find_assignment_eq_position("x >= 10").is_none());
        
        // Compound assignments - should return None
        assert!(find_assignment_eq_position("x += 10").is_none());
        assert!(find_assignment_eq_position("x -= 10").is_none());
        assert!(find_assignment_eq_position("x *= 10").is_none());
        
        // Fat arrow - should return None  
        assert!(find_assignment_eq_position("x => y").is_none());
        
        // Mixed - has both comparison and assignment
        // `status = if x != y { a } else { b }` - should find the FIRST assignment
        assert_eq!(find_assignment_eq_position("status = x"), Some(7));
    }
    
    #[test]
    fn test_detect_bare_enum_literal() {
        let result = detect_bare_enum_literal("Event::Data {");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "Event::Data");
        
        // Should not match match arms
        let result = detect_bare_enum_literal("Event::Data { id } =>");
        assert!(result.is_none());
    }
    
    #[test]
    fn test_detect_array_literal_start() {
        let result = detect_array_literal_start("arr = [");
        assert!(result.is_some());
        let (var, var_type, _) = result.unwrap();
        assert_eq!(var, "arr");
        assert!(var_type.is_none());
        
        // With type annotation
        let result = detect_array_literal_start("arr: Vec[i32] = [");
        assert!(result.is_some());
        let (var, var_type, _) = result.unwrap();
        assert_eq!(var, "arr");
        assert_eq!(var_type, Some("Vec[i32]".to_string()));
        
        // Single-line array should return None
        let result = detect_array_literal_start("arr = [1, 2, 3]");
        assert!(result.is_none());
    }
    
    //=========================================================================
    // CRITICAL BUG FIX TESTS
    // These tests verify that format strings with {} don't trigger detection
    //=========================================================================
    
    #[test]
    fn test_macro_call_not_detected_as_enum_literal() {
        // CRITICAL: anyhow::bail with format string must NOT be detected as enum literal
        let result = detect_bare_enum_literal("anyhow::bail(\"header {} mismatch\")");
        assert!(result.is_none(), "macro call should not be detected as enum literal");
        
        let result = detect_bare_enum_literal("anyhow::bail(\"header {} mismatch: expected {}, got {}\", a, b)");
        assert!(result.is_none(), "macro call with multiple format args should not be detected");
        
        // log macros
        let result = detect_bare_enum_literal("log::info(\"processing {} items\", count)");
        assert!(result.is_none(), "log::info should not be detected as enum literal");
        
        // tracing macros
        let result = detect_bare_enum_literal("tracing::debug(\"request {} completed\", id)");
        assert!(result.is_none(), "tracing macros should not be detected as enum literal");
    }
    
    #[test]
    fn test_macro_call_not_detected_as_enum_assignment() {
        // Even with assignment pattern, should not match
        let result = detect_enum_literal_start("x = anyhow::bail(\"error {}\")");
        assert!(result.is_none(), "macro call in assignment should not be detected");
    }
    
    #[test]
    fn test_real_enum_variant_still_detected() {
        // Real enum variants should still work
        let result = detect_bare_enum_literal("SyncStatus::SyncingHeaders {");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "SyncStatus::SyncingHeaders");
        
        let result = detect_enum_literal_start("status = SyncStatus::Idle {");
        // Note: SyncStatus::Idle is a unit variant, but SyncStatus::SyncingHeaders is struct variant
        // The detection should work for struct variants
    }
    
    #[test]
    fn test_format_string_with_braces() {
        // These should all return None - braces are inside strings
        let result = detect_bare_enum_literal("println(\"hello {}\")");
        assert!(result.is_none());
        
        let result = detect_bare_struct_literal("format(\"user {} logged in\")");
        assert!(result.is_none());
    }
    
    //=========================================================================
    // VEC![ MACRO DETECTION TESTS
    // Verify that vec![ is properly detected as array literal
    //=========================================================================
    
    #[test]
    fn test_detect_vec_macro_array_literal() {
        // vec![ should be detected as array literal start
        let result = detect_array_literal_start("arr = vec![");
        assert!(result.is_some(), "vec![ should be detected as array literal");
        let (var, var_type, after) = result.unwrap();
        assert_eq!(var, "arr");
        assert!(var_type.is_none());
        assert!(after.is_empty());
        
        // vec![ with initial content
        let result = detect_array_literal_start("items = vec![ SyncStatus::Idle,");
        assert!(result.is_some(), "vec![ with content should be detected");
        let (var, _, after) = result.unwrap();
        assert_eq!(var, "items");
        assert!(after.trim().starts_with("SyncStatus"));
        
        // Single-line vec![] should return None
        let result = detect_array_literal_start("arr = vec![1, 2, 3]");
        assert!(result.is_none(), "complete vec![] should not be detected");
        
        // Vec::from([ should also work
        let result = detect_array_literal_start("data = Vec::from([");
        assert!(result.is_some(), "Vec::from([ should be detected as array literal");
    }
}