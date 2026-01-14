//! RustS+ Parser - Converts source text to AST
//!
//! This is a simplified parser that works with the existing lowering system.
//! For full IR-based compilation, this would be replaced with a proper
//! recursive descent or PEG parser.
//!
//! ## Current Approach
//!
//! Since RustS+ is very close to Rust, we use a hybrid approach:
//! 1. Parse function signatures and effects declarations properly
//! 2. Extract structure information (structs, enums, impls)
//! 3. Use existing line-based analysis for body content
//!
//! This allows gradual migration to full AST-based parsing.

use crate::ast::*;

//=============================================================================
// LEXER TOKENS (Simplified)
//=============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Fn, Pub, Let, Mut, If, Else, Match, While, For, Loop,
    Return, Break, Continue, Struct, Enum, Impl, Trait,
    Mod, Use, Const, Static, Outer, Effects, Move,
    
    // Literals
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    CharLit(char),
    BoolLit(bool),
    
    // Identifiers
    Ident(String),
    
    // Operators
    Plus, Minus, Star, Slash, Percent,
    Eq, EqEq, Ne, Lt, Le, Gt, Ge,
    And, Or, Not, BitAnd, BitOr, BitXor,
    Shl, Shr,
    
    // Delimiters
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,
    Comma, Colon, ColonColon, Semi, Dot, Arrow, FatArrow,
    Question, At, Hash, Dollar,
    
    // Special
    Ampersand,  // &
    Pipe,       // |
    
    // End of file
    Eof,
}

//=============================================================================
// LEXER
//=============================================================================

