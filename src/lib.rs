// Core modules
pub mod variable;
pub mod scope;
pub mod function;
pub mod struct_def;
pub mod enum_def;
pub mod control_flow;
pub mod error_msg;
pub mod semantic_check;
pub mod anti_fail_logic;
pub mod rust_sanity;

//IR-based modules
pub mod ast;
pub mod hir;
pub mod eir;
pub mod parser;

// Re-export IR types for convenience
pub use ast::{Span, Spanned, EffectDecl};
pub use hir::{BindingId, BindingInfo, ScopeResolver, HirModule};
pub use eir::{Effect, EffectSet, EffectContext, EffectInference};
pub use parser::{Lexer, FunctionParser, extract_function_signatures};

use std::collections::{HashSet, HashMap};

use variable::{VariableTracker, parse_rusts_assignment, parse_rusts_assignment_ext, expand_value};
use scope::ScopeAnalyzer;
use function::{
    parse_function_line, signature_to_rust, FunctionParseResult,
    FunctionRegistry, CurrentFunctionContext,
    transform_string_concat, transform_call_args, should_be_tail_return
};
use struct_def::{
    StructRegistry, is_struct_definition, parse_struct_header, 
    transform_struct_field,
};
use enum_def::{
    EnumRegistry, EnumParseContext,
    is_enum_definition, parse_enum_header, transform_enum_variant,
};
use control_flow::{
    MatchModeStack, is_match_start, is_match_arm_pattern,
    transform_arm_pattern, transform_arm_close, transform_arm_close_with_parens,
    is_if_assignment, parse_control_flow_assignment,
    is_single_line_arm, transform_single_line_arm,
    transform_enum_struct_init,
    MatchStringContext, transform_match_for_string_patterns, pattern_is_string_literal,
};

/// Strip inline comments from a line, preserving string literals
fn strip_inline_comment(line: &str) -> String {
    let mut result = String::new();
    let mut in_string = false;
    let mut prev_char = ' ';
    let chars: Vec<char> = line.chars().collect();
    
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        
        if c == '"' && prev_char != '\\' {
            in_string = !in_string;
        }
        
        if !in_string && c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            break;
        }
        
        result.push(c);
        prev_char = c;
        i += 1;
    }
    
    result.trim_end().to_string()
}

/// Check if a line needs a semicolon (ONLY for non-literal mode)
fn needs_semicolon(trimmed: &str) -> bool {
    if trimmed.is_empty() { return false; }
    if trimmed.ends_with(';') { return false; }
    if trimmed.ends_with('{') || trimmed.ends_with('}') { return false; }
    if trimmed.ends_with(',') { return false; }
    
    if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") 
       || trimmed.starts_with("struct ") || trimmed.starts_with("enum ")
       || trimmed.starts_with("impl ") || trimmed.starts_with("trait ")
       || trimmed.starts_with("mod ") {
        return false;
    }
    
    if trimmed.starts_with("if ") || trimmed.starts_with("else") 
       || trimmed.starts_with("for ") || trimmed.starts_with("while ")
       || trimmed.starts_with("loop") || trimmed.starts_with("match ") {
        return false;
    }
    
    if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("*") {
        return false;
    }
    
    if trimmed.starts_with('#') { return false; }
    if trimmed == ")" || trimmed == ");" { return false; }
    
    true
}

/// L-08: Transform RustS+ macro calls to Rust macro calls
/// 
/// RustS+ allows: `println("hello")`, `format("x={}", x)`
/// Rust requires: `println!("hello")`, `format!("x={}", x)`
/// 
/// This function adds the `!` for known macro names.
fn transform_macro_calls(line: &str) -> String {
    // List of common Rust macros that need `!`
    const MACROS: &[&str] = &[
        "println", "print", "eprintln", "eprint",
        "format", "panic", "todo", "unimplemented",
        "vec", "dbg", "assert", "assert_eq", "assert_ne",
        "debug_assert", "debug_assert_eq", "debug_assert_ne",
        "write", "writeln", "format_args",
        "include_str", "include_bytes", "concat", "stringify",
        "env", "option_env", "cfg", "line", "column", "file",
        "module_path", "compile_error",
    ];
    
    let mut result = line.to_string();
    
    for macro_name in MACROS {
        // Pattern: `macro_name(` but not `macro_name!(` (already has !)
        // Also need to handle: start of line, after space, after `=`, etc.
        
        // Find all occurrences of macro_name followed by ( but not !
        let search_pattern = format!("{}(", macro_name);
        let correct_pattern = format!("{}!(", macro_name);
        
        // Only replace if it's not already correct
        if result.contains(&search_pattern) && !result.contains(&correct_pattern) {
            // Need to be careful: only replace when it's actually the macro call
            // (not part of another word like "my_println")
            
            let mut new_result = String::new();
            let mut chars: Vec<char> = result.chars().collect();
            let mut i = 0;
            
            while i < chars.len() {
                // Check if we're at the start of macro_name
                let remaining: String = chars[i..].iter().collect();
                
                if remaining.starts_with(&search_pattern) {
                    // Check that it's not part of another identifier
                    let is_word_start = i == 0 || !chars[i-1].is_alphanumeric() && chars[i-1] != '_';
                    
                    if is_word_start {
                        // Check it's not already `macro_name!(`
                        let before_paren: String = chars[i..i+macro_name.len()].iter().collect();
                        if before_paren == *macro_name {
                            new_result.push_str(macro_name);
                            new_result.push('!');
                            i += macro_name.len();
                            continue;
                        }
                    }
                }
                
                new_result.push(chars[i]);
                i += 1;
            }
            
            result = new_result;
        }
    }
    
    result
}

//===========================================================================
// LITERAL MODE CONTEXT
// Tracks when we are inside a struct/enum literal expression.
// In literal mode: NO `let`, NO `;`, ONLY field transformation.
//===========================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
enum LiteralKind {
    Struct,      // Inside StructName { ... }
    EnumVariant, // Inside Enum::Variant { ... }
}

#[derive(Debug, Clone)]
struct LiteralModeEntry {
    kind: LiteralKind,
    start_depth: usize, // Brace depth when we entered
    is_assignment: bool, // true = `x = Struct {}`, false = bare `Struct {}` (return expr)
}

#[derive(Debug, Clone)]
struct LiteralModeStack {
    stack: Vec<LiteralModeEntry>,
}

impl LiteralModeStack {
    fn new() -> Self {
        LiteralModeStack { stack: Vec::new() }
    }
    
    fn enter(&mut self, kind: LiteralKind, depth: usize, is_assignment: bool) {
        self.stack.push(LiteralModeEntry { kind, start_depth: depth, is_assignment });
    }
    
    fn is_active(&self) -> bool {
        !self.stack.is_empty()
    }
    
    #[allow(dead_code)]
    fn current_kind(&self) -> Option<LiteralKind> {
        self.stack.last().map(|e| e.kind)
    }
    
    fn current_is_assignment(&self) -> bool {
        self.stack.last().map(|e| e.is_assignment).unwrap_or(true)
    }
    
    /// Check if we should exit (depth went back to entry point)
    fn should_exit(&self, current_depth: usize) -> bool {
        if let Some(entry) = self.stack.last() {
            current_depth <= entry.start_depth
        } else {
            false
        }
    }
    
    fn exit(&mut self) {
        self.stack.pop();
    }
}

//===========================================================================
// ARRAY LITERAL MODE CONTEXT
// Tracks when we are inside an array literal expression: [elem1, elem2, ...]
// In array mode: NO semicolons inside, elements separated by commas.
// Array literals are ATOMIC - must be emitted as one complete expression.
//===========================================================================

#[derive(Debug, Clone)]
struct ArrayModeEntry {
    start_bracket_depth: usize, // Bracket depth when we entered
    is_assignment: bool,        // true = `x = [...]`, false = bare `[...]`
    var_name: String,           // Variable being assigned to
    var_type: Option<String>,   // Explicit type annotation if any
    needs_let: bool,            // Whether to emit `let`
    needs_mut: bool,            // Whether to emit `mut`
}

#[derive(Debug, Clone)]
struct ArrayModeStack {
    stack: Vec<ArrayModeEntry>,
}

impl ArrayModeStack {
    fn new() -> Self {
        ArrayModeStack { stack: Vec::new() }
    }
    
    fn enter(&mut self, bracket_depth: usize, is_assignment: bool, var_name: String, 
             var_type: Option<String>, needs_let: bool, needs_mut: bool) {
        self.stack.push(ArrayModeEntry { 
            start_bracket_depth: bracket_depth, 
            is_assignment,
            var_name,
            var_type,
            needs_let,
            needs_mut,
        });
    }
    
    fn is_active(&self) -> bool {
        !self.stack.is_empty()
    }
    
    fn current(&self) -> Option<&ArrayModeEntry> {
        self.stack.last()
    }
    
    /// Check if we should exit (bracket depth went back to entry point)
    fn should_exit(&self, current_bracket_depth: usize) -> bool {
        if let Some(entry) = self.stack.last() {
            current_bracket_depth <= entry.start_bracket_depth
        } else {
            false
        }
    }
    
    fn exit(&mut self) -> Option<ArrayModeEntry> {
        self.stack.pop()
    }
}

