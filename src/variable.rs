use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    pub var_type: Option<String>,
    pub is_mutable: bool,
    pub assigned_count: usize,
    pub is_borrow: bool,
    pub is_borrow_mut: bool,
    pub first_line: usize,
}

impl Variable {
    pub fn new(name: String) -> Self {
        Variable {
            name,
            var_type: None,
            is_mutable: false,
            assigned_count: 0,
            is_borrow: false,
            is_borrow_mut: false,
            first_line: 0,
        }
    }
}

#[derive(Debug)]
pub struct VariableTracker {
    variables: HashMap<String, Variable>,
    assignments: Vec<Assignment>,
    // Track mutability per variable scope (keyed by var_name + first_line)
    scope_mutability: HashMap<(String, usize), bool>,
    // Track variables that are borrowed as &mut anywhere in the code
    mut_borrowed_vars: std::collections::HashSet<String>,
    // Track variables that have mutating methods called on them (.push(), .insert(), etc.)
    mutated_via_method: std::collections::HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct Assignment {
    pub line_num: usize,
    pub var_name: String,
    pub var_type: Option<String>,
    pub value: String,
    pub is_rust_native: bool,
    pub is_borrow: bool,
    pub is_borrow_mut: bool,
}

impl VariableTracker {
    pub fn new() -> Self {
        VariableTracker {
            variables: HashMap::new(),
            assignments: Vec::new(),
            scope_mutability: HashMap::new(),
            mut_borrowed_vars: std::collections::HashSet::new(),
            mutated_via_method: std::collections::HashSet::new(),
        }
    }

    pub fn detect_string_literal(value: &str) -> bool {
        let trimmed = value.trim();
        trimmed.starts_with('"') && trimmed.ends_with('"') && !trimmed.contains("String::from")
    }

    pub fn detect_borrow(value: &str) -> (bool, bool, String) {
        let trimmed = value.trim();
        if trimmed.starts_with("&mut ") {
            (true, true, trimmed[5..].to_string())
        } else if trimmed.starts_with('&') && !trimmed.starts_with("&&") {
            (true, false, trimmed[1..].trim().to_string())
        } else {
            (false, false, trimmed.to_string())
        }
    }

    pub fn infer_type(value: &str) -> Option<String> {
        let trimmed = value.trim();
        
        if trimmed.starts_with('"') && trimmed.ends_with('"') {
            return Some("String".to_string());
        }
        
        if trimmed.starts_with("String::from") {
            return Some("String".to_string());
        }
        
        if trimmed == "true" || trimmed == "false" {
            return Some("bool".to_string());
        }
        
        if trimmed.parse::<i64>().is_ok() {
            return Some("i32".to_string());
        }
        
        if trimmed.parse::<f64>().is_ok() && trimmed.contains('.') {
            return Some("f64".to_string());
        }
        
        if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() == 3 {
            return Some("char".to_string());
        }
        
        if trimmed.starts_with("vec![") || trimmed.starts_with("Vec::") {
            return Some("Vec".to_string());
        }
        
        if trimmed.starts_with('&') {
            return Some("ref".to_string());
        }
        
        None
    }

    pub fn track_assignment(&mut self, line_num: usize, var_name: &str, var_type: Option<String>, value: &str, is_rust_native: bool) {
        let (is_borrow, is_borrow_mut, _clean_value) = Self::detect_borrow(value);
        
        let assignment = Assignment {
            line_num,
            var_name: var_name.to_string(),
            var_type: var_type.clone(),
            value: value.to_string(),
            is_rust_native,
            is_borrow,
            is_borrow_mut,
        };
        self.assignments.push(assignment);

        let inferred_type = var_type.clone().or_else(|| Self::infer_type(value));

        if let Some(existing) = self.variables.get_mut(var_name) {
            let existing_type = existing.var_type.clone();
            let new_type = inferred_type.clone();
            
            let is_shadowing = match (&existing_type, &new_type) {
                (Some(et), Some(nt)) => et != nt,
                _ => false,
            };
            
            if is_shadowing {
                // Save the mutability info for the OLD scope before creating new one
                let old_first_line = existing.first_line;
                let old_is_mutable = existing.is_mutable;
                self.scope_mutability.insert((var_name.to_string(), old_first_line), old_is_mutable);
                
                // Create new variable for the new scope (shadowing)
                let mut new_var = Variable::new(var_name.to_string());
                new_var.var_type = inferred_type;
                new_var.assigned_count = 1;
                new_var.first_line = line_num;
                new_var.is_borrow = is_borrow;
                new_var.is_borrow_mut = is_borrow_mut;
                self.variables.insert(var_name.to_string(), new_var);
            } else {
                // Same scope - increment assignment count
                let first_line = existing.first_line;
                existing.assigned_count += 1;
                if existing.assigned_count > 1 {
                    existing.is_mutable = true;
                    // Update scope mutability
                    self.scope_mutability.insert((var_name.to_string(), first_line), true);
                }
            }
        } else {
            // Brand new variable
            let mut var = Variable::new(var_name.to_string());
            var.var_type = inferred_type;
            var.assigned_count = 1;
            var.first_line = line_num;
            var.is_borrow = is_borrow;
            var.is_borrow_mut = is_borrow_mut;
            self.variables.insert(var_name.to_string(), var);
        }
    }

