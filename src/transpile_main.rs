//! Main Transpilation Loop
//!
//! Orchestrates the line-by-line transpilation of RustS+ to Rust.
//! This module coordinates all the lowering and translation modules.

use crate::variable::{VariableTracker, parse_rusts_assignment_ext};
use crate::scope::ScopeAnalyzer;
use crate::function::{
    parse_function_line, CurrentFunctionContext, FunctionParseResult,
};
use crate::enum_def::EnumParseContext;
use crate::modes::{LiteralModeStack, ArrayModeStack, UseImportMode};
use crate::control_flow::MatchModeStack;
use crate::hex_normalizer::normalize_hex_literals;
use crate::helpers::{strip_inline_comment, transform_generic_brackets};
use crate::first_pass::run_first_pass;
use crate::postprocess_output::apply_postprocessing;
use crate::rust_sanity;

// Import lowering modules
use crate::depth_tracking_lowering::{
    count_braces_outside_strings, count_brackets_outside_strings, update_multiline_depth,
};
use crate::lookahead_lowering::{
    check_before_closing_brace, check_next_line_is_where,
    check_next_line_starts_with_pipe, check_next_line_is_method_chain,
    check_next_line_closes_expr,
};
use crate::multiline_fn_lowering::{is_multiline_fn_start, process_multiline_fn_signature, MultilineFnResult};
use crate::multiline_assign_lowering::{
    is_multiline_assign_start, is_multiline_assign_complete, process_complete_multiline_assign,
};
use crate::use_import_lowering::{process_use_import_line, UseImportResult};
use crate::array_mode_lowering::{process_array_mode_line, ArrayModeResult};
use crate::literal_mode_lowering::{process_literal_mode_line, LiteralModeResult};
use crate::match_mode_lowering::{process_match_mode_line, MatchModeResult};

// Import translation modules
use crate::struct_def_translate::{process_struct_def_line, StructDefResult};
use crate::enum_def_translate::{process_enum_def_line, EnumDefResult};
use crate::literal_start_translate::{
    process_struct_literal_start, process_enum_literal_start,
    process_literal_in_call, process_bare_struct_literal, process_bare_enum_literal,
    LiteralStartResult,
};
use crate::function_def_translate::{process_function_def, FunctionDefResult};
use crate::const_static_translate::transform_const_or_static;
use crate::native_passthrough_translate::{is_rust_native_line, process_native_line};
use crate::array_literal_translate::{process_array_literal_start, ArrayLiteralResult};
use crate::expression_translate::{process_non_assignment, process_tuple_destructuring};
use crate::assignment_translate::process_assignment;
use crate::macro_translate::transform_macros_to_correct_syntax;

// Import for match/if handling
use crate::control_flow::{
    is_match_start, is_if_assignment, parse_control_flow_assignment,
    MatchStringContext, transform_match_for_string_patterns, pattern_is_string_literal,
};
use crate::assignment_translate::parse_var_type_annotation;

