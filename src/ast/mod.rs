//! Abstract Syntax Tree (AST) types for JavaScript
//!
//! This module defines the AST node types that represent parsed JavaScript code.
//! The AST follows the ESTree specification with some modifications for Rust idioms.

mod expr;
mod pattern;
mod stmt;

pub use expr::*;
pub use pattern::*;
pub use stmt::*;

use crate::error::SourceLocation;

/// A span in the source code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    /// Start location
    pub start: SourceLocation,
    /// End location
    pub end: SourceLocation,
}

impl Span {
    /// Create a new span
    pub fn new(start: SourceLocation, end: SourceLocation) -> Self {
        Self { start, end }
    }

    /// Merge two spans into one covering both
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: if self.start.offset < other.start.offset {
                self.start
            } else {
                other.start
            },
            end: if self.end.offset > other.end.offset {
                self.end
            } else {
                other.end
            },
        }
    }
}

/// A complete JavaScript program
#[derive(Debug, Clone)]
pub struct Program {
    /// The statements in the program
    pub body: Vec<Statement>,
    /// Source type (script or module)
    pub source_type: SourceType,
    /// Whether this program is in strict mode
    pub strict: bool,
    /// Span in source
    pub span: Span,
}

/// Source type of the program
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceType {
    /// Script mode (no import/export)
    #[default]
    Script,
    /// Module mode (import/export allowed)
    Module,
}

/// A JavaScript identifier
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identifier {
    /// The name of the identifier
    pub name: String,
    /// Span in source
    pub span: Span,
}

impl Identifier {
    /// Create a new identifier
    pub fn new(name: impl Into<String>, span: Span) -> Self {
        Self {
            name: name.into(),
            span,
        }
    }
}

/// Variable declaration kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableKind {
    /// var declaration
    Var,
    /// let declaration
    Let,
    /// const declaration
    Const,
}

/// A single variable declarator (id = init)
#[derive(Debug, Clone)]
pub struct VariableDeclarator {
    /// The binding pattern
    pub id: Pattern,
    /// Optional initializer expression
    pub init: Option<Box<Expression>>,
    /// Span in source
    pub span: Span,
}

/// A variable declaration (let x = 1, y = 2)
#[derive(Debug, Clone)]
pub struct VariableDeclaration {
    /// The kind of variable declaration
    pub kind: VariableKind,
    /// The declarators
    pub declarations: Vec<VariableDeclarator>,
    /// Span in source
    pub span: Span,
}

/// Function parameters
#[derive(Debug, Clone, Default)]
pub struct FunctionParams {
    /// Regular parameters
    pub params: Vec<Pattern>,
    /// Rest parameter (...args)
    pub rest: Option<Box<Pattern>>,
}

/// A function (can be declaration, expression, or method)
#[derive(Debug, Clone)]
pub struct Function {
    /// Optional function name
    pub id: Option<Identifier>,
    /// Parameters
    pub params: FunctionParams,
    /// Function body (None for arrow with expression body)
    pub body: FunctionBody,
    /// Is this an async function?
    pub is_async: bool,
    /// Is this a generator function?
    pub is_generator: bool,
    /// Span in source
    pub span: Span,
}

/// Function body - either a block or a single expression (for arrows)
#[derive(Debug, Clone)]
pub enum FunctionBody {
    /// Block statement body
    Block(BlockStatement),
    /// Expression body (arrow functions only)
    Expression(Box<Expression>),
}

/// A class definition
#[derive(Debug, Clone)]
pub struct Class {
    /// Optional class name
    pub id: Option<Identifier>,
    /// Superclass expression
    pub super_class: Option<Box<Expression>>,
    /// Class body
    pub body: ClassBody,
    /// Span in source
    pub span: Span,
}

/// Class body containing methods and fields
#[derive(Debug, Clone)]
pub struct ClassBody {
    /// Class elements (methods, fields)
    pub body: Vec<ClassElement>,
    /// Span in source
    pub span: Span,
}

/// A class element (method, field, static block)
#[derive(Debug, Clone)]
pub enum ClassElement {
    /// Method definition
    Method(MethodDefinition),
    /// Property definition
    Property(PropertyDefinition),
    /// Static initialization block
    StaticBlock(BlockStatement),
}

/// A method definition in a class or object
#[derive(Debug, Clone)]
pub struct MethodDefinition {
    /// Method key
    pub key: PropertyKey,
    /// Method value (function)
    pub value: Function,
    /// Method kind
    pub kind: MethodKind,
    /// Is this a computed property?
    pub computed: bool,
    /// Is this a static method?
    pub is_static: bool,
    /// Span in source
    pub span: Span,
}

/// Kind of method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodKind {
    /// Regular method
    Method,
    /// Getter method
    Get,
    /// Setter method
    Set,
    /// Constructor
    Constructor,
}

/// A property definition in a class
#[derive(Debug, Clone)]
pub struct PropertyDefinition {
    /// Property key
    pub key: PropertyKey,
    /// Property value
    pub value: Option<Box<Expression>>,
    /// Is this a computed property?
    pub computed: bool,
    /// Is this a static property?
    pub is_static: bool,
    /// Span in source
    pub span: Span,
}

/// Property key in objects and classes
#[derive(Debug, Clone)]
pub enum PropertyKey {
    /// Identifier key
    Identifier(Identifier),
    /// String literal key
    String(String),
    /// Number literal key
    Number(f64),
    /// Computed key [expr]
    Computed(Box<Expression>),
    /// Private name (#name)
    PrivateName(String),
}

/// Template literal element
#[derive(Debug, Clone)]
pub struct TemplateElement {
    /// Raw string value
    pub raw: String,
    /// Cooked (processed) string value
    pub cooked: Option<String>,
    /// Is this the last element?
    pub tail: bool,
    /// Span in source
    pub span: Span,
}

/// Import specifier kinds
#[derive(Debug, Clone)]
pub enum ImportSpecifier {
    /// import { foo } from 'mod'
    Named {
        local: Identifier,
        imported: Identifier,
        span: Span,
    },
    /// import * as foo from 'mod'
    Namespace { local: Identifier, span: Span },
    /// import foo from 'mod'
    Default { local: Identifier, span: Span },
}

/// Export specifier kinds
#[derive(Debug, Clone)]
pub enum ExportSpecifier {
    /// export { foo }
    Named {
        local: Identifier,
        exported: Identifier,
        span: Span,
    },
    /// export * from 'mod'
    All { span: Span },
    /// export * as foo from 'mod'
    AllAs { exported: Identifier, span: Span },
}

/// Switch case
#[derive(Debug, Clone)]
pub struct SwitchCase {
    /// Test expression (None for default)
    pub test: Option<Expression>,
    /// Consequent statements
    pub consequent: Vec<Statement>,
    /// Span in source
    pub span: Span,
}

/// Catch clause
#[derive(Debug, Clone)]
pub struct CatchClause {
    /// Catch parameter (optional in ES2019+)
    pub param: Option<Pattern>,
    /// Catch body
    pub body: BlockStatement,
    /// Span in source
    pub span: Span,
}
