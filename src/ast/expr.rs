//! Expression AST node types

use super::*;

/// A JavaScript expression
#[derive(Debug, Clone)]
pub enum Expression {
    /// Identifier reference
    Identifier(Identifier),

    /// Literal value
    Literal(Literal),

    /// Template literal `hello ${name}`
    TemplateLiteral(TemplateLiteral),

    /// Tagged template literal tag`hello`
    TaggedTemplate(TaggedTemplate),

    /// Array literal [1, 2, 3]
    Array(ArrayExpression),

    /// Object literal {a: 1, b: 2}
    Object(ObjectExpression),

    /// Function expression
    Function(Box<Function>),

    /// Arrow function expression
    Arrow(Box<Function>),

    /// Class expression
    Class(Box<Class>),

    /// this expression
    This(Span),

    /// super expression (in method calls)
    Super(Span),

    /// Member expression obj.prop or obj[prop]
    Member(Box<MemberExpression>),

    /// Optional member expression obj?.prop
    OptionalMember(Box<MemberExpression>),

    /// Call expression func(args)
    Call(Box<CallExpression>),

    /// Optional call expression func?.(args)
    OptionalCall(Box<CallExpression>),

    /// new expression new Foo(args)
    New(Box<NewExpression>),

    /// Unary expression !x, -x, typeof x
    Unary(Box<UnaryExpression>),

    /// Update expression ++x, x++
    Update(Box<UpdateExpression>),

    /// Binary expression x + y, x === y
    Binary(Box<BinaryExpression>),

    /// Logical expression x && y, x || y, x ?? y
    Logical(Box<LogicalExpression>),

    /// Assignment expression x = y, x += y
    Assignment(Box<AssignmentExpression>),

    /// Conditional expression x ? y : z
    Conditional(Box<ConditionalExpression>),

    /// Sequence expression x, y, z
    Sequence(Box<SequenceExpression>),

    /// Spread element ...x
    Spread(Box<SpreadElement>),

    /// Yield expression yield x
    Yield(Box<YieldExpression>),

    /// Await expression await x
    Await(Box<AwaitExpression>),

    /// import.meta
    MetaProperty(MetaProperty),

    /// import(source)
    Import(Box<ImportExpression>),

    /// Perform effect operation: perform EffectType.operation(args)
    Perform(Box<PerformExpression>),

    /// Parenthesized expression (for preserving parens)
    Parenthesized(Box<Expression>),
}

impl Expression {
    /// Get the span of this expression
    pub fn span(&self) -> Span {
        match self {
            Expression::Identifier(id) => id.span,
            Expression::Literal(lit) => lit.span,
            Expression::TemplateLiteral(t) => t.span,
            Expression::TaggedTemplate(t) => t.span,
            Expression::Array(a) => a.span,
            Expression::Object(o) => o.span,
            Expression::Function(f) => f.span,
            Expression::Arrow(f) => f.span,
            Expression::Class(c) => c.span,
            Expression::This(span) => *span,
            Expression::Super(span) => *span,
            Expression::Member(m) => m.span,
            Expression::OptionalMember(m) => m.span,
            Expression::Call(c) => c.span,
            Expression::OptionalCall(c) => c.span,
            Expression::New(n) => n.span,
            Expression::Unary(u) => u.span,
            Expression::Update(u) => u.span,
            Expression::Binary(b) => b.span,
            Expression::Logical(l) => l.span,
            Expression::Assignment(a) => a.span,
            Expression::Conditional(c) => c.span,
            Expression::Sequence(s) => s.span,
            Expression::Spread(s) => s.span,
            Expression::Yield(y) => y.span,
            Expression::Await(a) => a.span,
            Expression::MetaProperty(m) => m.span,
            Expression::Import(i) => i.span,
            Expression::Perform(p) => p.span,
            Expression::Parenthesized(e) => e.span(),
        }
    }

    /// Check if this expression is a valid assignment target
    pub fn is_valid_assignment_target(&self) -> bool {
        match self {
            Expression::Identifier(_) => true,
            Expression::Member(_) | Expression::OptionalMember(_) => true,
            Expression::Parenthesized(e) => e.is_valid_assignment_target(),
            _ => false,
        }
    }
}

