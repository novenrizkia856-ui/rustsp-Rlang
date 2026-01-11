//! Function & Parameter Grammar for RustS+
//!
//! Syntax transformations:
//! - RustS+: `fn add(a i32, b i32) i32 { body }` → Rust: `fn add(a: i32, b: i32) -> i32 { body }`
//! - RustS+: `fn add(a i32, b i32) i32 = expr` → Rust: `fn add(a: i32, b: i32) -> i32 { expr }`
//! - RustS+: `fn id[T](x T) T { x }` → Rust: `fn id<T>(x: T) -> T { x }`
//! - Borrow: `fn read(x &String)` → `fn read(x: &String)`
//! - Rust passthrough: `fn foo(a: i32) -> i32` remains unchanged
//!
//! Expression transformations:
//! - String concat: `&String + &str` → `lhs.to_owned() + ...`
//! - Call coercion: `foo("lit")` where param is &String → `foo(&String::from("lit"))`
//! - Tail return: last expr in non-() function has no semicolon

use std::collections::HashMap;

/// A parsed function parameter
#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub param_type: String,
    pub is_borrow: bool,
    pub is_mut_borrow: bool,
}

/// A parsed function signature
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub name: String,
    pub generics: Option<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub is_pub: bool,
    pub is_single_line: bool,
    pub single_line_expr: Option<String>,
}

/// Result of parsing a function line
#[derive(Debug)]
pub enum FunctionParseResult {
    RustSPlusSignature(FunctionSignature),
    RustPassthrough,
    NotAFunction,
    Error(String),
}

// ============================================================================
// FUNCTION REGISTRY - Tracks function signatures for call-site coercion
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct FunctionRegistry {
    functions: HashMap<String, FunctionSignature>,
}

impl FunctionRegistry {
    pub fn new() -> Self {
        FunctionRegistry { functions: HashMap::new() }
    }
    
    pub fn register(&mut self, sig: FunctionSignature) {
        self.functions.insert(sig.name.clone(), sig);
    }
    
    pub fn get(&self, name: &str) -> Option<&FunctionSignature> {
        self.functions.get(name)
    }
}

// ============================================================================
// CURRENT FUNCTION CONTEXT
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct CurrentFunctionContext {
    pub name: Option<String>,
    pub params: HashMap<String, String>,
    pub return_type: Option<String>,
    pub start_depth: usize,
}

impl CurrentFunctionContext {
    pub fn new() -> Self {
        CurrentFunctionContext {
            name: None,
            params: HashMap::new(),
            return_type: None,
            start_depth: 0,
        }
    }
    
    pub fn enter(&mut self, sig: &FunctionSignature, depth: usize) {
        self.name = Some(sig.name.clone());
        self.params.clear();
        for p in &sig.parameters {
            self.params.insert(p.name.clone(), p.param_type.clone());
        }
        self.return_type = sig.return_type.clone();
        self.start_depth = depth;
    }
    
    pub fn exit(&mut self) {
        self.name = None;
        self.params.clear();
        self.return_type = None;
        self.start_depth = 0;
    }
    
    pub fn is_inside(&self) -> bool {
        self.name.is_some()
    }
    
    pub fn has_return_value(&self) -> bool {
        self.return_type.is_some()
    }
    
    pub fn is_ref_string_param(&self, name: &str) -> bool {
        self.params.get(name)
            .map(|t| t == "&String" || t == "&mut String")
            .unwrap_or(false)
    }
}

// ============================================================================
// STRING CONCATENATION TRANSFORMER
// ============================================================================

