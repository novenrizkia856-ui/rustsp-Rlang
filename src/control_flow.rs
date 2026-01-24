//! Control Flow System for RustS+
//!
//! Handles transformation of control flow expressions:
//! - `if` / `else if` / `else` as expressions (mostly passthrough to Rust)
//! - `match` as expression with RustS+ syntax transformation
//!
//! ## RustS+ Match Syntax
//!
//! RustS+ uses a cleaner match syntax without `=>` and commas:
//! ```text
//! match expr {
//!     Pattern {
//!         body
//!     }
//!     Pattern(x) {
//!         body
//!     }
//!     x if guard {
//!         body  
//!     }
//!     _ {
//!         default
//!     }
//! }
//! ```
//!
//! Transforms to Rust:
//! ```text
//! match expr {
//!     Pattern => {
//!         body
//!     },
//!     Pattern(x) => {
//!         body
//!     },
//!     x if guard => {
//!         body
//!     },
//!     _ => {
//!         default
//!     },
//! }
//! ```
//!
//! ## Multi-pattern Support (CRITICAL FIX)
//!
//! RustS+ supports multi-pattern match arms with `|`:
//! ```text
//! Pattern1 { field }
//! | Pattern2 { field }
//! | Pattern3 { field } { body }
//! ```
//!
//! The `|` lines are CONTINUATIONS of the previous pattern, NOT new arms!
//! Only the FINAL pattern (with `{ body }`) gets the `=>` transformation.
//!
//! ## Key Design Decisions
//!
//! 1. Match arms are detected by: inside match + pattern followed by `{`
//! 2. Multi-pattern continuations (starting with `|`) are passed through
//! 3. Arm body closing `}` gets comma appended
//! 4. Nested matches are supported via depth tracking
//! 5. Guards (`if condition`) are passed through unchanged

/// Stack-based context for tracking nested match expressions
#[derive(Debug, Clone)]
pub struct MatchModeStack {
    stack: Vec<MatchModeEntry>,
}

#[derive(Debug, Clone)]
struct MatchModeEntry {
    /// Brace depth when match started (after the opening `{`)
    match_depth: usize,
    /// Are we currently inside an arm body?
    in_arm_body: bool,
    /// Brace depth when arm body started
    arm_body_depth: usize,
    /// Is this match part of an assignment (needs ; at end)?
    is_assignment: bool,
    /// L-02: Does current arm use parentheses instead of braces?
    /// This is true when arm body is an if/else expression
    arm_uses_parens: bool,
    /// Are we in a multi-pattern sequence (after seeing first pattern, before body)?
    in_multi_pattern: bool,
}

impl MatchModeStack {
    pub fn new() -> Self {
        MatchModeStack { stack: Vec::new() }
    }
    
    /// Enter a new match expression
    pub fn enter_match(&mut self, depth: usize, is_assignment: bool) {
        self.stack.push(MatchModeEntry {
            match_depth: depth,
            in_arm_body: false,
            arm_body_depth: 0,
            is_assignment,
            arm_uses_parens: false,
            in_multi_pattern: false,
        });
    }
    
    /// Check if we're inside any match expression
    pub fn is_active(&self) -> bool {
        !self.stack.is_empty()
    }
    
    /// Check if we're inside a match but NOT in an arm body
    /// This is when we should look for arm patterns
    pub fn expecting_arm_pattern(&self) -> bool {
        if let Some(entry) = self.stack.last() {
            !entry.in_arm_body
        } else {
            false
        }
    }
    
    /// Check if we're inside an arm body
    pub fn in_arm_body(&self) -> bool {
        if let Some(entry) = self.stack.last() {
            entry.in_arm_body
        } else {
            false
        }
    }
    
    /// Check if we're in a multi-pattern sequence
    pub fn in_multi_pattern(&self) -> bool {
        if let Some(entry) = self.stack.last() {
            entry.in_multi_pattern
        } else {
            false
        }
    }
    
    /// Enter multi-pattern mode (first pattern of a `|` sequence seen)
    pub fn enter_multi_pattern(&mut self) {
        if let Some(entry) = self.stack.last_mut() {
            entry.in_multi_pattern = true;
        }
    }
    
    /// Exit multi-pattern mode (arm body started)
    pub fn exit_multi_pattern(&mut self) {
        if let Some(entry) = self.stack.last_mut() {
            entry.in_multi_pattern = false;
        }
    }
    
    /// Enter an arm body
    /// L-02: uses_parens indicates if arm uses `(...)` instead of `{...}`
    pub fn enter_arm_body(&mut self, depth: usize, uses_parens: bool) {
        if let Some(entry) = self.stack.last_mut() {
            entry.in_arm_body = true;
            entry.arm_body_depth = depth;
            entry.arm_uses_parens = uses_parens;
            entry.in_multi_pattern = false; // Reset multi-pattern when entering body
        }
    }
    