    pub fn needs_mut(&self, var_name: &str, line_num: usize) -> bool {
        // Check if variable is borrowed as &mut anywhere
        if self.mut_borrowed_vars.contains(var_name) {
            return true;
        }
        
        // Check if variable has mutating methods called on it (.push(), .insert(), etc.)
        if self.mutated_via_method.contains(var_name) {
            return true;
        }
        
        // Check if this line is a first assignment of any scope for this variable
        // and if that scope needs mutability
        if let Some(&is_mut) = self.scope_mutability.get(&(var_name.to_string(), line_num)) {
            return is_mut;
        }
        
        // Check current variable state
        if let Some(var) = self.variables.get(var_name) {
            if var.first_line == line_num && var.is_mutable {
                return true;
            }
        }
        false
    }
    
    /// Check if variable is borrowed as &mut (not reassignment-based)
    /// Use this when scope.rs handles reassignment-based mut detection
    pub fn is_mut_borrowed(&self, var_name: &str) -> bool {
        self.mut_borrowed_vars.contains(var_name)
    }
    
    /// Scan a line for &mut <identifier> patterns and mark those variables as needing mutability
    pub fn scan_for_mut_borrows(&mut self, line: &str) {
        let trimmed = line.trim();
        
        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with("//") {
            return;
        }
        
        // Find all occurrences of &mut followed by an identifier
        let mut remaining = trimmed;
        while let Some(pos) = remaining.find("&mut ") {
            let after_mut = &remaining[pos + 5..];
            
            // Extract the identifier after &mut
            let ident: String = after_mut
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            
            if !ident.is_empty() && is_valid_identifier(&ident) {
                self.mut_borrowed_vars.insert(ident);
            }
            
            // Move past this occurrence
            if pos + 5 < remaining.len() {
                remaining = &remaining[pos + 5..];
            } else {
                break;
            }
        }
    }
    
    /// Mark a variable as being borrowed mutably
    pub fn mark_mut_borrowed(&mut self, var_name: &str) {
        self.mut_borrowed_vars.insert(var_name.to_string());
    }
    
    /// Scan a line for mutating method calls like .push(), .insert(), etc.
    /// These require the variable to be declared as `mut`
    pub fn scan_for_mutating_methods(&mut self, line: &str) {
        let trimmed = line.trim();
        
        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with("//") {
            return;
        }
        
        // List of mutating methods that require &mut self
        const MUTATING_METHODS: &[&str] = &[
            // Vec methods
            ".push(", ".pop()", ".insert(", ".remove(", ".clear()", 
            ".append(", ".truncate(", ".resize(", ".extend(",
            ".sort(", ".sort_by(", ".sort_by_key(", ".reverse()",
            ".drain(", ".retain(", ".dedup(", ".swap(",
            ".split_off(", ".swap_remove(",
            // HashMap/HashSet methods
            ".entry(", ".or_insert(", ".and_modify(",
            // String methods
            ".push_str(",
            // Common mutation patterns
            ".get_mut(",
        ];
        
        // Compound assignment operators that indicate mutation
        const COMPOUND_ASSIGNS: &[&str] = &[
            " += ", " -= ", " *= ", " /= ", " %= ",
            " &= ", " |= ", " ^= ", " <<= ", " >>= ",
        ];
        
        // Check for mutating methods: var.method(...)
        for method in MUTATING_METHODS {
            if let Some(pos) = trimmed.find(method) {
                // Extract variable name before the method call
                let before_method = &trimmed[..pos];
                if let Some(var_name) = extract_var_name_before_dot(before_method) {
                    self.mutated_via_method.insert(var_name);
                }
            }
        }
        
        // Check for compound assignments: var += value
        for op in COMPOUND_ASSIGNS {
            if let Some(pos) = trimmed.find(op) {
                let before_op = trimmed[..pos].trim();
                // Handle simple variable or field access
                if let Some(var_name) = extract_root_var(before_op) {
                    self.mutated_via_method.insert(var_name);
                }
            }
        }
    }
    
