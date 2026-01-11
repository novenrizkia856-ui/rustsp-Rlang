//! RustS+ Anti-Fail Logic System (Stage 1 Contract Enforcer)
//!
//! # KONSTITUSI RUSTS+
//!
//! Modul ini adalah GERBANG AKHIR sebelum kode diteruskan ke Rust.
//! Jika kode:
//! - Salah logika
//! - Ambigu niat
//! - Melanggar kontrak bahasa
//!
//! → KOMPILASI BERHENTI DI SINI.
//!
//! ## Logic Rules yang Ditegakkan
//!
//! - **Logic-01**: Expression Completeness (if/match sebagai value harus punya semua branch)
//! - **Logic-02**: Ambiguous Shadowing (assignment di inner scope tanpa `outer`)
//! - **Logic-03**: Illegal Statement in Expression (no `let` dalam if/match expression)
//! - **Logic-04**: Implicit Mutation (reassignment harus eksplisit)
//! - **Logic-05**: Unclear Intent (block kosong, implicit (), dll)

use crate::error_msg::{RsplError, ErrorCode, SourceLocation};
use std::collections::{HashMap, HashSet};

//=============================================================================
// ANSI COLOR CODES (Cross-platform terminal colors)
//=============================================================================

/// ANSI escape codes untuk terminal colors
pub mod ansi {
    pub const RED: &str = "\x1b[31m";
    pub const BOLD_RED: &str = "\x1b[1;31m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BOLD_YELLOW: &str = "\x1b[1;33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const BOLD_BLUE: &str = "\x1b[1;34m";
    pub const CYAN: &str = "\x1b[36m";
    pub const BOLD_CYAN: &str = "\x1b[1;36m";
    pub const GREEN: &str = "\x1b[32m";
    pub const BOLD_GREEN: &str = "\x1b[1;32m";
    pub const WHITE: &str = "\x1b[37m";
    pub const BOLD_WHITE: &str = "\x1b[1;37m";
    pub const BOLD: &str = "\x1b[1m";
    pub const RESET: &str = "\x1b[0m";
}

//=============================================================================
// LOGIC VIOLATION CATEGORIES
//=============================================================================

/// Kategori pelanggaran logika RustS+
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicViolation {
    /// Logic-01: if/match sebagai expression tapi tidak semua branch return value
    IncompleteExpression,
    /// Logic-02: Shadowing ambigu tanpa keyword `outer`
    AmbiguousShadowing,
    /// Logic-03: Statement ilegal dalam expression context (let, dll)
    IllegalStatementInExpression,
    /// Logic-04: Mutasi implisit tanpa deklarasi eksplisit
    ImplicitMutation,
    /// Logic-05: Intent tidak jelas (block kosong, implicit (), dll)
    UnclearIntent,
    /// Logic-06: Same-scope reassignment tanpa `mut`
    SameScopeReassignment,
}

impl LogicViolation {
    /// Get violation code untuk display
    pub fn code(&self) -> &'static str {
        match self {
            Self::IncompleteExpression => "Logic-01",
            Self::AmbiguousShadowing => "Logic-02",
            Self::IllegalStatementInExpression => "Logic-03",
            Self::ImplicitMutation => "Logic-04",
            Self::UnclearIntent => "Logic-05",
            Self::SameScopeReassignment => "Logic-06",
        }
    }
    
    /// Get deskripsi singkat
    pub fn description(&self) -> &'static str {
        match self {
            Self::IncompleteExpression => "incomplete expression branches",
            Self::AmbiguousShadowing => "ambiguous variable shadowing",
            Self::IllegalStatementInExpression => "illegal statement in expression",
            Self::ImplicitMutation => "implicit mutation without declaration",
            Self::UnclearIntent => "unclear code intent",
            Self::SameScopeReassignment => "same-scope reassignment without mut",
        }
    }
}

//=============================================================================
// SCOPE TRACKING
//=============================================================================

/// Represents a scope dalam program
#[derive(Debug, Clone)]
struct Scope {
    /// Variables yang dideklarasi di scope ini, with their line numbers
    variables: HashMap<String, usize>,
    /// Variables yang dideklarasi sebagai mutable
    mutable_vars: HashSet<String>,
    /// Depth level scope
    depth: usize,
    /// Apakah scope ini adalah expression context?
    is_expression_context: bool,
    /// Line dimana scope dimulai
    #[allow(dead_code)]
    start_line: usize,
}

