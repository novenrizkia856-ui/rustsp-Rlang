//! RustS+ Abstract Syntax Tree (AST)
//!
//! This module defines the formal AST nodes for RustS+.
//! The AST is produced by parsing and consumed by the HIR builder.
//!
//! ## Design Principles
//!
//! 1. **Explicit over Implicit**: Every syntactic construct has a node
//! 2. **Location Tracking**: Every node tracks its source location
//! 3. **No String Heuristics**: Parsing is done by proper lexing, not regex

use std::collections::HashMap;

//=============================================================================
// SOURCE LOCATION
//=============================================================================

/// Source location for error reporting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl Span {
    pub fn new(line: usize, col: usize) -> Self {
        Span {
            start_line: line,
            start_col: col,
            end_line: line,
            end_col: col,
        }
    }
    
    pub fn merge(&self, other: &Span) -> Span {
        Span {
            start_line: self.start_line.min(other.start_line),
            start_col: if self.start_line <= other.start_line { self.start_col } else { other.start_col },
            end_line: self.end_line.max(other.end_line),
            end_col: if self.end_line >= other.end_line { self.end_col } else { other.end_col },
        }
    }
}

/// A node with source location
#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Spanned { node, span }
    }
}

//=============================================================================
// IDENTIFIERS AND PATHS
//=============================================================================

/// An identifier (variable name, function name, etc.)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ident {
    pub name: String,
}

impl Ident {
    pub fn new(name: impl Into<String>) -> Self {
        Ident { name: name.into() }
    }
}

/// A path like `std::io::Read` or `Enum::Variant`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    pub segments: Vec<Ident>,
}

impl Path {
    pub fn single(name: impl Into<String>) -> Self {
        Path { segments: vec![Ident::new(name)] }
    }
    
    pub fn is_single(&self) -> bool {
        self.segments.len() == 1
    }
    
    pub fn last(&self) -> Option<&Ident> {
        self.segments.last()
    }
    
    pub fn to_string(&self) -> String {
        self.segments.iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join("::")
    }
}

//=============================================================================
// TYPES
//=============================================================================

/// Type representation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// Simple type: `i32`, `String`, `bool`
    Path(Path),
    /// Reference type: `&T`, `&mut T`
    Reference {
        mutable: bool,
        inner: Box<Type>,
    },
    /// Array type: `[T; N]`
    Array {
        element: Box<Type>,
        size: Option<usize>,
    },
    /// Slice type: `[T]`
    Slice {
        element: Box<Type>,
    },
    /// Tuple type: `(T1, T2, ...)`
    Tuple(Vec<Type>),
    /// Generic type: `Vec<T>`, `HashMap<K, V>`
    Generic {
        base: Path,
        args: Vec<Type>,
    },
    /// Function type: `fn(T1, T2) -> R`
    Fn {
        params: Vec<Type>,
        ret: Option<Box<Type>>,
    },
    /// Unit type: `()`
    Unit,
    /// Inferred type (used during HIR building)
    Inferred,
}

impl Type {
    pub fn simple(name: impl Into<String>) -> Self {
        Type::Path(Path::single(name))
    }
    
    pub fn reference(inner: Type, mutable: bool) -> Self {
        Type::Reference {
            mutable,
            inner: Box::new(inner),
        }
    }
}

//=============================================================================
// EXPRESSIONS
//=============================================================================

/// Literal values
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    String(String),
    Unit,
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Arithmetic
    Add, Sub, Mul, Div, Rem,
    // Comparison
    Eq, Ne, Lt, Le, Gt, Ge,
    // Logical
    And, Or,
    // Bitwise
    BitAnd, BitOr, BitXor, Shl, Shr,
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,    // -x
    Not,    // !x
    Deref,  // *x
    Ref,    // &x
    RefMut, // &mut x
}

/// Expression node
#[derive(Debug, Clone)]
pub enum Expr {
    /// Literal: `42`, `"hello"`, `true`
    Literal(Literal),
    
    /// Variable reference: `x`
    Var(Ident),
    
    /// Path expression: `Enum::Variant`, `std::io::stdin`
    Path(Path),
    
    /// Binary operation: `a + b`
    Binary {
        op: BinOp,
        left: Box<Spanned<Expr>>,
        right: Box<Spanned<Expr>>,
    },
    
    /// Unary operation: `-x`, `!cond`
    Unary {
        op: UnaryOp,
        operand: Box<Spanned<Expr>>,
    },
    
    /// Field access: `x.field`
    Field {
        base: Box<Spanned<Expr>>,
        field: Ident,
    },
    
    /// Index access: `arr[i]`
    Index {
        base: Box<Spanned<Expr>>,
        index: Box<Spanned<Expr>>,
    },
    
    /// Function call: `foo(a, b)`
    Call {
        func: Box<Spanned<Expr>>,
        args: Vec<Spanned<Expr>>,
    },
    
    /// Method call: `obj.method(a, b)`
    MethodCall {
        receiver: Box<Spanned<Expr>>,
        method: Ident,
        args: Vec<Spanned<Expr>>,
    },
    