    /// Check if closing brace exits arm body
    /// CRITICAL: Use `<` not `<=` because nested blocks (if/else/while/for) 
    /// close back to arm_body_depth. We only exit arm when depth goes BELOW arm_body_depth.
    pub fn should_exit_arm(&self, current_depth: usize) -> bool {
        if let Some(entry) = self.stack.last() {
            entry.in_arm_body && current_depth < entry.arm_body_depth
        } else {
            false
        }
    }
    
    /// Exit arm body
    pub fn exit_arm_body(&mut self) {
        if let Some(entry) = self.stack.last_mut() {
            entry.in_arm_body = false;
        }
    }
    
    /// L-02: Check if current arm uses parentheses instead of braces
    pub fn arm_uses_parens(&self) -> bool {
        self.stack.last().map(|e| e.arm_uses_parens).unwrap_or(false)
    }
    
    /// Check if closing brace exits the match entirely
    pub fn should_exit_match(&self, current_depth: usize) -> bool {
        if let Some(entry) = self.stack.last() {
            !entry.in_arm_body && current_depth <= entry.match_depth
        } else {
            false
        }
    }
    
    /// Check if current match is an assignment (needs semicolon at end)
    pub fn current_is_assignment(&self) -> bool {
        self.stack.last().map(|e| e.is_assignment).unwrap_or(false)
    }
    
    /// Exit the current match
    pub fn exit_match(&mut self) {
        self.stack.pop();
    }
    
    /// Get current match depth for debugging
    #[allow(dead_code)]
    pub fn current_match_depth(&self) -> Option<usize> {
        self.stack.last().map(|e| e.match_depth)
    }
}

/// Detect if a line starts a match expression
/// Patterns: `match expr {` or `var = match expr {`
pub fn is_match_start(line: &str) -> bool {
    let trimmed = line.trim();
    
    // Direct: `match expr {`
    if trimmed.starts_with("match ") && trimmed.ends_with('{') {
        return true;
    }
    
    // Assignment: `x = match expr {`
    if trimmed.contains("= match ") && trimmed.ends_with('{') {
        return true;
    }
    
    false
}

//=============================================================================
// MULTI-PATTERN DETECTION (CRITICAL FIX)
//=============================================================================

/// Check if line is a multi-pattern continuation (starts with `|`)
/// These lines are CONTINUATIONS of the previous pattern, NOT new arms!
///
/// Examples:
/// - `| Pattern2 { field }` - continuation without body (pass-through)
/// - `| Pattern2 { field } { body }` - final pattern with body (needs transform)
pub fn is_multi_pattern_continuation(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') || trimmed.starts_with("| ")
}

/// Check if line is a multi-pattern FINAL (has body)
/// Input: `| Pattern { field } { body }`
/// This is the LAST pattern in a multi-pattern sequence and has the arm body.
pub fn is_multi_pattern_final(line: &str) -> bool {
    let trimmed = line.trim();
    
    // Must start with `|`
    if !trimmed.starts_with('|') {
        return false;
    }
    
    // Must end with `}`
    if !trimmed.ends_with('}') {
        return false;
    }
    
    // Count braces - needs at least 2 `{` for pattern + body
    // Pattern: `| EnumVariant { field } { body }`
    //           ^-- 1st brace (destruct) ^-- 2nd brace (body)
    // Or: `| SimpleVariant { body }` 
    //                       ^-- only 1 brace (body), no destruct
    
    // Find brace pairs to determine if there's a body
    let brace_count = trimmed.matches('{').count();
    let close_count = trimmed.matches('}').count();
    
    // If balanced braces and at least one pair, check if it's body
    if brace_count >= 1 && brace_count == close_count {
        // Find last `{` and last `}`
        if let (Some(last_open), Some(last_close)) = (trimmed.rfind('{'), trimmed.rfind('}')) {
            if last_close > last_open {
                // There's content between last { and last }
                let body = &trimmed[last_open + 1..last_close];
                if !body.trim().is_empty() {
                    return true;
                }
            }
        }
    }
    
    false
}

/// Check if a first pattern line will be followed by multi-pattern continuation
/// This looks ahead to see if next pattern starts with `|`
/// 
/// If true, the first pattern should NOT get `=> {` - wait for the final pattern
pub fn first_pattern_has_continuation(line: &str) -> bool {
    let trimmed = line.trim();
    
    // First pattern ends with `}` (struct destruct) but no body follows on same line
    // Pattern: `EnumVariant { field, .. }` where next line starts with `|`
    
    // Check if line ends with `}` but doesn't have a second `{ ... }` body
    if trimmed.ends_with('}') {
        let brace_count = trimmed.matches('{').count();
        let close_count = trimmed.matches('}').count();
        
        // If exactly 1 open and 1 close, it's just destructuring, no body
        // E.g., `TxPayload::Transfer { gas_limit, fee, nonce, .. }`
        if brace_count == 1 && close_count == 1 {
            return true;
        }
    }
    
    // Check if line ends with `{` - multi-line arm start
    // This case the first pattern is just the pattern, body will come later
    false
}