impl Scope {
    fn new(depth: usize, is_expression_context: bool, start_line: usize) -> Self {
        Scope {
            variables: HashMap::new(),
            mutable_vars: HashSet::new(),
            depth,
            is_expression_context,
            start_line,
        }
    }
    
    fn declare(&mut self, var: &str, line: usize) {
        self.variables.insert(var.to_string(), line);
    }
    
    fn declare_mut(&mut self, var: &str, line: usize) {
        self.variables.insert(var.to_string(), line);
        self.mutable_vars.insert(var.to_string());
    }
    
    fn has(&self, var: &str) -> bool {
        self.variables.contains_key(var)
    }
    
    fn is_mutable(&self, var: &str) -> bool {
        self.mutable_vars.contains(var)
    }
    
    fn get_declaration_line(&self, var: &str) -> Option<usize> {
        self.variables.get(var).copied()
    }
}

//=============================================================================
// CONTROL FLOW TRACKING  
//=============================================================================

/// Tracks if/match expression untuk completeness checking
#[derive(Debug, Clone)]
struct ControlFlowExpr {
    /// Line dimana expression dimulai
    start_line: usize,
    /// Apakah digunakan dalam value context?
    is_value_context: bool,
    /// Apakah punya else branch (untuk if)?
    has_else: bool,
    /// Jenis expression
    kind: ControlFlowKind,
    /// Variable yang di-assign (jika ada)
    assigned_to: Option<String>,
    /// Brace depth saat dimulai
    start_depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ControlFlowKind {
    If,
    Match,
}

//=============================================================================
// ANTI-FAIL LOGIC CHECKER (Main Engine)
//=============================================================================

/// Engine utama untuk validasi logika RustS+.
/// Ini adalah Stage-1 Contract Enforcer.
#[derive(Debug)]
pub struct AntiFailLogicChecker {
    /// Stack of scopes
    scopes: Vec<Scope>,
    /// Current brace depth
    brace_depth: usize,
    /// Stack of control flow expressions
    control_flow_stack: Vec<ControlFlowExpr>,
    /// Collected errors
    errors: Vec<RsplError>,
    /// Source file name
    file_name: String,
    /// Source lines untuk error reporting
    source_lines: Vec<String>,
    /// Variables assigned di function level
    function_vars: HashMap<String, usize>,
    /// Variables yang sudah di-reassign
    reassigned_vars: HashSet<String>,
    /// Apakah di dalam function?
    in_function: bool,
    /// Brace depth saat function dimulai
    function_depth: usize,
    /// Mode strict (semua checks aktif)
    strict_mode: bool,
}

impl AntiFailLogicChecker {
    /// Create new checker instance
    pub fn new(file_name: &str) -> Self {
        AntiFailLogicChecker {
            scopes: vec![Scope::new(0, false, 0)], // Global scope
            brace_depth: 0,
            control_flow_stack: Vec::new(),
            errors: Vec::new(),
            file_name: file_name.to_string(),
            source_lines: Vec::new(),
            function_vars: HashMap::new(),
            reassigned_vars: HashSet::new(),
            in_function: false,
            function_depth: 0,
            strict_mode: true,
        }
    }
    
    /// Run anti-fail logic check pada source code.
    /// 
    /// # Returns
    /// - `Ok(())` jika code lolos semua checks
    /// - `Err(Vec<RsplError>)` jika ada logic violations
    pub fn check(&mut self, source: &str) -> Result<(), Vec<RsplError>> {
        self.source_lines = source.lines().map(String::from).collect();
        
        for (line_num, line) in source.lines().enumerate() {
            let line_num = line_num + 1; // 1-indexed
            self.analyze_line(line, line_num);
        }
        
        // Check untuk unclosed expressions
        self.check_unclosed_expressions();
        
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }
    
    //=========================================================================
    // LINE ANALYSIS
    //=========================================================================
    
