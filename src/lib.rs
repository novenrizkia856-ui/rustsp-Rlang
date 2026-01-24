//! RustS+ Transpiler Library
//!
//! This is the main entry point for the RustS+ to Rust transpiler.
//! The transpilation is organized into multiple phases:
//! 1. First pass: Register types and track clone requirements
//! 2. Second pass: Line-by-line transformation
//! 3. Post-processing: Final cleanup and validation

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
pub mod hex_normalizer;

// IR-based modules
pub mod ast;
pub mod hir;
pub mod eir;
pub mod parser;
pub mod type_env;  // Phase 1.1: Type-driven effect inference

pub mod source_map;

// Modularized transpiler components
pub mod helpers;
pub mod modes;
pub mod detection;
pub mod transform_literal;
pub mod transform_array;
pub mod clone_helpers;
pub mod postprocess;

// New modular components
pub mod first_pass;
pub mod parser_state;
pub mod inline_literal_transform;
pub mod postprocess_output;
pub mod tests;

// Re-export IR types for convenience
pub use ast::{Span, Spanned, EffectDecl};
pub use hir::{BindingId, BindingInfo, ScopeResolver, HirModule};
pub use eir::{Effect, EffectSet, EffectContext, EffectInference};
pub use parser::{Lexer, FunctionParser, extract_function_signatures};

// Re-export Type Environment for type-driven inference (Phase 1.1)
pub use type_env::{
    TypeEnv, TypeEnvBuilder, TypeDrivenInference,
    FunctionType, EffectSignature, ParamEffect,
};

use variable::{VariableTracker, parse_rusts_assignment_ext, expand_value};
use scope::ScopeAnalyzer;
use function::{
    parse_function_line, signature_to_rust, FunctionParseResult,
    CurrentFunctionContext,
    transform_string_concat, transform_call_args, should_be_tail_return
};
use struct_def::{
    is_struct_definition, parse_struct_header, 
    transform_struct_field,
};
use enum_def::{
    EnumParseContext,
    is_enum_definition, parse_enum_header, transform_enum_variant,
};
use control_flow::{
    MatchModeStack, is_match_start, is_match_arm_pattern,
    transform_arm_pattern, transform_arm_close_with_parens,
    is_if_assignment, parse_control_flow_assignment,
    is_single_line_arm, transform_single_line_arm,
    transform_enum_struct_init,
    MatchStringContext, transform_match_for_string_patterns, pattern_is_string_literal,
};
use hex_normalizer::normalize_hex_literals;

// Import from modularized components
use helpers::{
    strip_inline_comment,
    ends_with_continuation_operator, needs_semicolon,
    transform_struct_field_slice_to_vec,
    is_field_access, is_tuple_pattern,
};
use modes::{
    LiteralKind, LiteralModeStack, ArrayModeStack, UseImportMode,
    is_multiline_use_import_start, transform_use_import_item,
};
use detection::{
    detect_struct_literal_start, detect_bare_struct_literal,
    detect_bare_enum_literal, detect_enum_literal_start, detect_array_literal_start,
};
use transform_literal::{
    transform_literal_field_with_ctx,
    is_string_literal,
};
use transform_array::transform_array_element;
use clone_helpers::{
    transform_array_access_clone, extract_arm_pattern,
};

// Import from new modules
use first_pass::run_first_pass;
use inline_literal_transform::{
    transform_single_line_struct_literal, transform_single_line_enum_literal,
    transform_bare_struct_literal,
};
use postprocess_output::apply_postprocessing;


//===========================================================================
// MAIN PARSER
//===========================================================================

