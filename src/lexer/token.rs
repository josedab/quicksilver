//! Token definitions for the JavaScript lexer

use crate::error::SourceLocation;

/// A token produced by the lexer
#[derive(Debug, Clone, PartialEq)]
pub struct Token<'src> {
    /// The kind of token
    pub kind: TokenKind,
    /// The source text of the token
    pub text: &'src str,
    /// Location in source
    pub location: SourceLocation,
}

/// The kind of a token
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    // Literals
    /// Numeric literal (42, 3.14, 0xFF)
    NumberLiteral,
    /// BigInt literal (42n)
    BigIntLiteral,
    /// String literal ("hello", 'world')
    StringLiteral,
    /// Template literal with no substitutions (`hello`)
    TemplateLiteral,
    /// Template head (`hello ${)
    TemplateHead,
    /// Template middle (} middle ${)
    TemplateMiddle,
    /// Template tail (} tail`)
    TemplateTail,
    /// Regular expression literal (/pattern/flags)
    RegexLiteral,

    // Identifiers and keywords
    /// Identifier (foo, bar, $baz)
    Identifier,
    /// Private name (#foo, #bar)
    PrivateName,
    /// Keyword (let, const, function, etc.)
    Keyword(Keyword),

    // Punctuators
    /// `(`
    LeftParen,
    /// `)`
    RightParen,
    /// `{`
    LeftBrace,
    /// `}`
    RightBrace,
    /// `[`
    LeftBracket,
    /// `]`
    RightBracket,
    /// `.`
    Dot,
    /// `...`
    DotDotDot,
    /// `;`
    Semicolon,
    /// `,`
    Comma,
    /// `:`
    Colon,
    /// `?`
    Question,
    /// `?.`
    QuestionDot,
    /// `??`
    QuestionQuestion,
    /// `??=`
    QuestionQuestionEquals,

    // Operators
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `**`
    StarStar,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `++`
    PlusPlus,
    /// `--`
    MinusMinus,

    // Comparison operators
    /// `<`
    Less,
    /// `>`
    Greater,
    /// `<=`
    LessEquals,
    /// `>=`
    GreaterEquals,
    /// `==`
    EqualsEquals,
    /// `===`
    EqualsEqualsEquals,
    /// `!=`
    BangEquals,
    /// `!==`
    BangEqualsEquals,

    // Bitwise operators
    /// `&`
    Ampersand,
    /// `|`
    Pipe,
    /// `^`
    Caret,
    /// `~`
    Tilde,
    /// `<<`
    LessLess,
    /// `>>`
    GreaterGreater,
    /// `>>>`
    GreaterGreaterGreater,

    // Logical operators
    /// `!`
    Bang,
    /// `&&`
    AmpersandAmpersand,
    /// `||`
    PipePipe,

    // Assignment operators
    /// `=`
    Equals,
    /// `+=`
    PlusEquals,
    /// `-=`
    MinusEquals,
    /// `*=`
    StarEquals,
    /// `**=`
    StarStarEquals,
    /// `/=`
    SlashEquals,
    /// `%=`
    PercentEquals,
    /// `<<=`
    LessLessEquals,
    /// `>>=`
    GreaterGreaterEquals,
    /// `>>>=`
    GreaterGreaterGreaterEquals,
    /// `&=`
    AmpersandEquals,
    /// `|=`
    PipeEquals,
    /// `^=`
    CaretEquals,
    /// `&&=`
    AmpersandAmpersandEquals,
    /// `||=`
    PipePipeEquals,

    // Arrow
    /// `=>`
    Arrow,

    // End of file
    /// End of input
    Eof,
}

/// JavaScript keywords
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyword {
    // Reserved keywords
    Await,
    Break,
    Case,
    Catch,
    Class,
    Const,
    Continue,
    Debugger,
    Default,
    Delete,
    Do,
    Else,
    Enum,
    Export,
    Extends,
    False,
    Finally,
    For,
    Function,
    If,
    Import,
    In,
    Instanceof,
    Let,
    New,
    Null,
    Return,
    Static,
    Super,
    Switch,
    This,
    Throw,
    True,
    Try,
    Typeof,
    Var,
    Void,
    While,
    With,
    Yield,

    // Contextual keywords
    As,
    Async,
    From,
    Get,
    Of,
    Set,
    Target,
}