    fn analyze_line(&mut self, line: &str, line_num: usize) {
        let trimmed = line.trim();
        
        // Skip empty lines dan comments
        if trimmed.is_empty() || trimmed.starts_with("//") {
            return;
        }
        
        // Count braces
        let opens = self.count_open_braces(trimmed);
        let closes = self.count_close_braces(trimmed);
        
        // Check untuk function definition
        if self.is_function_start(trimmed) {
            self.enter_function(line_num, opens);
        } else if opens > 0 && self.in_function {
            let is_control_flow = self.check_control_flow_start(trimmed, line_num);
            
            // Untuk standalone blocks, enter scope
            if !is_control_flow && !self.is_definition(trimmed) {
                for _ in 0..opens {
                    self.enter_scope(false, line_num);
                }
            }
        } else {
            self.check_control_flow_start(trimmed, line_num);
        }
        
        // Logic-03: Check untuk illegal statements dalam expression context
        self.check_illegal_statement(trimmed, line_num);
        
        // Logic-02 & Logic-04: Check assignments
        self.check_assignment(trimmed, line_num);
        
        // Logic-05: Check untuk unclear intent patterns
        if self.strict_mode {
            self.check_unclear_intent(trimmed, line_num);
        }
        
        // Update brace depth
        for _ in 0..opens {
            self.brace_depth += 1;
        }
        
        for _ in 0..closes {
            self.handle_close_brace();
        }
        
        // Check jika function ended
        if self.in_function && self.brace_depth < self.function_depth {
            self.exit_function();
        }
    }
    
    //=========================================================================
    // LOGIC-01: EXPRESSION COMPLETENESS
    //=========================================================================
    
    fn check_control_flow_start(&mut self, trimmed: &str, line_num: usize) -> bool {
        // Check untuk if/match dalam value context: `x = if cond {`
        if let Some(cf_expr) = self.detect_control_flow_expr(trimmed, line_num) {
            self.control_flow_stack.push(cf_expr.clone());
            
            if cf_expr.is_value_context {
                self.enter_scope(true, line_num);
            }
            return true;
        }
        
        // Check untuk else branch
        if trimmed.starts_with("else") || trimmed.contains("} else") {
            if let Some(cf) = self.control_flow_stack.last_mut() {
                if cf.kind == ControlFlowKind::If {
                    cf.has_else = true;
                }
            }
            return true;
        }
        
        // Standalone control flow
        if (trimmed.starts_with("if ") || trimmed.starts_with("while ") ||
            trimmed.starts_with("for ") || trimmed.starts_with("loop ") ||
            trimmed.starts_with("match ")) && trimmed.contains('{') {
            return true;
        }
        
        false
    }
    
    fn detect_control_flow_expr(&self, trimmed: &str, line_num: usize) -> Option<ControlFlowExpr> {
        // Pattern: `x = if cond {`
        if trimmed.contains("= if ") && trimmed.contains('{') {
            let assigned_to = self.extract_assignment_target(trimmed);
            return Some(ControlFlowExpr {
                start_line: line_num,
                is_value_context: true,
                has_else: false,
                kind: ControlFlowKind::If,
                assigned_to,
                start_depth: self.brace_depth,
            });
        }
        
        // Pattern: `x = match expr {`
        if trimmed.contains("= match ") && trimmed.contains('{') {
            let assigned_to = self.extract_assignment_target(trimmed);
            return Some(ControlFlowExpr {
                start_line: line_num,
                is_value_context: true,
                has_else: false,
                kind: ControlFlowKind::Match,
                assigned_to,
                start_depth: self.brace_depth,
            });
        }
        
        None
    }
    
    fn check_unclosed_expressions(&mut self) {
        let unclosed: Vec<_> = self.control_flow_stack.drain(..).collect();
        
        for cf in unclosed {
            if cf.is_value_context && cf.kind == ControlFlowKind::If && !cf.has_else {
                self.emit_logic01_error(cf.start_line, cf.assigned_to.as_deref());
            }
        }
    }
    