/// Detect if line starts a struct literal: `varname = StructName {`
/// Returns (var_name, struct_name) if matched, excludes Enum::Variant
fn detect_struct_literal_start(line: &str, registry: &StructRegistry) -> Option<(String, String)> {
    let trimmed = line.trim();
    
    if !trimmed.contains('=') || !trimmed.contains('{') {
        return None;
    }
    
    let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
    if parts.len() != 2 { return None; }
    
    let var_name = parts[0].trim();
    let rhs = parts[1].trim();
    
    // EXCLUDE enum paths (:: before {)
    if let Some(brace_pos) = rhs.find('{') {
        let before_brace = &rhs[..brace_pos];
        if before_brace.contains("::") {
            return None;
        }
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
fn detect_bare_struct_literal(line: &str, registry: &StructRegistry) -> Option<String> {
    let trimmed = line.trim();
    
    // Must have { but NOT have = before it (or = is inside the braces)
    if !trimmed.contains('{') {
        return None;
    }
    
    // If there's a = BEFORE {, it's an assignment, not bare literal
    if let Some(brace_pos) = trimmed.find('{') {
        let before_brace = &trimmed[..brace_pos];
        // Check for = outside of any context
        if before_brace.contains('=') {
            return None;
        }
        
        // EXCLUDE enum paths (has ::)
        if before_brace.contains("::") {
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
    }
    
    None
}

/// Detect BARE enum struct variant literal (without assignment): `Enum::Variant {`
fn detect_bare_enum_literal(line: &str) -> Option<String> {
    let trimmed = line.trim();
    
    // EXCLUDE match arms: `Event::Data { id, body } =>`
    if trimmed.contains("=>") {
        return None;
    }
    
    // Must have :: and {
    if !trimmed.contains("::") || !trimmed.contains('{') {
        return None;
    }
    
    // If there's a = BEFORE {, it's an assignment
    if let Some(brace_pos) = trimmed.find('{') {
        let before_brace = &trimmed[..brace_pos];
        if before_brace.contains('=') {
            return None;
        }
        
        let enum_path = before_brace.trim();
        if !enum_path.is_empty() && enum_path.contains("::") {
            return Some(enum_path.to_string());
        }
    }
    
    None
}

fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() { return false; }
    let first = s.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' { return false; }
    s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Detect if line starts an enum struct variant literal: `varname = Enum::Variant {`
fn detect_enum_literal_start(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    
    // EXCLUDE match arms: `Event::Data { id, body } =>`
    if trimmed.contains("=>") {
        return None;
    }
    
    if !trimmed.contains('=') || !trimmed.contains("::") || !trimmed.contains('{') {
        return None;
    }
    
    let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
    if parts.len() != 2 { return None; }
    
    let var_name = parts[0].trim();
    let rhs = parts[1].trim();
    
    // Must have :: before {
    if let Some(brace_pos) = rhs.find('{') {
        let before_brace = rhs[..brace_pos].trim();
        if before_brace.contains("::") {
            return Some((var_name.to_string(), before_brace.to_string()));
        }
    }
    
    None
}

//===========================================================================
// ARRAY LITERAL DETECTION
// Detects array literal start: `var = [` where bracket is not closed on same line
//===========================================================================

/// Detect if line starts an array literal: `varname = [` or `varname = [\n`
/// Returns (var_name, var_type, remaining_content) if matched
fn detect_array_literal_start(line: &str) -> Option<(String, Option<String>, String)> {
    let trimmed = line.trim();
    
    // Must have = and [
    if !trimmed.contains('=') || !trimmed.contains('[') {
        return None;
    }
    
    // Split by first =
    let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
    if parts.len() != 2 { return None; }
    
    let left = parts[0].trim();
    let rhs = parts[1].trim();
    
    // RHS must start with [ (after trimming)
    if !rhs.starts_with('[') {
        return None;
    }
    
    // If the line ends with ], it's a single-line array - let normal handling take it
    // Count brackets to see if array is complete on this line
    let open_brackets = rhs.matches('[').count();
    let close_brackets = rhs.matches(']').count();
    if open_brackets == close_brackets && close_brackets > 0 {
        return None; // Complete on one line, handle normally
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
    
    // Content after [ (may be empty or have first element)
    let after_bracket = &rhs[1..];
    
    Some((var_name, var_type, after_bracket.to_string()))
}

/// Transform an array element line - handle enum struct variants, string literals, etc.
fn transform_array_element(line: &str) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // Empty line
    if trimmed.is_empty() {
        return String::new();
    }
    
    // Closing bracket
    if trimmed == "]" || trimmed == "];" {
        return format!("{}]", leading_ws);
    }
    
    // Comments pass through
    if trimmed.starts_with("//") {
        return line.to_string();
    }
    
    // Transform enum struct init: Event::C { x = 4 } -> Event::C { x: 4 }
    let transformed = transform_enum_struct_init_in_array(trimmed);
    
    // Ensure element ends with comma (unless it's just a closing bracket)
    let with_comma = if transformed.ends_with(',') || transformed.ends_with('{') 
                        || transformed.ends_with('[') || transformed == "]" {
        transformed
    } else {
        format!("{},", transformed)
    };
    
    format!("{}{}", leading_ws, with_comma)
}

/// Transform enum struct variant inside array: Event::C { x = 4 } -> Event::C { x: 4 }
fn transform_enum_struct_init_in_array(s: &str) -> String {
    // Check if this is an enum struct variant
    if !s.contains("::") || !s.contains('{') {
        return s.to_string();
    }
    
    // Find { and transform fields inside
    if let Some(brace_start) = s.find('{') {
        let before = &s[..brace_start + 1];
        let after_brace = &s[brace_start + 1..];
        
        // Find matching }
        if let Some(brace_end) = after_brace.rfind('}') {
            let fields_part = &after_brace[..brace_end];
            let after_close = &after_brace[brace_end..];
            
            // Transform fields: x = 1 -> x: 1
            let transformed_fields = transform_enum_fields_inline(fields_part);
            return format!("{}{}{}", before, transformed_fields, after_close);
        }
    }
    
    s.to_string()
}

/// Transform inline enum fields: "x = 1, y = 2" → "x: 1, y: 2"
fn transform_enum_fields_inline(fields: &str) -> String {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut brace_depth: usize = 0;
    
    for c in fields.chars() {
        if c == '"' && !current.ends_with('\\') {
            in_string = !in_string;
        }
        if !in_string {
            if c == '{' { brace_depth += 1; }
            if c == '}' { brace_depth = brace_depth.saturating_sub(1); }
        }
        
        if c == ',' && !in_string && brace_depth == 0 {
            let transformed = transform_single_enum_field(&current);
            if !transformed.is_empty() {
                result.push(transformed);
            }
            current.clear();
        } else {
            current.push(c);
        }
    }
    
    // Last field
    let transformed = transform_single_enum_field(&current);
    if !transformed.is_empty() {
        result.push(transformed);
    }
    
    result.join(", ")
}

/// Transform a single enum field: `x = 1` → `x: 1`
fn transform_single_enum_field(field: &str) -> String {
    let trimmed = field.trim();
    if trimmed.is_empty() { return String::new(); }
    
    // Already transformed (has : but not ::)
    if trimmed.contains(':') && !trimmed.contains("::") { 
        return trimmed.to_string(); 
    }
    
    // Find = that's not ==, !=, etc
    if let Some(eq_pos) = find_field_eq(trimmed) {
        let name = trimmed[..eq_pos].trim();
        let value = trimmed[eq_pos + 1..].trim();
        
        if is_valid_field_name(name) {
            return format!("{}: {}", name, value);
        }
    }
    
    trimmed.to_string()
}

/// Transform a literal field line: `field = value` → `field: value,`
/// NO `let`, NO `;` - this is expression-only context!
fn transform_literal_field(line: &str) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // Handle spread syntax
    if trimmed.starts_with("..") {
        return format!("{}{}", leading_ws, trimmed);
    }
    
    // Skip braces
    if trimmed.is_empty() || trimmed == "{" || trimmed == "}" || trimmed == "}," {
        return line.to_string();
    }
    
    // Already has colon (except ::)
    if trimmed.contains(':') && !trimmed.contains("::") {
        let clean = trimmed.trim_end_matches(',');
        return format!("{}{},", leading_ws, clean);
    }
    
    // Nested literal start: `header = Header {` - transform = to :
    if trimmed.contains('{') {
        if let Some(eq_pos) = find_field_eq(trimmed) {
            let field = trimmed[..eq_pos].trim();
            let value = trimmed[eq_pos + 1..].trim();
            if is_valid_field_name(field) {
                return format!("{}{}: {}", leading_ws, field, value);
            }
        }
        return format!("{}{}", leading_ws, trimmed);
    }
    
    // Simple field: `field = value`
    if let Some(eq_pos) = find_field_eq(trimmed) {
        let field = trimmed[..eq_pos].trim();
        let value = trimmed[eq_pos + 1..].trim();
        
        if is_valid_field_name(field) && !value.is_empty() {
            // Transform string literals to String::from
            let transformed_value = if is_string_literal(value) {
                let inner = &value[1..value.len()-1];
                format!("String::from(\"{}\")", inner)
            } else {
                value.to_string()
            };
            
            return format!("{}{}: {},", leading_ws, field, transformed_value);
        }
    }
    
    format!("{}{}", leading_ws, trimmed)
}

/// Find the `=` that's a field assignment (not ==, !=, <=, >=, =>)
fn find_field_eq(s: &str) -> Option<usize> {
    let chars: Vec<char> = s.chars().collect();
    for i in 0..chars.len() {
        if chars[i] == '=' {
            let prev = if i > 0 { chars[i-1] } else { ' ' };
            let next = if i + 1 < chars.len() { chars[i+1] } else { ' ' };
            
            if prev != '!' && prev != '<' && prev != '>' && prev != '=' && next != '=' && next != '>' {
                return Some(i);
            }
        }
    }
    None
}

fn is_valid_field_name(s: &str) -> bool {
    if s.is_empty() { return false; }
    let first = s.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' { return false; }
    s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn is_string_literal(s: &str) -> bool {
    let t = s.trim();
    t.starts_with('"') && t.ends_with('"') && !t.contains("String::from")
}

//===========================================================================
// L-04: NON-COPY ARRAY ACCESS TRANSFORMATION
// Transform `arr[i]` to `arr[i].clone()` for non-Copy types
// This ensures ownership semantics are handled correctly
//===========================================================================

/// Transform array index access to add .clone() for non-Copy types
/// 
/// L-04 RULE: Array access on non-Copy elements MUST use explicit strategy
/// We choose `.clone()` as the deterministic strategy.
/// 
/// Examples:
/// - `events[i]` → `events[i].clone()`
/// - `arr[0]` → `arr[0].clone()`
/// - `data[idx]` → `data[idx].clone()`
/// 
/// EXCEPTIONS (no clone needed):
/// - Already has .clone() 
/// - Is a method call on indexed element: `arr[i].len()`
/// - Is a field access: `arr[i].field`
/// - Is inside string literal
fn transform_array_access_clone(value: &str) -> String {
    let trimmed = value.trim();
    
    // Skip if empty or already has clone
    if trimmed.is_empty() || trimmed.ends_with(".clone()") {
        return value.to_string();
    }
    
    // Skip if not a simple array index pattern
    // Must match: identifier[expr]
    if !trimmed.contains('[') || !trimmed.contains(']') {
        return value.to_string();
    }
    
    // Skip complex expressions (multiple operations)
    if trimmed.contains(" + ") || trimmed.contains(" - ") || 
       trimmed.contains(" * ") || trimmed.contains(" / ") ||
       trimmed.contains(" && ") || trimmed.contains(" || ") {
        return value.to_string();
    }
    
    // Skip if there's a method call or field access after the ]
    // e.g., arr[i].len() or arr[i].field
    if let Some(bracket_end) = trimmed.rfind(']') {
        let after_bracket = &trimmed[bracket_end + 1..];
        if after_bracket.starts_with('.') {
            // Already has method/field access
            return value.to_string();
        }
    }
    
    // Skip if it's a string/char literal
    if trimmed.starts_with('"') || trimmed.starts_with('\'') {
        return value.to_string();
    }
    
    // Skip if it's a number literal
    if trimmed.parse::<i64>().is_ok() || trimmed.parse::<f64>().is_ok() {
        return value.to_string();
    }
    
    // Check for simple array index pattern: identifier[expr]
    // Find the first [ that's not inside a string
    let mut in_string = false;
    let mut bracket_start = None;
    let mut bracket_end = None;
    
    for (i, c) in trimmed.char_indices() {
        if c == '"' {
            in_string = !in_string;
        }
        if !in_string {
            if c == '[' && bracket_start.is_none() {
                bracket_start = Some(i);
            } else if c == ']' {
                bracket_end = Some(i);
            }
        }
    }
    
    if let (Some(start), Some(end)) = (bracket_start, bracket_end) {
        // Verify the part before [ is a valid identifier
        let before_bracket = &trimmed[..start];
        if is_valid_array_base(before_bracket) {
            // This is an array access - add .clone()
            return format!("{}.clone()", trimmed);
        }
    }
    
    value.to_string()
}

/// Check if the base of an array access is a valid identifier or field access
fn is_valid_array_base(base: &str) -> bool {
    let trimmed = base.trim();
    if trimmed.is_empty() {
        return false;
    }
    
    // Simple identifier: events, arr, data
    if is_valid_identifier(trimmed) {
        return true;
    }
    
    // Field access: self.events, obj.data
    if trimmed.contains('.') {
        let parts: Vec<&str> = trimmed.split('.').collect();
        return parts.iter().all(|p| is_valid_identifier(p.trim()));
    }
    
    false
}

/// Extract the pattern part from a match arm line: `Pattern {` → `Pattern`
fn extract_arm_pattern(line: &str) -> String {
    let trimmed = line.trim();
    
    // Find the last { which starts the body
    if let Some(brace_pos) = trimmed.rfind('{') {
        return trimmed[..brace_pos].trim().to_string();
    }
    
    trimmed.to_string()
}

//===========================================================================
// CLONE INJECTION HELPERS (L-04 Enhancement)
// When .clone() is generated, ensure target type has #[derive(Clone)]
//===========================================================================

/// Detect element type from array literal element
/// Examples:
/// - `Event::Credit { id = 1 }` → Some("Event")
/// - `Node { id = 1 }` → Some("Node")
/// - `123` → None (primitive)
/// - `"hello"` → None (primitive)
fn detect_type_from_element(element: &str) -> Option<String> {
    let trimmed = element.trim().trim_end_matches(',');
    
    // Skip empty, primitives
    if trimmed.is_empty() {
        return None;
    }
    
    // Skip string literals
    if trimmed.starts_with('"') {
        return None;
    }
    
    // Skip numeric literals
    if trimmed.parse::<i64>().is_ok() || trimmed.parse::<f64>().is_ok() {
        return None;
    }
    
    // Skip bool literals
    if trimmed == "true" || trimmed == "false" {
        return None;
    }
    
    // Pattern: Enum::Variant or Enum::Variant { ... } or Enum::Variant(...)
    if trimmed.contains("::") {
        // Extract type before ::
        if let Some(pos) = trimmed.find("::") {
            let type_name = trimmed[..pos].trim();
            if !type_name.is_empty() && type_name.chars().next().unwrap().is_uppercase() {
                return Some(type_name.to_string());
            }
        }
    }
    
    // Pattern: StructName { ... }
    if trimmed.contains('{') {
        if let Some(pos) = trimmed.find('{') {
            let type_name = trimmed[..pos].trim();
            if !type_name.is_empty() && type_name.chars().next().unwrap().is_uppercase() {
                return Some(type_name.to_string());
            }
        }
    }
    
    // Pattern: TupleStruct(...)
    if trimmed.contains('(') && !trimmed.starts_with('(') {
        if let Some(pos) = trimmed.find('(') {
            let type_name = trimmed[..pos].trim();
            if !type_name.is_empty() && type_name.chars().next().unwrap().is_uppercase() {
                return Some(type_name.to_string());
            }
        }
    }
    
    None
}

/// Extract array variable name from array access expression
/// `events[i]` → Some("events")
/// `self.data[0]` → Some("self.data")
fn extract_array_var_from_access(expr: &str) -> Option<String> {
    let trimmed = expr.trim();
    
    if !trimmed.contains('[') {
        return None;
    }
    
    if let Some(pos) = trimmed.find('[') {
        let var_name = trimmed[..pos].trim();
        if !var_name.is_empty() {
            return Some(var_name.to_string());
        }
    }
    
    None
}

/// Check if an expression is an array access that would get .clone()
fn is_cloneable_array_access(expr: &str) -> bool {
    let trimmed = expr.trim();
    
    // Must have brackets
    if !trimmed.contains('[') || !trimmed.contains(']') {
        return false;
    }
    
    // Skip if already has .clone()
    if trimmed.ends_with(".clone()") {
        return false;
    }
    
    // Skip if has method call after ]
    if let Some(bracket_end) = trimmed.rfind(']') {
        let after = &trimmed[bracket_end + 1..];
        if after.starts_with('.') {
            return false;
        }
    }
    
    true
}

//===========================================================================
// MAIN PARSER
//===========================================================================

pub fn parse_rusts(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let mut tracker = VariableTracker::new();
    
    // Run scope analysis
    let mut scope_analyzer = ScopeAnalyzer::new();
    scope_analyzer.analyze(source);
    
    // Build registries
    let mut fn_registry = FunctionRegistry::new();
    let mut struct_registry = StructRegistry::new();
    let mut enum_registry = EnumRegistry::new();
    
    //=========================================================================
    // CLONE INJECTION TRACKING (L-04 Enhancement)
    // Track: array_var → element_type, types that need Clone
    //=========================================================================
    let mut array_element_types: HashMap<String, String> = HashMap::new();
    let mut types_need_clone: HashSet<String> = HashSet::new();
    let mut current_array_var: Option<String> = None;
    
    let mut brace_depth: usize = 0;
    
    // First pass: register structs, enums, functions, track assignments
    for (line_num, line) in lines.iter().enumerate() {
        let clean_line = strip_inline_comment(line);
        let trimmed = clean_line.trim();
        
        tracker.scan_for_mut_borrows(&clean_line);
        
        // Register struct names
        if is_struct_definition(trimmed) {
            if let Some(name) = parse_struct_header(trimmed) {
                struct_registry.register(&name);
            }
        }
        
        // Register enum names
        if is_enum_definition(trimmed) {
            if let Some(name) = parse_enum_header(trimmed) {
                enum_registry.register(&name);
            }
        }
        
        // Register function signatures
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            if let FunctionParseResult::RustSPlusSignature(sig) = parse_function_line(trimmed) {
                fn_registry.register(sig);
            }
        }
        
        //=====================================================================
        // CLONE TRACKING: Detect array assignments and their element types
        //=====================================================================
        
        // Detect array literal start: `events = [`
        if let Some((var_name, _, _)) = detect_array_literal_start(trimmed) {
            current_array_var = Some(var_name);
        }
        
        // Detect array elements and extract type
        if current_array_var.is_some() && !trimmed.starts_with('[') && !trimmed.is_empty() {
            if trimmed == "]" {
                current_array_var = None;
            } else if let Some(ref var) = current_array_var {
                // Try to detect type from this element
                if let Some(elem_type) = detect_type_from_element(trimmed) {
                    array_element_types.insert(var.clone(), elem_type);
                }
            }
        }
        
        // Detect array access that will get .clone(): `x = arr[i]`
        if let Some((_, _, value, _, _)) = parse_rusts_assignment_ext(&clean_line) {
            if is_cloneable_array_access(&value) {
                if let Some(arr_var) = extract_array_var_from_access(&value) {
                    // Mark the element type as needing Clone
                    if let Some(elem_type) = array_element_types.get(&arr_var) {
                        types_need_clone.insert(elem_type.clone());
                    }
                }
            }
        }
        
        brace_depth += trimmed.matches('{').count();
        brace_depth = brace_depth.saturating_sub(trimmed.matches('}').count());
        
        if trimmed.starts_with("let ") { continue; }
        
        // CRITICAL: Use extended parser to detect explicit `mut` keyword
        // `mut x = 10` means x is DECLARED here, subsequent `x = ...` are mutations
        if let Some((var_name, var_type, value, _is_outer, is_explicit_mut)) = parse_rusts_assignment_ext(&clean_line) {
            tracker.track_assignment(line_num, &var_name, var_type, &value, false);
            // If explicit mut, mark variable as mutable immediately
            if is_explicit_mut {
                tracker.mark_mut_borrowed(&var_name); // Use existing mechanism to ensure mut
            }
        }
    }
    
    //=========================================================================
    // TRANSITIVE CLONE DETECTION
    // If Event needs Clone and contains Node, then Node also needs Clone
    // We scan type definitions to find nested type references
    //=========================================================================
    let mut in_type_def: Option<String> = None;  // Currently inside which type definition
    let mut type_contents: HashMap<String, Vec<String>> = HashMap::new(); // type → contained types
    
    for line in lines.iter() {
        let clean_line = strip_inline_comment(line);
        let trimmed = clean_line.trim();
        
        // Detect struct/enum definition start
        if is_struct_definition(trimmed) {
            if let Some(name) = parse_struct_header(trimmed) {
                in_type_def = Some(name);
            }
        } else if is_enum_definition(trimmed) {
            if let Some(name) = parse_enum_header(trimmed) {
                in_type_def = Some(name);
            }
        } else if trimmed == "}" && in_type_def.is_some() {
            in_type_def = None;
        } else if let Some(ref type_name) = in_type_def {
            // We're inside a type definition - look for references to other types
            // Check for patterns like: Init(Node), field Node, Node,
            for struct_name in struct_registry.names.iter() {
                if trimmed.contains(struct_name) {
                    type_contents.entry(type_name.clone())
                        .or_insert_with(Vec::new)
                        .push(struct_name.clone());
                }
            }
            for enum_name in enum_registry.names.iter() {
                if trimmed.contains(enum_name) && enum_name != type_name {
                    type_contents.entry(type_name.clone())
                        .or_insert_with(Vec::new)
                        .push(enum_name.clone());
                }
            }
        }
    }
    
    // Propagate Clone requirement transitively
    // Repeat until no new types are added
    loop {
        let mut added_any = false;
        for type_name in types_need_clone.clone().iter() {
            if let Some(contained) = type_contents.get(type_name) {
                for contained_type in contained {
                    if !types_need_clone.contains(contained_type) {
                        types_need_clone.insert(contained_type.clone());
                        added_any = true;
                    }
                }
            }
        }
        if !added_any {
            break;
        }
    }
    
    let mut output_lines: Vec<String> = Vec::new();
    
    // Reset for second pass
    brace_depth = 0;
    let mut in_function_body = false;
    let mut function_start_brace = 0;
    let mut current_fn_ctx = CurrentFunctionContext::new();
    
    // Struct/enum definition contexts
    let mut in_struct_def = false;
    let mut struct_def_depth = 0;
    let mut enum_ctx = EnumParseContext::new();
    
    // LITERAL MODE - the key to fixing the bug!
    let mut literal_mode = LiteralModeStack::new();
    
    // ARRAY MODE - for multiline array literal expressions
    let mut array_mode = ArrayModeStack::new();
    // Bracket depth tracking for array mode
    let mut bracket_depth: usize = 0;
    
    // MATCH MODE - for RustS+ match syntax transformation
    let mut match_mode = MatchModeStack::new();
    
    // IF EXPRESSION ASSIGNMENT MODE - tracks `x = if cond {` for semicolon at end
    let mut if_expr_assignment_depth: Option<usize> = None;
    
    for (line_num, line) in lines.iter().enumerate() {
        let clean_line = strip_inline_comment(line);
        let trimmed = clean_line.trim();
        let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        
        // Track function context
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            in_function_body = true;
            function_start_brace = brace_depth + 1;
            
            if let FunctionParseResult::RustSPlusSignature(ref sig) = parse_function_line(trimmed) {
                current_fn_ctx.enter(sig, function_start_brace);
            }
        }
        
        // Calculate brace depth BEFORE processing
        let prev_depth = brace_depth;
        let opens = trimmed.matches('{').count();
        let closes = trimmed.matches('}').count();
        brace_depth += opens;
        brace_depth = brace_depth.saturating_sub(closes);
        
        // Calculate bracket depth for array literals
        let prev_bracket_depth = bracket_depth;
        let bracket_opens = trimmed.matches('[').count();
        let bracket_closes = trimmed.matches(']').count();
        bracket_depth += bracket_opens;
        bracket_depth = bracket_depth.saturating_sub(bracket_closes);
        
        // Exit function context
        if in_function_body && brace_depth < function_start_brace && trimmed == "}" {
            in_function_body = false;
            current_fn_ctx.exit();
        }
        
        // Check for return detection
        let is_before_closing_brace = {
            let mut found = false;
            for future_line in lines.iter().skip(line_num + 1) {
                let ft = strip_inline_comment(future_line);
                let ft = ft.trim();
                if !ft.is_empty() {
                    found = ft == "}" || ft.starts_with("}");
                    break;
                }
            }
            found
        };
        
        if trimmed.is_empty() {
            output_lines.push(String::new());
            continue;
        }
        
        //=======================================================================
        // ARRAY MODE EXIT CHECK - must happen BEFORE other processing
        //=======================================================================
        if array_mode.is_active() && trimmed.contains(']') {
            // Check if this closes the array
            if array_mode.should_exit(bracket_depth) {
                if let Some(entry) = array_mode.exit() {
                    // Transform the closing line (may have content before ])
                    let transformed = transform_array_element(&clean_line);
                    
                    // Determine suffix based on context
                    let suffix = if entry.is_assignment { ";" } else { "" };
                    
                    // Output the closing with proper suffix
                    let close_line = if transformed.trim() == "]" {
                        format!("{}]{}", leading_ws, suffix)
                    } else {
                        // Has content before ], like `    Event::Query(1)]`
                        let without_bracket = transformed.trim().trim_end_matches(']').trim_end_matches(',');
                        format!("{}    {},\n{}]{}", leading_ws, without_bracket, leading_ws, suffix)
                    };
                    output_lines.push(close_line);
                    continue;
                }
            }
        }
        
        //=======================================================================
        // ARRAY MODE ACTIVE - transform as element, NO let, NO ;
        //=======================================================================
        if array_mode.is_active() {
            let transformed = transform_array_element(&clean_line);
            output_lines.push(transformed);
            continue;
        }
        
        //=======================================================================
        // LITERAL MODE EXIT CHECK - must happen BEFORE processing
        //=======================================================================
        if literal_mode.is_active() && trimmed == "}" {
            // Check if this closes the literal
            if literal_mode.should_exit(brace_depth) {
                let was_assignment = literal_mode.current_is_assignment();
                literal_mode.exit();
                // Determine suffix:
                // - Nested literal (still in parent literal): `,`
                // - Top-level assignment literal: `;`
                // - Top-level return expression (bare literal): no suffix
                let suffix = if literal_mode.is_active() { 
                    "," 
                } else if was_assignment {
                    ";"
                } else {
                    "" // bare return expression
                };
                output_lines.push(format!("{}}}{}", leading_ws, suffix));
                continue;
            }
        }
        
        //=======================================================================
        // LITERAL MODE ACTIVE - transform as field, NO let, NO ;
        //=======================================================================
        if literal_mode.is_active() {
            let transformed = transform_literal_field(&clean_line);
            
            // Check if this line ALSO starts a nested literal
            if trimmed.contains('{') && !trimmed.ends_with('}') {
                // Enter nested literal mode - nested literals are fields, not assignments
                let kind = if trimmed.contains("::") { 
                    LiteralKind::EnumVariant 
                } else { 
                    LiteralKind::Struct 
                };
                literal_mode.enter(kind, prev_depth + opens, false);
            }
            
            output_lines.push(transformed);
            continue;
        }
        
        //=======================================================================
        // MATCH MODE - RustS+ match syntax transformation
        //=======================================================================
        
        // Check for match arm body close FIRST (before other checks)
        if match_mode.is_active() && trimmed == "}" {
            if match_mode.should_exit_arm(brace_depth) {
                // L-02: Get parens state BEFORE exiting arm body
                let uses_parens = match_mode.arm_uses_parens();
                match_mode.exit_arm_body();
                // Add comma after arm body close
                // L-02: Close with ), if using parenthesized form
                output_lines.push(transform_arm_close_with_parens(&clean_line, uses_parens));
                continue;
            }
            // Check if this closes the entire match
            if match_mode.should_exit_match(brace_depth) {
                let needs_semi = match_mode.current_is_assignment();
                match_mode.exit_match();
                // Add semicolon if this was an assignment
                let suffix = if needs_semi { ";" } else { "" };
                output_lines.push(format!("{}}}{}", leading_ws, suffix));
                continue;
            }
        }
        
        // Check for match arm pattern (when expecting one)
        if match_mode.expecting_arm_pattern() && is_match_arm_pattern(trimmed) {
            // Check if it's a SINGLE-LINE arm: `pattern { body }`
            if is_single_line_arm(trimmed) {
                // Get return type for string transformation
                let ret_type = current_fn_ctx.return_type.as_deref();
                let transformed = transform_single_line_arm(&clean_line, ret_type);
                output_lines.push(transformed);
                // Stay in expecting_arm_pattern mode (don't enter arm body)
                continue;
            }
            
            //===================================================================
            // L-02: Expression Context for Match Arms with if/else
            // 
            // In Rust, if-else expressions in match arms work directly:
            //   `Pattern => if cond { a } else { b },`
            // No parentheses needed (they cause compiler warnings).
            //
            // We only need to ensure proper comma after the arm body.
            //===================================================================
            let mut arm_has_if_expr = false;
            let mut arm_body_lines = Vec::new();
            let mut temp_depth = prev_depth + opens;
            
            // Look ahead to collect arm body and detect if expression
            for future_line in lines.iter().skip(line_num + 1) {
                let ft = strip_inline_comment(future_line);
                let ft_trim = ft.trim();
                
                // Track depth
                let ft_opens = ft_trim.matches('{').count();
                let ft_closes = ft_trim.matches('}').count();
                
                temp_depth += ft_opens;
                temp_depth = temp_depth.saturating_sub(ft_closes);
                
                // Check if this closes the arm
                if ft_trim == "}" && temp_depth < prev_depth + opens {
                    break;
                }
                
                arm_body_lines.push(ft_trim.to_string());
                
                // Detect if first meaningful line is an if expression
                if arm_body_lines.len() == 1 && !ft_trim.is_empty() {
                    if ft_trim.starts_with("if ") && !ft_trim.contains("let ") {
                        arm_has_if_expr = true;
                    }
                }
            }
            
            // Transform arm pattern - NEVER use parentheses, just => 
            if arm_has_if_expr {
                // L-09: Use plain => for if expression arms (no parens)
                let pattern = extract_arm_pattern(trimmed);
                output_lines.push(format!("{}{} =>", leading_ws, pattern));
            } else {
                // Standard: Use => { for block arms
                let transformed = transform_arm_pattern(&clean_line);
                output_lines.push(transformed);
            }
            
            // Enter arm body
            // L-02: Pass arm_has_if_expr so we know to close with ) instead of }
            // CRITICAL: Use brace_depth (which accounts for both opens AND closes on this line)
            // instead of prev_depth + opens (which only counts opens).
            // This is important for patterns like `Event::Credit { id, amount } {`
            // where opens=2, closes=1, so net depth increase is 1, not 2.
            match_mode.enter_arm_body(brace_depth, arm_has_if_expr);
            continue;
        }
        
        // Check for match expression start
        if is_match_start(trimmed) {
            // Check if it's an assignment: `x = match expr {`
            let is_assignment = parse_control_flow_assignment(trimmed).is_some();
            
            //===================================================================
            // BUG C FIX: Detect if match has string literal patterns
            // If so, transform match expr to add .as_str()
            // Look ahead to find patterns
            //===================================================================
            let mut match_string_ctx = MatchStringContext::from_match_line(trimmed);
            
            // Look ahead to detect string literal patterns
            for future_line in lines.iter().skip(line_num + 1) {
                let ft = strip_inline_comment(future_line);
                let ft_trim = ft.trim();
                
                // Stop at match closing brace at same depth (heuristic)
                if ft_trim == "}" {
                    break;
                }
                
                // Check if this is a pattern line with string literal
                if pattern_is_string_literal(ft_trim) {
                    match_string_ctx.has_string_patterns = true;
                    break; // Found one, no need to continue
                }
                
                // Also check single-line arms with string patterns
                if ft_trim.starts_with('"') && ft_trim.contains('{') {
                    match_string_ctx.has_string_patterns = true;
                    break;
                }
            }
            
            // Determine if we need .as_str() transformation
            let needs_as_str = match_string_ctx.needs_as_str();
            
            if let Some((var_name, match_expr)) = parse_control_flow_assignment(trimmed) {
                // Need to check if this is first assignment
                let is_first = tracker.is_first_assignment(&var_name, line_num);
                let is_shadowing = tracker.is_shadowing(&var_name, line_num);
                let needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
                let needs_let = is_first || is_shadowing;
                
                // Transform match expression if needed
                let transformed_match_expr = if needs_as_str {
                    transform_match_for_string_patterns(&match_expr, true)
                } else {
                    match_expr
                };
                
                if needs_let {
                    let keyword = if needs_mut { "let mut" } else { "let" };
                    output_lines.push(format!("{}{} {} = {}", leading_ws, keyword, var_name, transformed_match_expr));
                } else {
                    output_lines.push(format!("{}{} = {}", leading_ws, var_name, transformed_match_expr));
                }
            } else {
                // Bare match expression (like return expression)
                let transformed = if needs_as_str {
                    transform_match_for_string_patterns(trimmed, true)
                } else {
                    trimmed.to_string()
                };
                output_lines.push(format!("{}{}", leading_ws, transformed));
            }
            // Enter match mode with assignment tracking
            match_mode.enter_match(prev_depth, is_assignment);
            continue;
        }
        
        //=======================================================================
        // IF EXPRESSION AS ASSIGNMENT: `x = if cond {`
        // Must handle BEFORE normal assignment to avoid adding semicolon
        // CRITICAL BUG B FIX: Must wrap in parentheses for valid Rust
        // RustS+: `x = if cond { a } else { b }`
        // Rust:   `let x = (if cond { a } else { b });`
        //=======================================================================
        if is_if_assignment(trimmed) {
            if let Some((var_name, if_expr)) = parse_control_flow_assignment(trimmed) {
                // Need to check if this is first assignment
                let is_first = tracker.is_first_assignment(&var_name, line_num);
                let is_shadowing = tracker.is_shadowing(&var_name, line_num);
                let needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
                let needs_let = is_first || is_shadowing;
                
                // CRITICAL: Wrap if expression in parentheses for valid Rust expression context
                if needs_let {
                    let keyword = if needs_mut { "let mut" } else { "let" };
                    output_lines.push(format!("{}{} {} = ({}", leading_ws, keyword, var_name, if_expr));
                } else {
                    output_lines.push(format!("{}{} = ({}", leading_ws, var_name, if_expr));
                }
                // Track that we're in an if expression assignment
                if_expr_assignment_depth = Some(prev_depth);
                continue;
            }
        }
        
        // Check for if expression assignment end (closing `}` at right depth)
        if if_expr_assignment_depth.is_some() && trimmed == "}" {
            let start_depth = if_expr_assignment_depth.unwrap();
            // Check if this } is at the same level as where if started
            // and not an else follows
            let next_is_else = {
                let mut found_else = false;
                for future_line in lines.iter().skip(line_num + 1) {
                    let ft = strip_inline_comment(future_line).trim().to_string();
                    if ft.is_empty() { continue; }
                    found_else = ft.starts_with("else") || ft.starts_with("} else");
                    break;
                }
                found_else
            };
            
            if brace_depth <= start_depth && !next_is_else {
                if_expr_assignment_depth = None;
                // CRITICAL: Close with }); to complete parenthesized expression
                output_lines.push(format!("{}}}); ", leading_ws));
                continue;
            }
        }
        
        // Inside match arm body - process lines including assignments
        if match_mode.in_arm_body() {
            // First check if it's a RustS+ assignment (including outer/mut keyword)
            if let Some((var_name, var_type, value, is_outer, is_explicit_mut)) = parse_rusts_assignment_ext(&clean_line) {
                let is_decl = scope_analyzer.is_decl(line_num);
                let is_mutation = scope_analyzer.is_mut(line_num);
                let borrowed_mut = tracker.is_mut_borrowed(&var_name);
                let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
                // CRITICAL: is_explicit_mut ALWAYS forces `let mut`
                let needs_mut = is_explicit_mut || borrowed_mut || scope_needs_mut;
                
                let mut expanded_value = expand_value(&value, var_type.as_deref());
                
                // L-04: Add .clone() for array access
                expanded_value = transform_array_access_clone(&expanded_value);
                
                if current_fn_ctx.is_inside() {
                    expanded_value = transform_string_concat(&expanded_value, &current_fn_ctx);
                }
                expanded_value = transform_call_args(&expanded_value, &fn_registry);
                
                // PRIORITY ORDER:
                // 1. outer keyword → mutation
                // 2. is_mutation (from scope analyzer) → mutation (NO let)
                // 3. is_explicit_mut OR is_decl → new declaration with let
                
                if is_outer {
                    output_lines.push(format!("{}{} = {};", leading_ws, var_name, expanded_value));
                } else if is_mutation && !is_explicit_mut {
                    // L-03: Mutation to existing variable - NO let
                    output_lines.push(format!("{}{} = {};", leading_ws, var_name, expanded_value));
                } else if is_explicit_mut || is_decl {
                    // CRITICAL: `mut x = 10` MUST become `let mut x = 10;`
                    let let_keyword = if needs_mut { "let mut" } else { "let" };
                    let type_annotation = if let Some(ref t) = var_type {
                        format!(": {}", t)
                    } else if VariableTracker::detect_string_literal(&value) {
                        ": String".to_string()
                    } else {
                        String::new()
                    };
                    output_lines.push(format!("{}{} {}{} = {};", 
                        leading_ws, let_keyword, var_name, type_annotation, expanded_value));
                } else {
                    let is_first = tracker.is_first_assignment(&var_name, line_num);
                    let is_shadowing = tracker.is_shadowing(&var_name, line_num);
                    
                    if is_first || is_shadowing {
                        let let_keyword = if needs_mut { "let mut" } else { "let" };
                        let type_annotation = if let Some(ref t) = var_type {
                            format!(": {}", t)
                        } else if VariableTracker::detect_string_literal(&value) {
                            ": String".to_string()
                        } else {
                            String::new()
                        };
                        output_lines.push(format!("{}{} {}{} = {};", 
                            leading_ws, let_keyword, var_name, type_annotation, expanded_value));
                    } else {
                        output_lines.push(format!("{}{} = {};", leading_ws, var_name, expanded_value));
                    }
                }
                continue;
            }
            
            // Not an assignment - apply transformations
            let mut transformed = trimmed.to_string();
            
            //=================================================================
            // L-01 CRITICAL FIX: Handle bare `mut x = value` in match arm body
            // If parse_rusts_assignment_ext failed but line starts with `mut `,
            // this is a bug - we should force proper transformation
            //=================================================================
            if trimmed.starts_with("mut ") && trimmed.contains('=') && !trimmed.contains("==") {
                // This is a bare `mut x = value` that should have been transformed!
                let rest = trimmed.strip_prefix("mut ").unwrap().trim();
                if let Some(eq_pos) = rest.find('=') {
                    let var_part = rest[..eq_pos].trim();
                    let val_part = rest[eq_pos + 1..].trim().trim_end_matches(';');
                    
                    let (var_name, type_annotation) = if var_part.contains(':') {
                        let parts: Vec<&str> = var_part.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            (parts[0].trim(), format!(": {}", parts[1].trim()))
                        } else {
                            (var_part, String::new())
                        }
                    } else {
                        (var_part, String::new())
                    };
                    
                    let mut expanded_value = expand_value(val_part, None);
                    expanded_value = transform_array_access_clone(&expanded_value);
                    if current_fn_ctx.is_inside() {
                        expanded_value = transform_string_concat(&expanded_value, &current_fn_ctx);
                    }
                    expanded_value = transform_call_args(&expanded_value, &fn_registry);
                    
                    let output = format!("{}let mut {}{} = {};", 
                        leading_ws, var_name, type_annotation, expanded_value);
                    output_lines.push(output);
                    continue;
                }
            }
            
            // Apply function context transformations
            if current_fn_ctx.is_inside() {
                transformed = transform_string_concat(&transformed, &current_fn_ctx);
                transformed = transform_call_args(&transformed, &fn_registry);
            }
            
            // Check if this is a tail expression (last line before `}`)
            let is_tail = is_before_closing_brace;
            
            // String literal as tail expression in String-returning function
            if is_tail {
                if let Some(ref ret_type) = current_fn_ctx.return_type {
                    if ret_type == "String" && control_flow::is_string_literal(&transformed) {
                        transformed = control_flow::transform_string_to_owned(&transformed);
                    }
                }
            }
            
            // Add semicolon if needed (but not for tail expressions)
            if needs_semicolon(&transformed) && !is_tail {
                output_lines.push(format!("{}{};", leading_ws, transformed));
            } else {
                output_lines.push(format!("{}{}", leading_ws, transformed));
            }
            continue;
        }
        
        //=======================================================================
        // STRUCT DEFINITION (type definition, not instantiation)
        // L-12: Always inject #[derive(Clone)] for RustS+ value semantics
        // RustS+ has value semantics - all types should be cloneable
        //=======================================================================
        if is_struct_definition(trimmed) && !in_struct_def {
            in_struct_def = true;
            struct_def_depth = brace_depth;
            
            // L-12: Always add Clone for RustS+ structs
            if let Some(_struct_name) = parse_struct_header(trimmed) {
                // Check if previous line already has #[derive(...)]
                let prev_line = output_lines.last().map(|s| s.trim().to_string());
                if let Some(ref prev) = prev_line {
                    if prev.starts_with("#[derive(") && prev.ends_with(")]") {
                        // Check if Clone is already present
                        if !prev.contains("Clone") {
                            // Merge Clone into existing derive
                            let last_idx = output_lines.len() - 1;
                            let existing = output_lines[last_idx].clone();
                            let merged = existing.replace(")]", ", Clone)]");
                            output_lines[last_idx] = merged;
                        }
                    } else {
                        // Add new derive line
                        output_lines.push(format!("{}#[derive(Clone)]", leading_ws));
                    }
                } else {
                    output_lines.push(format!("{}#[derive(Clone)]", leading_ws));
                }
            }
            
            output_lines.push(format!("{}{}", leading_ws, trimmed));
            continue;
        }
        
        if in_struct_def {
            if trimmed == "}" && brace_depth <= struct_def_depth {
                in_struct_def = false;
                output_lines.push(format!("{}}}", leading_ws));
                continue;
            }
            let transformed = transform_struct_field(&clean_line);
            output_lines.push(transformed);
            continue;
        }
        
        //=======================================================================
        // ENUM DEFINITION
        // L-12: Always inject #[derive(Clone)] for RustS+ value semantics
        //=======================================================================
        if is_enum_definition(trimmed) && !enum_ctx.in_enum_def {
            enum_ctx.enter_enum(brace_depth);
            
            // L-12: Always add Clone for RustS+ enums
            if let Some(_enum_name) = parse_enum_header(trimmed) {
                // Check if previous line already has #[derive(...)]
                let prev_line = output_lines.last().map(|s| s.trim().to_string());
                if let Some(ref prev) = prev_line {
                    if prev.starts_with("#[derive(") && prev.ends_with(")]") {
                        // Check if Clone is already present
                        if !prev.contains("Clone") {
                            // Merge Clone into existing derive
                            let last_idx = output_lines.len() - 1;
                            let existing = output_lines[last_idx].clone();
                            let merged = existing.replace(")]", ", Clone)]");
                            output_lines[last_idx] = merged;
                        }
                    } else {
                        // Add new derive line
                        output_lines.push(format!("{}#[derive(Clone)]", leading_ws));
                    }
                } else {
                    output_lines.push(format!("{}#[derive(Clone)]", leading_ws));
                }
            }
            
            output_lines.push(format!("{}{}", leading_ws, trimmed));
            continue;
        }
        
        if enum_ctx.in_enum_def {
            // Check struct variant exit FIRST (before enum exit check)
            if trimmed == "}" && enum_ctx.in_struct_variant {
                enum_ctx.exit_struct_variant();
                output_lines.push(format!("{}}},", leading_ws));
                continue;
            }
            
            // Then check enum exit
            if trimmed == "}" && brace_depth <= enum_ctx.start_depth {
                enum_ctx.exit_enum();
                output_lines.push(format!("{}}}", leading_ws));
                continue;
            }
            
            // Struct variant start tracking
            if trimmed.contains('{') && !trimmed.ends_with('}') {
                enum_ctx.enter_struct_variant();
            }
            
            let transformed = transform_enum_variant(&clean_line, enum_ctx.in_struct_variant);
            output_lines.push(transformed);
            continue;
        }
        
        //=======================================================================
        // STRUCT LITERAL START: `u = User {`
        // ENTER LITERAL MODE - NO let inside!
        // L-07: Check if variable needs mut (reassigned in child scope)
        //=======================================================================
        if let Some((var_name, struct_name)) = detect_struct_literal_start(trimmed, &struct_registry) {
            // L-07: Determine if this binding needs mut
            let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
            let borrowed_mut = tracker.is_mut_borrowed(&var_name);
            let needs_mut = scope_needs_mut || borrowed_mut;
            let let_keyword = if needs_mut { "let mut" } else { "let" };
            
            // Check if single-line: `u = User { id = 1 }`
            if trimmed.ends_with('}') {
                let transformed = transform_single_line_struct_literal(trimmed, &var_name);
                // L-07: Replace "let " with appropriate keyword
                let transformed = if needs_mut {
                    transformed.replacen("let ", "let mut ", 1)
                } else {
                    transformed
                };
                output_lines.push(format!("{}{}", leading_ws, transformed));
                continue;
            }
            
            // Multi-line struct literal - ENTER LITERAL MODE (is_assignment = true)
            literal_mode.enter(LiteralKind::Struct, prev_depth + opens, true);
            output_lines.push(format!("{}{} {} = {} {{", leading_ws, let_keyword, var_name, struct_name));
            continue;
        }
        
        //=======================================================================
        // ENUM STRUCT VARIANT LITERAL: `e = Event::Data {`
        // ENTER LITERAL MODE - NO let inside!
        // L-07: Check if variable needs mut (reassigned in child scope)
        //=======================================================================
        if let Some((var_name, enum_path)) = detect_enum_literal_start(trimmed) {
            // L-07: Determine if this binding needs mut
            let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
            let borrowed_mut = tracker.is_mut_borrowed(&var_name);
            let needs_mut = scope_needs_mut || borrowed_mut;
            let let_keyword = if needs_mut { "let mut" } else { "let" };
            
            // Check if single-line
            if trimmed.ends_with('}') {
                let transformed = transform_single_line_enum_literal(trimmed, &var_name, &enum_path);
                // L-07: Replace "let " with appropriate keyword
                let transformed = if needs_mut {
                    transformed.replacen("let ", "let mut ", 1)
                } else {
                    transformed
                };
                output_lines.push(format!("{}{}", leading_ws, transformed));
                continue;
            }
            
            // Multi-line - ENTER LITERAL MODE (is_assignment = true)
            literal_mode.enter(LiteralKind::EnumVariant, prev_depth + opens, true);
            output_lines.push(format!("{}{} {} = {} {{", leading_ws, let_keyword, var_name, enum_path));
            continue;
        }
        
        //=======================================================================
        // BARE STRUCT LITERAL (return expression): `Packet {`
        // ENTER LITERAL MODE - NO let, this is a return expression!
        //=======================================================================
        if let Some(struct_name) = detect_bare_struct_literal(trimmed, &struct_registry) {
            // Check if single-line: `Packet { header: h, payload: p }`
            if trimmed.ends_with('}') {
                let transformed = transform_bare_struct_literal(trimmed);
                output_lines.push(format!("{}{}", leading_ws, transformed));
                continue;
            }
            
            // Multi-line - ENTER LITERAL MODE (is_assignment = false, return expression)
            literal_mode.enter(LiteralKind::Struct, prev_depth + opens, false);
            output_lines.push(format!("{}{} {{", leading_ws, struct_name));
            continue;
        }
        
        //=======================================================================
        // BARE ENUM STRUCT VARIANT LITERAL: `Event::Data {`
        // ENTER LITERAL MODE - NO let, this is a return expression!
        //=======================================================================
        if let Some(enum_path) = detect_bare_enum_literal(trimmed) {
            // Check if single-line
            if trimmed.ends_with('}') {
                let transformed = transform_bare_struct_literal(trimmed);
                output_lines.push(format!("{}{}", leading_ws, transformed));
                continue;
            }
            
            // Multi-line - ENTER LITERAL MODE (is_assignment = false, return expression)
            literal_mode.enter(LiteralKind::EnumVariant, prev_depth + opens, false);
            output_lines.push(format!("{}{} {{", leading_ws, enum_path));
            continue;
        }
        
        //=======================================================================
        // FUNCTION DEFINITION
        //=======================================================================
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            match parse_function_line(trimmed) {
                FunctionParseResult::RustSPlusSignature(sig) => {
                    let rust_sig = signature_to_rust(&sig);
                    output_lines.push(format!("{}{}", leading_ws, rust_sig));
                    continue;
                }
                FunctionParseResult::RustPassthrough => {
                    // L-05 CRITICAL FIX: Even for Rust passthrough, strip effects clause
                    // User may write: fn foo(a: i32) effects(io) { }
                    // which has Rust-style params but RustS+ effects
                    let mut output = clean_line.clone();
                    
                    // Strip effects annotation if present
                    if output.contains("effects(") {
                        // Find and remove effects(...) clause
                        // Pattern: "effects(" ... ")" followed by optional whitespace and then rest
                        if let Some(effects_start) = output.find("effects(") {
                            // Find the matching closing paren
                            let mut paren_depth = 0;
                            let mut effects_end = effects_start;
                            for (i, c) in output[effects_start..].char_indices() {
                                match c {
                                    '(' => paren_depth += 1,
                                    ')' => {
                                        paren_depth -= 1;
                                        if paren_depth == 0 {
                                            effects_end = effects_start + i + 1;
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            // Remove the effects clause
                            let before = &output[..effects_start];
                            let after = output[effects_end..].trim_start();
                            output = format!("{}{}", before.trim_end(), 
                                if after.is_empty() || after.starts_with('{') { 
                                    format!(" {}", after) 
                                } else { 
                                    format!(" {}", after) 
                                });
                        }
                    }
                    
                    // Extract return type for tail return handling
                    if output.contains(" -> ") {
                        if let Some(arrow_pos) = output.find(" -> ") {
                            let after_arrow = &output[arrow_pos + 4..];
                            let ret_end = after_arrow.find(|c: char| c == '{' || c.is_whitespace())
                                .unwrap_or(after_arrow.len());
                            let ret_type = after_arrow[..ret_end].trim();
                            if !ret_type.is_empty() {
                                current_fn_ctx.return_type = Some(ret_type.to_string());
                                current_fn_ctx.start_depth = function_start_brace;
                            }
                        }
                    }
                    if needs_semicolon(trimmed) {
                        output = format!("{};", output);
                    }
                    output_lines.push(output);
                    continue;
                }
                FunctionParseResult::Error(e) => {
                    output_lines.push(format!("{}// COMPILE ERROR: {}", leading_ws, e));
                    output_lines.push(clean_line.clone());
                    continue;
                }
                FunctionParseResult::NotAFunction => {
                    output_lines.push(clean_line.clone());
                    continue;
                }
            }
        }
        
        //=======================================================================
        // L-07: EFFECT STATEMENT SKIP
        // `effect write(account)` is a RustS+-only declaration
        // It must NOT appear in Rust output - skip entirely
        //=======================================================================
        if trimmed.starts_with("effect ") {
            // Effect statements are purely for the effect ownership system
            // They do not produce any Rust output
            continue;
        }
        
        //=======================================================================
        // RUST NATIVE PASSTHROUGH
        //=======================================================================
        let is_rust_native = trimmed.starts_with("let ") 
            || trimmed.starts_with("const ") 
            || trimmed.starts_with("static ")
            || trimmed.starts_with("use ")
            || trimmed.starts_with("mod ") 
            || trimmed.starts_with("impl ") 
            || trimmed.starts_with("trait ") 
            || trimmed.starts_with("type ")
            || trimmed.starts_with("//") 
            || trimmed.starts_with("/*") 
            || trimmed.starts_with("*")
            || trimmed.starts_with('#') 
            || trimmed == "{" 
            || trimmed == "}"
            || trimmed.starts_with("if ")
            || trimmed.starts_with("else")
            || trimmed.starts_with("for ")
            || trimmed.starts_with("while ")
            || trimmed.starts_with("loop")
            || trimmed.starts_with("match ")
            || trimmed.starts_with("return ")
            || trimmed.starts_with("break")
            || trimmed.starts_with("continue")
            || trimmed.starts_with("pub ");
        
        if is_rust_native {
            let mut transformed = trimmed.to_string();
            if current_fn_ctx.is_inside() {
                transformed = transform_string_concat(&transformed, &current_fn_ctx);
                transformed = transform_call_args(&transformed, &fn_registry);
            }
            
            // Transform enum struct init: Event::C { x = 4 } -> Event::C { x: 4 }
            transformed = transform_enum_struct_init(&transformed);
            
            let is_return_expr = should_be_tail_return(&transformed, &current_fn_ctx, is_before_closing_brace);
            
            let output = if needs_semicolon(&transformed) && !is_return_expr {
                format!("{}{};", leading_ws, transformed)
            } else {
                format!("{}{}", leading_ws, transformed)
            };
            
            output_lines.push(output);
            continue;
        }
        
        //=======================================================================
        // ARRAY LITERAL START: `events = [` (multiline)
        // ENTER ARRAY MODE - NO semicolons inside!
        //=======================================================================
        if let Some((var_name, var_type, after_bracket)) = detect_array_literal_start(trimmed) {
            // Determine if we need let/mut
            let is_first = tracker.is_first_assignment(&var_name, line_num);
            let is_shadowing = tracker.is_shadowing(&var_name, line_num);
            let borrowed_mut = tracker.is_mut_borrowed(&var_name);
            let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
            let needs_mut = borrowed_mut || scope_needs_mut;
            let needs_let = is_first || is_shadowing;
            
            // Enter array mode
            array_mode.enter(
                prev_bracket_depth + bracket_opens, 
                true, // is_assignment
                var_name.clone(),
                var_type.clone(),
                needs_let,
                needs_mut
            );
            
            // Emit the opening line
            let let_keyword = if needs_let {
                if needs_mut { "let mut " } else { "let " }
            } else {
                ""
            };
            
            let type_annotation = if let Some(ref t) = var_type {
                format!(": {}", t)
            } else {
                String::new()
            };
            
            // Check if there's content after the [
            let after = after_bracket.trim();
            if after.is_empty() {
                output_lines.push(format!("{}{}{}{} = [", leading_ws, let_keyword, var_name, type_annotation));
            } else {
                // Has first element on same line
                let transformed_first = transform_array_element(&format!("    {}", after));
                output_lines.push(format!("{}{}{}{} = [", leading_ws, let_keyword, var_name, type_annotation));
                if !transformed_first.trim().is_empty() {
                    output_lines.push(transformed_first);
                }
            }
            continue;
        }
        
        //=======================================================================
        // RUSTS+ ASSIGNMENT (normal variable assignment, NOT in literal mode)
        //=======================================================================
        if let Some((var_name, var_type, value, is_outer, is_explicit_mut)) = parse_rusts_assignment_ext(&clean_line) {
            let is_decl = scope_analyzer.is_decl(line_num);
            let is_mutation = scope_analyzer.is_mut(line_num);
            let borrowed_mut = tracker.is_mut_borrowed(&var_name);
            let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
            // CRITICAL: is_explicit_mut ALWAYS forces `let mut`
            let needs_mut = is_explicit_mut || borrowed_mut || scope_needs_mut;
            
            let mut expanded_value = expand_value(&value, var_type.as_deref());
            
            //===================================================================
            // L-04: Non-Copy Array/Collection Access
            // If RHS is array indexing like `arr[i]`, add .clone() for non-Copy
            // Heuristic: any array index access gets .clone() unless it's a
            // primitive numeric/bool literal context
            //===================================================================
            expanded_value = transform_array_access_clone(&expanded_value);
            
            if current_fn_ctx.is_inside() {
                expanded_value = transform_string_concat(&expanded_value, &current_fn_ctx);
            }
            expanded_value = transform_call_args(&expanded_value, &fn_registry);
            
            // Transform enum struct init: Event::C { x = 4 } -> Event::C { x: 4 }
            expanded_value = transform_enum_struct_init(&expanded_value);
            
            if is_outer {
                let output_line = format!("{}{} = {};", leading_ws, var_name, expanded_value);
                output_lines.push(output_line);
            } else if is_explicit_mut || is_decl {
                // CRITICAL: `mut x = 10` MUST become `let mut x = 10;`
                let let_keyword = if needs_mut { "let mut" } else { "let" };
                let type_annotation = if let Some(ref t) = var_type {
                    format!(": {}", t)
                } else if VariableTracker::detect_string_literal(&value) && var_type.as_deref() != Some("&str") {
                    ": String".to_string()
                } else {
                    String::new()
                };
                let output_line = format!("{}{} {}{} = {};", 
                    leading_ws, let_keyword, var_name, type_annotation, expanded_value);
                output_lines.push(output_line);
            } else if is_mutation {
                let output_line = format!("{}{} = {};", leading_ws, var_name, expanded_value);
                output_lines.push(output_line);
            } else {
                let is_first = tracker.is_first_assignment(&var_name, line_num);
                let is_shadowing = tracker.is_shadowing(&var_name, line_num);
                let needs_let = is_first || is_shadowing;
                
                if needs_let {
                    let let_keyword = if needs_mut { "let mut" } else { "let" };
                    let type_annotation = if let Some(ref t) = var_type {
                        format!(": {}", t)
                    } else if VariableTracker::detect_string_literal(&value) && var_type.as_deref() != Some("&str") {
                        ": String".to_string()
                    } else {
                        String::new()
                    };
                    let output_line = format!("{}{} {}{} = {};", 
                        leading_ws, let_keyword, var_name, type_annotation, expanded_value);
                    output_lines.push(output_line);
                } else {
                    let output_line = format!("{}{} = {};", leading_ws, var_name, expanded_value);
                    output_lines.push(output_line);
                }
            }
        } else {
            // Not an assignment - transform and pass through
            let mut transformed = trimmed.to_string();
            
            //=================================================================
            // L-01 CRITICAL FIX: Handle bare `mut x = value` that wasn't parsed
            // If parse_rusts_assignment_ext failed but line starts with `mut `,
            // this is a bug - we should force proper transformation
            //=================================================================
            if trimmed.starts_with("mut ") && trimmed.contains('=') && !trimmed.contains("==") {
                // This is a bare `mut x = value` that should have been transformed!
                // Try to manually parse and transform it
                let rest = trimmed.strip_prefix("mut ").unwrap().trim();
                if let Some(eq_pos) = rest.find('=') {
                    let var_part = rest[..eq_pos].trim();
                    let val_part = rest[eq_pos + 1..].trim().trim_end_matches(';');
                    
                    // Check for type annotation
                    let (var_name, type_annotation) = if var_part.contains(':') {
                        let parts: Vec<&str> = var_part.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            (parts[0].trim(), format!(": {}", parts[1].trim()))
                        } else {
                            (var_part, String::new())
                        }
                    } else {
                        (var_part, String::new())
                    };
                    
                    // Transform the value
                    let mut expanded_value = expand_value(val_part, None);
                    expanded_value = transform_array_access_clone(&expanded_value);
                    if current_fn_ctx.is_inside() {
                        expanded_value = transform_string_concat(&expanded_value, &current_fn_ctx);
                    }
                    expanded_value = transform_call_args(&expanded_value, &fn_registry);
                    
                    // Output as `let mut var = value;`
                    let output = format!("{}let mut {}{} = {};", 
                        leading_ws, var_name, type_annotation, expanded_value);
                    output_lines.push(output);
                    continue;
                }
            }
            
            if current_fn_ctx.is_inside() {
                transformed = transform_string_concat(&transformed, &current_fn_ctx);
            }
            transformed = transform_call_args(&transformed, &fn_registry);
            
            // Transform enum struct init: Event::C { x = 4 } -> Event::C { x: 4 }
            transformed = transform_enum_struct_init(&transformed);
            
            let is_return_expr = should_be_tail_return(&transformed, &current_fn_ctx, is_before_closing_brace);
            
            // CRITICAL: Convert string literal tail returns to String::from when return type is String
            if is_return_expr {
                if let Some(ref ret_type) = current_fn_ctx.return_type {
                    if ret_type == "String" && is_string_literal(&transformed) {
                        let inner = &transformed[1..transformed.len()-1];
                        transformed = format!("String::from(\"{}\")", inner);
                    }
                }
            }
            
            if needs_semicolon(&transformed) && !is_return_expr {
                let output = format!("{}{};", leading_ws, transformed);
                output_lines.push(output);
            } else {
                let output = format!("{}{}", leading_ws, transformed);
                output_lines.push(output);
            }
        }
    }
    
    // L-08: Transform macro calls (println -> println!, etc.)
    let transformed_lines: Vec<String> = output_lines
        .into_iter()
        .map(|line| transform_macro_calls(&line))
        .collect();
    
    //==========================================================================
    // L-01 POST-PROCESSING FIX: Catch any remaining bare `mut x = value`
    // This is a safety net for cases that slipped through the main processing.
    // Convert `mut x = value` to `let mut x = value;`
    //==========================================================================
    let fixed_lines: Vec<String> = transformed_lines
        .into_iter()
        .map(|line| fix_bare_mut_declaration(&line))
        .collect();
    
    //==========================================================================
    // L-05 POST-PROCESSING FIX: Strip any remaining effect annotations
    // This catches effect annotations that may have leaked through other paths.
    //==========================================================================
    let final_lines: Vec<String> = fixed_lines
        .into_iter()
        .map(|line| strip_effects_from_line(&line))
        .collect();
    
    let result = final_lines.join("\n");
    
    //==========================================================================
    // L-05: RUST SANITY CHECK
    // Validate that generated Rust code is syntactically valid BEFORE output.
    // If invalid → panic with INTERNAL COMPILER ERROR (not rustc error)
    //==========================================================================
    #[cfg(not(test))]
    {
        let sanity = rust_sanity::check_rust_output(&result);
        if !sanity.is_valid {
            // In non-test mode, print error but still return the code
            // (actual error handling is in main.rs)
            eprintln!("{}", rust_sanity::format_internal_error(&sanity));
        }
    }
    
    result
}

/// Transform single-line struct literal: `u = User { id = 1, name = "x" }`
fn transform_single_line_struct_literal(line: &str, var_name: &str) -> String {
    let trimmed = line.trim();
    
    if let Some(eq_pos) = trimmed.find('=') {
        let rhs = trimmed[eq_pos + 1..].trim();
        
        if let Some(brace_start) = rhs.find('{') {
            let struct_name = rhs[..brace_start].trim();
            let brace_end = rhs.rfind('}').unwrap_or(rhs.len());
            let fields_part = &rhs[brace_start + 1..brace_end];
            
            let transformed_fields = transform_literal_fields_inline(fields_part);
            
            return format!("let {} = {} {{ {} }};", var_name, struct_name, transformed_fields);
        }
    }
    
    format!("let {};", line)
}

/// Transform single-line enum literal: `e = Event::Data { id = 1 }`
fn transform_single_line_enum_literal(line: &str, var_name: &str, enum_path: &str) -> String {
    let trimmed = line.trim();
    
    if let Some(brace_start) = trimmed.find('{') {
        let brace_end = trimmed.rfind('}').unwrap_or(trimmed.len());
        let fields_part = &trimmed[brace_start + 1..brace_end];
        
        let transformed_fields = transform_literal_fields_inline(fields_part);
        
        return format!("let {} = {} {{ {} }};", var_name, enum_path, transformed_fields);
    }
    
    format!("let {};", line)
}

/// Transform BARE struct/enum literal (return expression): `Packet { header = h }`
/// NO let - this is a return expression!
fn transform_bare_struct_literal(line: &str) -> String {
    let trimmed = line.trim();
    
    if let Some(brace_start) = trimmed.find('{') {
        let name_part = trimmed[..brace_start].trim();
        let brace_end = trimmed.rfind('}').unwrap_or(trimmed.len());
        let fields_part = &trimmed[brace_start + 1..brace_end];
        
        let transformed_fields = transform_literal_fields_inline(fields_part);
        
        return format!("{} {{ {} }}", name_part, transformed_fields);
    }
    
    trimmed.to_string()
}

/// Transform inline literal fields: `id = 1, name = "x"` → `id: 1, name: String::from("x"),`
fn transform_literal_fields_inline(fields: &str) -> String {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut brace_depth: usize = 0;
    
    for c in fields.chars() {
        if c == '"' && !current.ends_with('\\') {
            in_string = !in_string;
        }
        if !in_string {
            if c == '{' { brace_depth += 1; }
            if c == '}' { brace_depth = brace_depth.saturating_sub(1); }
        }
        
        if c == ',' && !in_string && brace_depth == 0 {
            let transformed = transform_single_literal_field(&current);
            if !transformed.is_empty() {
                result.push(transformed);
            }
            current.clear();
        } else {
            current.push(c);
        }
    }
    
    // Last field
    let transformed = transform_single_literal_field(&current);
    if !transformed.is_empty() {
        result.push(transformed);
    }
    
    result.join(", ")
}

/// Transform a single field: `id = 1` → `id: 1`
fn transform_single_literal_field(field: &str) -> String {
    let trimmed = field.trim();
    if trimmed.is_empty() { return String::new(); }
    
    // Spread syntax
    if trimmed.starts_with("..") { return trimmed.to_string(); }
    
    // Already transformed
    if trimmed.contains(':') && !trimmed.contains("::") { return trimmed.to_string(); }
    
    if let Some(eq_pos) = find_field_eq(trimmed) {
        let name = trimmed[..eq_pos].trim();
        let value = trimmed[eq_pos + 1..].trim();
        
        if is_valid_field_name(name) {
            let transformed_value = if is_string_literal(value) {
                let inner = &value[1..value.len()-1];
                format!("String::from(\"{}\")", inner)
            } else {
                value.to_string()
            };
            return format!("{}: {}", name, transformed_value);
        }
    }
    
    trimmed.to_string()
}

//=============================================================================
// L-01 POST-PROCESSING: Fix bare `mut x = value` declarations
// This is a safety net that catches any bare mut that slipped through.
//=============================================================================

/// Fix bare `mut x = value` declarations that weren't properly transformed.
/// Converts them to `let mut x = value;`
/// 
/// This function is a safety net - ideally the main processing should handle
/// all cases, but this ensures no bare mut slips through to the output.
fn fix_bare_mut_declaration(line: &str) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // Skip comments
    if trimmed.starts_with("//") || trimmed.starts_with("/*") {
        return line.to_string();
    }
    
    // Check for bare `mut x = value` pattern
    // Must start with "mut " and contain "=" but not "==" 
    // Must NOT already have "let mut" or "&mut"
    if trimmed.starts_with("mut ") && trimmed.contains('=') && !trimmed.contains("==") 
       && !line.contains("let mut") && !line.contains("&mut") {
        // Parse the declaration
        let rest = trimmed.strip_prefix("mut ").unwrap().trim();
        
        if let Some(eq_pos) = rest.find('=') {
            let var_part = rest[..eq_pos].trim();
            let val_part = rest[eq_pos + 1..].trim();
            
            // Handle type annotation if present
            let (var_name, type_annotation) = if var_part.contains(':') {
                let parts: Vec<&str> = var_part.splitn(2, ':').collect();
                if parts.len() == 2 {
                    (parts[0].trim(), format!(": {}", parts[1].trim()))
                } else {
                    (var_part, String::new())
                }
            } else {
                (var_part, String::new())
            };
            
            // Validate var_name is a valid identifier
            if !var_name.is_empty() && 
               var_name.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false) &&
               var_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                // Ensure semicolon at end
                let val_clean = val_part.trim_end_matches(';');
                return format!("{}let mut {}{} = {};", leading_ws, var_name, type_annotation, val_clean);
            }
        }
    }
    
    line.to_string()
}