    /// Struct literal: `Point { x = 1, y = 2 }`
    Struct {
        path: Path,
        fields: Vec<(Ident, Spanned<Expr>)>,
        spread: Option<Box<Spanned<Expr>>>, // ..other
    },
    
    /// Tuple literal: `(a, b, c)`
    Tuple(Vec<Spanned<Expr>>),
    
    /// Array literal: `[1, 2, 3]`
    Array(Vec<Spanned<Expr>>),
    
    /// If expression: `if cond { then } else { else }`
    If {
        condition: Box<Spanned<Expr>>,
        then_branch: Box<Spanned<Block>>,
        else_branch: Option<Box<Spanned<Expr>>>,
    },
    
    /// Match expression: `match value { arms }`
    Match {
        value: Box<Spanned<Expr>>,
        arms: Vec<MatchArm>,
    },
    
    /// Block expression: `{ stmts; expr }`
    Block(Box<Spanned<Block>>),
    
    /// Closure: `|x, y| x + y` or `move |x| x`
    Closure {
        capture_by_move: bool,
        params: Vec<(Ident, Option<Type>)>,
        body: Box<Spanned<Expr>>,
    },
    
    /// Return expression: `return value`
    Return(Option<Box<Spanned<Expr>>>),
    
    /// Break expression: `break value`
    Break(Option<Box<Spanned<Expr>>>),
    
    /// Continue expression: `continue`
    Continue,
    
    /// Range expression: `start..end`, `start..=end`
    Range {
        start: Option<Box<Spanned<Expr>>>,
        end: Option<Box<Spanned<Expr>>>,
        inclusive: bool,
    },
    
    /// Assignment expression: `x = value` (RustS+ style)
    Assign {
        target: Box<Spanned<Expr>>,
        value: Box<Spanned<Expr>>,
    },
    
    /// Compound assignment: `x += 1`
    AssignOp {
        op: BinOp,
        target: Box<Spanned<Expr>>,
        value: Box<Spanned<Expr>>,
    },
    
    /// Macro invocation: `println!("hello")`
    Macro {
        name: Ident,
        args: String, // Raw macro arguments (macros are complex)
    },
    
    /// Cast expression: `x as i64`
    Cast {
        expr: Box<Spanned<Expr>>,
        target_type: Type,
    },
}

/// Match arm
#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Spanned<Pattern>,
    pub guard: Option<Spanned<Expr>>,
    pub body: Spanned<Expr>,
    pub span: Span,
}

//=============================================================================
// PATTERNS
//=============================================================================

/// Pattern for match arms and destructuring
#[derive(Debug, Clone)]
pub enum Pattern {
    /// Wildcard: `_`
    Wildcard,
    
    /// Binding: `x`, `mut x`
    Binding {
        name: Ident,
        mutable: bool,
        subpattern: Option<Box<Spanned<Pattern>>>, // x @ pattern
    },
    
    /// Literal pattern: `42`, `"hello"`
    Literal(Literal),
    
    /// Tuple pattern: `(a, b, c)`
    Tuple(Vec<Spanned<Pattern>>),
    
    /// Struct pattern: `Point { x, y }`
    Struct {
        path: Path,
        fields: Vec<(Ident, Option<Spanned<Pattern>>)>,
        rest: bool, // .. at the end
    },
    
    /// Enum variant pattern: `Some(x)`, `None`
    Variant {
        path: Path,
        fields: VariantFields,
    },
    
    /// Or pattern: `A | B | C`
    Or(Vec<Spanned<Pattern>>),
    
    /// Range pattern: `1..=5`
    Range {
        start: Option<Box<Spanned<Expr>>>,
        end: Option<Box<Spanned<Expr>>>,
        inclusive: bool,
    },
    
    /// Reference pattern: `&x`, `&mut x`
    Ref {
        mutable: bool,
        inner: Box<Spanned<Pattern>>,
    },
}

/// Fields in a variant pattern
#[derive(Debug, Clone)]
pub enum VariantFields {
    /// Unit variant: `None`
    Unit,
    /// Tuple variant: `Some(x)`
    Tuple(Vec<Spanned<Pattern>>),
    /// Struct variant: `Message { id, body }`
    Struct(Vec<(Ident, Option<Spanned<Pattern>>)>),
}

//=============================================================================
// STATEMENTS
//=============================================================================

/// Statement node
#[derive(Debug, Clone)]
pub enum Stmt {
    /// Variable binding: `x = 10` or `mut x = 10`
    Let {
        pattern: Spanned<Pattern>,
        ty: Option<Type>,
        init: Option<Spanned<Expr>>,
        mutable: bool,
        /// RustS+ `outer` keyword for explicit shadowing
        outer: bool,
    },
    
    /// Expression statement: `foo()`
    Expr(Spanned<Expr>),
    
    /// Item declaration (function, struct, etc.)
    Item(Box<Item>),
}

/// A block of statements
#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Spanned<Stmt>>,
    /// Optional trailing expression (value of block)
    pub expr: Option<Spanned<Expr>>,
}

//=============================================================================
// ITEMS (TOP-LEVEL DECLARATIONS)
//=============================================================================