pub fn parse_rusts(source: &str) -> String {
    // CRITICAL: Normalize custom hex literals FIRST
    let normalized_source = normalize_hex_literals(source);
    
    let lines: Vec<&str> = normalized_source.lines().collect();
    let mut tracker = VariableTracker::new();
    
    // Run scope analysis
    let mut scope_analyzer = ScopeAnalyzer::new();
    scope_analyzer.analyze(&normalized_source);
    
    // Run first pass to register types and track clone requirements
    let first_pass_result = run_first_pass(&lines, &mut tracker);
    let fn_registry = first_pass_result.fn_registry;
    let struct_registry = first_pass_result.struct_registry;
    let enum_registry = first_pass_result.enum_registry;
    
    // CRITICAL: Scan all lines for mutating method calls (.push(), .insert(), etc.)
    // This must happen AFTER first pass but BEFORE second pass
    // so that `needs_mut` returns correct results
    for line in &lines {
        tracker.scan_for_mutating_methods(line);
    }
    
    let mut output_lines: Vec<String> = Vec::new();
    
    // Parser state
    let mut brace_depth: usize = 0;
    let mut bracket_depth: usize = 0;
    let mut in_function_body = false;
    let mut function_start_brace = 0;
    let mut current_fn_ctx = CurrentFunctionContext::new();
    
    // Struct/enum definition contexts
    let mut in_struct_def = false;
    let mut struct_def_depth = 0;
    let mut enum_ctx = EnumParseContext::new();
    
    // Mode stacks
    let mut literal_mode = LiteralModeStack::new();
    let mut array_mode = ArrayModeStack::new();
    let mut match_mode = MatchModeStack::new();
    let mut use_import_mode = UseImportMode::new();
    
    // If expression assignment tracking
    let mut if_expr_assignment_depth: Option<usize> = None;
    
    // Multi-line function signature accumulation
    let mut multiline_fn_acc: Option<String> = None;
    let mut multiline_fn_leading_ws: String = String::new();
    
    // Expression continuation tracking
    let mut prev_line_was_continuation = false;
    let mut multiline_expr_depth: i32 = 0;
    
    for (line_num, line) in lines.iter().enumerate() {
        // Strip BOM
        let line = line.trim_start_matches('\u{FEFF}');
        
        let clean_line = strip_inline_comment(line);
        let trimmed = clean_line.trim();
        let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        
        // Update multiline expression depth
        let multiline_depth_before = multiline_expr_depth;
        update_multiline_depth(&mut multiline_expr_depth, trimmed);
        let inside_multiline_expr = multiline_depth_before > 0 && multiline_expr_depth > 0;
        
        // Look-ahead for method chain continuation
        let next_line_is_method_chain = lines.get(line_num + 1)
            .map(|next| strip_inline_comment(next).trim().starts_with('.'))
            .unwrap_or(false);
        
        // CRITICAL: Look-ahead for closing paren/bracket
        // If next line starts with ) or ] or }) etc., we're the last item in an expression
        let next_line_closes_expr = lines.get(line_num + 1)
            .map(|next| {
                let binding = strip_inline_comment(next);
                let t = binding.trim();
                t.starts_with(')') || t.starts_with(']') || t.starts_with("})") || t.starts_with(");") || t.starts_with("];")
            })
            .unwrap_or(false);
        
        // Handle multi-line function signature accumulation
        if let Some(ref mut acc) = multiline_fn_acc {
            acc.push(' ');
            acc.push_str(trimmed);
            
            let paren_opens = acc.matches('(').count();
            let paren_closes = acc.matches(')').count();
            
            if paren_opens == paren_closes && acc.ends_with('{') {
                let complete_sig = acc.clone();
                multiline_fn_acc = None;
                
                in_function_body = true;
                function_start_brace = brace_depth + 1;
                
                if let FunctionParseResult::RustSPlusSignature(ref sig) = parse_function_line(&complete_sig) {
                    current_fn_ctx.enter(sig, function_start_brace);
                }
                
                match parse_function_line(&complete_sig) {
                    FunctionParseResult::RustSPlusSignature(sig) => {
                        let rust_sig = signature_to_rust(&sig);
                        output_lines.push(format!("{}{}", multiline_fn_leading_ws, rust_sig));
                    }
                    FunctionParseResult::RustPassthrough => {
                        output_lines.push(format!("{}{}", multiline_fn_leading_ws, complete_sig));
                    }
                    FunctionParseResult::Error(e) => {
                        output_lines.push(format!("{}// COMPILE ERROR: {}", multiline_fn_leading_ws, e));
                        output_lines.push(format!("{}{}", multiline_fn_leading_ws, complete_sig));
                    }
                    FunctionParseResult::NotAFunction => {
                        output_lines.push(format!("{}{}", multiline_fn_leading_ws, complete_sig));
                    }
                }
                
                brace_depth += 1;
                continue;
            } else {
                continue;
            }
        }
        
        // Check for multi-line function signature start
        if (trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ")) && trimmed.contains('(') {
            let paren_opens = trimmed.matches('(').count();
            let paren_closes = trimmed.matches(')').count();
            
            if paren_opens > paren_closes {
                multiline_fn_acc = Some(trimmed.to_string());
                multiline_fn_leading_ws = leading_ws.clone();
                continue;
            }
        }
        
        // Track function context
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            in_function_body = true;
            function_start_brace = brace_depth + 1;
            
            if let FunctionParseResult::RustSPlusSignature(ref sig) = parse_function_line(trimmed) {
                current_fn_ctx.enter(sig, function_start_brace);
            }
        }
        
        // Calculate depths - CRITICAL: Must ignore braces inside string literals!
        let prev_depth = brace_depth;
        let (opens, closes) = count_braces_outside_strings(trimmed);
        brace_depth += opens;
        brace_depth = brace_depth.saturating_sub(closes);
        
        let prev_bracket_depth = bracket_depth;
        let (bracket_opens, bracket_closes) = count_brackets_outside_strings(trimmed);
        bracket_depth += bracket_opens;
        bracket_depth = bracket_depth.saturating_sub(bracket_closes);
        
        // Exit function context
        if in_function_body && brace_depth < function_start_brace && trimmed == "}" {
            in_function_body = false;
            current_fn_ctx.exit();
        }
        
        // Check for return detection
        let is_before_closing_brace = check_before_closing_brace(&lines, line_num);
        
        if trimmed.is_empty() {
            prev_line_was_continuation = false;
            output_lines.push(String::new());
            continue;
        }
        
        // USE IMPORT MODE
        if use_import_mode.is_active() && trimmed == "}" {
            if use_import_mode.should_exit(brace_depth) {
                use_import_mode.exit();
                output_lines.push(format!("{}}};", leading_ws));
                continue;
            }
        }
        
        if use_import_mode.is_active() {
            let transformed = transform_use_import_item(&clean_line);
            output_lines.push(transformed);
            continue;
        }
        
        if let Some(is_pub) = is_multiline_use_import_start(trimmed) {
            use_import_mode.enter(brace_depth, is_pub);
            output_lines.push(format!("{}{}", leading_ws, trimmed));
            continue;
        }
        
        // ARRAY MODE
        if array_mode.is_active() && trimmed.contains(']') {
            if array_mode.should_exit(bracket_depth) {
                if let Some(entry) = array_mode.exit() {
                    let transformed = transform_array_element(&clean_line);
                    let suffix = if entry.is_assignment { ";" } else { "" };
                    
                    let close_line = if transformed.trim() == "]" {
                        format!("{}]{}", leading_ws, suffix)
                    } else {
                        let without_bracket = transformed.trim().trim_end_matches(']').trim_end_matches(',');
                        format!("{}    {},\n{}]{}", leading_ws, without_bracket, leading_ws, suffix)
                    };
                    output_lines.push(close_line);
                    continue;
                }
            }
        }
        
        // ARRAY MODE - with nested literal support for multi-line elements
        if array_mode.is_active() {
            // CRITICAL FIX: If also in literal mode, let literal mode handle it
            // This allows multi-line struct/enum literals inside arrays to work
            if literal_mode.is_active() {
                // Fall through to literal mode handling below
            } else {
                // Check if this line starts a multi-line struct/enum literal
                // Pattern: `Enum::Variant {` or `StructName {` where { is not closed
                let starts_multiline_literal = if opens > closes {
                    // Has unclosed brace - could be struct or enum literal start
                    if trimmed.contains("::") {
                        // Potential enum variant: SyncStatus::SyncingHeaders {
                        detect_bare_enum_literal(trimmed).is_some()
                    } else {
                        // Potential struct literal: User {
                        detect_bare_struct_literal(trimmed, &struct_registry).is_some()
                    }
                } else {
                    false
                };
                
                if starts_multiline_literal {
                    // Transform the start line and enter literal mode
                    let transformed = transform_array_element(&clean_line);
                    output_lines.push(transformed);
                    
                    // Enter literal mode for the fields
                    let kind = if trimmed.contains("::") { 
                        LiteralKind::EnumVariant 
                    } else { 
                        LiteralKind::Struct 
                    };
                    // is_assignment=false because it's an array element, not assignment
                    literal_mode.enter(kind, prev_depth + opens, false);
                    continue;
                }
                
                // Regular array element (single-line)
                let transformed = transform_array_element(&clean_line);
                output_lines.push(transformed);
                continue;
            }
        }
        
        // LITERAL MODE CLOSING
        // Handle both "}" and "}," (user may or may not include comma)
        if literal_mode.is_active() && (trimmed == "}" || trimmed == "},") {
            if literal_mode.should_exit(brace_depth) {
                let was_assignment = literal_mode.current_is_assignment();
                literal_mode.exit();
                
                // CRITICAL FIX: When inside array, closing literal needs comma
                let suffix = if array_mode.is_active() {
                    ","  // Inside array - element needs comma
                } else if literal_mode.is_active() { 
                    ","  // Nested literal - needs comma
                } else if was_assignment {
                    ";"  // Assignment - needs semicolon
                } else {
                    ""   // Bare literal (return expression)
                };
                output_lines.push(format!("{}}}{}", leading_ws, suffix));
                continue;
            }
        }
        
        if literal_mode.is_active() {
            let transformed = transform_literal_field_with_ctx(&clean_line, Some(&current_fn_ctx));
            
            if trimmed.contains('{') && opens > closes {
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
        
        // MATCH MODE
        if match_mode.is_active() && trimmed == "}" {
            if match_mode.should_exit_arm(brace_depth) {
                let uses_parens = match_mode.arm_uses_parens();
                match_mode.exit_arm_body();
                output_lines.push(transform_arm_close_with_parens(&clean_line, uses_parens));
                continue;
            }
            if match_mode.should_exit_match(brace_depth) {
                let needs_semi = match_mode.current_is_assignment();
                match_mode.exit_match();
                let suffix = if needs_semi { ";" } else { "" };
                output_lines.push(format!("{}}}{}", leading_ws, suffix));
                continue;
            }
        }
        
        // Match arm pattern handling
        if match_mode.expecting_arm_pattern() && is_match_arm_pattern(trimmed) {
            if is_single_line_arm(trimmed) {
                let ret_type = current_fn_ctx.return_type.as_deref();
                let transformed = transform_single_line_arm(&clean_line, ret_type);
                output_lines.push(transformed);
                continue;
            }
            
            let arm_has_if_expr = detect_arm_has_if_expr(&lines, line_num, prev_depth + opens);
            
            if arm_has_if_expr {
                let pattern = extract_arm_pattern(trimmed);
                output_lines.push(format!("{}{} =>", leading_ws, pattern));
            } else {
                let transformed = transform_arm_pattern(&clean_line);
                output_lines.push(transformed);
            }
            
            match_mode.enter_arm_body(brace_depth, arm_has_if_expr);
            continue;
        }
        
        // Match expression start
        if is_match_start(trimmed) {
            let is_assignment = parse_control_flow_assignment(trimmed).is_some();
            let mut match_string_ctx = MatchStringContext::from_match_line(trimmed);
            
            // Look ahead for string patterns
            for future_line in lines.iter().skip(line_num + 1) {
                let ft = strip_inline_comment(future_line);
                let ft_trim = ft.trim();
                
                if ft_trim == "}" { break; }
                
                if pattern_is_string_literal(ft_trim) {
                    match_string_ctx.has_string_patterns = true;
                    break;
                }
                
                if ft_trim.starts_with('"') && ft_trim.contains('{') {
                    match_string_ctx.has_string_patterns = true;
                    break;
                }
            }
            
            let needs_as_str = match_string_ctx.needs_as_str();
            
            if let Some((var_name, match_expr)) = parse_control_flow_assignment(trimmed) {
                let is_param = current_fn_ctx.params.contains_key(&var_name);
                let is_decl = scope_analyzer.is_decl(line_num);
                let is_mutation = scope_analyzer.is_mut(line_num);
                let is_shadowing = tracker.is_shadowing(&var_name, line_num);
                let needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
                let needs_let = is_decl || (!is_mutation && !is_param) || is_shadowing;
                
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
                let transformed = if needs_as_str {
                    transform_match_for_string_patterns(trimmed, true)
                } else {
                    trimmed.to_string()
                };
                output_lines.push(format!("{}{}", leading_ws, transformed));
            }
            match_mode.enter_match(prev_depth, is_assignment);
            continue;
        }
        
        // IF EXPRESSION AS ASSIGNMENT
        if is_if_assignment(trimmed) {
            if let Some((var_name, if_expr)) = parse_control_flow_assignment(trimmed) {
                let is_param = current_fn_ctx.params.contains_key(&var_name);
                let is_decl = scope_analyzer.is_decl(line_num);
                let is_mutation = scope_analyzer.is_mut(line_num);
                let is_shadowing = tracker.is_shadowing(&var_name, line_num);
                let needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
                let needs_let = is_decl || (!is_mutation && !is_param) || is_shadowing;
                
                if needs_let {
                    let keyword = if needs_mut { "let mut" } else { "let" };
                    output_lines.push(format!("{}{} {} = ({}", leading_ws, keyword, var_name, if_expr));
                } else {
                    output_lines.push(format!("{}{} = ({}", leading_ws, var_name, if_expr));
                }
                if_expr_assignment_depth = Some(prev_depth);
                continue;
            }
        }
        
        // If expression assignment end
        if if_expr_assignment_depth.is_some() && trimmed == "}" {
            let start_depth = if_expr_assignment_depth.unwrap();
            let next_is_else = check_next_is_else(&lines, line_num);
            
            if brace_depth <= start_depth && !next_is_else {
                if_expr_assignment_depth = None;
                output_lines.push(format!("{}}}); ", leading_ws));
                continue;
            }
        }
        
        // Inside match arm body
        if match_mode.in_arm_body() {
            if let Some(result) = process_match_arm_body_line(
                &clean_line, trimmed, &leading_ws, line_num,
                &scope_analyzer, &tracker, &current_fn_ctx, &fn_registry,
                &struct_registry, &mut literal_mode, prev_depth, opens,
            ) {
                output_lines.push(result);
                continue;
            }
            
            // Bare struct/enum literal handling
            if let Some(struct_name) = detect_bare_struct_literal(trimmed, &struct_registry) {
                // CRITICAL FIX: Handle both `}` and `},` endings
                let is_complete_single_line = trimmed.ends_with('}') || 
                                              trimmed.ends_with("},") ||
                                              trimmed.ends_with("};");
                
                if is_complete_single_line && opens == closes {
                    let transformed = transform_bare_struct_literal(trimmed);
                    output_lines.push(format!("{}{}", leading_ws, transformed));
                    continue;
                }
                
                if opens > closes {
                    literal_mode.enter(LiteralKind::Struct, prev_depth + opens, false);
                    output_lines.push(format!("{}{} {{", leading_ws, struct_name));
                    continue;
                }
                
                let transformed = transform_bare_struct_literal(trimmed);
                output_lines.push(format!("{}{}", leading_ws, transformed));
                continue;
            }
            
            if let Some(enum_path) = detect_bare_enum_literal(trimmed) {
                // CRITICAL FIX: Handle both `}` and `},` endings
                let is_complete_single_line = trimmed.ends_with('}') || 
                                              trimmed.ends_with("},") ||
                                              trimmed.ends_with("};");
                
                if is_complete_single_line && opens == closes {
                    let transformed = transform_bare_struct_literal(trimmed);
                    output_lines.push(format!("{}{}", leading_ws, transformed));
                    continue;
                }
                
                if opens > closes {
                    literal_mode.enter(LiteralKind::EnumVariant, prev_depth + opens, false);
                    output_lines.push(format!("{}{} {{", leading_ws, enum_path));
                    continue;
                }
                
                let transformed = transform_bare_struct_literal(trimmed);
                output_lines.push(format!("{}{}", leading_ws, transformed));
                continue;
            }
            
            // Not an assignment - apply transformations
            let mut transformed = trimmed.to_string();
            
            // Handle bare mut
            if let Some(result) = handle_bare_mut_in_match(&clean_line, trimmed, &leading_ws, &current_fn_ctx, &fn_registry) {
                output_lines.push(result);
                continue;
            }
            
            if current_fn_ctx.is_inside() {
                transformed = transform_string_concat(&transformed, &current_fn_ctx);
                transformed = transform_call_args(&transformed, &fn_registry);
            }
            
            // CRITICAL FIX: Use should_be_tail_return instead of simple is_before_closing_brace
            // is_before_closing_brace only tells position, not whether it's actually a return expr
            // should_be_tail_return checks: function has return value + line is NOT mutating statement
            let is_tail = should_be_tail_return(&transformed, &current_fn_ctx, is_before_closing_brace);
            
            if is_tail {
                if let Some(ref ret_type) = current_fn_ctx.return_type {
                    if ret_type == "String" && control_flow::is_string_literal(&transformed) {
                        transformed = control_flow::transform_string_to_owned(&transformed);
                    }
                }
            }
            
            // Semicolon logic: suppress if:
            // 1. is_tail (return expression), OR
            // 2. inside multiline expr AND next line closes it (we're last arg in macro/function call)
            let suppress_semi = is_tail || (inside_multiline_expr && next_line_closes_expr);
            if needs_semicolon(&transformed) && !suppress_semi {
                output_lines.push(format!("{}{};", leading_ws, transformed));
            } else {
                output_lines.push(format!("{}{}", leading_ws, transformed));
            }
            continue;
        }
        
        // STRUCT DEFINITION
        if is_struct_definition(trimmed) && !in_struct_def {
            in_struct_def = true;
            struct_def_depth = brace_depth;
            
            if let Some(_struct_name) = parse_struct_header(trimmed) {
                inject_derive_clone(&mut output_lines, &leading_ws);
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
            let transformed = transform_struct_field_slice_to_vec(&transformed);
            // CRITICAL FIX: Transform generic brackets in struct field types
            // e.g., Option[Arc[T]] -> Option<Arc<T>>
            let transformed = helpers::transform_generic_brackets(&transformed);
            output_lines.push(transformed);
            continue;
        }
        
        // ENUM DEFINITION
        if is_enum_definition(trimmed) && !enum_ctx.in_enum_def {
            enum_ctx.enter_enum(brace_depth);
            
            if let Some(_enum_name) = parse_enum_header(trimmed) {
                inject_derive_clone(&mut output_lines, &leading_ws);
            }
            
            output_lines.push(format!("{}{}", leading_ws, trimmed));
            continue;
        }
        
        if enum_ctx.in_enum_def {
            if trimmed == "}" && enum_ctx.in_struct_variant {
                enum_ctx.exit_struct_variant();
                output_lines.push(format!("{}}},", leading_ws));
                continue;
            }
            
            if trimmed == "}" && brace_depth <= enum_ctx.start_depth {
                enum_ctx.exit_enum();
                output_lines.push(format!("{}}}", leading_ws));
                continue;
            }
            
            if trimmed.contains('{') && opens > closes {
                enum_ctx.enter_struct_variant();
            }
            
            let transformed = transform_enum_variant(&clean_line, enum_ctx.in_struct_variant);
            output_lines.push(transformed);
            continue;
        }
        
        // STRUCT LITERAL START
        if let Some((var_name, struct_name)) = detect_struct_literal_start(trimmed, &struct_registry) {
            // CRITICAL FIX: Check if var_name is a field access (e.g., self.field)
            // Field assignments should NOT get `let` prefix!
            let is_field = is_field_access(&var_name);
            let is_tuple = is_tuple_pattern(&var_name);
            
            let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
            let borrowed_mut = tracker.is_mut_borrowed(&var_name);
            let mutated_via_method = tracker.is_mutated_via_method(&var_name);
            let needs_mut = scope_needs_mut || borrowed_mut || mutated_via_method;
            
            // Determine if we need `let` keyword
            let needs_let = !is_field;
            let let_keyword = if !needs_let {
                ""
            } else if needs_mut {
                "let mut "
            } else {
                "let "
            };
            
            if trimmed.ends_with('}') {
                // Single-line struct literal
                if is_field {
                    // Field assignment - no let, transform fields
                    let transformed = transform_bare_struct_literal(trimmed);
                    output_lines.push(format!("{}{}", leading_ws, transformed));
                } else {
                    let transformed = transform_single_line_struct_literal(trimmed, &var_name);
                    let transformed = if needs_mut && needs_let {
                        transformed.replacen("let ", "let mut ", 1)
                    } else {
                        transformed
                    };
                    output_lines.push(format!("{}{}", leading_ws, transformed));
                }
                continue;
            }
            
            // CRITICAL FIX: Always mark as assignment (true) for semicolon handling,
            // regardless of needs_let. Field assignments (self.x = ...) need semicolons
            // even without `let` keyword.
            literal_mode.enter(LiteralKind::Struct, prev_depth + opens, true);
            output_lines.push(format!("{}{}{} = {} {{", leading_ws, let_keyword, var_name, struct_name));
            continue;
        }
        
        // ENUM STRUCT VARIANT LITERAL
        if let Some((var_name, enum_path)) = detect_enum_literal_start(trimmed) {
            // CRITICAL FIX: Check if var_name is a field access (e.g., self.status)
            // Field assignments should NOT get `let` prefix!
            let is_field = is_field_access(&var_name);
            let is_tuple = is_tuple_pattern(&var_name);
            
            let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
            let borrowed_mut = tracker.is_mut_borrowed(&var_name);
            let mutated_via_method = tracker.is_mutated_via_method(&var_name);
            let needs_mut = scope_needs_mut || borrowed_mut || mutated_via_method;
            
            // Determine if we need `let` keyword
            // - Field access: NO let (it's a mutation)
            // - Tuple pattern: YES let (it's a binding)  
            // - Simple identifier: YES let (it's a declaration)
            let needs_let = !is_field;
            let let_keyword = if !needs_let {
                ""
            } else if needs_mut {
                "let mut "
            } else {
                "let "
            };
            
            if trimmed.ends_with('}') {
                // Single-line enum literal
                if is_field {
                    // Field assignment - no let, transform fields
                    let transformed = transform_bare_struct_literal(trimmed);
                    output_lines.push(format!("{}{}", leading_ws, transformed));
                } else {
                    let transformed = transform_single_line_enum_literal(trimmed, &var_name, &enum_path);
                    let transformed = if needs_mut && needs_let {
                        transformed.replacen("let ", "let mut ", 1)
                    } else {
                        transformed
                    };
                    output_lines.push(format!("{}{}", leading_ws, transformed));
                }
                continue;
            }
            
            // CRITICAL FIX: Always mark as assignment (true) for semicolon handling,
            // regardless of needs_let. Field assignments (self.status = ...) need semicolons
            // even without `let` keyword.
            literal_mode.enter(LiteralKind::EnumVariant, prev_depth + opens, true);
            output_lines.push(format!("{}{}{} = {} {{", leading_ws, let_keyword, var_name, enum_path));
            continue;
        }
        
        // BARE STRUCT LITERAL
        if let Some(struct_name) = detect_bare_struct_literal(trimmed, &struct_registry) {
            // CRITICAL FIX: Check for COMPLETE single-line literals
            // Must handle both `}` and `},` endings (with trailing comma)
            let is_complete_single_line = trimmed.ends_with('}') || 
                                          trimmed.ends_with("},") ||
                                          trimmed.ends_with("};");
            
            if is_complete_single_line && opens == closes {
                let transformed = transform_bare_struct_literal(trimmed);
                output_lines.push(format!("{}{}", leading_ws, transformed));
                continue;
            }
            
            // Only enter literal mode if this is truly a multi-line start (opens > closes)
            if opens > closes {
                literal_mode.enter(LiteralKind::Struct, prev_depth + opens, false);
                output_lines.push(format!("{}{} {{", leading_ws, struct_name));
                continue;
            }
            
            // If opens == closes but not matching complete patterns,
            // just transform and output
            let transformed = transform_bare_struct_literal(trimmed);
            output_lines.push(format!("{}{}", leading_ws, transformed));
            continue;
        }
        
        // BARE ENUM STRUCT VARIANT LITERAL
        if let Some(enum_path) = detect_bare_enum_literal(trimmed) {
            // CRITICAL FIX: Check for COMPLETE single-line literals
            // Must handle both `}` and `},` endings (with trailing comma)
            let is_complete_single_line = trimmed.ends_with('}') || 
                                          trimmed.ends_with("},") ||
                                          trimmed.ends_with("};");
            
            if is_complete_single_line && opens == closes {
                let transformed = transform_bare_struct_literal(trimmed);
                output_lines.push(format!("{}{}", leading_ws, transformed));
                continue;
            }
            
            // Only enter literal mode if this is truly a multi-line start (opens > closes)
            if opens > closes {
                literal_mode.enter(LiteralKind::EnumVariant, prev_depth + opens, false);
                output_lines.push(format!("{}{} {{", leading_ws, enum_path));
                continue;
            }
            
            // If opens == closes but not matching the complete patterns, 
            // it might be invalid - just transform and output
            let transformed = transform_bare_struct_literal(trimmed);
            output_lines.push(format!("{}{}", leading_ws, transformed));
            continue;
        }
        
        // FUNCTION DEFINITION
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            match parse_function_line(trimmed) {
                FunctionParseResult::RustSPlusSignature(sig) => {
                    let rust_sig = signature_to_rust(&sig);
                    output_lines.push(format!("{}{}", leading_ws, rust_sig));
                    continue;
                }
                FunctionParseResult::RustPassthrough => {
                    let output = process_rust_passthrough_function(&clean_line, trimmed, &mut current_fn_ctx, function_start_brace);
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
        
        // CONST/STATIC DECLARATION
        // RustS+: `const NAME TYPE = VALUE` → Rust: `const NAME: TYPE = VALUE;`
        // RustS+: `static NAME TYPE = VALUE` → Rust: `static NAME: TYPE = VALUE;`
        // Must be handled BEFORE is_rust_native check
        if let Some(transformed) = transform_const_or_static(trimmed) {
            output_lines.push(format!("{}{}", leading_ws, transformed));
            continue;
        }
        
        // Effect statement skip
        if trimmed.starts_with("effect ") {
            continue;
        }
        
        // RUST NATIVE PASSTHROUGH
        let is_rust_native = is_rust_native_line(trimmed);
        
        if is_rust_native {
            let mut transformed = trimmed.to_string();
            if current_fn_ctx.is_inside() {
                transformed = transform_string_concat(&transformed, &current_fn_ctx);
                transformed = transform_call_args(&transformed, &fn_registry);
            }
            
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
        
        // ARRAY LITERAL START
        if let Some((var_name, var_type, after_bracket)) = detect_array_literal_start(trimmed) {
            let is_param = current_fn_ctx.params.contains_key(&var_name);
            let is_decl = scope_analyzer.is_decl(line_num);
            let is_mutation = scope_analyzer.is_mut(line_num);
            let is_shadowing = tracker.is_shadowing(&var_name, line_num);
            let borrowed_mut = tracker.is_mut_borrowed(&var_name);
            let mutated_via_method = tracker.is_mutated_via_method(&var_name);
            let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
            let needs_mut = borrowed_mut || mutated_via_method || scope_needs_mut;
            let needs_let = is_decl || (!is_mutation && !is_param) || is_shadowing;
            
            array_mode.enter(
                prev_bracket_depth + bracket_opens, 
                true,
                var_name.clone(),
                var_type.clone(),
                needs_let,
                needs_mut
            );
            
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
            
            // CRITICAL FIX: Detect if source uses vec![ and preserve it
            let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
            let rhs = parts.get(1).map(|s| s.trim()).unwrap_or("");
            let array_open = if rhs.starts_with("vec![") {
                "vec!["
            } else if rhs.starts_with("Vec::from([") {
                "Vec::from(["
            } else {
                "["
            };
            
            let after = after_bracket.trim();
            if after.is_empty() {
                output_lines.push(format!("{}{}{}{} = {}", leading_ws, let_keyword, var_name, type_annotation, array_open));
            } else {
                let transformed_first = transform_array_element(&format!("    {}", after));
                output_lines.push(format!("{}{}{}{} = {}", leading_ws, let_keyword, var_name, type_annotation, array_open));
                if !transformed_first.trim().is_empty() {
                    output_lines.push(transformed_first);
                }
            }
            continue;
        }
        
        // TUPLE DESTRUCTURING ASSIGNMENT
        // Pattern: `(a, b) = value` → `let (a, b) = value;`
        // Must be handled BEFORE regular assignment parser
        if trimmed.starts_with('(') && trimmed.contains(')') && trimmed.contains('=') {
            // Find the closing paren and check if = follows
            if let Some(paren_close) = trimmed.find(')') {
                let after_paren = trimmed[paren_close + 1..].trim();
                if after_paren.starts_with('=') && !after_paren.starts_with("==") && !after_paren.starts_with("=>") {
                    let tuple_part = &trimmed[..=paren_close];
                    let value_part = after_paren[1..].trim().trim_end_matches(';');
                    
                    // Verify it's a valid tuple pattern
                    if is_tuple_pattern(tuple_part) {
                        // Transform: add `let` and semicolon
                        let mut expanded_value = expand_value(value_part, None);
                        expanded_value = transform_array_access_clone(&expanded_value);
                        if current_fn_ctx.is_inside() {
                            expanded_value = transform_string_concat(&expanded_value, &current_fn_ctx);
                        }
                        expanded_value = transform_call_args(&expanded_value, &fn_registry);
                        
                        output_lines.push(format!("{}let {} = {};", leading_ws, tuple_part, expanded_value));
                        continue;
                    }
                }
            }
        }
        
        // RUSTS+ ASSIGNMENT
        if let Some((var_name, var_type, value, is_outer, is_explicit_mut)) = parse_rusts_assignment_ext(&clean_line) {
            let result = process_assignment(
                &var_name, var_type.as_deref(), &value, is_outer, is_explicit_mut,
                line_num, &leading_ws, &scope_analyzer, &tracker, &current_fn_ctx, &fn_registry,
                inside_multiline_expr, next_line_is_method_chain, next_line_closes_expr, &mut prev_line_was_continuation,
            );
            output_lines.push(result);
        } else {
            // Not an assignment
            let result = process_non_assignment(
                trimmed, &leading_ws, line_num, &current_fn_ctx, &fn_registry,
                is_before_closing_brace, inside_multiline_expr, next_line_is_method_chain, next_line_closes_expr,
                &mut prev_line_was_continuation,
            );
            output_lines.push(result);
        }
    }
    
    // Apply post-processing and return
    let result = apply_postprocessing(output_lines);
    
    // Rust sanity check (non-test only)
    #[cfg(not(test))]
    {
        let sanity = rust_sanity::check_rust_output(&result);
        if !sanity.is_valid {
            eprintln!("{}", rust_sanity::format_internal_error(&sanity));
        }
    }
    
    result
}

//===========================================================================
// HELPER FUNCTIONS
//===========================================================================

/// Count opening and closing braces OUTSIDE of string literals
/// This is CRITICAL to avoid counting format placeholders like {} in "hello {} world"
fn count_braces_outside_strings(s: &str) -> (usize, usize) {
    let mut opens = 0;
    let mut closes = 0;
    let mut in_string = false;
    let mut escape_next = false;
    
    for c in s.chars() {
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
        
        if !in_string {
            match c {
                '{' => opens += 1,
                '}' => closes += 1,
                _ => {}
            }
        }
    }
    
    (opens, closes)
}

/// Count opening and closing brackets OUTSIDE of string literals
fn count_brackets_outside_strings(s: &str) -> (usize, usize) {
    let mut opens = 0;
    let mut closes = 0;
    let mut in_string = false;
    let mut escape_next = false;
    
    for c in s.chars() {
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
        
        if !in_string {
            match c {
                '[' => opens += 1,
                ']' => closes += 1,
                _ => {}
            }
        }
    }
    
    (opens, closes)
}

fn update_multiline_depth(depth: &mut i32, trimmed: &str) {
    let mut in_string = false;
    let mut escape_next = false;
    for c in trimmed.chars() {
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
        if !in_string {
            match c {
                '(' | '[' => *depth += 1,
                ')' | ']' => *depth -= 1,
                _ => {}
            }
        }
    }
    if *depth < 0 {
        *depth = 0;
    }
}

/// Transform RustS+ const/static declarations to Rust syntax.
/// 
/// RustS+ syntax: `const NAME TYPE = VALUE` (no colon between name and type)
/// Rust syntax:   `const NAME: TYPE = VALUE;` (colon required, semicolon at end)
/// 
/// Handles:
/// - `const NAME TYPE = VALUE` → `const NAME: TYPE = VALUE;`
/// - `static NAME TYPE = VALUE` → `static NAME: TYPE = VALUE;`
/// - `pub const NAME TYPE = VALUE` → `pub const NAME: TYPE = VALUE;`
/// - `pub static NAME TYPE = VALUE` → `pub static NAME: TYPE = VALUE;`
/// - `pub static mut NAME TYPE = VALUE` → `pub static mut NAME: TYPE = VALUE;`
/// 
/// Returns None if:
/// - Not a const/static declaration
/// - Already in Rust syntax (has colon before `=`)
fn transform_const_or_static(trimmed: &str) -> Option<String> {
    // Quick check: must contain const or static
    if !trimmed.contains("const ") && !trimmed.contains("static ") {
        return None;
    }
    
    // Must contain `=` for value assignment
    if !trimmed.contains('=') {
        return None;
    }
    
    // Parse the declaration
    // Pattern: [pub] [static [mut] | const] NAME TYPE = VALUE
    
    let mut rest = trimmed;
    let mut prefix = String::new();
    
    // Check for `pub`
    if rest.starts_with("pub ") {
        prefix.push_str("pub ");
        rest = rest.strip_prefix("pub ").unwrap().trim();
    }
    
    // Check for `const` or `static`
    let keyword = if rest.starts_with("const ") {
        rest = rest.strip_prefix("const ").unwrap().trim();
        "const"
    } else if rest.starts_with("static mut ") {
        rest = rest.strip_prefix("static mut ").unwrap().trim();
        "static mut"
    } else if rest.starts_with("static ") {
        rest = rest.strip_prefix("static ").unwrap().trim();
        "static"
    } else {
        return None;
    };
    
    // Now rest should be: `NAME TYPE = VALUE` or `NAME: TYPE = VALUE`
    
    // Find the `=` sign
    let eq_pos = rest.find('=')?;
    let before_eq = rest[..eq_pos].trim();
    let after_eq = rest[eq_pos + 1..].trim();
    
    // Check if already in Rust syntax (has colon before =)
    // This includes patterns like `NAME: TYPE` or `NAME: &'static str`
    if before_eq.contains(':') {
        // Already Rust syntax - just ensure semicolon at end
        let trimmed_input = trimmed.trim_end_matches(';');
        return Some(format!("{};", trimmed_input));
    }
    
    // RustS+ syntax: NAME TYPE (space-separated, no colon)
    // Find the NAME (first identifier) and TYPE (rest before =)
    let parts: Vec<&str> = before_eq.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    
    let name = parts[0];
    // Type is everything after the name
    let type_str = parts[1..].join(" ");
    
    // Validate name is a valid identifier
    let first_char = name.chars().next()?;
    if name.is_empty() || (!first_char.is_alphabetic() && first_char != '_') {
        return None;
    }
    
    // Transform type (Vec[T] → Vec<T>)
    let transformed_type = helpers::transform_generic_brackets(&type_str);
    
    // Value without trailing semicolon (we'll add our own)
    let value = after_eq.trim_end_matches(';');
    
    Some(format!("{}{} {}: {} = {};", prefix, keyword, name, transformed_type, value))
}

fn check_before_closing_brace(lines: &[&str], line_num: usize) -> bool {
    for future_line in lines.iter().skip(line_num + 1) {
        let ft = strip_inline_comment(future_line);
        let ft = ft.trim();
        if !ft.is_empty() {
            return ft == "}" || ft.starts_with("}");
        }
    }
    false
}

fn check_next_is_else(lines: &[&str], line_num: usize) -> bool {
    for future_line in lines.iter().skip(line_num + 1) {
        let ft = strip_inline_comment(future_line).trim().to_string();
        if ft.is_empty() { continue; }
        return ft.starts_with("else") || ft.starts_with("} else");
    }
    false
}

fn detect_arm_has_if_expr(lines: &[&str], line_num: usize, start_depth: usize) -> bool {
    let mut temp_depth = start_depth;
    let mut found_first = false;
    
    for future_line in lines.iter().skip(line_num + 1) {
        let ft = strip_inline_comment(future_line);
        let ft_trim = ft.trim();
        
        let (ft_opens, ft_closes) = count_braces_outside_strings(ft_trim);
        
        temp_depth += ft_opens;
        temp_depth = temp_depth.saturating_sub(ft_closes);
        
        if ft_trim == "}" && temp_depth < start_depth {
            break;
        }
        
        if !found_first && !ft_trim.is_empty() {
            found_first = true;
            if ft_trim.starts_with("if ") && !ft_trim.contains("let ") {
                return true;
            }
        }
    }
    false
}

fn inject_derive_clone(output_lines: &mut Vec<String>, leading_ws: &str) {
    let prev_line = output_lines.last().map(|s| s.trim().to_string());
    if let Some(ref prev) = prev_line {
        if prev.starts_with("#[derive(") && prev.ends_with(")]") {
            if !prev.contains("Clone") {
                let last_idx = output_lines.len() - 1;
                let existing = output_lines[last_idx].clone();
                let merged = existing.replace(")]", ", Clone)]");
                output_lines[last_idx] = merged;
            }
        } else {
            output_lines.push(format!("{}#[derive(Clone)]", leading_ws));
        }
    } else {
        output_lines.push(format!("{}#[derive(Clone)]", leading_ws));
    }
}

fn is_rust_native_line(trimmed: &str) -> bool {
    // NOTE: const and static are NOT included here because RustS+ uses different syntax:
    // RustS+: `const NAME TYPE = VALUE` (no colon)
    // Rust:   `const NAME: TYPE = VALUE;` (has colon)
    // These are handled separately by transform_const_or_static()
    trimmed.starts_with("let ")
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
        || trimmed.starts_with("pub ")
}

fn process_rust_passthrough_function(
    clean_line: &str,
    trimmed: &str,
    current_fn_ctx: &mut CurrentFunctionContext,
    function_start_brace: usize,
) -> String {
    let mut output = clean_line.to_string();
    
    // Strip effects annotation if present
    if output.contains("effects(") {
        if let Some(effects_start) = output.find("effects(") {
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
    
    // Extract return type
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
    
    output
}

fn process_match_arm_body_line(
    clean_line: &str,
    trimmed: &str,
    leading_ws: &str,
    line_num: usize,
    scope_analyzer: &ScopeAnalyzer,
    tracker: &VariableTracker,
    current_fn_ctx: &CurrentFunctionContext,
    fn_registry: &function::FunctionRegistry,
    struct_registry: &struct_def::StructRegistry,
    literal_mode: &mut LiteralModeStack,
    prev_depth: usize,
    opens: usize,
) -> Option<String> {
    if let Some((var_name, var_type, value, is_outer, is_explicit_mut)) = parse_rusts_assignment_ext(clean_line) {
        // CRITICAL FIX: Check if var_name is a field access
        // Field assignments should NOT get `let` prefix!
        let is_field = is_field_access(&var_name);
        
        let is_decl = scope_analyzer.is_decl(line_num);
        let is_mutation = scope_analyzer.is_mut(line_num);
        let borrowed_mut = tracker.is_mut_borrowed(&var_name);
        let mutated_via_method = tracker.is_mutated_via_method(&var_name);
        let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
        let needs_mut = is_explicit_mut || borrowed_mut || mutated_via_method || scope_needs_mut;
        
        let mut expanded_value = expand_value(&value, var_type.as_deref());
        expanded_value = transform_array_access_clone(&expanded_value);
        
        if current_fn_ctx.is_inside() {
            expanded_value = transform_string_concat(&expanded_value, current_fn_ctx);
        }
        expanded_value = transform_call_args(&expanded_value, fn_registry);
        
        let is_param = current_fn_ctx.params.contains_key(&var_name);
        let is_shadowing = tracker.is_shadowing(&var_name, line_num);
        // Field access never gets `let`
        let should_have_let = !is_field && (is_decl || (!is_mutation && !is_param) || is_shadowing);
        
        let is_struct_literal_start = expanded_value.trim().ends_with('{');
        let semi = if is_struct_literal_start { "" } else { ";" };
        
        if is_struct_literal_start {
            literal_mode.enter(LiteralKind::Struct, prev_depth + opens, should_have_let);
        }
        
        let result = if is_outer || is_field {
            // Field access or outer - no let
            format!("{}{} = {}{}", leading_ws, var_name, expanded_value, semi)
        } else if is_explicit_mut {
            let type_annotation = var_type.as_ref().map(|t| format!(": {}", t)).unwrap_or_default();
            format!("{}let mut {}{} = {}{}", leading_ws, var_name, type_annotation, expanded_value, semi)
        } else if should_have_let {
            let let_keyword = if needs_mut { "let mut" } else { "let" };
            let type_annotation = var_type.as_ref().map(|t| format!(": {}", t)).unwrap_or_default();
            format!("{}{} {}{} = {}{}", leading_ws, let_keyword, var_name, type_annotation, expanded_value, semi)
        } else if is_mutation && is_param {
            format!("{}{} = {}{}", leading_ws, var_name, expanded_value, semi)
        } else {
            let let_keyword = if needs_mut { "let mut" } else { "let" };
            let type_annotation = var_type.as_ref().map(|t| format!(": {}", t)).unwrap_or_default();
            format!("{}{} {}{} = {}{}", leading_ws, let_keyword, var_name, type_annotation, expanded_value, semi)
        };
        
        return Some(result);
    }
    
    None
}

fn handle_bare_mut_in_match(
    _clean_line: &str,
    trimmed: &str,
    leading_ws: &str,
    current_fn_ctx: &CurrentFunctionContext,
    fn_registry: &function::FunctionRegistry,
) -> Option<String> {
    if trimmed.starts_with("mut ") && trimmed.contains('=') && !trimmed.contains("==") {
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
                expanded_value = transform_string_concat(&expanded_value, current_fn_ctx);
            }
            expanded_value = transform_call_args(&expanded_value, fn_registry);
            
            return Some(format!("{}let mut {}{} = {};", leading_ws, var_name, type_annotation, expanded_value));
        }
    }
    None
}

fn process_assignment(
    var_name: &str,
    var_type: Option<&str>,
    value: &str,
    is_outer: bool,
    is_explicit_mut: bool,
    line_num: usize,
    leading_ws: &str,
    scope_analyzer: &ScopeAnalyzer,
    tracker: &VariableTracker,
    current_fn_ctx: &CurrentFunctionContext,
    fn_registry: &function::FunctionRegistry,
    inside_multiline_expr: bool,
    next_line_is_method_chain: bool,
    next_line_closes_expr: bool,
    prev_line_was_continuation: &mut bool,
) -> String {
    let is_decl = scope_analyzer.is_decl(line_num);
    let is_mutation = scope_analyzer.is_mut(line_num);
    let borrowed_mut = tracker.is_mut_borrowed(var_name);
    let mutated_via_method = tracker.is_mutated_via_method(var_name);
    let scope_needs_mut = scope_analyzer.needs_mut(var_name, line_num);
    let needs_mut = is_explicit_mut || borrowed_mut || mutated_via_method || scope_needs_mut;
    
    let mut expanded_value = expand_value(value, var_type);
    expanded_value = transform_array_access_clone(&expanded_value);
    
    if current_fn_ctx.is_inside() {
        expanded_value = transform_string_concat(&expanded_value, current_fn_ctx);
    }
    expanded_value = transform_call_args(&expanded_value, fn_registry);
    expanded_value = transform_enum_struct_init(&expanded_value);
    
    let is_param = current_fn_ctx.params.contains_key(var_name);
    let is_shadowing = tracker.is_shadowing(var_name, line_num);
    let should_have_let = is_decl || (!is_mutation && !is_param) || is_shadowing;
    
    // CRITICAL FIX: Semicolon logic
    // 1. If value ends with continuation → no semicolon (expression continues)
    // 2. If next line is method chain → no semicolon (chained call)
    // 3. If inside multiline expr AND next line closes it → no semicolon (we're last arg)
    // 4. Otherwise → add semicolon
    let suppress_semi = ends_with_continuation_operator(&expanded_value)
        || next_line_is_method_chain
        || (inside_multiline_expr && next_line_closes_expr);
    let semi = if suppress_semi { "" } else { ";" };
    *prev_line_was_continuation = ends_with_continuation_operator(&expanded_value);
    
    let type_annotation = var_type.map(|t| format!(": {}", t)).unwrap_or_default();
    
    if is_outer {
        format!("{}{} = {}{}", leading_ws, var_name, expanded_value, semi)
    } else if is_explicit_mut {
        format!("{}let mut {}{} = {}{}", leading_ws, var_name, type_annotation, expanded_value, semi)
    } else if should_have_let {
        let let_keyword = if needs_mut { "let mut" } else { "let" };
        format!("{}{} {}{} = {}{}", leading_ws, let_keyword, var_name, type_annotation, expanded_value, semi)
    } else if is_mutation && is_param {
        format!("{}{} = {}{}", leading_ws, var_name, expanded_value, semi)
    } else {
        let let_keyword = if needs_mut { "let mut" } else { "let" };
        format!("{}{} {}{} = {}{}", leading_ws, let_keyword, var_name, type_annotation, expanded_value, semi)
    }
}

fn process_non_assignment(
    trimmed: &str,
    leading_ws: &str,
    _line_num: usize,
    current_fn_ctx: &CurrentFunctionContext,
    fn_registry: &function::FunctionRegistry,
    is_before_closing_brace: bool,
    inside_multiline_expr: bool,
    next_line_is_method_chain: bool,
    next_line_closes_expr: bool,
    prev_line_was_continuation: &mut bool,
) -> String {
    let mut transformed = trimmed.to_string();
    
    // Handle bare mut
    if trimmed.starts_with("mut ") && trimmed.contains('=') && !trimmed.contains("==") {
        let rest = trimmed.strip_prefix("mut ").unwrap().trim();
        if let Some(eq_pos) = rest.find('=') {
            let var_part = rest[..eq_pos].trim();
            let val_part = rest[eq_pos + 1..].trim().trim_end_matches(';');
            
            let (var_name, type_annotation) = parse_var_type_annotation(var_part);
            
            let mut expanded_value = expand_value(val_part, None);
            expanded_value = transform_array_access_clone(&expanded_value);
            if current_fn_ctx.is_inside() {
                expanded_value = transform_string_concat(&expanded_value, current_fn_ctx);
            }
            expanded_value = transform_call_args(&expanded_value, fn_registry);
            
            return format!("{}let mut {}{} = {};", leading_ws, var_name, type_annotation, expanded_value);
        }
    }
    
    if current_fn_ctx.is_inside() {
        transformed = transform_string_concat(&transformed, current_fn_ctx);
    }
    transformed = transform_call_args(&transformed, fn_registry);
    transformed = transform_enum_struct_init(&transformed);
    
    let is_return_expr = should_be_tail_return(&transformed, current_fn_ctx, is_before_closing_brace);
    
    if is_return_expr {
        if let Some(ref ret_type) = current_fn_ctx.return_type {
            if ret_type == "String" && is_string_literal(&transformed) {
                let inner = &transformed[1..transformed.len()-1];
                transformed = format!("String::from(\"{}\")", inner);
            }
        }
    }
    
    let this_line_ends_with_continuation = ends_with_continuation_operator(&transformed);
    *prev_line_was_continuation = this_line_ends_with_continuation;
    
    // CRITICAL FIX: Semicolon logic for non-assignment expressions
    // 1. If ends with continuation → no semicolon
    // 2. If return expression → no semicolon
    // 3. If next line is method chain → no semicolon
    // 4. If inside multiline expr AND next line closes it → no semicolon (last arg)
    // 5. Otherwise → add semicolon if needed
    let suppress_semi = this_line_ends_with_continuation
        || is_return_expr
        || next_line_is_method_chain
        || (inside_multiline_expr && next_line_closes_expr);
    
    let should_add_semi = !suppress_semi && needs_semicolon(&transformed);
    
    if should_add_semi {
        format!("{}{};", leading_ws, transformed)
    } else {
        format!("{}{}", leading_ws, transformed)
    }
}

fn parse_var_type_annotation(var_part: &str) -> (&str, String) {
    if var_part.contains(' ') {
        let space_pos = var_part.find(' ').unwrap();
        let vname = var_part[..space_pos].trim();
        let vtype = var_part[space_pos + 1..].trim();
        
        let vname_valid = !vname.is_empty() 
            && vname.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
            && vname.chars().all(|c| c.is_alphanumeric() || c == '_');
        
        let vtype_valid = !vtype.is_empty() && (
            vtype.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
            || vtype.starts_with("Vec[") || vtype.starts_with("Vec<")
            || vtype.starts_with("Option[") || vtype.starts_with("Option<")
            || vtype.starts_with("Result[") || vtype.starts_with("Result<")
            || vtype.starts_with("HashMap[") || vtype.starts_with("HashMap<")
            || vtype.starts_with("HashSet[") || vtype.starts_with("HashSet<")
            || vtype.starts_with('&')
        );
        
        if vname_valid && vtype_valid {
            return (vname, format!(": {}", vtype));
        }
    } else if var_part.contains(':') {
        let parts: Vec<&str> = var_part.splitn(2, ':').collect();
        if parts.len() == 2 {
            return (parts[0].trim(), format!(": {}", parts[1].trim()));
        }
    }
    
    (var_part, String::new())
}