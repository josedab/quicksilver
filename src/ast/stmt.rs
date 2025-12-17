//! Statement AST node types

use super::*;

/// A JavaScript statement
#[derive(Debug, Clone)]
pub enum Statement {
    /// Block statement { ... }
    Block(BlockStatement),

    /// Empty statement ;
    Empty(Span),

    /// Expression statement
    Expression(ExpressionStatement),

    /// If statement
    If(Box<IfStatement>),

    /// While statement
    While(Box<WhileStatement>),

    /// Do-while statement
    DoWhile(Box<DoWhileStatement>),

    /// For statement
    For(Box<ForStatement>),

    /// For-in statement
    ForIn(Box<ForInStatement>),

    /// For-of statement
    ForOf(Box<ForOfStatement>),

    /// Switch statement
    Switch(Box<SwitchStatement>),

    /// Break statement
    Break(BreakStatement),

    /// Continue statement
    Continue(ContinueStatement),

    /// Return statement
    Return(ReturnStatement),

    /// Throw statement
    Throw(Box<ThrowStatement>),

    /// Try statement
    Try(Box<TryStatement>),

    /// Labeled statement
    Labeled(Box<LabeledStatement>),

    /// With statement
    With(Box<WithStatement>),

    /// Debugger statement
    Debugger(Span),

    /// Variable declaration
    VariableDeclaration(VariableDeclaration),

    /// Function declaration
    FunctionDeclaration(Box<Function>),

    /// Class declaration
    ClassDeclaration(Box<Class>),

    /// Import declaration
    Import(Box<ImportDeclaration>),

    /// Export declaration
    Export(Box<ExportDeclaration>),
}

impl Statement {
    /// Get the span of this statement
    pub fn span(&self) -> Span {
        match self {
            Statement::Block(b) => b.span,
            Statement::Empty(span) => *span,
            Statement::Expression(e) => e.span,
            Statement::If(i) => i.span,
            Statement::While(w) => w.span,
            Statement::DoWhile(d) => d.span,
            Statement::For(f) => f.span,
            Statement::ForIn(f) => f.span,
            Statement::ForOf(f) => f.span,
            Statement::Switch(s) => s.span,
            Statement::Break(b) => b.span,
            Statement::Continue(c) => c.span,
            Statement::Return(r) => r.span,
            Statement::Throw(t) => t.span,
            Statement::Try(t) => t.span,
            Statement::Labeled(l) => l.span,
            Statement::With(w) => w.span,
            Statement::Debugger(span) => *span,
            Statement::VariableDeclaration(v) => v.span,
            Statement::FunctionDeclaration(f) => f.span,
            Statement::ClassDeclaration(c) => c.span,
            Statement::Import(i) => i.span,
            Statement::Export(e) => e.span,
        }
    }
}

/// Block statement
#[derive(Debug, Clone)]
pub struct BlockStatement {
    /// Statements in the block
    pub body: Vec<Statement>,
    /// Span in source
    pub span: Span,
}

/// Expression statement
#[derive(Debug, Clone)]
pub struct ExpressionStatement {
    /// Expression
    pub expression: Expression,
    /// Span in source
    pub span: Span,
}

/// If statement
#[derive(Debug, Clone)]
pub struct IfStatement {
    /// Test condition
    pub test: Expression,
    /// Consequent statement
    pub consequent: Statement,
    /// Alternate statement (else)
    pub alternate: Option<Statement>,
    /// Span in source
    pub span: Span,
}

/// While statement
#[derive(Debug, Clone)]
pub struct WhileStatement {
    /// Test condition
    pub test: Expression,
    /// Loop body
    pub body: Statement,
    /// Span in source
    pub span: Span,
}

/// Do-while statement
#[derive(Debug, Clone)]
pub struct DoWhileStatement {
    /// Loop body
    pub body: Statement,
    /// Test condition
    pub test: Expression,
    /// Span in source
    pub span: Span,
}

/// For loop init
#[derive(Debug, Clone)]
pub enum ForInit {
    /// Variable declaration
    Declaration(VariableDeclaration),
    /// Expression
    Expression(Expression),
}