/// Transform a multi-pattern continuation line
/// 
/// For continuation WITHOUT body: pass-through
/// Input:  `    | Pattern2 { field }`  
/// Output: `    | Pattern2 { field }`
///
/// For continuation WITH body (final): transform
/// Input:  `    | Pattern3 { field } { body }`
/// Output: `    | Pattern3 { field } => { body },`
pub fn transform_multi_pattern_line(line: &str, return_type: Option<&str>) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    if is_multi_pattern_final(trimmed) {
        // This is the final pattern with body - transform it
        // Use find_body_braces for robust body detection
        if let Some((body_open, body_close)) = find_body_braces(trimmed) {
            let pattern = trimmed[..body_open].trim();
            let body = trimmed[body_open + 1..body_close].trim();
            
            // Transform string literal if return type is String
            let transformed_body = if let Some(rt) = return_type {
                if rt == "String" && is_string_literal(body) {
                    transform_string_to_owned(body)
                } else {
                    body.to_string()
                }
            } else {
                body.to_string()
            };
            
            return format!("{}{} => {{ {} }},", leading_ws, pattern, transformed_body);
        }
    }
    
    // Continuation without body - pass-through
    line.to_string()
}

//=============================================================================
// ORIGINAL MATCH ARM DETECTION (UPDATED)
//=============================================================================

/// Detect if a line is a match arm pattern (when inside match, not in arm body)
/// 
/// Valid patterns:
/// - `Pattern {` - simple pattern
/// - `Pattern(x) {` - tuple pattern
/// - `Pattern { x, y } {` - struct destructure (rare but valid)
/// - `x if x > 0 {` - pattern with guard
/// - `_ {` - wildcard
/// - `0 {` - literal pattern
/// - `"string" {` - string literal pattern
///
/// NOT valid (should NOT match):
/// - `| Pattern {` - multi-pattern continuation (handled separately!)
/// - `if condition {` - if expression
/// - `else {` - else branch
/// - `match expr {` - nested match
/// - `while cond {` - loop
/// - `for x in iter {` - loop
/// - `fn name() {` - function
/// - `struct Name {` - definition
/// - `enum Name {` - definition
/// - `impl Trait {` - impl block
/// - `{` - bare block
pub fn is_match_arm_pattern(line: &str) -> bool {
    let trimmed = line.trim();
    
    // CRITICAL FIX: Multi-pattern continuations are NOT arm patterns!
    // They are handled by is_multi_pattern_continuation()
    if trimmed.starts_with('|') {
        return false;
    }
    
    // Must contain `{`
    if !trimmed.contains('{') {
        return false;
    }
    
    // Must end with `{` (for multi-line arms) OR contain `{}` pair (for single-line)
    if !trimmed.ends_with('{') && !trimmed.ends_with('}') {
        return false;
    }
    
    // Exclude control flow and definitions
    let excluded_starts = [
        "if ", "else", "while ", "for ", "loop", "match ",
        "fn ", "pub fn ", "struct ", "pub struct ",
        "enum ", "pub enum ", "impl ", "trait ", "mod ",
        "unsafe ", "async ", "const ", "static ", "type ",
    ];
    
    for prefix in &excluded_starts {
        if trimmed.starts_with(prefix) {
            return false;
        }
    }
    
    // Exclude bare brace
    if trimmed == "{" {
        return false;
    }
    
    // For multi-line arms (ends with `{`), find the LAST `{`
    // For single-line arms (ends with `}`), find the first `{`
    let brace_pos = if trimmed.ends_with('{') {
        trimmed.rfind('{')
    } else {
        trimmed.find('{')
    };
    
    let brace_pos = match brace_pos {
        Some(pos) => pos,
        None => return false,
    };
    
    // Get content before the brace
    let before_brace = trimmed[..brace_pos].trim();
    
    // Must have something before `{`
    if before_brace.is_empty() {
        return false;
    }
    
    // This looks like an arm pattern
    true
}

/// Check if line is a single-line match arm: `pattern { body }`
pub fn is_single_line_arm(line: &str) -> bool {
    let trimmed = line.trim();
    
    // CRITICAL FIX: Multi-pattern lines are handled separately
    if trimmed.starts_with('|') {
        return is_multi_pattern_final(trimmed);
    }
    
    // Must contain both `{` and `}`
    if !trimmed.contains('{') || !trimmed.contains('}') {
        return false;
    }
    
    // Must end with `}`
    if !trimmed.ends_with('}') {
        return false;
    }
    
    // Find first `{` and last `}`
    let open_pos = match trimmed.find('{') {
        Some(pos) => pos,
        None => return false,
    };
    
    // Content before `{` must exist (the pattern)
    let before_brace = trimmed[..open_pos].trim();
    if before_brace.is_empty() {
        return false;
    }
    
    // Content between `{` and `}` must exist (the body)
    let close_pos = trimmed.rfind('}').unwrap();
    if close_pos <= open_pos + 1 {
        return false; // No content between braces
    }
    
    true
}