    fn emit_logic01_error(&mut self, line_num: usize, assigned_to: Option<&str>) {
        let source_line = self.get_source_line(line_num);
        let var_info = assigned_to
            .map(|v| format!(" (assigning to `{}`)", v))
            .unwrap_or_default();
        
        let error = RsplError::new(
            ErrorCode::RSPL060,
            format!("`if` expression used as value but missing `else` branch{}", var_info)
        )
        .at(self.make_location(line_num, &source_line))
        .note(
            format!(
                "{} VIOLATION: Expression Completeness\n\n\
                 dalam RustS+, ketika `if` digunakan sebagai expression (assigned ke variable),\n\
                 HARUS menghasilkan value di SEMUA branches.\n\n\
                 `if` tanpa `else` menghasilkan `()` (unit type) saat condition false,\n\
                 yang hampir pasti BUKAN yang kamu inginkan.",
                LogicViolation::IncompleteExpression.code()
            )
        )
        .help(
            "tambahkan `else` branch untuk provide value di semua cases:\n\n\
                 x = if condition {\n\
                     value_when_true\n\
                 } else {\n\
                     value_when_false\n\
                 }"
        );
        
        self.errors.push(error);
    }
    
    //=========================================================================
    // LOGIC-02: AMBIGUOUS SHADOWING
    //=========================================================================
    
    fn check_shadowing(&mut self, var_name: &str, line_num: usize, trimmed: &str) {
        // Only check jika:
        // 1. Di dalam function
        // 2. Di nested scope (depth > 2 = global + function)
        // 3. Variable exists di outer scope
        // 4. Tidak ada keyword `outer`
        
        if !self.in_function || self.scopes.len() <= 2 {
            return;
        }
        
        if self.is_defined_in_outer_scope(var_name) {
            self.emit_logic02_error(var_name, line_num, trimmed);
        }
    }
    
    fn is_defined_in_outer_scope(&self, var_name: &str) -> bool {
        // Skip current scope, check outer scopes
        for scope in self.scopes.iter().rev().skip(1) {
            if scope.has(var_name) {
                return true;
            }
        }
        // Also check function-level vars
        self.function_vars.contains_key(var_name)
    }
    
    fn emit_logic02_error(&mut self, var_name: &str, line_num: usize, source: &str) {
        let error = RsplError::new(
            ErrorCode::RSPL081,
            format!("ambiguous shadowing of outer variable `{}`", var_name)
        )
        .at(self.make_location(line_num, source))
        .note(
            format!(
                "{} VIOLATION: Ambiguous Shadowing\n\n\
                 dalam RustS+, assignment di dalam block membuat variable BARU by default.\n\
                 variable outer `{}` akan TIDAK BERUBAH setelah block ini selesai.\n\n\
                 ini hampir pasti BUKAN yang kamu inginkan.\n\
                 jika ingin modify outer variable, HARUS gunakan `outer`.",
                LogicViolation::AmbiguousShadowing.code(),
                var_name
            )
        )
        .help(
            format!(
                "untuk modify outer variable, tulis:\n\n\
                     outer {} = ...\n\n\
                 untuk intentionally shadow (buat variable baru), tambah comment:\n\n\
                     // shadow: {}\n\
                     {} = ...",
                var_name, var_name, var_name
            )
        );
        
        self.errors.push(error);
    }
    
    //=========================================================================
    // LOGIC-03: ILLEGAL STATEMENT IN EXPRESSION
    //=========================================================================
    
    fn check_illegal_statement(&mut self, trimmed: &str, line_num: usize) {
        // Check jika kita di expression context
        let in_expr_context = self.scopes.last()
            .map(|s| s.is_expression_context)
            .unwrap_or(false);
        
        if !in_expr_context {
            return;
        }
        
        // Illegal: `let` dalam expression context
        if trimmed.starts_with("let ") {
            self.emit_logic03_error(line_num, trimmed, "`let` statement");
        }
    }
    
    fn emit_logic03_error(&mut self, line_num: usize, source: &str, stmt_type: &str) {
        let error = RsplError::new(
            ErrorCode::RSPL041,
            format!("{} not allowed in expression context", stmt_type)
        )
        .at(self.make_location(line_num, source))
        .note(
            format!(
                "{} VIOLATION: Illegal Statement in Expression\n\n\
                 dalam RustS+, ketika `if` atau `match` digunakan sebagai expression,\n\
                 body hanya boleh berisi EXPRESSIONS yang menghasilkan values.\n\n\
                 statements seperti `let` tidak menghasilkan values dan DILARANG\n\
                 dalam context ini.",
                LogicViolation::IllegalStatementInExpression.code()
            )
        )
        .help(
            "pindahkan statement ke luar expression block:\n\n\
                 let a = 10\n\
                 x = if condition {\n\
                     a + 5\n\
                 } else {\n\
                     0\n\
                 }"
        );
        
        self.errors.push(error);
    }
    