pub fn transform_string_concat(expr: &str, ctx: &CurrentFunctionContext) -> String {
    let expr = expr.trim();
    if !expr.contains('+') {
        return expr.to_string();
    }
    
    let parts = split_by_plus(expr);
    if parts.len() < 2 {
        return expr.to_string();
    }
    
    let has_string_literal = parts.iter().any(|p| {
        let t = p.trim();
        t.starts_with('"') || t.starts_with("&\"")
    });
    
    let first_part = parts[0].trim();
    let first_is_ref_string = ctx.is_ref_string_param(first_part);
    
    if !has_string_literal && !first_is_ref_string {
        return expr.to_string();
    }
    
    let mut result_parts: Vec<String> = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let part = part.trim();
        if i == 0 && ctx.is_ref_string_param(part) {
            result_parts.push(format!("{}.to_owned()", part));
        } else {
            result_parts.push(part.to_string());
        }
    }
    
    result_parts.join(" + ")
}

fn split_by_plus(expr: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut in_string = false;
    let mut prev = ' ';
    
    for c in expr.chars() {
        if c == '"' && prev != '\\' {
            in_string = !in_string;
        }
        
        if !in_string {
            match c {
                '(' | '[' | '{' => { depth += 1; current.push(c); }
                ')' | ']' | '}' => { depth -= 1; current.push(c); }
                '+' if depth == 0 => {
                    parts.push(current.trim().to_string());
                    current = String::new();
                    prev = c;
                    continue;
                }
                _ => current.push(c),
            }
        } else {
            current.push(c);
        }
        prev = c;
    }
    
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

// ============================================================================
// CALL ARGUMENT COERCION
// ============================================================================

pub fn transform_call_args(line: &str, registry: &FunctionRegistry) -> String {
    let line = line.trim();
    if !line.contains('(') {
        return line.to_string();
    }
    
    let mut result = line.to_string();
    
    if let Some((func_name, paren_pos)) = find_function_call(line) {
        if let Some(sig) = registry.get(&func_name) {
            if let Some(close_paren) = find_matching_paren_from(line, paren_pos) {
                let before = &line[..paren_pos - func_name.len()];
                let args_str = &line[paren_pos + 1..close_paren];
                let after = &line[close_paren + 1..];
                
                let args = split_call_args(args_str);
                let mut new_args = Vec::new();
                
                for (i, arg) in args.iter().enumerate() {
                    let arg = arg.trim();
                    if let Some(param) = sig.parameters.get(i) {
                        new_args.push(coerce_argument(arg, &param.param_type));
                    } else {
                        new_args.push(arg.to_string());
                    }
                }
                
                result = format!("{}{}({}){}", before, func_name, new_args.join(", "), after);
            }
        }
    }
    
    result
}

fn find_function_call(expr: &str) -> Option<(String, usize)> {
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    
    while i < chars.len() {
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            
            if i < chars.len() && chars[i] == '(' {
                if !matches!(name.as_str(), "if" | "while" | "for" | "match" | "let" | "return" | "println" | "print" | "eprintln" | "format" | "vec" | "panic" | "assert") {
                    return Some((name, i));
                }
            }
        } else {
            i += 1;
        }
    }
    None
}