    /// Check if a variable is mutated via method calls
    pub fn is_mutated_via_method(&self, var_name: &str) -> bool {
        self.mutated_via_method.contains(var_name)
    }

    pub fn is_first_assignment(&self, var_name: &str, line_num: usize) -> bool {
        for assignment in &self.assignments {
            if assignment.var_name == var_name {
                return assignment.line_num == line_num;
            }
        }
        // CRITICAL FIX: If variable is NOT in tracker, it's a NEW variable
        // Therefore this IS its first assignment - return TRUE not false!
        true
    }

    pub fn is_shadowing(&self, var_name: &str, line_num: usize) -> bool {
        // Get the FIRST known type for this variable (from first assignment)
        let mut first_known_type: Option<String> = None;
        let mut current_type: Option<String> = None;
        
        for assignment in &self.assignments {
            if assignment.var_name == var_name {
                if assignment.line_num < line_num {
                    // Only set first_known_type if we haven't set it yet
                    // This captures the ORIGINAL type of the variable
                    if first_known_type.is_none() {
                        first_known_type = assignment.var_type.clone()
                            .or_else(|| Self::infer_type(&assignment.value));
                    }
                } else if assignment.line_num == line_num {
                    current_type = assignment.var_type.clone()
                        .or_else(|| Self::infer_type(&assignment.value));
                    break;
                }
            }
        }
        
        match (first_known_type, current_type) {
            (Some(pt), Some(ct)) => pt != ct,
            _ => false,
        }
    }