/// Transform a single-line match arm from RustS+ to Rust
/// Input:  `    0 { "zero" }`
/// Output: `    0 => { "zero" },`
/// Input:  `    x if x > 0 { "positive" }`
/// Output: `    x if x > 0 => { "positive" },`
/// Input:  `    Enum::Variant { field, .. } { body_expr }`
/// Output: `    Enum::Variant { field, .. } => { body_expr },`
pub fn transform_single_line_arm(line: &str, return_type: Option<&str>) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // CRITICAL FIX: Handle multi-pattern final
    if trimmed.starts_with('|') {
        return transform_multi_pattern_line(line, return_type);
    }
    
    // CRITICAL FIX: Find the BODY braces (last balanced `{ }` pair)
    // For `Pattern { destruct } { body }`, we want:
    //   pattern = "Pattern { destruct }"
    //   body = "body"
    
    // Find body boundaries by scanning from the end
    let (body_open, body_close) = match find_body_braces(trimmed) {
        Some(positions) => positions,
        None => return line.to_string(),
    };
    
    // Extract pattern (everything before body opening brace)
    let pattern = trimmed[..body_open].trim();
    
    // Extract body (between body braces)
    let mut body = trimmed[body_open + 1..body_close].trim().to_string();
    
    // Transform string literal if return type is String
    if let Some(rt) = return_type {
        if rt == "String" && is_string_literal(&body) {
            body = transform_string_to_owned(&body);
        }
    }
    
    // Construct: `pattern => { body },`
    format!("{}{} => {{ {} }},", leading_ws, pattern, body)
}

/// Find the body braces in a single-line match arm
/// Returns (open_pos, close_pos) for the LAST balanced `{ }` pair (byte positions)
/// 
/// For `Pattern { body }` returns positions of the single `{ }`
/// For `Enum { field } { body }` returns positions of the LAST `{ }` pair (body)
fn find_body_braces(line: &str) -> Option<(usize, usize)> {
    if line.is_empty() {
        return None;
    }
    
    let bytes = line.as_bytes();
    
    // Find the last `}`
    let close_pos = line.rfind('}')?;
    
    // Scan backwards from close_pos to find matching `{`
    // Track brace depth, ignoring braces inside string literals
    let mut depth = 0;
    let mut in_string = false;
    
    for i in (0..=close_pos).rev() {
        let c = bytes[i];
        
        // Handle string literals
        if c == b'"' {
            // Check if this quote is escaped by counting preceding backslashes
            let mut backslash_count = 0;
            let mut j = i;
            while j > 0 && bytes[j - 1] == b'\\' {
                backslash_count += 1;
                j -= 1;
            }
            // Quote is escaped if preceded by odd number of backslashes
            if backslash_count % 2 == 0 {
                in_string = !in_string;
            }
        }
        
        if !in_string {
            if c == b'}' {
                depth += 1;
            } else if c == b'{' {
                depth -= 1;
                if depth == 0 {
                    // Found the matching `{` for our `}`
                    return Some((i, close_pos));
                }
            }
        }
    }
    
    None
}

/// Transform a match arm pattern line from RustS+ to Rust
/// Input:  `    Pattern {`
/// Output: `    Pattern => {`
/// Input:  `    Struct { x, y } {`
/// Output: `    Struct { x, y } => {`
pub fn transform_arm_pattern(line: &str) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // CRITICAL FIX: Multi-pattern continuations without body pass through
    if trimmed.starts_with('|') {
        // If this is a continuation without body, pass through
        if !is_multi_pattern_final(trimmed) {
            return line.to_string();
        }
        // If it's the final (with body), let transform_multi_pattern_line handle it
        return transform_multi_pattern_line(line, None);
    }
    
    // Find the LAST `{` which should be the body start
    // For `Pattern {` - last `{` is at end
    // For `Struct { x, y } {` - last `{` is at end (body start, not struct destruct)
    let brace_pos = match trimmed.rfind('{') {
        Some(pos) => pos,
        None => return line.to_string(),
    };
    
    // Extract pattern (everything before the last `{`)
    let pattern = trimmed[..brace_pos].trim();
    
    if pattern.is_empty() {
        return line.to_string();
    }
    
    // CRITICAL FIX: Add `ref` to String-like field bindings ONLY
    // This is a heuristic - add ref to fields that are likely String type
    // based on common naming patterns (reason, message, name, error, etc.)
    let pattern = add_ref_to_string_fields(pattern);
    
    // Add `=>` between pattern and `{`
    format!("{}{} => {{", leading_ws, pattern)
}