/// For statement
#[derive(Debug, Clone)]
pub struct ForStatement {
    /// Initialization
    pub init: Option<ForInit>,
    /// Test condition
    pub test: Option<Expression>,
    /// Update expression
    pub update: Option<Expression>,
    /// Loop body
    pub body: Statement,
    /// Span in source
    pub span: Span,
}

/// For-in left-hand side
#[derive(Debug, Clone)]
pub enum ForInLeft {
    /// Variable declaration
    Declaration(VariableDeclaration),
    /// Assignment target
    Pattern(Pattern),
    /// Expression (must be valid assignment target)
    Expression(Expression),
}

/// For-in statement
#[derive(Debug, Clone)]
pub struct ForInStatement {
    /// Left-hand side
    pub left: ForInLeft,
    /// Right-hand side (object to iterate)
    pub right: Expression,
    /// Loop body
    pub body: Statement,
    /// Span in source
    pub span: Span,
}

/// For-of statement
#[derive(Debug, Clone)]
pub struct ForOfStatement {
    /// Left-hand side
    pub left: ForInLeft,
    /// Right-hand side (iterable)
    pub right: Expression,
    /// Loop body
    pub body: Statement,
    /// Is this a for-await-of?
    pub is_await: bool,
    /// Span in source
    pub span: Span,
}

/// Switch statement
#[derive(Debug, Clone)]
pub struct SwitchStatement {
    /// Discriminant expression
    pub discriminant: Expression,
    /// Switch cases
    pub cases: Vec<SwitchCase>,
    /// Span in source
    pub span: Span,
}

/// Break statement
#[derive(Debug, Clone)]
pub struct BreakStatement {
    /// Optional label
    pub label: Option<Identifier>,
    /// Span in source
    pub span: Span,
}

/// Continue statement
#[derive(Debug, Clone)]
pub struct ContinueStatement {
    /// Optional label
    pub label: Option<Identifier>,
    /// Span in source
    pub span: Span,
}

/// Return statement
#[derive(Debug, Clone)]
pub struct ReturnStatement {
    /// Return value
    pub argument: Option<Expression>,
    /// Span in source
    pub span: Span,
}

/// Throw statement
#[derive(Debug, Clone)]
pub struct ThrowStatement {
    /// Exception to throw
    pub argument: Expression,
    /// Span in source
    pub span: Span,
}

/// Try statement
#[derive(Debug, Clone)]
pub struct TryStatement {
    /// Try block
    pub block: BlockStatement,
    /// Catch clause
    pub handler: Option<CatchClause>,
    /// Finally block
    pub finalizer: Option<BlockStatement>,
    /// Span in source
    pub span: Span,
}

/// Labeled statement
#[derive(Debug, Clone)]
pub struct LabeledStatement {
    /// Label
    pub label: Identifier,
    /// Body statement
    pub body: Statement,
    /// Span in source
    pub span: Span,
}

/// With statement
#[derive(Debug, Clone)]
pub struct WithStatement {
    /// Object expression
    pub object: Expression,
    /// Body statement
    pub body: Statement,
    /// Span in source
    pub span: Span,
}

/// Import declaration
#[derive(Debug, Clone)]
pub struct ImportDeclaration {
    /// Import specifiers
    pub specifiers: Vec<ImportSpecifier>,
    /// Source module
    pub source: String,
    /// Span in source
    pub span: Span,
}

/// Export declaration
#[derive(Debug, Clone)]
pub struct ExportDeclaration {
    /// Export kind
    pub kind: ExportKind,
    /// Span in source
    pub span: Span,
}

/// Kind of export
#[derive(Debug, Clone)]
pub enum ExportKind {
    /// export { foo, bar }
    Named {
        specifiers: Vec<ExportSpecifier>,
        source: Option<String>,
    },
    /// export default expression
    Default(Expression),
    /// export default function/class
    DefaultDeclaration(Box<Statement>),
    /// export var/let/const/function/class
    Declaration(Box<Statement>),
    /// export * from 'mod'
    All { source: String },
    /// export * as foo from 'mod'
    AllAs {
        exported: Identifier,
        source: String,
    },
}