//=============================================================================
// L-05 POST-PROCESSING: Strip effect annotations from output lines
// This catches any effect annotations that leaked through other paths.
//=============================================================================

/// Strip effect annotations from a line of output.
/// Effect annotations like `effects(...)` must not appear in Rust output.
fn strip_effects_from_line(line: &str) -> String {
    // Quick check - if no "effects(" present, return early
    if !line.contains("effects(") {
        return line.to_string();
    }
    
    // Check if it's a comment - don't modify comments
    let trimmed = line.trim();
    if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("*") {
        return line.to_string();
    }
    
    // Check if "effects(" is inside a string literal
    let mut in_string = false;
    let mut escape_next = false;
    let chars: Vec<char> = line.chars().collect();
    let mut effects_positions: Vec<usize> = Vec::new();
    
    for (i, &c) in chars.iter().enumerate() {
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
        
        // Look for "effects(" outside string
        if !in_string && i + 8 <= chars.len() {
            let slice: String = chars[i..i+8].iter().collect();
            if slice == "effects(" {
                effects_positions.push(i);
            }
        }
    }
    
    if effects_positions.is_empty() {
        return line.to_string();
    }
    
    // Strip all effect annotations found
    let mut result = line.to_string();
    for pos in effects_positions.iter().rev() {
        // Find the matching closing paren
        let start = *pos;
        let substring = &result[start..];
        
        let mut paren_depth = 0;
        let mut end = start;
        
        for (i, c) in substring.char_indices() {
            match c {
                '(' => paren_depth += 1,
                ')' => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        end = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        
        if end > start {
            // Remove the effects(...) clause and any trailing space
            let before = &result[..start];
            let after = &result[end..];
            let after_trimmed = after.trim_start();
            
            // Reconstruct with proper spacing
            if before.ends_with(' ') && !after_trimmed.is_empty() {
                result = format!("{}{}", before, after_trimmed);
            } else if !before.ends_with(' ') && !after_trimmed.is_empty() && !after_trimmed.starts_with('{') {
                result = format!("{} {}", before.trim_end(), after_trimmed);
            } else {
                result = format!("{}{}", before.trim_end(), if after_trimmed.is_empty() { "" } else { " " }.to_string() + after_trimmed);
            }
        }
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;

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
}