fn find_matching_paren_from(s: &str, start: usize) -> Option<usize> {
    let mut depth = 0;
    let mut in_string = false;
    let mut prev = ' ';
    
    for (i, c) in s[start..].char_indices() {
        if c == '"' && prev != '\\' {
            in_string = !in_string;
        }
        if !in_string {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(start + i);
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    None
}

fn split_call_args(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut in_string = false;
    let mut prev = ' ';
    
    for c in s.chars() {
        if c == '"' && prev != '\\' {
            in_string = !in_string;
        }
        
        if !in_string {
            match c {
                '(' | '[' | '{' | '<' => { depth += 1; current.push(c); }
                ')' | ']' | '}' | '>' => { depth -= 1; current.push(c); }
                ',' if depth == 0 => {
                    result.push(current.trim().to_string());
                    current = String::new();
                    prev = c;
                    continue;
                }
                _ => current.push(c),
            }
        } else {
            current.push(c);
        }
        prev = c;
    }
    
    if !current.trim().is_empty() {
        result.push(current.trim().to_string());
    }
    result
}

fn coerce_argument(arg: &str, param_type: &str) -> String {
    let arg = arg.trim();
    
    if param_type == "&String" {
        if arg.starts_with('"') && arg.ends_with('"') {
            return format!("&String::from({})", arg);
        }
        if arg.starts_with("&\"") && arg.ends_with('"') {
            let inner = &arg[1..];
            return format!("&String::from({})", inner);
        }
        return arg.to_string();
    }
    
    if param_type == "String" {
        if arg.starts_with('"') && arg.ends_with('"') {
            return format!("String::from({})", arg);
        }
    }
    
    arg.to_string()
}

// ============================================================================
// TAIL RETURN DETECTION
// ============================================================================

pub fn should_be_tail_return(
    line: &str,
    ctx: &CurrentFunctionContext,
    is_before_closing_brace: bool,
) -> bool {
    if !ctx.has_return_value() || !is_before_closing_brace {
        return false;
    }
    
    let trimmed = line.trim();
    
    if trimmed.ends_with(';') {
        return false;
    }
    
    if trimmed.starts_with("let ")
        || trimmed.starts_with("if ")
        || trimmed.starts_with("while ")
        || trimmed.starts_with("for ")
        || trimmed.starts_with("match ")
        || trimmed.starts_with("return ")
        || trimmed.contains("println!")
        || trimmed.contains("print!")
        || trimmed.contains("eprintln!")
        || trimmed.contains("panic!")
        || trimmed.contains("assert!")
    {
        return false;
    }
    
    if trimmed.contains(".push(")
        || trimmed.contains(".insert(")
        || trimmed.contains(".remove(")
        || trimmed.contains(".clear(")
    {
        return false;
    }
    
    true
}

// ============================================================================
// PARSING FUNCTIONS
// ============================================================================

pub fn parse_function_line(line: &str) -> FunctionParseResult {
    let trimmed = line.trim();
    
    let (is_pub, fn_part) = if trimmed.starts_with("pub fn ") {
        (true, trimmed.strip_prefix("pub fn ").unwrap())
    } else if trimmed.starts_with("fn ") {
        (false, trimmed.strip_prefix("fn ").unwrap())
    } else {
        return FunctionParseResult::NotAFunction;
    };
    
    if is_rust_syntax(trimmed) {
        return FunctionParseResult::RustPassthrough;
    }
    
    match parse_rustsplus_function(fn_part, is_pub) {
        Ok(sig) => FunctionParseResult::RustSPlusSignature(sig),
        Err(e) => FunctionParseResult::Error(e),
    }
}

fn is_rust_syntax(line: &str) -> bool {
    // Check parameter syntax - if params contain ": " it's Rust syntax
    if let Some(paren_start) = line.find('(') {
        if let Some(paren_end) = find_matching_paren(line, paren_start) {
            let params = &line[paren_start..=paren_end];
            // If params have colon-space, it's Rust syntax
            if params.contains(": ") {
                return true;
            }
            // Empty params () could be either - check for -> for determination
            // But if params exist without `:`, it's RustS+ syntax
            let inner = &params[1..params.len()-1].trim();
            if !inner.is_empty() && !params.contains(':') {
                // Non-empty params without colon = RustS+ syntax
                return false;
            }
        }
    }
    // No params with colons - check if it looks like pure Rust
    // But only if there are NO params at all or empty params
    false
}

fn find_matching_paren(s: &str, start: usize) -> Option<usize> {
    let mut depth = 0;
    let mut in_string = false;
    let mut prev = ' ';
    
    for (i, c) in s[start..].char_indices() {
        if c == '"' && prev != '\\' {
            in_string = !in_string;
        }
        if !in_string {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(start + i);
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    None
}

fn parse_rustsplus_function(after_fn: &str, is_pub: bool) -> Result<FunctionSignature, String> {
    let mut rest = after_fn.trim();
    
    let name_end = rest.find(|c: char| c == '(' || c == '[' || c.is_whitespace())
        .ok_or("Invalid function: missing name")?;
    let name = rest[..name_end].to_string();
    rest = rest[name_end..].trim();
    
    if name.is_empty() {
        return Err("Function name cannot be empty".to_string());
    }
    
    let generics = if rest.starts_with('[') {
        let bracket_end = rest.find(']').ok_or("Invalid generics: missing ']'")?;
        let gen = rest[1..bracket_end].trim().to_string();
        rest = rest[bracket_end + 1..].trim();
        Some(gen)
    } else {
        None
    };
    
    if !rest.starts_with('(') {
        return Err("Invalid function: expected '(' after name".to_string());
    }
    
    let paren_end = find_matching_paren(rest, 0).ok_or("Invalid function: unmatched '('")?;
    let params_str = &rest[1..paren_end];
    rest = rest[paren_end + 1..].trim();
    
    let parameters = parse_parameters(params_str)?;
    
    let (return_type, is_single_line, single_line_expr) = if rest.is_empty() || rest == "{" {
        (None, false, None)
    } else if rest.starts_with('=') {
        let expr = rest[1..].trim().trim_end_matches(';').to_string();
        (None, true, Some(expr))
    } else {
        // Handle both RustS+ syntax (just type) and hybrid syntax (-> type)
        let mut type_rest = rest;
        if type_rest.starts_with("->") {
            type_rest = type_rest[2..].trim();
        }
        
        let type_end = type_rest.find(|c: char| c == '{' || c == '=').unwrap_or(type_rest.len());
        let ret_type = type_rest[..type_end].trim().to_string();
        let after_type = type_rest[type_end..].trim();
        
        if after_type.starts_with('=') {
            let expr = after_type[1..].trim().trim_end_matches(';').to_string();
            (Some(ret_type), true, Some(expr))
        } else {
            (Some(ret_type), false, None)
        }
    };
    
    Ok(FunctionSignature {
        name, generics, parameters, return_type, is_pub, is_single_line, single_line_expr,
    })
}

fn parse_parameters(params_str: &str) -> Result<Vec<Parameter>, String> {
    let trimmed = params_str.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    
    let mut parameters = Vec::new();
    let parts = split_by_comma(trimmed);
    
    for part in parts {
        let part = part.trim();
        if part.is_empty() { continue; }
        parameters.push(parse_single_param(part)?);
    }
    
    Ok(parameters)
}

fn split_by_comma(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    
    for c in s.chars() {
        match c {
            '<' | '[' | '(' => { depth += 1; current.push(c); }
            '>' | ']' | ')' => { depth -= 1; current.push(c); }
            ',' if depth == 0 => {
                result.push(current.trim().to_string());
                current = String::new();
            }
            _ => current.push(c),
        }
    }
    
    if !current.trim().is_empty() {
        result.push(current.trim().to_string());
    }
    result
}

fn parse_single_param(param: &str) -> Result<Parameter, String> {
    let param = param.trim();
    
    let first_space = param.find(' ').ok_or_else(|| format!(
        "Parameter '{}' has no type annotation. All parameters must have explicit types in RustS+.",
        param
    ))?;
    
    let name = param[..first_space].trim().to_string();
    let type_str = param[first_space..].trim().to_string();
    
    if name.is_empty() {
        return Err("Parameter name cannot be empty".to_string());
    }
    
    if type_str.is_empty() {
        return Err(format!(
            "Parameter '{}' has no type annotation. All parameters must have explicit types in RustS+.",
            name
        ));
    }
    
    let (is_borrow, is_mut_borrow) = if type_str.starts_with("&mut ") {
        (true, true)
    } else if type_str.starts_with('&') {
        (true, false)
    } else {
        (false, false)
    };
    
    Ok(Parameter { name, param_type: type_str, is_borrow, is_mut_borrow })
}

pub fn signature_to_rust(sig: &FunctionSignature) -> String {
    let mut result = String::new();
    
    if sig.is_pub { result.push_str("pub "); }
    
    result.push_str("fn ");
    result.push_str(&sig.name);
    
    if let Some(ref gen) = sig.generics {
        result.push('<');
        result.push_str(gen);
        result.push('>');
    }
    
    result.push('(');
    let params: Vec<String> = sig.parameters.iter()
        .map(|p| format!("{}: {}", p.name, p.param_type))
        .collect();
    result.push_str(&params.join(", "));
    result.push(')');
    
    if let Some(ref ret) = sig.return_type {
        result.push_str(" -> ");
        result.push_str(ret);
    }
    
    if sig.is_single_line {
        if let Some(ref expr) = sig.single_line_expr {
            let mut ctx = CurrentFunctionContext::new();
            ctx.enter(sig, 0);
            let transformed_expr = transform_string_concat(expr, &ctx);
            result.push_str(" { ");
            result.push_str(&transformed_expr);
            result.push_str(" }");
        }
    } else {
        result.push_str(" {");
    }
    
    result
}

// Legacy compatibility
#[derive(Debug, Clone)]
pub struct FunctionContext {
    pub name: String,
    pub params: HashMap<String, ParamInfo>,
}

#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub param_type: String,
    pub is_mutable: bool,
}

impl FunctionContext {
    pub fn from_signature(sig: &FunctionSignature) -> Self {
        let mut params = HashMap::new();
        for p in &sig.parameters {
            params.insert(p.name.clone(), ParamInfo {
                param_type: p.param_type.clone(),
                is_mutable: p.is_mut_borrow,
            });
        }
        FunctionContext { name: sig.name.clone(), params }
    }
    
    pub fn is_param(&self, name: &str) -> bool {
        self.params.contains_key(name)
    }
    
    pub fn is_param_mutable(&self, name: &str) -> bool {
        self.params.get(name).map(|p| p.is_mutable).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_function() {
        let line = "fn add(a i32, b i32) i32 {";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                assert_eq!(sig.name, "add");
                let rust = signature_to_rust(&sig);
                assert!(rust.contains("fn add(a: i32, b: i32) -> i32 {"));
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    #[test]
    fn test_single_line_function() {
        let line = "fn add(a i32, b i32) i32 = a + b";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                assert!(sig.is_single_line);
                let rust = signature_to_rust(&sig);
                assert!(rust.contains("fn add(a: i32, b: i32) -> i32 { a + b }"));
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    #[test]
    fn test_borrow_param() {
        let line = "fn read(x &String) {";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                assert!(sig.parameters[0].is_borrow);
                let rust = signature_to_rust(&sig);
                assert!(rust.contains("fn read(x: &String) {"));
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    #[test]
    fn test_rust_passthrough() {
        let line = "fn add(a: i32, b: i32) -> i32 {";
        match parse_function_line(line) {
            FunctionParseResult::RustPassthrough => {}
            _ => panic!("Expected RustPassthrough"),
        }
    }
    
    #[test]
    fn test_string_concat_transform() {
        let mut ctx = CurrentFunctionContext::new();
        ctx.params.insert("a".to_string(), "&String".to_string());
        ctx.params.insert("b".to_string(), "&String".to_string());
        
        let expr = r#"a + ", " + b"#;
        let result = transform_string_concat(expr, &ctx);
        assert!(result.contains("a.to_owned()"), "Got: {}", result);
    }
    
    #[test]
    fn test_argument_coercion() {
        assert_eq!(coerce_argument(r#""hello""#, "&String"), r#"&String::from("hello")"#);
        assert_eq!(coerce_argument(r#"&"hello""#, "&String"), r#"&String::from("hello")"#);
    }
    
    #[test]
    fn test_tail_return() {
        let mut ctx = CurrentFunctionContext::new();
        ctx.return_type = Some("String".to_string());
        
        assert!(should_be_tail_return("a + b", &ctx, true));
        assert!(!should_be_tail_return("println!(\"hi\")", &ctx, true));
        assert!(!should_be_tail_return("a + b;", &ctx, true));
    }
}