/// Add `ref` to pattern bindings that are likely String types
/// This uses heuristics based on common field names for strings
fn add_ref_to_string_fields(pattern: &str) -> String {
    // Check if pattern has struct destructuring
    if !pattern.contains('{') || !pattern.contains('}') {
        return pattern.to_string();
    }
    
    // Common String field names that should get `ref`
    let string_field_names = [
        "reason", "message", "error", "name", "description", "text",
        "content", "body", "title", "label", "value", "data", "info",
        "msg", "err", "str", "string", "s"
    ];
    
    // Find the struct part: everything between first `{` and last `}` in pattern
    if let Some(open_brace) = pattern.find('{') {
        if let Some(close_brace) = pattern.rfind('}') {
            if close_brace > open_brace {
                let before = &pattern[..open_brace + 1];
                let fields_str = &pattern[open_brace + 1..close_brace];
                let after = &pattern[close_brace..];
                
                // Split fields and selectively add ref
                let fields: Vec<&str> = fields_str.split(',').collect();
                let new_fields: Vec<String> = fields.iter()
                    .map(|f| {
                        let f = f.trim();
                        if f.is_empty() {
                            String::new()
                        } else if f.starts_with("ref ") || f.starts_with("mut ") || f.contains(':') {
                            // Already has ref/mut or is a rename pattern (x: y)
                            f.to_string()
                        } else {
                            // Check if field name suggests it's a String
                            let field_lower = f.to_lowercase();
                            if string_field_names.iter().any(|&s| field_lower == s || field_lower.ends_with(s)) {
                                format!("ref {}", f)
                            } else {
                                f.to_string()
                            }
                        }
                    })
                    .filter(|s| !s.is_empty())
                    .collect();
                
                return format!("{} {} {}", before, new_fields.join(", "), after);
            }
        }
    }
    
    pattern.to_string()
}

/// Add `ref` to struct pattern bindings - DISABLED
/// This function is kept for reference but no longer used because
/// automatic ref addition causes type mismatches for Copy types.
#[allow(dead_code)]
fn add_ref_to_struct_pattern(pattern: &str) -> String {
    pattern.to_string()
}

/// Transform match arm closing brace - add comma
/// Input:  `    }`
/// Output: `    },`
pub fn transform_arm_close(line: &str) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    if trimmed == "}" {
        format!("{}}},", leading_ws)
    } else {
        line.to_string()
    }
}

/// L-02: Transform arm close with parentheses for if/else arm bodies
/// Input:  `    }`
/// Output: `    }),` if uses_parens is true
/// Output: `    },` if uses_parens is false
pub fn transform_arm_close_with_parens(line: &str, uses_parens: bool) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    if trimmed == "}" {
        if uses_parens {
            format!("{}}}),", leading_ws)
        } else {
            format!("{}}},", leading_ws)
        }
    } else {
        line.to_string()
    }
}

//=============================================================================
// IF ASSIGNMENT DETECTION
//=============================================================================

/// Check if line is an if/match assignment
/// Pattern: `var = if/match ...`
pub fn is_if_assignment(line: &str) -> bool {
    let trimmed = line.trim();
    
    // Must contain `= if` or `= match` (not `==`)
    if trimmed.contains("= if ") && !trimmed.contains("== if") {
        return trimmed.ends_with('{');
    }
    
    if trimmed.contains("= match ") && !trimmed.contains("== match") {
        return trimmed.ends_with('{');
    }
    
    false
}

/// Parse control flow assignment
/// Input: `x = if cond {` -> ("x", "if cond {")
/// Input: `x = match val {` -> ("x", "match val {")
pub fn parse_control_flow_assignment(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    
    // Look for `= if` or `= match` but NOT `== if/match`
    let control_pos = if let Some(pos) = trimmed.find("= if ") {
        if pos > 0 && trimmed.chars().nth(pos - 1) == Some('=') {
            return None; // This is `==`
        }
        Some(pos)
    } else if let Some(pos) = trimmed.find("= match ") {
        if pos > 0 && trimmed.chars().nth(pos - 1) == Some('=') {
            return None; // This is `==`
        }
        Some(pos)
    } else {
        None
    };
    
    let pos = control_pos?;
    
    let var_part = trimmed[..pos].trim();
    let expr_part = trimmed[pos + 2..].trim(); // Skip `= `
    
    // Handle `let var = ...` and `let mut var = ...`
    let var_name = if var_part.starts_with("let mut ") {
        var_part.strip_prefix("let mut ")?.trim()
    } else if var_part.starts_with("let ") {
        var_part.strip_prefix("let ")?.trim()
    } else if var_part.starts_with("mut ") {
        var_part.strip_prefix("mut ")?.trim()
    } else {
        var_part
    };
    
    if var_name.is_empty() || expr_part.is_empty() {
        return None;
    }
    
    Some((var_name.to_string(), expr_part.to_string()))
}

//=============================================================================
// ENUM STRUCT INITIALIZATION TRANSFORM
//=============================================================================