/// Effect declaration in function signature
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EffectDecl {
    /// Read from parameter: `read(param)`
    Read(Ident),
    /// Write to parameter: `write(param)`
    Write(Ident),
    /// I/O effect: `io`
    Io,
    /// Allocation effect: `alloc`
    Alloc,
    /// Panic effect: `panic`
    Panic,
}

impl EffectDecl {
    pub fn to_string(&self) -> String {
        match self {
            EffectDecl::Read(p) => format!("read({})", p.name),
            EffectDecl::Write(p) => format!("write({})", p.name),
            EffectDecl::Io => "io".to_string(),
            EffectDecl::Alloc => "alloc".to_string(),
            EffectDecl::Panic => "panic".to_string(),
        }
    }
}

/// Function parameter
#[derive(Debug, Clone)]
pub struct FnParam {
    pub name: Ident,
    pub ty: Type,
    pub span: Span,
}

/// Function definition
#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: Ident,
    pub generics: Vec<Ident>,
    pub params: Vec<FnParam>,
    pub return_type: Option<Type>,
    pub effects: Vec<EffectDecl>,
    pub body: Option<Spanned<Block>>,
    pub is_pub: bool,
    pub span: Span,
}

/// Struct field
#[derive(Debug, Clone)]
pub struct StructField {
    pub name: Ident,
    pub ty: Type,
    pub is_pub: bool,
    pub span: Span,
}

/// Struct definition
#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: Ident,
    pub generics: Vec<Ident>,
    pub fields: Vec<StructField>,
    pub is_pub: bool,
    pub span: Span,
}

/// Enum variant
#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: Ident,
    pub fields: EnumVariantKind,
    pub span: Span,
}

/// Kind of enum variant
#[derive(Debug, Clone)]
pub enum EnumVariantKind {
    /// Unit variant: `None`
    Unit,
    /// Tuple variant: `Some(T)`
    Tuple(Vec<Type>),
    /// Struct variant: `Message { id: u64, body: String }`
    Struct(Vec<StructField>),
}

/// Enum definition
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: Ident,
    pub generics: Vec<Ident>,
    pub variants: Vec<EnumVariant>,
    pub is_pub: bool,
    pub span: Span,
}

/// Impl block
#[derive(Debug, Clone)]
pub struct ImplDef {
    pub generics: Vec<Ident>,
    pub self_type: Type,
    pub trait_path: Option<Path>,
    pub items: Vec<Spanned<Item>>,
    pub span: Span,
}

/// Top-level item
#[derive(Debug, Clone)]
pub enum Item {
    /// Function definition
    Fn(FnDef),
    /// Struct definition
    Struct(StructDef),
    /// Enum definition
    Enum(EnumDef),
    /// Impl block
    Impl(ImplDef),
    /// Use statement
    Use(Path),
    /// Module declaration
    Mod {
        name: Ident,
        items: Vec<Spanned<Item>>,
    },
    /// Type alias
    TypeAlias {
        name: Ident,
        ty: Type,
    },
    /// Const declaration
    Const {
        name: Ident,
        ty: Type,
        value: Spanned<Expr>,
    },
    /// Static declaration
    Static {
        name: Ident,
        ty: Type,
        value: Spanned<Expr>,
        mutable: bool,
    },
}

//=============================================================================
// MODULE (COMPILATION UNIT)
//=============================================================================

/// A RustS+ source file
#[derive(Debug, Clone)]
pub struct Module {
    pub items: Vec<Spanned<Item>>,
    pub file_name: String,
}

impl Module {
    pub fn new(file_name: impl Into<String>) -> Self {
        Module {
            items: Vec::new(),
            file_name: file_name.into(),
        }
    }
    
    /// Get all function definitions
    pub fn functions(&self) -> impl Iterator<Item = &FnDef> {
        self.items.iter().filter_map(|item| {
            if let Item::Fn(f) = &item.node {
                Some(f)
            } else {
                None
            }
        })
    }
    
    /// Get all struct definitions
    pub fn structs(&self) -> impl Iterator<Item = &StructDef> {
        self.items.iter().filter_map(|item| {
            if let Item::Struct(s) = &item.node {
                Some(s)
            } else {
                None
            }
        })
    }
    
    /// Get all enum definitions
    pub fn enums(&self) -> impl Iterator<Item = &EnumDef> {
        self.items.iter().filter_map(|item| {
            if let Item::Enum(e) = &item.node {
                Some(e)
            } else {
                None
            }
        })
    }
}

//=============================================================================
// TESTS
//=============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_path_construction() {
        let path = Path::single("foo");
        assert!(path.is_single());
        assert_eq!(path.to_string(), "foo");
    }
    
    #[test]
    fn test_type_construction() {
        let ty = Type::simple("i32");
        assert!(matches!(ty, Type::Path(_)));
        
        let ref_ty = Type::reference(Type::simple("String"), false);
        assert!(matches!(ref_ty, Type::Reference { mutable: false, .. }));
    }
    
    #[test]
    fn test_effect_decl() {
        let eff = EffectDecl::Write(Ident::new("acc"));
        assert_eq!(eff.to_string(), "write(acc)");
    }
}