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
//! ## Key Design Decisions
//!
//! 1. Match arms are detected by: inside match + pattern followed by `{`
//! 2. Arm body closing `}` gets comma appended
//! 3. Nested matches are supported via depth tracking
//! 4. Guards (`if condition`) are passed through unchanged

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
    
    /// Enter an arm body
    /// L-02: uses_parens indicates if arm uses `(...)` instead of `{...}`
    pub fn enter_arm_body(&mut self, depth: usize, uses_parens: bool) {
        if let Some(entry) = self.stack.last_mut() {
            entry.in_arm_body = true;
            entry.arm_body_depth = depth;
            entry.arm_uses_parens = uses_parens;
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
pub fn transform_single_line_arm(line: &str, return_type: Option<&str>) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // Find first `{`
    let open_pos = match trimmed.find('{') {
        Some(pos) => pos,
        None => return line.to_string(),
    };
    
    // Find last `}`
    let close_pos = match trimmed.rfind('}') {
        Some(pos) => pos,
        None => return line.to_string(),
    };
    
    // Extract pattern (before `{`)
    let pattern = trimmed[..open_pos].trim();
    
    // Extract body (between `{` and `}`)
    let mut body = trimmed[open_pos + 1..close_pos].trim().to_string();
    
    // Transform string literal if return type is String
    if let Some(rt) = return_type {
        if rt == "String" && is_string_literal(&body) {
            body = transform_string_to_owned(&body);
        }
    }
    
    // Construct: `pattern => { body },`
    format!("{}{} => {{ {} }},", leading_ws, pattern, body)
}

/// Transform a match arm pattern line from RustS+ to Rust
/// Input:  `    Pattern {`
/// Output: `    Pattern => {`
/// Input:  `    Struct { x, y } {`
/// Output: `    Struct { x, y } => {`
pub fn transform_arm_pattern(line: &str) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
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
    
    // Add `=>` between pattern and `{`
    format!("{}{} => {{", leading_ws, pattern)
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

/// L-02: Transform match arm closing with parenthesis support
/// When uses_parens is true, close with `),` instead of `},`
/// Input:  `    }`
/// Output: `    ),` (if uses_parens) or `    },` (if not)
pub fn transform_arm_close_with_parens(line: &str, uses_parens: bool) -> String {
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    if trimmed == "}" {
        if uses_parens {
            // L-09: For if-expression arms, just add comma (no closing paren)
            // The arm body is already a valid expression
            format!("{},", leading_ws)
        } else {
            format!("{}}},", leading_ws)
        }
    } else {
        line.to_string()
    }
}

/// Check if a value is a string literal that needs conversion
pub fn is_string_literal(s: &str) -> bool {
    let trimmed = s.trim();
    trimmed.starts_with('"') && trimmed.ends_with('"') && !trimmed.contains("String::from")
}

/// Transform string literal to String::from for return contexts
pub fn transform_string_to_owned(value: &str) -> String {
    let trimmed = value.trim();
    if is_string_literal(trimmed) {
        let inner = &trimmed[1..trimmed.len()-1];
        format!("String::from(\"{}\")", inner)
    } else {
        value.to_string()
    }
}

/// Detect if line is an if/else expression that's part of an assignment
/// Pattern: `x = if cond {` or `x = if cond { value } else { value }`
pub fn is_if_assignment(line: &str) -> bool {
    let trimmed = line.trim();
    
    if !trimmed.contains('=') {
        return false;
    }
    
    // Find first `=` that's not `==`
    let chars: Vec<char> = trimmed.chars().collect();
    for i in 0..chars.len() {
        if chars[i] == '=' {
            // Check not `==`
            let prev = if i > 0 { chars.get(i - 1) } else { None };
            let next = chars.get(i + 1);
            
            if prev != Some(&'=') && prev != Some(&'!') && 
               prev != Some(&'<') && prev != Some(&'>') &&
               next != Some(&'=') && next != Some(&'>') {
                // Found assignment `=`
                let after_eq = &trimmed[i + 1..].trim();
                if after_eq.starts_with("if ") || after_eq.starts_with("match ") {
                    return true;
                }
                break;
            }
        }
    }
    
    false
}

/// Extract variable name and expression from if/match assignment
/// Input: `x = if cond {` or `result = match value {`
/// Output: Some(("x", "if cond {")) or Some(("result", "match value {"))
pub fn parse_control_flow_assignment(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    
    // Find assignment `=`
    let chars: Vec<char> = trimmed.chars().collect();
    for i in 0..chars.len() {
        if chars[i] == '=' {
            let prev = if i > 0 { chars.get(i - 1) } else { None };
            let next = chars.get(i + 1);
            
            if prev != Some(&'=') && prev != Some(&'!') && 
               prev != Some(&'<') && prev != Some(&'>') &&
               next != Some(&'=') && next != Some(&'>') {
                let var_part = trimmed[..i].trim();
                let expr_part = trimmed[i + 1..].trim();
                
                if expr_part.starts_with("if ") || expr_part.starts_with("match ") {
                    return Some((var_part.to_string(), expr_part.to_string()));
                }
                break;
            }
        }
    }
    
    None
}

/// Transform enum struct instantiation anywhere in a line
/// Input:  "println!(\"x\", eval(Event::C { x = 4 }))"
/// Output: "println!(\"x\", eval(Event::C { x: 4 }))"
/// 
/// This handles the pattern: `Path::Variant { field = value }` -> `Path::Variant { field: value }`
/// Works for nested cases and multiple occurrences.
pub fn transform_enum_struct_init(line: &str) -> String {
    // Quick check - if no `::` or no `{` or no `=`, nothing to transform
    if !line.contains("::") || !line.contains('{') || !line.contains('=') {
        return line.to_string();
    }
    
    let mut result = String::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    
    while i < chars.len() {
        // Look for pattern: `::Identifier {`
        if i + 1 < chars.len() && chars[i] == ':' && chars[i + 1] == ':' {
            result.push(':');
            result.push(':');
            i += 2;
            
            // Skip whitespace
            while i < chars.len() && chars[i].is_whitespace() {
                result.push(chars[i]);
                i += 1;
            }
            
            // Collect identifier (variant name)
            let ident_start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                result.push(chars[i]);
                i += 1;
            }
            
            if i == ident_start {
                continue; // No identifier found
            }
            
            // Skip whitespace
            while i < chars.len() && chars[i].is_whitespace() {
                result.push(chars[i]);
                i += 1;
            }
            
            // Check for `{` - this indicates struct variant init
            if i < chars.len() && chars[i] == '{' {
                result.push('{');
                i += 1;
                
                // Transform content inside braces: `field = value` -> `field: value`
                let mut brace_depth = 1;
                let mut in_string = false;
                
                while i < chars.len() && brace_depth > 0 {
                    let c = chars[i];
                    
                    // Track string literals
                    if c == '"' && (i == 0 || chars[i - 1] != '\\') {
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
                    
                    // Track brace depth
                    if c == '{' {
                        brace_depth += 1;
                        result.push(c);
                        i += 1;
                        continue;
                    }
                    
                    if c == '}' {
                        brace_depth -= 1;
                        result.push(c);
                        i += 1;
                        continue;
                    }
                    
                    // Transform `=` to `:` at depth 1 (direct field assignment)
                    // But NOT `==`, `!=`, `<=`, `>=`, `=>`
                    if c == '=' && brace_depth == 1 {
                        let prev = if i > 0 { Some(chars[i - 1]) } else { None };
                        let next = chars.get(i + 1).copied();
                        
                        let is_comparison = prev == Some('=') || prev == Some('!') || 
                                           prev == Some('<') || prev == Some('>') ||
                                           next == Some('=') || next == Some('>');
                        
                        if !is_comparison {
                            result.push(':');
                            i += 1;
                            continue;
                        }
                    }
                    
                    result.push(c);
                    i += 1;
                }
                continue;
            }
            continue;
        }
        
        result.push(chars[i]);
        i += 1;
    }
    
    result
}

//=============================================================================
// MATCH STRING DETECTION
// Detects when matching a variable against string literal patterns
// In this case, we need to add .as_str() to the match expression
//=============================================================================

/// Context for tracking if a match expression needs .as_str() transformation
#[derive(Debug, Clone)]
pub struct MatchStringContext {
    /// Is the match expr a simple identifier that might be String?
    pub match_expr_is_simple_var: bool,
    /// The match expression
    pub match_expr: String,
    /// Have we seen string literal patterns?
    pub has_string_patterns: bool,
    /// Transformed expression (with .as_str() if needed)
    pub transformed_expr: String,
}

impl MatchStringContext {
    pub fn new() -> Self {
        MatchStringContext {
            match_expr_is_simple_var: false,
            match_expr: String::new(),
            has_string_patterns: false,
            transformed_expr: String::new(),
        }
    }
    
    /// Parse match expression from line like "match status {" or "x = match expr {"
    pub fn from_match_line(line: &str) -> Self {
        let trimmed = line.trim();
        let mut ctx = MatchStringContext::new();
        
        // Extract the match expression
        let match_start = if let Some(pos) = trimmed.find("match ") {
            pos + 6 // After "match "
        } else {
            return ctx;
        };
        
        // Find the opening brace
        let brace_pos = match trimmed.rfind('{') {
            Some(pos) => pos,
            None => return ctx,
        };
        
        let expr = trimmed[match_start..brace_pos].trim();
        ctx.match_expr = expr.to_string();
        ctx.transformed_expr = expr.to_string();
        
        // Check if it's a simple variable (no method calls, no indexing, etc.)
        ctx.match_expr_is_simple_var = is_simple_identifier(expr);
        
        ctx
    }
    
    /// Check if a pattern is a string literal
    pub fn check_pattern(&mut self, pattern: &str) {
        let trimmed = pattern.trim();
        
        // Check if pattern starts with " (string literal)
        if trimmed.starts_with('"') && trimmed.contains('"') {
            self.has_string_patterns = true;
        }
    }
    
    /// Get the transformed match expression
    /// If matching on a simple var with string patterns, add .as_str()
    pub fn get_transformed_expr(&self) -> String {
        if self.match_expr_is_simple_var && self.has_string_patterns {
            format!("{}.as_str()", self.match_expr)
        } else {
            self.match_expr.clone()
        }
    }
    
    /// Should we transform this match expression?
    pub fn needs_as_str(&self) -> bool {
        self.match_expr_is_simple_var && self.has_string_patterns
    }
}

/// Check if a string is a simple identifier (no dots, brackets, parens)
fn is_simple_identifier(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return false;
    }
    
    // Must start with letter or underscore
    let first = trimmed.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    
    // Must only contain alphanumeric and underscores
    trimmed.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Transform a match line to add .as_str() if needed
/// Input: "match status {" with string patterns detected
/// Output: "match status.as_str() {"
pub fn transform_match_for_string_patterns(line: &str, needs_as_str: bool) -> String {
    if !needs_as_str {
        return line.to_string();
    }
    
    let trimmed = line.trim();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    
    // Handle assignment form: "x = match expr {"
    if trimmed.contains("= match ") {
        if let Some(eq_pos) = trimmed.find("= match ") {
            let var_part = &trimmed[..eq_pos + 2]; // "x = "
            let match_part = &trimmed[eq_pos + 2..]; // "match expr {"
            
            // Transform match part
            if let Some(match_start) = match_part.find("match ") {
                let after_match = &match_part[match_start + 6..];
                if let Some(brace_pos) = after_match.rfind('{') {
                    let expr = after_match[..brace_pos].trim();
                    return format!("{}{}match {}.as_str() {{", leading_ws, var_part, expr);
                }
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