/// Main entry point for RustS+ to Rust transpilation
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
    let _enum_registry = first_pass_result.enum_registry;
    
    // Scan all lines for mutating method calls
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
    
    // Multi-line accumulation
    let mut multiline_fn_acc: Option<String> = None;
    let mut multiline_fn_leading_ws: String = String::new();
    let mut multiline_assign_acc: Option<String> = None;
    let mut multiline_assign_leading_ws: String = String::new();
    
    // Expression continuation tracking
    let mut prev_line_was_continuation = false;
    let mut multiline_expr_depth: i32 = 0;
    
    for (line_num, line) in lines.iter().enumerate() {
        let line = line.trim_start_matches('\u{FEFF}');
        
        let clean_line = strip_inline_comment(line);
        let trimmed = clean_line.trim();
        let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        
        // Update multiline expression depth
        let multiline_depth_before = multiline_expr_depth;
        update_multiline_depth(&mut multiline_expr_depth, trimmed);
        let inside_multiline_expr = multiline_depth_before > 0 && multiline_expr_depth > 0;
        
        // Look-ahead computations
        let next_line_is_method_chain = check_next_line_is_method_chain(&lines, line_num);
        let next_line_closes_expr = check_next_line_closes_expr(&lines, line_num);
        let next_line_starts_with_pipe = check_next_line_starts_with_pipe(&lines, line_num);
        
        // Handle multi-line function signature accumulation
        if let Some(ref mut acc) = multiline_fn_acc {
            acc.push(' ');
            acc.push_str(trimmed);
            
            match process_multiline_fn_signature(
                acc, &lines, line_num, &multiline_fn_leading_ws,
                &mut current_fn_ctx, brace_depth,
            ) {
                MultilineFnResult::Continue => continue,
                MultilineFnResult::Complete { output, has_body } => {
                    multiline_fn_acc = None;
                    output_lines.push(output);
                    if has_body {
                        in_function_body = true;
                        function_start_brace = brace_depth + 1;
                        brace_depth += 1;
                    }
                    continue;
                }
            }
        }
        
        // Check for multi-line function signature start
        if is_multiline_fn_start(trimmed) {
            multiline_fn_acc = Some(trimmed.to_string());
            multiline_fn_leading_ws = leading_ws.clone();
            continue;
        }
        
        // Handle multi-line assignment accumulation
        if let Some(ref mut acc) = multiline_assign_acc {
            acc.push(' ');
            acc.push_str(trimmed);
            
            if is_multiline_assign_complete(acc) {
                let complete = acc.clone();
                let ws = multiline_assign_leading_ws.clone();
                multiline_assign_acc = None;
                
                let result = process_complete_multiline_assign(
                    &complete, &ws, line_num, &scope_analyzer, &tracker,
                    &current_fn_ctx, &fn_registry, inside_multiline_expr,
                    next_line_is_method_chain, next_line_closes_expr,
                    &mut prev_line_was_continuation,
                );
                output_lines.push(result);
                continue;
            } else {
                continue;
            }
        }
        
        // Check for multi-line assignment start
        if is_multiline_assign_start(trimmed) {
            multiline_assign_acc = Some(trimmed.to_string());
            multiline_assign_leading_ws = leading_ws.clone();
            continue;
        }
        
        // Track function context
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            in_function_body = true;
            function_start_brace = brace_depth + 1;
            if let FunctionParseResult::RustSPlusSignature(ref sig) = parse_function_line(trimmed) {
                current_fn_ctx.enter(sig, function_start_brace);
            }
        }
        
        // Calculate depths
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
        
        let is_before_closing_brace = check_before_closing_brace(&lines, line_num);
        
        // Empty line
        if trimmed.is_empty() {
            prev_line_was_continuation = false;
            output_lines.push(String::new());
            continue;
        }
        
        // Use import mode
        match process_use_import_line(trimmed, &clean_line, &leading_ws, brace_depth, &mut use_import_mode) {
            UseImportResult::Handled(s) => { output_lines.push(s); continue; }
            UseImportResult::NotHandled => {}
        }
        
        // Array mode
        match process_array_mode_line(
            trimmed, &clean_line, &leading_ws, bracket_depth, opens, closes, prev_depth,
            &mut array_mode, &mut literal_mode, &struct_registry,
        ) {
            ArrayModeResult::Handled(s) => { output_lines.push(s); continue; }
            ArrayModeResult::FallThroughToLiteral => {} // Continue to literal mode
            ArrayModeResult::NotHandled => {}
        }
        
        // Literal mode
        match process_literal_mode_line(
            trimmed, &clean_line, &leading_ws, brace_depth, opens, closes, prev_depth,
            &mut literal_mode, &array_mode, Some(&current_fn_ctx),
        ) {
            LiteralModeResult::Handled(s) => { output_lines.push(s); continue; }
            LiteralModeResult::NotHandled => {}
        }
        
        // Match mode
        match process_match_mode_line(
            line, trimmed, &clean_line, &leading_ws, &lines, line_num,
            brace_depth, prev_depth, opens, next_line_starts_with_pipe,
            &current_fn_ctx, &mut match_mode,
        ) {
            MatchModeResult::Handled(s) => { output_lines.push(s); continue; }
            MatchModeResult::ProcessAsArmBody => {
                // Process as match arm body (handled below in assignment/expression)
            }
            MatchModeResult::NotHandled => {}
        }
        
        // Match expression start
        if is_match_start(trimmed) {
            let output = process_match_start(
                trimmed, &leading_ws, &lines, line_num,
                &scope_analyzer, &tracker, &current_fn_ctx, &mut match_mode, prev_depth,
            );
            output_lines.push(output);
            continue;
        }
        
        // If expression assignment
        if is_if_assignment(trimmed) {
            if let Some(output) = process_if_assignment(
                trimmed, &leading_ws, line_num,
                &scope_analyzer, &tracker, &current_fn_ctx, prev_depth,
                &mut if_expr_assignment_depth,
            ) {
                output_lines.push(output);
                continue;
            }
        }
        
        // If expression assignment end
        if if_expr_assignment_depth.is_some() && trimmed == "}" {
            let start_depth = if_expr_assignment_depth.unwrap();
            let next_is_else = crate::lookahead_lowering::check_next_is_else(&lines, line_num);
            if brace_depth <= start_depth && !next_is_else {
                if_expr_assignment_depth = None;
                output_lines.push(format!("{}}}); ", leading_ws));
                continue;
            }
        }
        
        // Struct definition
        match process_struct_def_line(
            trimmed, &clean_line, &leading_ws, brace_depth,
            &mut in_struct_def, &mut struct_def_depth,
        ) {
            StructDefResult::Started(s) | StructDefResult::Closed(s) | StructDefResult::Field(s) => {
                output_lines.push(s);
                continue;
            }
            StructDefResult::NotStructDef => {}
        }
        
        // Enum definition
        match process_enum_def_line(
            trimmed, &clean_line, &leading_ws, brace_depth, opens, closes, &mut enum_ctx,
        ) {
            EnumDefResult::Started(s) | EnumDefResult::ClosedStructVariant(s) 
            | EnumDefResult::ClosedEnum(s) | EnumDefResult::Variant(s) => {
                output_lines.push(s);
                continue;
            }
            EnumDefResult::NotEnumDef => {}
        }
        
        // Struct literal start
        match process_struct_literal_start(
            trimmed, &leading_ws, line_num, opens, prev_depth,
            &scope_analyzer, &tracker, &struct_registry, &mut literal_mode,
        ) {
            LiteralStartResult::Handled(s) => { output_lines.push(s); continue; }
            LiteralStartResult::NotLiteralStart => {}
        }
        
        // Enum literal start
        match process_enum_literal_start(
            trimmed, &leading_ws, line_num, opens, prev_depth,
            &scope_analyzer, &tracker, &mut literal_mode,
        ) {
            LiteralStartResult::Handled(s) => { output_lines.push(s); continue; }
            LiteralStartResult::NotLiteralStart => {}
        }
        
        // Literal in function call
        match process_literal_in_call(
            trimmed, &leading_ws, opens, closes, prev_depth,
            &struct_registry, &mut literal_mode,
        ) {
            LiteralStartResult::Handled(s) => { output_lines.push(s); continue; }
            LiteralStartResult::NotLiteralStart => {}
        }
        
        // Bare struct literal
        match process_bare_struct_literal(
            trimmed, &leading_ws, opens, closes, prev_depth,
            &struct_registry, &mut literal_mode,
        ) {
            LiteralStartResult::Handled(s) => { output_lines.push(s); continue; }
            LiteralStartResult::NotLiteralStart => {}
        }
        
        // Bare enum literal
        match process_bare_enum_literal(
            trimmed, &leading_ws, opens, closes, prev_depth, &mut literal_mode,
        ) {
            LiteralStartResult::Handled(s) => { output_lines.push(s); continue; }
            LiteralStartResult::NotLiteralStart => {}
        }
        
        // Function definition
        match process_function_def(
            trimmed, &clean_line, &leading_ws, &lines, line_num,
            &mut current_fn_ctx, function_start_brace,
        ) {
            FunctionDefResult::Handled(s) => { output_lines.push(s); continue; }
            FunctionDefResult::NotFunctionDef => {}
        }
        
        // Const/static declaration
        if let Some(transformed) = transform_const_or_static(trimmed) {
            output_lines.push(format!("{}{}", leading_ws, transformed));
            continue;
        }
        
        // Effect statement skip
        if trimmed.starts_with("effect ") {
            continue;
        }
        
        // Rust native passthrough
        if is_rust_native_line(trimmed) {
            let output = process_native_line(
                trimmed, &leading_ws, &current_fn_ctx, &fn_registry, is_before_closing_brace,
            );
            output_lines.push(output);
            continue;
        }
        
        // Array literal start
        match process_array_literal_start(
            trimmed, &leading_ws, line_num, prev_bracket_depth, bracket_opens,
            &scope_analyzer, &tracker, &current_fn_ctx, &mut array_mode,
        ) {
            ArrayLiteralResult::Started(s) => { output_lines.push(s); continue; }
            ArrayLiteralResult::NotArrayLiteral => {}
        }
        
        // Tuple destructuring
        if let Some(output) = process_tuple_destructuring(
            trimmed, &leading_ws, &current_fn_ctx, &fn_registry,
        ) {
            output_lines.push(output);
            continue;
        }
        
        // RustS+ assignment
        if let Some((var_name, var_type, value, is_outer, is_explicit_mut)) = parse_rusts_assignment_ext(&clean_line) {
            let transformed_type = var_type.map(|t| transform_generic_brackets(&t));
            let result = process_assignment(
                &var_name, transformed_type.as_deref(), &value, is_outer, is_explicit_mut,
                line_num, &leading_ws, &scope_analyzer, &tracker, &current_fn_ctx, &fn_registry,
                inside_multiline_expr, next_line_is_method_chain, next_line_closes_expr,
                &mut prev_line_was_continuation,
            );
            output_lines.push(result);
        } else {
            // Non-assignment
            let result = process_non_assignment(
                trimmed, &leading_ws, line_num, &current_fn_ctx, &fn_registry,
                is_before_closing_brace, inside_multiline_expr, next_line_is_method_chain,
                next_line_closes_expr, &mut prev_line_was_continuation,
            );
            output_lines.push(result);
        }
    }
    
    // Apply post-processing
    let mut result = apply_postprocessing(output_lines);
    result = transform_macros_to_correct_syntax(&result);
    
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