impl Keyword {
    /// Check if this keyword is a reserved word that cannot be used as an identifier
    pub fn is_reserved(&self) -> bool {
        !matches!(
            self,
            Keyword::As
                | Keyword::Async
                | Keyword::From
                | Keyword::Get
                | Keyword::Of
                | Keyword::Set
                | Keyword::Target
        )
    }

    /// Get the string representation of the keyword
    pub fn as_str(&self) -> &'static str {
        match self {
            Keyword::Await => "await",
            Keyword::Break => "break",
            Keyword::Case => "case",
            Keyword::Catch => "catch",
            Keyword::Class => "class",
            Keyword::Const => "const",
            Keyword::Continue => "continue",
            Keyword::Debugger => "debugger",
            Keyword::Default => "default",
            Keyword::Delete => "delete",
            Keyword::Do => "do",
            Keyword::Else => "else",
            Keyword::Enum => "enum",
            Keyword::Export => "export",
            Keyword::Extends => "extends",
            Keyword::False => "false",
            Keyword::Finally => "finally",
            Keyword::For => "for",
            Keyword::Function => "function",
            Keyword::If => "if",
            Keyword::Import => "import",
            Keyword::In => "in",
            Keyword::Instanceof => "instanceof",
            Keyword::Let => "let",
            Keyword::New => "new",
            Keyword::Null => "null",
            Keyword::Return => "return",
            Keyword::Static => "static",
            Keyword::Super => "super",
            Keyword::Switch => "switch",
            Keyword::This => "this",
            Keyword::Throw => "throw",
            Keyword::True => "true",
            Keyword::Try => "try",
            Keyword::Typeof => "typeof",
            Keyword::Var => "var",
            Keyword::Void => "void",
            Keyword::While => "while",
            Keyword::With => "with",
            Keyword::Yield => "yield",
            Keyword::As => "as",
            Keyword::Async => "async",
            Keyword::From => "from",
            Keyword::Get => "get",
            Keyword::Of => "of",
            Keyword::Set => "set",
            Keyword::Target => "target",
        }
    }
}

impl std::fmt::Display for Keyword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl TokenKind {
    /// Check if this token is a valid assignment target
    pub fn is_assignment_operator(&self) -> bool {
        matches!(
            self,
            TokenKind::Equals
                | TokenKind::PlusEquals
                | TokenKind::MinusEquals
                | TokenKind::StarEquals
                | TokenKind::StarStarEquals
                | TokenKind::SlashEquals
                | TokenKind::PercentEquals
                | TokenKind::LessLessEquals
                | TokenKind::GreaterGreaterEquals
                | TokenKind::GreaterGreaterGreaterEquals
                | TokenKind::AmpersandEquals
                | TokenKind::PipeEquals
                | TokenKind::CaretEquals
                | TokenKind::AmpersandAmpersandEquals
                | TokenKind::PipePipeEquals
                | TokenKind::QuestionQuestionEquals
        )
    }

    /// Check if this token starts an expression
    pub fn can_start_expression(&self) -> bool {
        matches!(
            self,
            TokenKind::Identifier
                | TokenKind::NumberLiteral
                | TokenKind::BigIntLiteral
                | TokenKind::StringLiteral
                | TokenKind::TemplateLiteral
                | TokenKind::TemplateHead
                | TokenKind::RegexLiteral
                | TokenKind::LeftParen
                | TokenKind::LeftBracket
                | TokenKind::LeftBrace
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Bang
                | TokenKind::Tilde
                | TokenKind::PlusPlus
                | TokenKind::MinusMinus
                | TokenKind::Keyword(Keyword::True)
                | TokenKind::Keyword(Keyword::False)
                | TokenKind::Keyword(Keyword::Null)
                | TokenKind::Keyword(Keyword::This)
                | TokenKind::Keyword(Keyword::Super)
                | TokenKind::Keyword(Keyword::New)
                | TokenKind::Keyword(Keyword::Function)
                | TokenKind::Keyword(Keyword::Class)
                | TokenKind::Keyword(Keyword::Async)
                | TokenKind::Keyword(Keyword::Typeof)
                | TokenKind::Keyword(Keyword::Void)
                | TokenKind::Keyword(Keyword::Delete)
                | TokenKind::Keyword(Keyword::Await)
                | TokenKind::Keyword(Keyword::Yield)
        )
    }
}