    pub fn get_variable(&self, name: &str) -> Option<&Variable> {
        self.variables.get(name)
    }
}

/// Parse RustS+ assignment, returns (var_name, var_type, value, is_outer, is_explicit_mut)
/// 
/// Handles:
/// - `x = 10`              -> (x, None, 10, false, false)
/// - `mut x = 10`          -> (x, None, 10, false, true)  -- EXPLICIT MUT DECLARATION
/// - `outer x = 10`        -> (x, None, 10, true, false)
/// - `x: i32 = 10`         -> (x, Some(i32), 10, false, false)
/// - `mut x: i32 = 10`     -> (x, Some(i32), 10, false, true)
pub fn parse_rusts_assignment_ext(line: &str) -> Option<(String, Option<String>, String, bool, bool)> {
    let trimmed = line.trim();
    
    // Check for `outer` keyword prefix
    let (is_outer, remaining) = if trimmed.starts_with("outer ") {
        (true, trimmed.strip_prefix("outer ").unwrap().trim())
    } else {
        (false, trimmed)
    };
    
    // Check for `mut` keyword prefix (RustS+ explicit mutable declaration)
    // CRITICAL: `mut x = 10` in RustS+ MUST become `let mut x = 10;` in Rust
    let (is_explicit_mut, remaining) = if remaining.starts_with("mut ") {
        (true, remaining.strip_prefix("mut ").unwrap().trim())
    } else {
        (false, remaining)
    };
    
    if remaining.starts_with("let ") || remaining.starts_with("const ") || remaining.starts_with("static ") {
        return None;
    }
    
    if remaining.starts_with("fn ") || remaining.starts_with("pub ") || remaining.starts_with("use ") 
       || remaining.starts_with("mod ") || remaining.starts_with("struct ") || remaining.starts_with("enum ")
       || remaining.starts_with("impl ") || remaining.starts_with("trait ") || remaining.starts_with("type ")
       || remaining.starts_with("//") || remaining.starts_with("/*") || remaining.starts_with("*")
       || remaining.starts_with('#') {
        return None;
    }
    
    // =========================================================================
    // CRITICAL FIX: Reject control flow statements BEFORE checking for `=`
    // These are NEVER assignments, even if they contain `=` somewhere
    // Bug: `if self.status != SyncStatus::Idle {` was detected as assignment
    // because `!= ` contains `= ` as substring!
    // =========================================================================
    if remaining.starts_with("if ") || remaining.starts_with("while ") 
       || remaining.starts_with("for ") || remaining.starts_with("match ")
       || remaining.starts_with("loop ") || remaining.starts_with("unsafe ")
       || remaining.starts_with("return ") || remaining.starts_with("break ")
       || remaining.starts_with("continue") {
        return None;
    }
    
    if remaining.is_empty() || remaining == "{" || remaining == "}" || remaining == "}" || remaining.ends_with(',') {
        return None;
    }
    
    if !remaining.contains('=') {
        return None;
    }
    
    // =========================================================================
    // CRITICAL FIX: Use helper function to find standalone assignment `=`
    // The old logic had a bug where `"!= "` contains `"= "` as substring,
    // causing lines like `if x != y {` to be incorrectly parsed as assignments.
    // =========================================================================
    let eq_pos = match find_standalone_assignment_eq(remaining) {
        Some(pos) => pos,
        None => return None, // No standalone `=` found - not an assignment
    };
    
    let left = remaining[..eq_pos].trim();
    let right = remaining[eq_pos + 1..].trim().trim_end_matches(';');
    
    if left.is_empty() || right.is_empty() {
        return None;
    }
    
    // CRITICAL FIX: Handle RustS+ style type annotations like `var Type[T]`
    // Must check for space-separated `var Type` BEFORE rejecting lines with `[`
    // because the type might contain `[` like `Vec[T]`
    
    // Check if left contains space (potential RustS+ style: `var Type`)
    if left.contains(' ') {
        // RustS+ style: var Type (no colon)
        // Split by first space to get var_name and type
        let space_pos = left.find(' ').unwrap();
        let vname = left[..space_pos].trim();
        let vtype = left[space_pos + 1..].trim();
        
        // Validate: vname must be valid identifier
        let vname_valid = !vname.is_empty() 
            && vname.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
            && vname.chars().all(|c| c.is_alphanumeric() || c == '_');
        
        // Type typically starts with uppercase, or is a known generic like Vec[, Option[, etc.
        // Also handle reference types like &Type, &mut Type
        // CRITICAL FIX: Also handle path-qualified types like std::collections::HashSet[T]
        // These start with lowercase but are valid types!
        let vtype_valid = !vtype.is_empty() && (
            vtype.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
            // Path-qualified types (std::, crate::, super::, self::, or any module::)
            || vtype.contains("::")
            || vtype.starts_with("Vec[") || vtype.starts_with("Vec<")
            || vtype.starts_with("Option[") || vtype.starts_with("Option<")
            || vtype.starts_with("Result[") || vtype.starts_with("Result<")
            || vtype.starts_with("HashMap[") || vtype.starts_with("HashMap<")
            || vtype.starts_with("HashSet[") || vtype.starts_with("HashSet<")
            || vtype.starts_with("BTreeMap[") || vtype.starts_with("BTreeMap<")
            || vtype.starts_with("BTreeSet[") || vtype.starts_with("BTreeSet<")
            || vtype.starts_with("Box[") || vtype.starts_with("Box<")
            || vtype.starts_with("Arc[") || vtype.starts_with("Arc<")
            || vtype.starts_with("Rc[") || vtype.starts_with("Rc<")
            || vtype.starts_with('&')  // Reference types
            || vtype.starts_with('(')  // Tuple types
            || vtype.starts_with('[')  // Slice/array types
            || vtype == "i8" || vtype == "i16" || vtype == "i32" || vtype == "i64" || vtype == "i128"
            || vtype == "u8" || vtype == "u16" || vtype == "u32" || vtype == "u64" || vtype == "u128"
            || vtype == "f32" || vtype == "f64"
            || vtype == "bool" || vtype == "char" || vtype == "usize" || vtype == "isize"
        );
        
        if vname_valid && vtype_valid {
            return Some((vname.to_string(), Some(vtype.to_string()), right.to_string(), is_outer, is_explicit_mut));
        }
    }
    
    // For simple identifiers (no space), reject if contains special chars
    // These are likely not assignments but other constructs
    if left.contains('(') || left.contains('[') || left.contains('{') {
        return None;
    }
    
    if left.contains(':') {
        let type_parts: Vec<&str> = left.splitn(2, ':').collect();
        if type_parts.len() == 2 {
            let var_name = type_parts[0].trim();
            let var_type = type_parts[1].trim();
            
            if !is_valid_identifier(var_name) {
                return None;
            }
            
            return Some((var_name.to_string(), Some(var_type.to_string()), right.to_string(), is_outer, is_explicit_mut));
        }
    }
    
    if !is_valid_identifier(left) {
        return None;
    }
    
    Some((left.to_string(), None, right.to_string(), is_outer, is_explicit_mut))
}

/// Find the position of a standalone assignment `=` that's NOT part of an operator.
/// 
/// Returns None if no such `=` exists (meaning the line is NOT an assignment).
/// 
/// This function properly handles:
/// - `==` (equality comparison)
/// - `!=` (not equal) - CRITICAL: `"!= "` contains `"= "` as substring!
/// - `<=` (less than or equal)
/// - `>=` (greater than or equal)  
/// - `=>` (fat arrow / match arm)
/// - `+=`, `-=`, `*=`, `/=`, `%=` (compound assignment)
/// - `&=`, `|=`, `^=` (bitwise compound)
/// - `<<=`, `>>=` (shift compound)
/// - Nested structures (braces, brackets, parens)
/// - String literals
fn find_standalone_assignment_eq(s: &str) -> Option<usize> {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    
    // Track nested structures - MUST specify type for saturating_sub to work
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
            // Get characters before and after
            let prev = if i > 0 { chars[i - 1] } else { ' ' };
            let next = if i + 1 < len { chars[i + 1] } else { ' ' };
            
            // Reject if part of an operator
            
            // Check NEXT char: `==`, `=>`
            if next == '=' || next == '>' {
                prev_char = c;
                continue;
            }
            
            // Check PREV char: `!=`, `<=`, `>=`, `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`
            if prev == '!' || prev == '<' || prev == '>' 
               || prev == '+' || prev == '-' || prev == '*' || prev == '/' || prev == '%' 
               || prev == '&' || prev == '|' || prev == '^' {
                prev_char = c;
                continue;
            }
            
            // Check for `<<=` and `>>=` (prev is < or > and the one before that is also < or >)
            if i >= 2 {
                let prev_prev = chars[i - 2];
                if (prev == '<' && prev_prev == '<') || (prev == '>' && prev_prev == '>') {
                    prev_char = c;
                    continue;
                }
            }
            
            // This is a standalone assignment `=`
            return Some(i);
        }
        