// Helper function for match start processing
fn process_match_start(
    trimmed: &str,
    leading_ws: &str,
    lines: &[&str],
    line_num: usize,
    scope_analyzer: &ScopeAnalyzer,
    tracker: &VariableTracker,
    current_fn_ctx: &CurrentFunctionContext,
    match_mode: &mut MatchModeStack,
    prev_depth: usize,
) -> String {
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
    }
    
    let needs_as_str = match_string_ctx.needs_as_str();
    
    let output = if let Some((var_name_raw, match_expr)) = parse_control_flow_assignment(trimmed) {
        let (actual_var_name, type_annotation) = parse_var_type_annotation(&var_name_raw);
        let is_param = current_fn_ctx.params.contains_key(actual_var_name);
        let is_decl = scope_analyzer.is_decl(line_num);
        let is_mutation = scope_analyzer.is_mut(line_num);
        let is_shadowing = tracker.is_shadowing(actual_var_name, line_num);
        let needs_mut = scope_analyzer.needs_mut(actual_var_name, line_num);
        let needs_let = is_decl || (!is_mutation && !is_param) || is_shadowing;
        
        let transformed_match_expr = if needs_as_str {
            transform_match_for_string_patterns(&match_expr, true)
        } else {
            match_expr
        };
        
        if needs_let {
            let keyword = if needs_mut { "let mut" } else { "let" };
            format!("{}{} {}{} = {}", leading_ws, keyword, actual_var_name, type_annotation, transformed_match_expr)
        } else {
            format!("{}{}{} = {}", leading_ws, actual_var_name, type_annotation, transformed_match_expr)
        }
    } else {
        let transformed = if needs_as_str {
            transform_match_for_string_patterns(trimmed, true)
        } else {
            trimmed.to_string()
        };
        format!("{}{}", leading_ws, transformed)
    };
    
    match_mode.enter_match(prev_depth, is_assignment);
    output
}