    //=========================================================================
    // LOGIC-05: UNCLEAR INTENT
    //=========================================================================
    
    fn check_unclear_intent(&mut self, trimmed: &str, line_num: usize) {
        // Check untuk empty blocks yang produce ()
        if trimmed == "{}" {
            let error = RsplError::new(
                ErrorCode::RSPL001,
                "empty block produces implicit `()`"
            )
            .at(self.make_location(line_num, trimmed))
            .note(
                format!(
                    "{} WARNING: Unclear Intent\n\n\
                     pattern ini mungkin tidak melakukan apa yang kamu expect.\n\
                     RustS+ membutuhkan intent yang eksplisit.",
                    LogicViolation::UnclearIntent.code()
                )
            )
            .help("review code ini dan buat intent-mu eksplisit");
            
            self.errors.push(error);
        }
    }
    
    //=========================================================================
    // ASSIGNMENT CHECKING
    //=========================================================================
    
    fn check_assignment(&mut self, trimmed: &str, line_num: usize) {
        // Skip non-assignments
        if !trimmed.contains('=') {
            return;
        }
        
        // Skip operators dan special cases
        if self.is_not_assignment(trimmed) {
            return;
        }
        
        // Detect keywords: `outer`, `mut`
        let is_outer = trimmed.starts_with("outer ");
        let is_mut = trimmed.starts_with("mut ") || 
                     (is_outer && trimmed.starts_with("outer mut "));
        
        // Strip keywords untuk extract variable name
        let clean = trimmed
            .strip_prefix("outer ")
            .unwrap_or(trimmed)
            .strip_prefix("mut ")
            .unwrap_or(trimmed);
        
        if let Some(eq_pos) = clean.find('=') {
            let before = clean[..eq_pos].trim();
            
            // Extract var name (handle type annotations)
            let var_name = if before.contains(' ') {
                before.split_whitespace().next().unwrap_or(before)
            } else {
                before
            };
            
            if !self.is_valid_identifier(var_name) {
                return;
            }
            
            // =====================================================================
            // LOGIC-06: SAME-SCOPE REASSIGNMENT BAN
            // =====================================================================
            // Check jika variable sudah dideklarasi di SAME scope
            // dan bukan mutable
            
            if !is_outer {
                if let Some(scope) = self.scopes.last() {
                    if scope.has(var_name) {
                        // Variable exists in same scope - check if mutable
                        if !scope.is_mutable(var_name) {
                            let original_line = scope.get_declaration_line(var_name).unwrap_or(0);
                            self.emit_logic06_error(var_name, line_num, original_line, trimmed);
                        }
                        // If mutable, this is a valid reassignment - don't emit error
                        return;
                    }
                }
            }
            
            // =====================================================================
            // LOGIC-02: AMBIGUOUS SHADOWING (inner scope)
            // =====================================================================
            // Only check jika tidak pakai `outer` dan variable ada di outer scope
            
            if !is_outer {
                self.check_shadowing(var_name, line_num, trimmed);
            }
            
            // Track variable declaration
            if let Some(scope) = self.scopes.last_mut() {
                if !is_outer {
                    if is_mut {
                        scope.declare_mut(var_name, line_num);
                    } else {
                        scope.declare(var_name, line_num);
                    }
                }
            }
            
            // Track di function level
            if self.in_function && !is_outer {
                if self.function_vars.contains_key(var_name) {
                    self.reassigned_vars.insert(var_name.to_string());
                } else {
                    self.function_vars.insert(var_name.to_string(), line_num);
                }
            }
        }
    }
    
    //=========================================================================
    // LOGIC-06: SAME-SCOPE REASSIGNMENT ERROR
    //=========================================================================
    