        prev_char = c;
    }
    
    None
}

/// Original parse function for backward compatibility (no outer/mut info)
pub fn parse_rusts_assignment(line: &str) -> Option<(String, Option<String>, String)> {
    parse_rusts_assignment_ext(line).map(|(name, typ, val, _, _)| (name, typ, val))
}

/// Extract variable name from expression before a dot
/// Examples:
/// - "result" -> Some("result")
/// - "self.items" -> Some("self") (root var)
/// - "items[0]" -> Some("items")
fn extract_var_name_before_dot(expr: &str) -> Option<String> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return None;
    }
    
    // If it contains a dot, we want the root variable
    let root = extract_root_var(trimmed)?;
    Some(root)
}

/// Extract the root variable from an expression
/// Examples:
/// - "result" -> Some("result")
/// - "self.items" -> Some("self")
/// - "items[0].field" -> Some("items")
/// - "(*ptr)" -> Some("ptr")
fn extract_root_var(expr: &str) -> Option<String> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return None;
    }
    
    // Handle dereference: (*ptr) -> ptr
    let cleaned = if trimmed.starts_with("(*") && trimmed.ends_with(')') {
        &trimmed[2..trimmed.len()-1]
    } else if trimmed.starts_with('*') {
        &trimmed[1..]
    } else {
        trimmed
    };
    
    // Find the root variable (before any . or [)
    let root: String = cleaned
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    
    if root.is_empty() {
        return None;
    }
    
    // Validate it's an identifier
    if is_valid_identifier(&root) {
        Some(root)
    } else {
        None
    }
}

pub fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    
    let first = s.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    
    s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