/// Transform enum struct initialization from RustS+ to Rust
/// Input:  `ev = Event::Data { id = 1, msg = "hello" }`
/// Output: `ev = Event::Data { id: 1, msg: "hello" }`
pub fn transform_enum_struct_init(line: &str) -> String {
    let trimmed = line.trim();
    
    // Must have `::` and `{` for enum struct variant
    if !trimmed.contains("::") || !trimmed.contains('{') {
        return line.to_string();
    }
    
    // Find the brace positions
    let brace_start = match trimmed.find('{') {
        Some(pos) => pos,
        None => return line.to_string(),
    };
    
    let brace_end = match trimmed.rfind('}') {
        Some(pos) => pos,
        None => return line.to_string(),
    };
    
    if brace_end <= brace_start {
        return line.to_string();
    }
    
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    let before_brace = &trimmed[..brace_start + 1];
    let inside_braces = &trimmed[brace_start + 1..brace_end];
    let after_brace = &trimmed[brace_end..];
    
    // Transform field assignments inside braces: `name = value` -> `name: value`
    let transformed_inside = transform_field_assignments(inside_braces);
    
    format!("{}{}{}{}", leading_ws, before_brace, transformed_inside, after_brace)
}

/// Transform field assignments: `x = 1, y = 2` -> `x: 1, y: 2`
fn transform_field_assignments(fields: &str) -> String {
    let mut result = String::new();
    let mut in_string = false;
    let mut escape_next = false;
    let mut depth: i32 = 0; // Track nested braces/parens
    
    let chars: Vec<char> = fields.chars().collect();
    let len = chars.len();
    let mut i = 0;
    
    while i < len {
        let c = chars[i];
        
        if escape_next {
            result.push(c);
            escape_next = false;
            i += 1;
            continue;
        }
        
        if c == '\\' && in_string {
            result.push(c);
            escape_next = true;
            i += 1;
            continue;
        }
        
        if c == '"' {
            in_string = !in_string;
            result.push(c);
            i += 1;
            continue;
        }
        
        if in_string {
            result.push(c);
            i += 1;
            continue;
        }
        
        // Track depth
        if c == '{' || c == '(' || c == '[' {
            depth += 1;
            result.push(c);
            i += 1;
            continue;
        }
        
        if c == '}' || c == ')' || c == ']' {
            depth = depth.saturating_sub(1);
            result.push(c);
            i += 1;
            continue;
        }
        
        // Only transform `=` to `:` at top level (depth 0)
        if c == '=' && depth == 0 {
            // Check it's not `==`, `!=`, `<=`, `>=`, `=>`, etc.
            let prev = if i > 0 { chars[i - 1] } else { ' ' };
            let next = if i + 1 < len { chars[i + 1] } else { ' ' };
            
            if prev != '=' && prev != '!' && prev != '<' && prev != '>' && next != '=' && next != '>' {
                result.push(':');
                i += 1;
                continue;
            }
        }
        
        result.push(c);
        i += 1;
    }
    
    result
}

//=============================================================================
// STRING LITERAL HELPERS
//=============================================================================

/// Check if value is a string literal (starts and ends with ")
pub fn is_string_literal(value: &str) -> bool {
    let v = value.trim();
    v.starts_with('"') && v.ends_with('"') && v.len() >= 2
}

/// Transform string literal to String::from()
pub fn transform_string_to_owned(value: &str) -> String {
    let v = value.trim();
    if v.starts_with('"') && v.ends_with('"') {
        format!("String::from({})", v)
    } else {
        value.to_string()
    }
}

//=============================================================================
// STRING MATCHING SUPPORT
//=============================================================================

/// Context for tracking if match needs .as_str() for string patterns
#[derive(Debug, Clone)]
pub struct MatchStringContext {
    /// The match expression (what's being matched on)
    pub match_expr: String,
    /// Does this match have string literal patterns?
    pub has_string_patterns: bool,
}

impl MatchStringContext {
    pub fn new() -> Self {
        MatchStringContext {
            match_expr: String::new(),
            has_string_patterns: false,
        }
    }
    