pub struct Lexer<'a> {
    input: &'a str,
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer {
            input,
            chars: input.char_indices().peekable(),
            line: 1,
            col: 1,
        }
    }
    
    fn peek_char(&mut self) -> Option<char> {
        self.chars.peek().map(|(_, c)| *c)
    }
    
    fn next_char(&mut self) -> Option<char> {
        let result = self.chars.next().map(|(_, c)| c);
        if let Some(c) = result {
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
        result
    }
    
    fn span(&self) -> Span {
        Span::new(self.line, self.col)
    }
    
    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek_char() {
            if c.is_whitespace() {
                self.next_char();
            } else if c == '/' {
                // Check for comments
                let mut chars_copy = self.chars.clone();
                chars_copy.next();
                if let Some((_, '/')) = chars_copy.next() {
                    // Line comment
                    while let Some(c) = self.peek_char() {
                        if c == '\n' {
                            break;
                        }
                        self.next_char();
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }
    
    fn read_ident(&mut self, first: char) -> String {
        let mut s = String::new();
        s.push(first);
        while let Some(c) = self.peek_char() {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                self.next_char();
            } else {
                break;
            }
        }
        s
    }
    
    fn read_number(&mut self, first: char) -> Token {
        let mut s = String::new();
        s.push(first);
        let mut is_float = false;
        
        while let Some(c) = self.peek_char() {
            if c.is_ascii_digit() {
                s.push(c);
                self.next_char();
            } else if c == '.' && !is_float {
                // Check if it's a float or method call
                let mut chars_copy = self.chars.clone();
                chars_copy.next();
                if let Some((_, next)) = chars_copy.peek() {
                    if next.is_ascii_digit() {
                        is_float = true;
                        s.push(c);
                        self.next_char();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else if c == '_' {
                self.next_char(); // Skip underscores in numbers
            } else {
                break;
            }
        }
        
        if is_float {
            Token::FloatLit(s.parse().unwrap_or(0.0))
        } else {
            Token::IntLit(s.parse().unwrap_or(0))
        }
    }
    
    fn read_string(&mut self) -> Token {
        let mut s = String::new();
        while let Some(c) = self.next_char() {
            if c == '"' {
                break;
            } else if c == '\\' {
                if let Some(escaped) = self.next_char() {
                    match escaped {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        _ => {
                            s.push('\\');
                            s.push(escaped);
                        }
                    }
                }
            } else {
                s.push(c);
            }
        }
        Token::StringLit(s)
    }
    
    fn read_char(&mut self) -> Token {
        let c = self.next_char().unwrap_or(' ');
        let result = if c == '\\' {
            match self.next_char() {
                Some('n') => '\n',
                Some('t') => '\t',
                Some('r') => '\r',
                Some('\\') => '\\',
                Some('\'') => '\'',
                Some(x) => x,
                None => ' ',
            }
        } else {
            c
        };
        self.next_char(); // Consume closing quote
        Token::CharLit(result)
    }
    
    pub fn next_token(&mut self) -> (Token, Span) {
        self.skip_whitespace();
        let span = self.span();
        
        let c = match self.next_char() {
            Some(c) => c,
            None => return (Token::Eof, span),
        };
        
        let token = match c {
            // Delimiters
            '(' => Token::LParen,
            ')' => Token::RParen,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            '[' => Token::LBracket,
            ']' => Token::RBracket,
            ',' => Token::Comma,
            ';' => Token::Semi,
            '.' => Token::Dot,
            '?' => Token::Question,
            '@' => Token::At,
            '#' => Token::Hash,
            '$' => Token::Dollar,
            '|' => Token::Pipe,
            
            // Operators that might be multi-char
            ':' => {
                if self.peek_char() == Some(':') {
                    self.next_char();
                    Token::ColonColon
                } else {
                    Token::Colon
                }
            }
            '=' => {
                match self.peek_char() {
                    Some('=') => { self.next_char(); Token::EqEq }
                    Some('>') => { self.next_char(); Token::FatArrow }
                    _ => Token::Eq
                }
            }
            '!' => {
                if self.peek_char() == Some('=') {
                    self.next_char();
                    Token::Ne
                } else {
                    Token::Not
                }
            }
            '<' => {
                match self.peek_char() {
                    Some('=') => { self.next_char(); Token::Le }
                    Some('<') => { self.next_char(); Token::Shl }
                    _ => Token::Lt
                }
            }
            '>' => {
                match self.peek_char() {
                    Some('=') => { self.next_char(); Token::Ge }
                    Some('>') => { self.next_char(); Token::Shr }
                    _ => Token::Gt
                }
            }
            '-' => {
                if self.peek_char() == Some('>') {
                    self.next_char();
                    Token::Arrow
                } else {
                    Token::Minus
                }
            }
            '&' => {
                if self.peek_char() == Some('&') {
                    self.next_char();
                    Token::And
                } else {
                    Token::Ampersand
                }
            }
            '^' => Token::BitXor,
            '+' => Token::Plus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '%' => Token::Percent,
            
            // String literal
            '"' => self.read_string(),
            
            // Char literal
            '\'' => self.read_char(),
            
            // Number
            c if c.is_ascii_digit() => self.read_number(c),
            
            // Identifier or keyword
            c if c.is_alphabetic() || c == '_' => {
                let ident = self.read_ident(c);
                match ident.as_str() {
                    "fn" => Token::Fn,
                    "pub" => Token::Pub,
                    "let" => Token::Let,
                    "mut" => Token::Mut,
                    "if" => Token::If,
                    "else" => Token::Else,
                    "match" => Token::Match,
                    "while" => Token::While,
                    "for" => Token::For,
                    "loop" => Token::Loop,
                    "return" => Token::Return,
                    "break" => Token::Break,
                    "continue" => Token::Continue,
                    "struct" => Token::Struct,
                    "enum" => Token::Enum,
                    "impl" => Token::Impl,
                    "trait" => Token::Trait,
                    "mod" => Token::Mod,
                    "use" => Token::Use,
                    "const" => Token::Const,
                    "static" => Token::Static,
                    "outer" => Token::Outer,
                    "effects" => Token::Effects,
                    "move" => Token::Move,
                    "true" => Token::BoolLit(true),
                    "false" => Token::BoolLit(false),
                    _ => Token::Ident(ident),
                }
            }
            
            _ => Token::Eof, // Unknown character
        };
        
        (token, span)
    }
    
    pub fn tokenize(input: &str) -> Vec<(Token, Span)> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let (token, span) = lexer.next_token();
            if token == Token::Eof {
                tokens.push((token, span));
                break;
            }
            tokens.push((token, span));
        }
        tokens
    }
}

//=============================================================================
// FUNCTION SIGNATURE PARSER
//=============================================================================

/// Parse function signature from tokens
pub struct FunctionParser<'a> {
    tokens: &'a [(Token, Span)],
    pos: usize,
}

impl<'a> FunctionParser<'a> {
    pub fn new(tokens: &'a [(Token, Span)]) -> Self {
        FunctionParser { tokens, pos: 0 }
    }
    
    fn current(&self) -> &Token {
        self.tokens.get(self.pos).map(|(t, _)| t).unwrap_or(&Token::Eof)
    }
    
    fn current_span(&self) -> Span {
        self.tokens.get(self.pos).map(|(_, s)| *s).unwrap_or_default()
    }
    
    fn advance(&mut self) {
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
    }
    
    fn expect(&mut self, expected: &Token) -> bool {
        if self.current() == expected {
            self.advance();
            true
        } else {
            false
        }
    }
    
    fn expect_ident(&mut self) -> Option<String> {
        if let Token::Ident(name) = self.current().clone() {
            self.advance();
            Some(name)
        } else {
            None
        }
    }
    
    /// Parse a function definition
    pub fn parse_function(&mut self) -> Option<FnDef> {
        let start_span = self.current_span();
        
        // Optional `pub`
        let is_pub = self.expect(&Token::Pub);
        
        // `fn`
        if !self.expect(&Token::Fn) {
            return None;
        }
        
        // Function name
        let name = Ident::new(self.expect_ident()?);
        
        // Optional generics [T, U]
        let generics = if self.expect(&Token::LBracket) {
            let mut generics = Vec::new();
            loop {
                if let Some(g) = self.expect_ident() {
                    generics.push(Ident::new(g));
                }
                if !self.expect(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RBracket);
            generics
        } else {
            Vec::new()
        };
        
        // Parameters
        if !self.expect(&Token::LParen) {
            return None;
        }
        
        let mut params = Vec::new();
        while *self.current() != Token::RParen && *self.current() != Token::Eof {
            let param_span = self.current_span();
            
            // Parameter name
            let param_name = match self.expect_ident() {
                Some(n) => Ident::new(n),
                None => break,
            };
            
            // Parameter type (RustS+ style: no colon, or Rust style: with colon)
            let ty = self.parse_type()?;
            
            params.push(FnParam {
                name: param_name,
                ty,
                span: param_span,
            });
            
            if !self.expect(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RParen);
        
        // Optional effects clause
        let effects = if self.expect(&Token::Effects) {
            self.parse_effects()
        } else {
            Vec::new()
        };
        
        // Optional return type
        let return_type = if *self.current() != Token::LBrace && *self.current() != Token::Eq {
            // Check for -> or direct type
            if self.expect(&Token::Arrow) {
                Some(self.parse_type()?)
            } else if let Token::Ident(_) = self.current() {
                Some(self.parse_type()?)
            } else {
                None
            }
        } else {
            None
        };
        
        // Body (simplified - just skip to end of function)
        // In full implementation, this would recursively parse the body
        let body = None;
        
        let end_span = self.current_span();
        
        Some(FnDef {
            name,
            generics,
            params,
            return_type,
            effects,
            body,
            is_pub,
            span: start_span.merge(&end_span),
        })
    }
    
    /// Parse effects clause: effects(write acc, io, alloc)
    fn parse_effects(&mut self) -> Vec<EffectDecl> {
        let mut effects = Vec::new();
        
        if !self.expect(&Token::LParen) {
            return effects;
        }
        
        while *self.current() != Token::RParen && *self.current() != Token::Eof {
            if let Some(eff) = self.parse_single_effect() {
                effects.push(eff);
            }
            if !self.expect(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RParen);
        
        effects
    }
    
    /// Parse a single effect
    fn parse_single_effect(&mut self) -> Option<EffectDecl> {
        let eff_name = self.expect_ident()?;
        
        match eff_name.as_str() {
            "io" => Some(EffectDecl::Io),
            "alloc" => Some(EffectDecl::Alloc),
            "panic" => Some(EffectDecl::Panic),
            "read" => {
                self.expect(&Token::LParen);
                let param = Ident::new(self.expect_ident()?);
                self.expect(&Token::RParen);
                Some(EffectDecl::Read(param))
            }
            "write" => {
                self.expect(&Token::LParen);
                let param = Ident::new(self.expect_ident()?);
                self.expect(&Token::RParen);
                Some(EffectDecl::Write(param))
            }
            _ => None,
        }
    }
    
    /// Parse a type
    fn parse_type(&mut self) -> Option<Type> {
        // Handle optional colon (Rust style)
        self.expect(&Token::Colon);
        
        // Handle reference types
        if self.expect(&Token::Ampersand) {
            let mutable = self.expect(&Token::Mut);
            let inner = self.parse_type()?;
            return Some(Type::Reference {
                mutable,
                inner: Box::new(inner),
            });
        }
        
        // Handle slice/array types
        if self.expect(&Token::LBracket) {
            let element = self.parse_type()?;
            if self.expect(&Token::Semi) {
                // Array with size
                if let Token::IntLit(size) = self.current().clone() {
                    self.advance();
                    self.expect(&Token::RBracket);
                    return Some(Type::Array {
                        element: Box::new(element),
                        size: Some(size as usize),
                    });
                }
            }
            self.expect(&Token::RBracket);
            return Some(Type::Slice {
                element: Box::new(element),
            });
        }
        
        // Handle tuple types
        if self.expect(&Token::LParen) {
            let mut types = Vec::new();
            while *self.current() != Token::RParen && *self.current() != Token::Eof {
                if let Some(ty) = self.parse_type() {
                    types.push(ty);
                }
                if !self.expect(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RParen);
            
            if types.is_empty() {
                return Some(Type::Unit);
            }
            return Some(Type::Tuple(types));
        }
        
        // Simple type or generic
        let base_name = self.expect_ident()?;
        let base_path = Path::single(base_name);
        
        // Check for generic arguments
        if self.expect(&Token::Lt) {
            let mut args = Vec::new();
            while *self.current() != Token::Gt && *self.current() != Token::Eof {
                if let Some(ty) = self.parse_type() {
                    args.push(ty);
                }
                if !self.expect(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::Gt);
            
            return Some(Type::Generic {
                base: base_path,
                args,
            });
        }
        
        Some(Type::Path(base_path))
    }
}

//=============================================================================
// MODULE PARSER (SIMPLIFIED)
//=============================================================================

/// Parse a complete module from source
pub fn parse_module(source: &str, file_name: &str) -> Module {
    let tokens = Lexer::tokenize(source);
    let mut module = Module::new(file_name);
    
    let mut parser = FunctionParser::new(&tokens);
    
    // Simplified parsing - just extract function signatures
    while *parser.current() != Token::Eof {
        if let Some(func) = parser.parse_function() {
            module.items.push(Spanned::new(
                Item::Fn(func.clone()),
                func.span,
            ));
        } else {
            parser.advance();
        }
    }
    
    module
}

//=============================================================================
// EXTRACT FUNCTIONS FROM SOURCE (HELPER)
//=============================================================================

/// Extract function signatures from RustS+ source code
/// This is a helper that works with the existing line-based system
pub fn extract_function_signatures(source: &str) -> Vec<(String, Vec<EffectDecl>, usize)> {
    let mut functions = Vec::new();
    
    for (line_num, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        
        // Check for function definition
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            // Tokenize this line
            let tokens = Lexer::tokenize(trimmed);
            let mut parser = FunctionParser::new(&tokens);
            
            if let Some(func) = parser.parse_function() {
                functions.push((
                    func.name.name,
                    func.effects,
                    line_num + 1,
                ));
            }
        }
    }
    
    functions
}

//=============================================================================
// TESTS
//=============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_lexer_basic() {
        let tokens = Lexer::tokenize("fn add(a i32, b i32) i32");
        assert!(tokens.iter().any(|(t, _)| *t == Token::Fn));
        assert!(tokens.iter().any(|(t, _)| matches!(t, Token::Ident(s) if s == "add")));
    }
    
    #[test]
    fn test_lexer_effects() {
        let tokens = Lexer::tokenize("fn transfer(acc Account) effects(write acc) Account");
        assert!(tokens.iter().any(|(t, _)| *t == Token::Effects));
    }
    
    #[test]
    fn test_parse_function_simple() {
        let tokens = Lexer::tokenize("fn add(a i32, b i32) i32 { a + b }");
        let mut parser = FunctionParser::new(&tokens);
        let func = parser.parse_function().unwrap();
        
        assert_eq!(func.name.name, "add");
        assert_eq!(func.params.len(), 2);
        assert!(func.effects.is_empty());
    }
    
    #[test]
    fn test_parse_function_with_effects() {
        let tokens = Lexer::tokenize("fn transfer(acc Account, amount i64) effects(write acc, io) Account");
        let mut parser = FunctionParser::new(&tokens);
        let func = parser.parse_function().unwrap();
        
        assert_eq!(func.name.name, "transfer");
        assert_eq!(func.effects.len(), 2);
        assert!(func.effects.iter().any(|e| matches!(e, EffectDecl::Write(_))));
        assert!(func.effects.iter().any(|e| matches!(e, EffectDecl::Io)));
    }
    
    #[test]
    fn test_extract_functions() {
        let source = r#"
fn pure_add(a i32, b i32) i32 { a + b }
fn log(msg String) effects(io) { println(msg) }
fn transfer(acc Account) effects(write acc) Account { acc }
"#;
        
        let funcs = extract_function_signatures(source);
        assert_eq!(funcs.len(), 3);
        
        // pure_add has no effects
        assert!(funcs[0].1.is_empty());
        
        // log has io effect
        assert!(funcs[1].1.iter().any(|e| matches!(e, EffectDecl::Io)));
        
        // transfer has write effect
        assert!(funcs[2].1.iter().any(|e| matches!(e, EffectDecl::Write(_))));
    }
}