/// A literal value
#[derive(Debug, Clone)]
pub struct Literal {
    /// The literal value
    pub value: LiteralValue,
    /// Raw source text
    pub raw: String,
    /// Span in source
    pub span: Span,
}

/// Literal value types
#[derive(Debug, Clone)]
pub enum LiteralValue {
    /// null
    Null,
    /// true or false
    Boolean(bool),
    /// Number (integer or float)
    Number(f64),
    /// BigInt
    BigInt(String),
    /// String
    String(String),
    /// Regular expression
    Regex { pattern: String, flags: String },
}

/// Template literal
#[derive(Debug, Clone)]
pub struct TemplateLiteral {
    /// String parts (quasis)
    pub quasis: Vec<TemplateElement>,
    /// Expression parts
    pub expressions: Vec<Expression>,
    /// Span in source
    pub span: Span,
}

/// Tagged template literal
#[derive(Debug, Clone)]
pub struct TaggedTemplate {
    /// Tag expression
    pub tag: Box<Expression>,
    /// Template literal
    pub quasi: TemplateLiteral,
    /// Span in source
    pub span: Span,
}

/// Array expression
#[derive(Debug, Clone)]
pub struct ArrayExpression {
    /// Array elements (None for holes like [1,,3])
    pub elements: Vec<Option<Expression>>,
    /// Span in source
    pub span: Span,
}

/// Object expression
#[derive(Debug, Clone)]
pub struct ObjectExpression {
    /// Object properties
    pub properties: Vec<ObjectProperty>,
    /// Span in source
    pub span: Span,
}

/// Object property
#[derive(Debug, Clone)]
pub enum ObjectProperty {
    /// Regular property {a: 1}
    Property {
        key: PropertyKey,
        value: Expression,
        computed: bool,
        shorthand: bool,
        method: bool,
        span: Span,
    },
    /// Spread property {...obj}
    Spread { argument: Expression, span: Span },
    /// Method definition {foo() {}}
    Method(MethodDefinition),
}

/// Member expression
#[derive(Debug, Clone)]
pub struct MemberExpression {
    /// Object being accessed
    pub object: Expression,
    /// Property being accessed
    pub property: MemberProperty,
    /// Is this a computed property access? (obj[prop])
    pub computed: bool,
    /// Span in source
    pub span: Span,
}

/// Member property (can be identifier or computed)
#[derive(Debug, Clone)]
pub enum MemberProperty {
    /// obj.prop
    Identifier(Identifier),
    /// obj[expr]
    Expression(Box<Expression>),
    /// obj.#privateProp
    PrivateName(String),
}

/// Call expression
#[derive(Debug, Clone)]
pub struct CallExpression {
    /// Callee expression
    pub callee: Expression,
    /// Arguments
    pub arguments: Vec<Expression>,
    /// Span in source
    pub span: Span,
}

/// New expression
#[derive(Debug, Clone)]
pub struct NewExpression {
    /// Constructor expression
    pub callee: Expression,
    /// Arguments
    pub arguments: Vec<Expression>,
    /// Span in source
    pub span: Span,
}

/// Unary expression
#[derive(Debug, Clone)]
pub struct UnaryExpression {
    /// Operator
    pub operator: UnaryOperator,
    /// Argument
    pub argument: Expression,
    /// Span in source
    pub span: Span,
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    /// -
    Minus,
    /// +
    Plus,
    /// !
    Not,
    /// ~
    BitwiseNot,
    /// typeof
    Typeof,
    /// void
    Void,
    /// delete
    Delete,
}

/// Update expression (++, --)
#[derive(Debug, Clone)]
pub struct UpdateExpression {
    /// Operator
    pub operator: UpdateOperator,
    /// Argument
    pub argument: Expression,
    /// Is prefix (++x) or postfix (x++)
    pub prefix: bool,
    /// Span in source
    pub span: Span,
}

/// Update operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOperator {
    /// ++
    Increment,
    /// --
    Decrement,
}