    fn emit_logic06_error(&mut self, var_name: &str, line_num: usize, original_line: usize, source: &str) {
        let error = RsplError::new(
            ErrorCode::RSPL071,
            format!("ambiguous reassignment to `{}` in the same scope", var_name)
        )
        .at(self.make_location(line_num, source))
        .note(
            format!(
                "{} VIOLATION: Same-Scope Reassignment Ban\n\n\
                 variable `{}` sudah dideklarasi di line {} dalam scope yang sama.\n\
                 reassignment ke nama yang sama membuat binding BARU di Rust,\n\
                 yang hampir pasti BUKAN yang kamu inginkan.\n\n\
                 pattern ini adalah sumber umum logic bugs dan DILARANG di RustS+.",
                LogicViolation::SameScopeReassignment.code(),
                var_name,
                original_line
            )
        )
        .help(
            format!(
                "jika ingin MUTATE variable, deklarasi dengan `mut`:\n\n\
                     mut {} = <initial_value>\n\
                     {} = <new_value>          // OK: mutates existing binding\n\n\
                 jika ingin SHADOW variable, gunakan inner scope:\n\n\
                     {} = <first_value>\n\
                     {{\n\
                         {} = <second_value>   // OK: shadows in inner scope\n\
                     }}",
                var_name, var_name, var_name, var_name
            )
        );
        
        self.errors.push(error);
    }
    
    fn is_not_assignment(&self, trimmed: &str) -> bool {
        trimmed.contains("==") || trimmed.contains("!=") ||
        trimmed.contains("<=") || trimmed.contains(">=") ||
        trimmed.contains("=>") ||
        trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") ||
        trimmed.starts_with("struct ") || trimmed.starts_with("pub struct ") ||
        trimmed.starts_with("enum ") || trimmed.starts_with("pub enum ") ||
        trimmed.starts_with("if ") || trimmed.starts_with("while ") ||
        trimmed.starts_with("match ") || trimmed.starts_with("for ") ||
        trimmed.contains("= if ") || trimmed.contains("= match ")
    }
    
    //=========================================================================
    // HELPER FUNCTIONS
    //=========================================================================
    
    fn is_function_start(&self, trimmed: &str) -> bool {
        (trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ")) &&
        trimmed.contains('(')
    }
    
    fn is_definition(&self, trimmed: &str) -> bool {
        trimmed.starts_with("struct ") || trimmed.starts_with("pub struct ") ||
        trimmed.starts_with("enum ") || trimmed.starts_with("pub enum ") ||
        trimmed.starts_with("impl ") || trimmed.starts_with("trait ")
    }
    
    fn is_valid_identifier(&self, s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        let first = s.chars().next().unwrap();
        if !first.is_alphabetic() && first != '_' {
            return false;
        }
        s.chars().all(|c| c.is_alphanumeric() || c == '_')
    }
    
    fn count_open_braces(&self, s: &str) -> usize {
        s.chars().filter(|&c| c == '{').count()
    }
    
    fn count_close_braces(&self, s: &str) -> usize {
        s.chars().filter(|&c| c == '}').count()
    }
    
    fn extract_assignment_target(&self, trimmed: &str) -> Option<String> {
        if let Some(eq_pos) = trimmed.find('=') {
            let before = &trimmed[..eq_pos];
            // Skip ==
            if eq_pos + 1 < trimmed.len() {
                let chars: Vec<char> = trimmed.chars().collect();
                if chars.get(eq_pos + 1) == Some(&'=') {
                    return None;
                }
            }
            let target = before.trim().trim_start_matches("outer ");
            if !target.is_empty() && self.is_valid_identifier(target) {
                return Some(target.to_string());
            }
        }
        None
    }
    
    fn enter_function(&mut self, line_num: usize, opens: usize) {
        self.in_function = true;
        self.function_depth = self.brace_depth + opens;
        self.function_vars.clear();
        self.reassigned_vars.clear();
        self.enter_scope(false, line_num);
    }
    
    fn exit_function(&mut self) {
        self.in_function = false;
        self.function_depth = 0;
        self.function_vars.clear();
        self.reassigned_vars.clear();
    }
    
    fn enter_scope(&mut self, is_expression_context: bool, line_num: usize) {
        self.scopes.push(Scope::new(
            self.brace_depth + 1,
            is_expression_context,
            line_num,
        ));
    }
    
    fn exit_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }
    