pub fn expand_value(value: &str, explicit_type: Option<&str>) -> String {
    let trimmed = value.trim();
    
    // Handle closure with RustS+ parameter syntax: `|param TYPE|` -> `|param: TYPE|`
    if trimmed.starts_with('|') {
        let transformed = transform_closure_params(trimmed);
        if transformed != trimmed {
            return transformed;
        }
    }
    
    // Handle string literal (simple case, no concatenation)
    // CRITICAL FIX: In Rust, bare string literals are &'static str by default
    // Only convert to String::from() if EXPLICITLY typed as String
    if VariableTracker::detect_string_literal(trimmed) {
        // If explicit type is &str OR no explicit type → keep as literal
        // Rust will infer &'static str which is correct
        if explicit_type.is_none() || explicit_type == Some("&str") {
            return trimmed.to_string();
        }
        // Only convert to String if EXPLICITLY typed as String
        if explicit_type == Some("String") {
            let inner = &trimmed[1..trimmed.len()-1];
            return format!("String::from(\"{}\")", inner);
        }
        // Default: keep as literal
        return trimmed.to_string();
    }
    
    // Handle string concatenation: String + String should become String + &String
    if trimmed.contains(" + ") {
        return expand_string_concatenation(trimmed);
    }
    
    trimmed.to_string()
}

/// Transform RustS+ closure parameters to Rust syntax
/// `|param TYPE|` -> `|param: TYPE|`
/// `|param TYPE| -> RetType { body }` -> `|param: TYPE| -> RetType { body }`
/// `|a &Address, b u32|` -> `|a: &Address, b: u32|`
fn transform_closure_params(closure: &str) -> String {
    let trimmed = closure.trim();
    
    // Must start with `|`
    if !trimmed.starts_with('|') {
        return trimmed.to_string();
    }
    
    // Find the closing `|` of the parameter list
    let mut depth = 0;
    let mut close_pipe_pos = None;
    let chars: Vec<char> = trimmed.chars().collect();
    
    for (i, &c) in chars.iter().enumerate().skip(1) {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            '|' if depth == 0 => {
                close_pipe_pos = Some(i);
                break;
            }
            _ => {}
        }
    }
    
    let close_pos = match close_pipe_pos {
        Some(pos) => pos,
        None => return trimmed.to_string(), // No closing `|` found
    };
    
    // Extract parameters between the pipes
    let params_str = &trimmed[1..close_pos];
    let after_params = &trimmed[close_pos..]; // Includes the closing `|`
    
    // Split parameters by comma
    let params: Vec<&str> = split_closure_params(params_str);
    
    // Transform each parameter
    let transformed_params: Vec<String> = params.iter()
        .map(|p| transform_single_closure_param(p.trim()))
        .collect();
    
    format!("|{}|{}", transformed_params.join(", "), &after_params[1..])
}