// Helper function for if expression assignment
fn process_if_assignment(
    trimmed: &str,
    leading_ws: &str,
    line_num: usize,
    scope_analyzer: &ScopeAnalyzer,
    tracker: &VariableTracker,
    current_fn_ctx: &CurrentFunctionContext,
    prev_depth: usize,
    if_expr_assignment_depth: &mut Option<usize>,
) -> Option<String> {
    let (var_name_raw, if_expr) = parse_control_flow_assignment(trimmed)?;
    
    let (actual_var_name, type_annotation) = parse_var_type_annotation(&var_name_raw);
    let is_param = current_fn_ctx.params.contains_key(actual_var_name);
    let is_decl = scope_analyzer.is_decl(line_num);
    let is_mutation = scope_analyzer.is_mut(line_num);
    let is_shadowing = tracker.is_shadowing(actual_var_name, line_num);
    let needs_mut = scope_analyzer.needs_mut(actual_var_name, line_num);
    let needs_let = is_decl || (!is_mutation && !is_param) || is_shadowing;
    
    let output = if needs_let {
        let keyword = if needs_mut { "let mut" } else { "let" };
        format!("{}{} {}{} = ({}", leading_ws, keyword, actual_var_name, type_annotation, if_expr)
    } else {
        format!("{}{}{} = ({}", leading_ws, actual_var_name, type_annotation, if_expr)
    };
    
    *if_expr_assignment_depth = Some(prev_depth);
    Some(output)
}