    pub fn from_match_line(line: &str) -> Self {
        let trimmed = line.trim();
        let match_expr = if let Some(pos) = trimmed.find("match ") {
            if let Some(brace_pos) = trimmed.rfind('{') {
                trimmed[pos + 6..brace_pos].trim().to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        
        MatchStringContext {
            match_expr,
            has_string_patterns: false,
        }
    }
    
    /// Check if we need to add .as_str() to the match expression
    pub fn needs_as_str(&self) -> bool {
        self.has_string_patterns && !self.match_expr.is_empty()
    }
}

/// Transform match expression for string patterns
/// Adds `.as_str()` to the match expression if needed
pub fn transform_match_for_string_patterns(line: &str, needs_as_str: bool) -> String {
    if !needs_as_str {
        return line.to_string();
    }
    
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // Handle assignment form: "var = match expr {"
    if trimmed.contains("= match ") {
        if let Some(eq_pos) = trimmed.find("= match ") {
            let var_part = &trimmed[..eq_pos + 2]; // Include "= "
            let after_match = &trimmed[eq_pos + 2..]; // "match expr {"
            
            if let Some(brace_pos) = after_match.rfind('{') {
                let expr = after_match[6..brace_pos].trim(); // Skip "match "
                return format!("{}{}match {}.as_str() {{", leading_ws, var_part, expr);
            }
        }
    }
    
    // Handle direct form: "match expr {"
    if trimmed.starts_with("match ") {
        if let Some(brace_pos) = trimmed.rfind('{') {
            let expr = trimmed[6..brace_pos].trim();
            return format!("{}match {}.as_str() {{", leading_ws, expr);
        }
    }
    
    line.to_string()
}

/// Detect if a match arm pattern line contains a string literal pattern
pub fn pattern_is_string_literal(line: &str) -> bool {
    let trimmed = line.trim();
    
    // Must end with { (multi-line) or } (single-line)
    if !trimmed.ends_with('{') && !trimmed.ends_with('}') {
        return false;
    }
    
    // Find the pattern part (before the last {)
    let brace_pos = if trimmed.ends_with('{') {
        trimmed.rfind('{')
    } else {
        trimmed.find('{')
    };
    
    let brace_pos = match brace_pos {
        Some(pos) => pos,
        None => return false,
    };
    
    let pattern = trimmed[..brace_pos].trim();
    
    // Check if pattern is a string literal (starts and ends with ")
    pattern.starts_with('"') && pattern.ends_with('"')
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_match_mode_stack() {
        let mut stack = MatchModeStack::new();
        assert!(!stack.is_active());
        
        // Enter match at depth 2 (not an assignment)
        stack.enter_match(2, false);
        assert!(stack.is_active());
        assert!(stack.expecting_arm_pattern());
        assert!(!stack.in_arm_body());
        
        // Check is_assignment
        assert!(!stack.current_is_assignment());
        
        // Enter arm body at depth 3
        stack.enter_arm_body(3, false);
        assert!(stack.in_arm_body());
        assert!(!stack.expecting_arm_pattern());
        
        // Check exit conditions - uses < not <=
        // Nested blocks (if/else) close back to arm_body_depth
        // We only exit when depth goes BELOW arm_body_depth
        assert!(!stack.should_exit_arm(4)); // Still inside nested block
        assert!(!stack.should_exit_arm(3)); // At arm depth - DON'T exit (nested if/else ends here)
        assert!(stack.should_exit_arm(2));  // Below arm depth - EXIT
        
        // Exit arm
        stack.exit_arm_body();
        assert!(!stack.in_arm_body());
        assert!(stack.expecting_arm_pattern());
        
        // Check match exit
        assert!(stack.should_exit_match(2));
        assert!(stack.should_exit_match(1));
        
        stack.exit_match();
        assert!(!stack.is_active());
    }
    
    #[test]
    fn test_match_assignment_tracking() {
        let mut stack = MatchModeStack::new();
        
        // Non-assignment match
        stack.enter_match(1, false);
        assert!(!stack.current_is_assignment());
        stack.exit_match();
        
        // Assignment match
        stack.enter_match(1, true);
        assert!(stack.current_is_assignment());
        stack.exit_match();
    }
    
    #[test]
    fn test_nested_match() {
        let mut stack = MatchModeStack::new();
        
        // Outer match (assignment)
        stack.enter_match(1, true);
        stack.enter_arm_body(2, false);
        
        // Inner match (not assignment - bare expression)
        stack.enter_match(3, false);
        assert_eq!(stack.current_match_depth(), Some(3));
        assert!(!stack.current_is_assignment());
        
        // Exit inner
        stack.exit_match();
        assert_eq!(stack.current_match_depth(), Some(1));
        assert!(stack.current_is_assignment());
        
        // Exit outer
        stack.exit_arm_body();
        stack.exit_match();
        assert!(!stack.is_active());
    }
    
    #[test]
    fn test_is_match_start() {
        assert!(is_match_start("match x {"));
        assert!(is_match_start("    match expr {"));
        assert!(is_match_start("result = match x {"));
        assert!(is_match_start("    let y = match x {"));
        assert!(!is_match_start("match"));
        assert!(!is_match_start("if x {"));
        assert!(!is_match_start("matching {"));
    }
    
    #[test]
    fn test_is_match_arm_pattern() {
        // Valid arm patterns
        assert!(is_match_arm_pattern("    Some(x) {"));
        assert!(is_match_arm_pattern("    None {"));
        assert!(is_match_arm_pattern("    Event::Ping {"));
        assert!(is_match_arm_pattern("    x if x > 0 {"));
        assert!(is_match_arm_pattern("    _ {"));
        assert!(is_match_arm_pattern("    0 {"));
        assert!(is_match_arm_pattern("    \"hello\" {"));
        assert!(is_match_arm_pattern("    (a, b) {"));
        assert!(is_match_arm_pattern("    Point { x, y } {"));
        
        // CRITICAL: Multi-pattern continuations are NOT arm patterns!
        assert!(!is_match_arm_pattern("    | Pattern2 {"));
        assert!(!is_match_arm_pattern("    | TxPayload::Stake { gas_limit, .. }"));
        assert!(!is_match_arm_pattern("| Some(x) {"));
        
        // Invalid - control flow
        assert!(!is_match_arm_pattern("    if x {"));
        assert!(!is_match_arm_pattern("    else {"));
        assert!(!is_match_arm_pattern("    while x {"));
        assert!(!is_match_arm_pattern("    for x in y {"));
        assert!(!is_match_arm_pattern("    loop {"));
        assert!(!is_match_arm_pattern("    match x {"));
        
        // Invalid - definitions
        assert!(!is_match_arm_pattern("    fn foo() {"));
        assert!(!is_match_arm_pattern("    struct Foo {"));
        assert!(!is_match_arm_pattern("    enum Foo {"));
        assert!(!is_match_arm_pattern("    impl Foo {"));
        
        // Invalid - bare brace
        assert!(!is_match_arm_pattern("    {"));
        assert!(!is_match_arm_pattern("{"));
    }
    
    #[test]
    fn test_multi_pattern_detection() {
        // Continuation detection
        assert!(is_multi_pattern_continuation("| Pattern2 { field }"));
        assert!(is_multi_pattern_continuation("    | TxPayload::Stake { gas_limit, .. }"));
        assert!(is_multi_pattern_continuation("| Some(x)"));
        assert!(!is_multi_pattern_continuation("Pattern1 { field }"));
        assert!(!is_multi_pattern_continuation("Some(x) {"));
        
        // Final (with body) detection
        assert!(is_multi_pattern_final("| Pattern { field } { body }"));
        assert!(is_multi_pattern_final("    | TxPayload::Custom { gas_limit, .. } { (*gas_limit, *fee) }"));
        assert!(!is_multi_pattern_final("| Pattern { field }"));
        assert!(!is_multi_pattern_final("| TxPayload::Stake { gas_limit, .. }"));
    }
    
    #[test]
    fn test_transform_multi_pattern() {
        // Continuation without body - pass through
        assert_eq!(
            transform_multi_pattern_line("    | Pattern2 { field }", None),
            "    | Pattern2 { field }"
        );
        
        // Final with body - transform
        assert_eq!(
            transform_multi_pattern_line("    | Pattern { x } { x * 2 }", None),
            "    | Pattern { x } => { x * 2 },"
        );
    }
    
    #[test]
    fn test_transform_arm_pattern() {
        assert_eq!(
            transform_arm_pattern("    Pattern {"),
            "    Pattern => {"
        );
        assert_eq!(
            transform_arm_pattern("    Some(x) {"),
            "    Some(x) => {"
        );
        assert_eq!(
            transform_arm_pattern("    x if x > 0 {"),
            "    x if x > 0 => {"
        );
        assert_eq!(
            transform_arm_pattern("    _ {"),
            "    _ => {"
        );
        assert_eq!(
            transform_arm_pattern("        Event::Data(d) {"),
            "        Event::Data(d) => {"
        );
        
        // Multi-pattern continuation - pass through
        assert_eq!(
            transform_arm_pattern("    | TxPayload::Stake { gas_limit, .. }"),
            "    | TxPayload::Stake { gas_limit, .. }"
        );
    }
    
    #[test]
    fn test_transform_arm_close() {
        assert_eq!(transform_arm_close("    }"), "    },");
        assert_eq!(transform_arm_close("}"), "},");
        assert_eq!(transform_arm_close("    };"), "    };"); // Already has semicolon
    }
    
    #[test]
    fn test_is_if_assignment() {
        assert!(is_if_assignment("x = if a > b {"));
        assert!(is_if_assignment("result = match x {"));
        assert!(is_if_assignment("    y = if cond {"));
        assert!(!is_if_assignment("if x == y {"));
        assert!(!is_if_assignment("if x {"));
        assert!(!is_if_assignment("x == if")); // Invalid
    }
    
    #[test]
    fn test_parse_control_flow_assignment() {
        assert_eq!(
            parse_control_flow_assignment("x = if a > b {"),
            Some(("x".to_string(), "if a > b {".to_string()))
        );
        assert_eq!(
            parse_control_flow_assignment("result = match n {"),
            Some(("result".to_string(), "match n {".to_string()))
        );
        assert_eq!(
            parse_control_flow_assignment("if x == y {"),
            None
        );
    }
    
    #[test]
    fn test_string_literal_detection() {
        assert!(is_string_literal("\"hello\""));
        assert!(is_string_literal("  \"test\"  "));
        assert!(!is_string_literal("String::from(\"x\")"));
        assert!(!is_string_literal("42"));
        assert!(!is_string_literal("variable"));
    }
    
    #[test]
    fn test_string_transform() {
        assert_eq!(
            transform_string_to_owned("\"hello\""),
            "String::from(\"hello\")"
        );
        assert_eq!(
            transform_string_to_owned("42"),
            "42"
        );
        assert_eq!(
            transform_string_to_owned("String::from(\"x\")"),
            "String::from(\"x\")"
        );
    }
}