/// Split closure parameters by comma, respecting nested angle brackets and parens
fn split_closure_params(params: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    
    for (i, c) in params.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                result.push(&params[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    
    if start < params.len() {
        result.push(&params[start..]);
    }
    
    result
}

/// Transform a single closure parameter: `param TYPE` -> `param: TYPE`
fn transform_single_closure_param(param: &str) -> String {
    let trimmed = param.trim();
    
    if trimmed.is_empty() {
        return String::new();
    }
    
    // Already has colon - pass through
    if trimmed.contains(':') {
        return trimmed.to_string();
    }
    
    // Check for RustS+ style: `param TYPE`
    // The type must start with uppercase, `&`, `(`, `[`, or be a known primitive
    if let Some(space_pos) = trimmed.find(' ') {
        let param_name = &trimmed[..space_pos].trim();
        let param_type = &trimmed[space_pos + 1..].trim();
        
        // Validate param_name is a valid identifier
        let name_valid = !param_name.is_empty() && 
            param_name.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false) &&
            param_name.chars().all(|c| c.is_alphanumeric() || c == '_');
        
        // Validate param_type looks like a type
        let type_valid = !param_type.is_empty() && (
            param_type.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
            || param_type.starts_with('&')
            || param_type.starts_with('(')
            || param_type.starts_with('[')
            || param_type.starts_with("Vec")
            || param_type.starts_with("Option")
            || param_type.starts_with("Result")
            || param_type.starts_with("Box")
            || param_type.starts_with("impl ")
            || param_type.starts_with("dyn ")
            || is_primitive_type(param_type)
        );
        
        if name_valid && type_valid {
            return format!("{}: {}", param_name, param_type);
        }
    }
    
    // No transformation needed
    trimmed.to_string()
}

/// Check if a type string is a primitive type
fn is_primitive_type(t: &str) -> bool {
    matches!(t, "i8" | "i16" | "i32" | "i64" | "i128" | "isize" |
               "u8" | "u16" | "u32" | "u64" | "u128" | "usize" |
               "f32" | "f64" | "bool" | "char" | "str")
}

/// Expands string concatenation to make it Rust-legal
/// e.g., `greeting + ", " + target` becomes `greeting + ", " + &target`
fn expand_string_concatenation(expr: &str) -> String {
    let parts: Vec<&str> = expr.split(" + ").collect();
    if parts.len() < 2 {
        return expr.to_string();
    }
    
    let mut result_parts: Vec<String> = Vec::new();
    
    for (i, part) in parts.iter().enumerate() {
        let part = part.trim();
        
        if i == 0 {
            // First part: can be consumed (String owns the data)
            if VariableTracker::detect_string_literal(part) {
                let inner = &part[1..part.len()-1];
                result_parts.push(format!("String::from(\"{}\")", inner));
            } else {
                result_parts.push(part.to_string());
            }
        } else {
            // Subsequent parts: need to be &str or &String
            if VariableTracker::detect_string_literal(part) {
                // String literal is already &str, keep as is
                result_parts.push(part.to_string());
            } else if part.starts_with('&') {
                // Already a reference, keep as is
                result_parts.push(part.to_string());
            } else if is_valid_identifier(part) {
                // Variable identifier - need to borrow it
                result_parts.push(format!("&{}", part));
            } else {
                // Other expression, keep as is
                result_parts.push(part.to_string());
            }
        }
    }
    
    result_parts.join(" + ")
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_expand_value_string_literal_no_type() {
        // CRITICAL: When no explicit type, string literal should stay as literal
        // This allows Rust to infer &'static str
        let result = expand_value("\"hello\"", None);
        assert_eq!(result, "\"hello\"", 
            "String literal without explicit type should stay as literal, got: {}", result);
    }
    
    #[test]
    fn test_expand_value_string_literal_explicit_str() {
        // When explicit type is &str, keep as literal
        let result = expand_value("\"hello\"", Some("&str"));
        assert_eq!(result, "\"hello\"", 
            "String literal with explicit &str should stay as literal, got: {}", result);
    }
    
    #[test]
    fn test_expand_value_string_literal_explicit_string() {
        // ONLY when explicit type is String, convert to String::from()
        let result = expand_value("\"hello\"", Some("String"));
        assert_eq!(result, "String::from(\"hello\")", 
            "String literal with explicit String should convert, got: {}", result);
    }
    
    #[test]
    fn test_expand_value_preserves_long_string() {
        // Test with a long string like SHA256 hash
        let hash = "\"3cca5fcf71bf8609a64c354abf4773110dd315159be317b4218b7b8fadb6d0ce\"";
        let result = expand_value(hash, None);
        assert_eq!(result, hash, 
            "Long string literal should stay as literal, got: {}", result);
    }
    
    #[test]
    fn test_infer_type_string_literal() {
        // infer_type should return "String" for string literals
        // (this is for type tracking, not for output generation)
        let result = VariableTracker::infer_type("\"hello\"");
        assert_eq!(result, Some("String".to_string()));
    }
    
    //=========================================================================
    // MUTATING METHOD DETECTION TESTS
    //=========================================================================
    
    #[test]
    fn test_scan_for_mutating_methods_push() {
        let mut tracker = VariableTracker::new();
        tracker.scan_for_mutating_methods("result.push(value)");
        assert!(tracker.is_mutated_via_method("result"),
            "result should be marked as mutated via .push()");
    }
    
    #[test]
    fn test_scan_for_mutating_methods_insert() {
        let mut tracker = VariableTracker::new();
        tracker.scan_for_mutating_methods("map.insert(key, value)");
        assert!(tracker.is_mutated_via_method("map"),
            "map should be marked as mutated via .insert()");
    }
    
    #[test]
    fn test_scan_for_mutating_methods_clear() {
        let mut tracker = VariableTracker::new();
        tracker.scan_for_mutating_methods("items.clear()");
        assert!(tracker.is_mutated_via_method("items"),
            "items should be marked as mutated via .clear()");
    }
    
    #[test]
    fn test_scan_for_compound_assignment() {
        let mut tracker = VariableTracker::new();
        tracker.scan_for_mutating_methods("counter += 1");
        assert!(tracker.is_mutated_via_method("counter"),
            "counter should be marked as mutated via +=");
    }
    
    #[test]
    fn test_scan_for_field_mutation() {
        let mut tracker = VariableTracker::new();
        tracker.scan_for_mutating_methods("self.items.push(x)");
        assert!(tracker.is_mutated_via_method("self"),
            "self should be marked as mutated via .push() on field");
    }
    
    #[test]
    fn test_extract_root_var() {
        assert_eq!(extract_root_var("result"), Some("result".to_string()));
        assert_eq!(extract_root_var("self.items"), Some("self".to_string()));
        assert_eq!(extract_root_var("items[0]"), Some("items".to_string()));
    }
    
    #[test]
    fn test_needs_mut_with_mutating_method() {
        let mut tracker = VariableTracker::new();
        // First, track an assignment
        tracker.track_assignment(1, "result", None, "Vec::new()", false);
        // Then scan for mutating method
        tracker.scan_for_mutating_methods("result.push(value)");
        // Should need mut
        assert!(tracker.needs_mut("result", 1),
            "result should need mut because .push() is called");
    }
    
    // =========================================================================
    // CRITICAL BUG FIX TESTS: Comparison operators not detected as assignment
    // =========================================================================
    
    #[test]
    fn test_not_equals_not_assignment() {
        // CRITICAL: This must NOT be detected as assignment
        // This was the main bug - `!= ` contains `= ` as substring
        let result = parse_rusts_assignment_ext("if self.status != SyncStatus::Idle {");
        assert!(result.is_none(), "!= should not be detected as assignment");
    }
    
    #[test]
    fn test_comparison_operators_not_assignment() {
        assert!(parse_rusts_assignment_ext("if x == 10 {").is_none(), "== should not be assignment");
        assert!(parse_rusts_assignment_ext("if x != 10 {").is_none(), "!= should not be assignment");
        assert!(parse_rusts_assignment_ext("if x <= 10 {").is_none(), "<= should not be assignment");
        assert!(parse_rusts_assignment_ext("if x >= 10 {").is_none(), ">= should not be assignment");
        assert!(parse_rusts_assignment_ext("while x != y {").is_none(), "!= in while should not be assignment");
    }
    
    #[test]
    fn test_compound_operators_not_assignment() {
        // Compound assignments are handled differently, not through this function
        assert!(parse_rusts_assignment_ext("x += 1").is_none(), "+= should not match");
        assert!(parse_rusts_assignment_ext("x -= 1").is_none(), "-= should not match");
    }
    
    #[test]
    fn test_fat_arrow_not_assignment() {
        assert!(parse_rusts_assignment_ext("Pattern => value").is_none(), "=> should not be assignment");
        assert!(parse_rusts_assignment_ext("Ok(_) => result").is_none());
    }
    
    #[test]
    fn test_control_flow_not_assignment() {
        assert!(parse_rusts_assignment_ext("if condition {").is_none());
        assert!(parse_rusts_assignment_ext("while condition {").is_none());
        assert!(parse_rusts_assignment_ext("for x in iter {").is_none());
        assert!(parse_rusts_assignment_ext("match expr {").is_none());
        assert!(parse_rusts_assignment_ext("return value").is_none());
    }
    
    #[test]
    fn test_valid_simple_assignment_still_works() {
        let result = parse_rusts_assignment_ext("x = 10");
        assert!(result.is_some());
        let (name, _, value, _, _) = result.unwrap();
        assert_eq!(name, "x");
        assert_eq!(value, "10");
    }
    
    #[test]
    fn test_find_standalone_assignment_eq() {
        // Should find `=` in simple assignment
        assert_eq!(find_standalone_assignment_eq("x = 10"), Some(2));
        
        // Should NOT find `=` in comparisons (these return None)
        assert!(find_standalone_assignment_eq("x == 10").is_none());
        assert!(find_standalone_assignment_eq("x != 10").is_none());
        assert!(find_standalone_assignment_eq("x <= 10").is_none());
        assert!(find_standalone_assignment_eq("x >= 10").is_none());
        
        // Should NOT find `=` in compound assignments
        assert!(find_standalone_assignment_eq("x += 1").is_none());
        assert!(find_standalone_assignment_eq("x -= 1").is_none());
        
        // Should NOT find `=` in fat arrow
        assert!(find_standalone_assignment_eq("x => y").is_none());
        
        // Should find `=` in assignment with comparison on RHS
        let pos = find_standalone_assignment_eq("result = x != y");
        assert_eq!(pos, Some(7)); // position of first `=`
    }
}