    fn handle_close_brace(&mut self) {
        if self.brace_depth == 0 {
            return;
        }
        
        self.brace_depth -= 1;
        
        // Close control flow expressions
        self.close_control_flow_at_depth();
        
        // Exit scopes
        while self.scopes.len() > 1 {
            if let Some(scope) = self.scopes.last() {
                if scope.depth > self.brace_depth {
                    self.exit_scope();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }
    
    fn close_control_flow_at_depth(&mut self) {
        while let Some(cf) = self.control_flow_stack.last() {
            if cf.start_depth >= self.brace_depth {
                let cf = self.control_flow_stack.pop().unwrap();
                
                // Logic-01: Check jika expression complete
                if cf.is_value_context && cf.kind == ControlFlowKind::If && !cf.has_else {
                    self.emit_logic01_error(cf.start_line, cf.assigned_to.as_deref());
                }
            } else {
                break;
            }
        }
    }
    
    fn get_source_line(&self, line_num: usize) -> String {
        self.source_lines.get(line_num - 1)
            .map(|s| s.to_string())
            .unwrap_or_default()
    }
    
    fn make_location(&self, line_num: usize, source: &str) -> SourceLocation {
        let source_line = self.get_source_line(line_num);
        let trimmed = source.trim();
        let highlight_start = source_line.find(trimmed).unwrap_or(0);
        let highlight_len = trimmed.len().min(60);
        
        SourceLocation {
            file: self.file_name.clone(),
            line: line_num,
            column: highlight_start + 1,
            source_line,
            highlight_start,
            highlight_len,
        }
    }
}

//=============================================================================
// PUBLIC API
//=============================================================================

/// Run anti-fail logic check pada RustS+ source code.
/// Ini adalah entry point utama untuk Stage-1 validation.
///
/// # Returns
/// - `Ok(())` jika code lolos semua logic checks
/// - `Err(Vec<RsplError>)` jika ada logic violations
pub fn check_logic(source: &str, file_name: &str) -> Result<(), Vec<RsplError>> {
    let mut checker = AntiFailLogicChecker::new(file_name);
    checker.check(source)
}

/// Format logic errors untuk terminal display dengan ANSI colors.
pub fn format_logic_errors(errors: &[RsplError]) -> String {
    use ansi::*;
    
    let mut output = String::new();
    
    
    // Setiap error dengan warna
    for error in errors {
        output.push_str(&format_error_colored(error));
        output.push('\n');
    }
    
    // Footer
    output.push_str(&format!(
        "\n{}error{}: aborting due to {} logic violation{}\n",
        BOLD_RED,
        RESET,
        errors.len(),
        if errors.len() == 1 { "" } else { "s" }
    ));
    
    output.push_str(&format!(
        "\n{}note{}: logic errors terdeteksi SEBELUM Rust compilation.\n",
        CYAN, RESET
    ));
    output.push_str(&format!(
        "{}      RustS+ TIDAK akan meneruskan kode yang tidak jujur ke rustc.{}\n",
        CYAN, RESET
    ));
    output.push_str(&format!(
        "{}      perbaiki errors ini untuk melanjutkan.{}\n",
        CYAN, RESET
    ));
    
    output
}

/// Format single error dengan ANSI colors
fn format_error_colored(error: &RsplError) -> String {
    use ansi::*;
    
    let mut output = String::new();
    let category = error.code.category();
    
    // Header dengan ERROR merah
    output.push_str(&format!(
        "{}error{}[{}][{}]: {}{}{}\n",
        BOLD_RED,
        RESET,
        error.code.code_str(),
        category,
        BOLD_WHITE,
        error.title,
        RESET
    ));
    
    // Location dengan warna biru
    if !error.location.file.is_empty() {
        output.push_str(&format!(
            "{}  --> {}{}:{}:{}{}\n",
            BLUE,
            RESET,
            error.location.file,
            error.location.line,
            error.location.column,
            RESET
        ));
    }
    
    // Source line dengan highlight
    if !error.location.source_line.is_empty() {
        let line_num_width = error.location.line.to_string().len();
        let padding = " ".repeat(line_num_width);
        
        output.push_str(&format!("{}{}  |{}\n", BLUE, padding, RESET));
        output.push_str(&format!(
            "{}{} |{}   {}\n",
            BLUE,
            error.location.line,
            RESET,
            error.location.source_line
        ));
        
        // Highlight dengan warna MERAH
        let highlight_padding = " ".repeat(error.location.highlight_start);
        let highlight = "^".repeat(error.location.highlight_len);
        output.push_str(&format!(
            "{}{}  |{}   {}{}{}{}\n",
            BLUE,
            padding,
            RESET,
            highlight_padding,
            BOLD_RED,
            highlight,
            RESET
        ));
    }
    
    // Note section dengan warna CYAN
    if let Some(ref note) = error.explanation {
        output.push_str(&format!("\n{}note{}:\n", BOLD_CYAN, RESET));
        for line in note.lines() {
            output.push_str(&format!("  {}\n", line));
        }
    }
    
    // Help section dengan warna KUNING/HIJAU
    if let Some(ref help) = error.suggestion {
        output.push_str(&format!("\n{}help{}:\n", BOLD_YELLOW, RESET));
        for line in help.lines() {
            output.push_str(&format!("  {}{}{}\n", GREEN, line, RESET));
        }
    }
    
    output
}

//=============================================================================
// TESTS
//=============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_logic01_if_without_else() {
        let source = r#"
fn main() {
    x = if true {
        10
    }
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, ErrorCode::RSPL060);
    }
    
    #[test]
    fn test_logic01_if_with_else_ok() {
        let source = r#"
fn main() {
    x = if true {
        10
    } else {
        20
    }
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_logic02_shadowing() {
        let source = r#"
fn main() {
    counter = 0
    {
        counter = counter + 1
    }
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors[0].code, ErrorCode::RSPL081);
    }
    
    #[test]
    fn test_logic02_outer_ok() {
        let source = r#"
fn main() {
    counter = 0
    {
        outer counter = counter + 1
    }
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_logic03_let_in_expression() {
        let source = r#"
fn main() {
    x = if true {
        let a = 10
        a
    } else {
        0
    }
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors[0].code, ErrorCode::RSPL041);
    }
    
    #[test]
    fn test_valid_code_passes() {
        let source = r#"
fn classify(n i32) String {
    match n {
        0 { "zero" }
        x if x > 0 { "positive" }
        _ { "negative" }
    }
}

fn main() {
    result = classify(5)
    println!("{}", result)
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_ok());
    }
    
    //=========================================================================
    // LOGIC-06: SAME-SCOPE REASSIGNMENT TESTS
    //=========================================================================
    
    #[test]
    fn test_logic06_same_scope_reassignment_error() {
        // HARUS ERROR: x = 10 lalu x = x + 1 di scope yang sama
        let source = r#"
fn main() {
    x = 10
    x = x + 1
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_err(), "Expected Logic-06 error for same-scope reassignment");
        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, ErrorCode::RSPL071);
    }
    
    #[test]
    fn test_logic06_simple_reassignment_error() {
        // HARUS ERROR: x = 10 lalu x = 20 di scope yang sama
        let source = r#"
fn main() {
    x = 10
    x = 20
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_err(), "Expected Logic-06 error for simple reassignment");
        let errors = result.unwrap_err();
        assert_eq!(errors[0].code, ErrorCode::RSPL071);
    }
    
    #[test]
    fn test_logic06_mut_ok() {
        // HARUS LOLOS: mut x = 10 lalu x = x + 1
        let source = r#"
fn main() {
    mut x = 10
    x = x + 1
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_ok(), "mut variable should allow reassignment");
    }
    
    #[test]
    fn test_logic06_inner_scope_ok() {
        // HARUS LOLOS: shadowing di inner scope
        let source = r#"
fn main() {
    x = 10
    {
        x = 20
    }
}
"#;
        // Note: This will trigger Logic-02 (ambiguous shadowing) not Logic-06
        // because it's a different scope. The user needs to use `outer` if they
        // want to modify the outer variable, or the shadowing is intentional.
        let result = check_logic(source, "test.rss");
        // This should error with RSPL081 (shadowing), not RSPL071
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors[0].code, ErrorCode::RSPL081);
    }
    
    #[test]
    fn test_logic06_different_vars_ok() {
        // HARUS LOLOS: different variables
        let source = r#"
fn main() {
    x = 10
    y = 20
    z = x + y
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_ok(), "Different variables should be fine");
    }
    
    #[test]
    fn test_logic06_mut_multiple_reassign_ok() {
        // HARUS LOLOS: mut allows multiple reassignment
        let source = r#"
fn main() {
    mut counter = 0
    counter = counter + 1
    counter = counter + 1
    counter = counter + 1
}
"#;
        let result = check_logic(source, "test.rss");
        assert!(result.is_ok(), "mut variable should allow multiple reassignments");
    }
}