/// Binary expression
#[derive(Debug, Clone)]
pub struct BinaryExpression {
    /// Operator
    pub operator: BinaryOperator,
    /// Left operand
    pub left: Expression,
    /// Right operand
    pub right: Expression,
    /// Span in source
    pub span: Span,
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    /// +
    Add,
    /// -
    Sub,
    /// *
    Mul,
    /// /
    Div,
    /// %
    Mod,
    /// **
    Pow,
    /// ==
    Eq,
    /// !=
    Ne,
    /// ===
    StrictEq,
    /// !==
    StrictNe,
    /// <
    Lt,
    /// <=
    Le,
    /// >
    Gt,
    /// >=
    Ge,
    /// <<
    Shl,
    /// >>
    Shr,
    /// >>>
    UShr,
    /// &
    BitwiseAnd,
    /// |
    BitwiseOr,
    /// ^
    BitwiseXor,
    /// in
    In,
    /// instanceof
    Instanceof,
}

/// Logical expression
#[derive(Debug, Clone)]
pub struct LogicalExpression {
    /// Operator
    pub operator: LogicalOperator,
    /// Left operand
    pub left: Expression,
    /// Right operand
    pub right: Expression,
    /// Span in source
    pub span: Span,
}

/// Logical operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOperator {
    /// &&
    And,
    /// ||
    Or,
    /// ??
    NullishCoalescing,
}

/// Assignment expression
#[derive(Debug, Clone)]
pub struct AssignmentExpression {
    /// Operator
    pub operator: AssignmentOperator,
    /// Left-hand side (assignment target)
    pub left: AssignmentTarget,
    /// Right-hand side
    pub right: Expression,
    /// Span in source
    pub span: Span,
}

/// Assignment target (left-hand side of assignment)
#[derive(Debug, Clone)]
pub enum AssignmentTarget {
    /// Simple identifier or member expression
    Simple(Expression),
    /// Destructuring pattern
    Pattern(Pattern),
}

/// Assignment operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentOperator {
    /// =
    Assign,
    /// +=
    AddAssign,
    /// -=
    SubAssign,
    /// *=
    MulAssign,
    /// /=
    DivAssign,
    /// %=
    ModAssign,
    /// **=
    PowAssign,
    /// <<=
    ShlAssign,
    /// >>=
    ShrAssign,
    /// >>>=
    UShrAssign,
    /// &=
    BitwiseAndAssign,
    /// |=
    BitwiseOrAssign,
    /// ^=
    BitwiseXorAssign,
    /// &&=
    AndAssign,
    /// ||=
    OrAssign,
    /// ??=
    NullishAssign,
}

/// Conditional expression
#[derive(Debug, Clone)]
pub struct ConditionalExpression {
    /// Test expression
    pub test: Expression,
    /// Consequent expression
    pub consequent: Expression,
    /// Alternate expression
    pub alternate: Expression,
    /// Span in source
    pub span: Span,
}

/// Sequence expression
#[derive(Debug, Clone)]
pub struct SequenceExpression {
    /// Expressions in sequence
    pub expressions: Vec<Expression>,
    /// Span in source
    pub span: Span,
}

/// Spread element
#[derive(Debug, Clone)]
pub struct SpreadElement {
    /// Argument being spread
    pub argument: Expression,
    /// Span in source
    pub span: Span,
}

/// Yield expression
#[derive(Debug, Clone)]
pub struct YieldExpression {
    /// Argument (optional)
    pub argument: Option<Expression>,
    /// Is this yield* (delegate)?
    pub delegate: bool,
    /// Span in source
    pub span: Span,
}

/// Await expression
#[derive(Debug, Clone)]
pub struct AwaitExpression {
    /// Argument
    pub argument: Expression,
    /// Span in source
    pub span: Span,
}

/// Meta property (import.meta, new.target)
#[derive(Debug, Clone)]
pub struct MetaProperty {
    /// Meta object
    pub meta: Identifier,
    /// Property
    pub property: Identifier,
    /// Span in source
    pub span: Span,
}

/// Import expression import(source)
#[derive(Debug, Clone)]
pub struct ImportExpression {
    /// Source module
    pub source: Expression,
    /// Span in source
    pub span: Span,
}

/// Perform expression perform Effect.operation(args)
#[derive(Debug, Clone)]
pub struct PerformExpression {
    /// Effect type name (e.g., "Log")
    pub effect_type: String,
    /// Operation name (e.g., "log")
    pub operation: String,
    /// Arguments to the operation
    pub arguments: Vec<Expression>,
    /// Span in source
    pub span: Span,
}
