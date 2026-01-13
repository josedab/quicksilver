//! JavaScript lexer/tokenizer
//!
//! This module implements a lexer for JavaScript that produces tokens
//! from source code. It supports ES2020 syntax including template literals,
//! regular expressions, and all keyword types.

mod token;

pub use token::{Keyword, Token, TokenKind};

use crate::error::{Error, Result, SourceLocation};

/// A lexer for JavaScript source code
pub struct Lexer<'src> {
    /// Source code being lexed
    source: &'src str,
    /// Source as bytes for faster access
    bytes: &'src [u8],
    /// Current position in bytes
    pos: usize,
    /// Current line number (1-indexed)
    line: u32,
    /// Current column number (1-indexed)
    column: u32,
    /// Start of current line in bytes
    line_start: usize,
}

impl<'src> Lexer<'src> {
    /// Create a new lexer for the given source code
    pub fn new(source: &'src str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            line: 1,
            column: 1,
            line_start: 0,
        }
    }

    /// Get current source location
    fn location(&self) -> SourceLocation {
        SourceLocation {
            line: self.line,
            column: self.column,
            offset: self.pos,
        }
    }

    /// Create a lexer error with source context
    fn error(&self, message: impl Into<String>, location: SourceLocation) -> Error {
        Error::lexer_error_with_context(message, location, self.source)
    }

    /// Check if we've reached the end of input
    fn is_eof(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// Peek at current character without consuming
    fn peek(&self) -> Option<char> {
        if self.is_eof() {
            None
        } else {
            self.source[self.pos..].chars().next()
        }
    }

    /// Peek at next character (one ahead)
    fn peek_next(&self) -> Option<char> {
        let mut chars = self.source[self.pos..].chars();
        chars.next();
        chars.next()
    }

    /// Advance and return current character
    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.column = 1;
            self.line_start = self.pos;
        } else {
            self.column += 1;
        }
        Some(c)
    }

    /// Skip whitespace and comments
    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip whitespace
            while let Some(c) = self.peek() {
                if c.is_whitespace() {
                    self.advance();
                } else {
                    break;
                }
            }

            // Check for comments
            if self.peek() == Some('/') {
                if self.peek_next() == Some('/') {
                    // Single-line comment
                    self.advance(); // /
                    self.advance(); // /
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                    continue;
                } else if self.peek_next() == Some('*') {
                    // Multi-line comment
                    self.advance(); // /
                    self.advance(); // *
                    let _start_loc = self.location(); // Saved for potential error location
                    loop {
                        match self.peek() {
                            None => break, // Unterminated comment, will error later
                            Some('*') if self.peek_next() == Some('/') => {
                                self.advance(); // *
                                self.advance(); // /
                                break;
                            }
                            _ => {
                                self.advance();
                            }
                        }
                    }
                    continue;
                }
            }

            break;
        }
    }

    /// Check if character can start an identifier
    fn is_id_start(c: char) -> bool {
        c == '_' || c == '$' || unicode_xid::UnicodeXID::is_xid_start(c)
    }

    /// Check if character can continue an identifier
    fn is_id_continue(c: char) -> bool {
        c == '_' || c == '$' || unicode_xid::UnicodeXID::is_xid_continue(c)
    }

    /// Scan an identifier or keyword
    fn scan_identifier(&mut self) -> Token<'src> {
        let start = self.pos;
        let start_loc = self.location();

        while let Some(c) = self.peek() {
            if Self::is_id_continue(c) {
                self.advance();
            } else {
                break;
            }
        }

        let text = &self.source[start..self.pos];
        let kind = match text {
            // Keywords
            "await" => TokenKind::Keyword(Keyword::Await),
            "break" => TokenKind::Keyword(Keyword::Break),
            "case" => TokenKind::Keyword(Keyword::Case),
            "catch" => TokenKind::Keyword(Keyword::Catch),
            "class" => TokenKind::Keyword(Keyword::Class),
            "const" => TokenKind::Keyword(Keyword::Const),
            "continue" => TokenKind::Keyword(Keyword::Continue),
            "debugger" => TokenKind::Keyword(Keyword::Debugger),
            "default" => TokenKind::Keyword(Keyword::Default),
            "delete" => TokenKind::Keyword(Keyword::Delete),
            "do" => TokenKind::Keyword(Keyword::Do),
            "else" => TokenKind::Keyword(Keyword::Else),
            "enum" => TokenKind::Keyword(Keyword::Enum),
            "export" => TokenKind::Keyword(Keyword::Export),
            "extends" => TokenKind::Keyword(Keyword::Extends),
            "false" => TokenKind::Keyword(Keyword::False),
            "finally" => TokenKind::Keyword(Keyword::Finally),
            "for" => TokenKind::Keyword(Keyword::For),
            "function" => TokenKind::Keyword(Keyword::Function),
            "if" => TokenKind::Keyword(Keyword::If),
            "import" => TokenKind::Keyword(Keyword::Import),
            "in" => TokenKind::Keyword(Keyword::In),
            "instanceof" => TokenKind::Keyword(Keyword::Instanceof),
            "let" => TokenKind::Keyword(Keyword::Let),
            "new" => TokenKind::Keyword(Keyword::New),
            "null" => TokenKind::Keyword(Keyword::Null),
            "return" => TokenKind::Keyword(Keyword::Return),
            "static" => TokenKind::Keyword(Keyword::Static),
            "super" => TokenKind::Keyword(Keyword::Super),
            "switch" => TokenKind::Keyword(Keyword::Switch),
            "this" => TokenKind::Keyword(Keyword::This),
            "throw" => TokenKind::Keyword(Keyword::Throw),
            "true" => TokenKind::Keyword(Keyword::True),
            "try" => TokenKind::Keyword(Keyword::Try),
            "typeof" => TokenKind::Keyword(Keyword::Typeof),
            "var" => TokenKind::Keyword(Keyword::Var),
            "void" => TokenKind::Keyword(Keyword::Void),
            "while" => TokenKind::Keyword(Keyword::While),
            "with" => TokenKind::Keyword(Keyword::With),
            "yield" => TokenKind::Keyword(Keyword::Yield),
            // Contextual keywords
            "as" => TokenKind::Keyword(Keyword::As),
            "async" => TokenKind::Keyword(Keyword::Async),
            "from" => TokenKind::Keyword(Keyword::From),
            "get" => TokenKind::Keyword(Keyword::Get),
            "of" => TokenKind::Keyword(Keyword::Of),
            "set" => TokenKind::Keyword(Keyword::Set),
            "target" => TokenKind::Keyword(Keyword::Target),
            "perform" => TokenKind::Keyword(Keyword::Perform),
            // Identifier
            _ => TokenKind::Identifier,
        };

        Token {
            kind,
            text,
            location: start_loc,
        }
    }

    /// Scan a numeric literal
    fn scan_number(&mut self) -> Result<Token<'src>> {
        let start = self.pos;
        let start_loc = self.location();

        // Check for hex, binary, or octal
        if self.peek() == Some('0') {
            self.advance();
            match self.peek() {
                Some('x') | Some('X') => {
                    self.advance();
                    while let Some(c) = self.peek() {
                        if c.is_ascii_hexdigit() {
                            self.advance();
                        } else if c == '_' {
                            self.advance(); // Numeric separator
                        } else {
                            break;
                        }
                    }
                    return Ok(Token {
                        kind: TokenKind::NumberLiteral,
                        text: &self.source[start..self.pos],
                        location: start_loc,
                    });
                }
                Some('b') | Some('B') => {
                    self.advance();
                    while let Some(c) = self.peek() {
                        if c == '0' || c == '1' || c == '_' {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    return Ok(Token {
                        kind: TokenKind::NumberLiteral,
                        text: &self.source[start..self.pos],
                        location: start_loc,
                    });
                }
                Some('o') | Some('O') => {
                    self.advance();
                    while let Some(c) = self.peek() {
                        if ('0'..='7').contains(&c) || c == '_' {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    return Ok(Token {
                        kind: TokenKind::NumberLiteral,
                        text: &self.source[start..self.pos],
                        location: start_loc,
                    });
                }
                Some('n') => {
                    self.advance();
                    return Ok(Token {
                        kind: TokenKind::BigIntLiteral,
                        text: &self.source[start..self.pos],
                        location: start_loc,
                    });
                }
                _ => {}
            }
        }

        // Decimal number
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }

        // Decimal point
        if self.peek() == Some('.') && self.peek_next().is_some_and(|c| c.is_ascii_digit()) {
            self.advance(); // .
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() || c == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        // Exponent
        if matches!(self.peek(), Some('e') | Some('E')) {
            self.advance();
            if matches!(self.peek(), Some('+') | Some('-')) {
                self.advance();
            }
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() || c == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        // BigInt suffix
        if self.peek() == Some('n') {
            self.advance();
            return Ok(Token {
                kind: TokenKind::BigIntLiteral,
                text: &self.source[start..self.pos],
                location: start_loc,
            });
        }

        Ok(Token {
            kind: TokenKind::NumberLiteral,
            text: &self.source[start..self.pos],
            location: start_loc,
        })
    }

    /// Scan a string literal
    fn scan_string(&mut self, quote: char) -> Result<Token<'src>> {
        let start = self.pos;
        let start_loc = self.location();
        self.advance(); // Opening quote

        loop {
            match self.peek() {
                None | Some('\n') | Some('\r') => {
                    return Err(self.error("Unterminated string literal", start_loc));
                }
                Some('\\') => {
                    self.advance();
                    self.advance(); // Escaped character
                }
                Some(c) if c == quote => {
                    self.advance();
                    break;
                }
                _ => {
                    self.advance();
                }
            }
        }

        Ok(Token {
            kind: TokenKind::StringLiteral,
            text: &self.source[start..self.pos],
            location: start_loc,
        })
    }

    /// Scan a template literal
    fn scan_template(&mut self) -> Result<Token<'src>> {
        let start = self.pos;
        let start_loc = self.location();
        self.advance(); // Opening backtick

        let mut has_substitution = false;
        loop {
            match self.peek() {
                None => {
                    return Err(self.error("Unterminated template literal", start_loc));
                }
                Some('\\') => {
                    self.advance();
                    self.advance();
                }
                Some('$') if self.peek_next() == Some('{') => {
                    // End template head/middle, substitution starts
                    // Include ${ in the token but it signals substitution
                    self.advance(); // consume $
                    self.advance(); // consume {
                    has_substitution = true;
                    break;
                }
                Some('`') => {
                    self.advance();
                    break;
                }
                _ => {
                    self.advance();
                }
            }
        }

        let kind = if has_substitution {
            TokenKind::TemplateHead
        } else {
            TokenKind::TemplateLiteral
        };

        Ok(Token {
            kind,
            text: &self.source[start..self.pos],
            location: start_loc,
        })
    }

    /// Continue scanning a template literal after a `}` closes a substitution
    /// This is called by the parser when it encounters `}` inside a template
    pub fn scan_template_continuation(&mut self) -> Result<Token<'src>> {
        let start = self.pos;
        let start_loc = self.location();

        let mut has_substitution = false;
        loop {
            match self.peek() {
                None => {
                    return Err(self.error("Unterminated template literal", start_loc));
                }
                Some('\\') => {
                    self.advance();
                    self.advance();
                }
                Some('$') if self.peek_next() == Some('{') => {
                    // Another substitution coming
                    self.advance(); // consume $
                    self.advance(); // consume {
                    has_substitution = true;
                    break;
                }
                Some('`') => {
                    self.advance();
                    break;
                }
                _ => {
                    self.advance();
                }
            }
        }

        let kind = if has_substitution {
            TokenKind::TemplateMiddle
        } else {
            TokenKind::TemplateTail
        };

        Ok(Token {
            kind,
            text: &self.source[start..self.pos],
            location: start_loc,
        })
    }

    /// Get the next token
    pub fn next_token(&mut self) -> Result<Token<'src>> {
        self.skip_whitespace_and_comments();

        if self.is_eof() {
            return Ok(Token {
                kind: TokenKind::Eof,
                text: "",
                location: self.location(),
            });
        }

        let start_loc = self.location();
        let c = self.peek().unwrap();

        // Identifiers and keywords
        if Self::is_id_start(c) {
            return Ok(self.scan_identifier());
        }

        // Numbers
        if c.is_ascii_digit() || (c == '.' && self.peek_next().is_some_and(|n| n.is_ascii_digit()))
        {
            return self.scan_number();
        }

        // Strings
        if c == '"' || c == '\'' {
            return self.scan_string(c);
        }

        // Template literals
        if c == '`' {
            return self.scan_template();
        }

        // Punctuators
        let start = self.pos;
        self.advance();

        let kind = match c {
            '(' => TokenKind::LeftParen,
            ')' => TokenKind::RightParen,
            '{' => TokenKind::LeftBrace,
            '}' => TokenKind::RightBrace,
            '[' => TokenKind::LeftBracket,
            ']' => TokenKind::RightBracket,
            ';' => TokenKind::Semicolon,
            ',' => TokenKind::Comma,
            ':' => TokenKind::Colon,
            '~' => TokenKind::Tilde,
            '?' => {
                if self.peek() == Some('.') && self.peek_next().is_none_or(|c| !c.is_ascii_digit())
                {
                    self.advance();
                    TokenKind::QuestionDot
                } else if self.peek() == Some('?') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::QuestionQuestionEquals
                    } else {
                        TokenKind::QuestionQuestion
                    }
                } else {
                    TokenKind::Question
                }
            }
            '.' => {
                if self.peek() == Some('.') && self.peek_next() == Some('.') {
                    self.advance();
                    self.advance();
                    TokenKind::DotDotDot
                } else {
                    TokenKind::Dot
                }
            }
            '+' => {
                if self.peek() == Some('+') {
                    self.advance();
                    TokenKind::PlusPlus
                } else if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::PlusEquals
                } else {
                    TokenKind::Plus
                }
            }
            '-' => {
                if self.peek() == Some('-') {
                    self.advance();
                    TokenKind::MinusMinus
                } else if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::MinusEquals
                } else {
                    TokenKind::Minus
                }
            }
            '*' => {
                if self.peek() == Some('*') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::StarStarEquals
                    } else {
                        TokenKind::StarStar
                    }
                } else if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::StarEquals
                } else {
                    TokenKind::Star
                }
            }
            '/' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::SlashEquals
                } else {
                    TokenKind::Slash
                }
            }
            '%' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::PercentEquals
                } else {
                    TokenKind::Percent
                }
            }
            '<' => {
                if self.peek() == Some('<') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::LessLessEquals
                    } else {
                        TokenKind::LessLess
                    }
                } else if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::LessEquals
                } else {
                    TokenKind::Less
                }
            }
            '>' => {
                if self.peek() == Some('>') {
                    self.advance();
                    if self.peek() == Some('>') {
                        self.advance();
                        if self.peek() == Some('=') {
                            self.advance();
                            TokenKind::GreaterGreaterGreaterEquals
                        } else {
                            TokenKind::GreaterGreaterGreater
                        }
                    } else if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::GreaterGreaterEquals
                    } else {
                        TokenKind::GreaterGreater
                    }
                } else if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::GreaterEquals
                } else {
                    TokenKind::Greater
                }
            }
            '=' => {
                if self.peek() == Some('=') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::EqualsEqualsEquals
                    } else {
                        TokenKind::EqualsEquals
                    }
                } else if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::Arrow
                } else {
                    TokenKind::Equals
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::BangEqualsEquals
                    } else {
                        TokenKind::BangEquals
                    }
                } else {
                    TokenKind::Bang
                }
            }
            '&' => {
                if self.peek() == Some('&') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::AmpersandAmpersandEquals
                    } else {
                        TokenKind::AmpersandAmpersand
                    }
                } else if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::AmpersandEquals
                } else {
                    TokenKind::Ampersand
                }
            }
            '|' => {
                if self.peek() == Some('|') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::PipePipeEquals
                    } else {
                        TokenKind::PipePipe
                    }
                } else if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::PipeEquals
                } else {
                    TokenKind::Pipe
                }
            }
            '^' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::CaretEquals
                } else {
                    TokenKind::Caret
                }
            }
            '#' => {
                // Private name - scan the identifier after #
                if let Some(next) = self.peek() {
                    if Self::is_id_start(next) {
                        while let Some(c) = self.peek() {
                            if Self::is_id_continue(c) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        return Ok(Token {
                            kind: TokenKind::PrivateName,
                            text: &self.source[start..self.pos],
                            location: start_loc,
                        });
                    }
                }
                return Err(self.error("Invalid private name", start_loc));
            }
            _ => {
                return Err(self.error(format!("Unexpected character '{}'", c), start_loc));
            }
        };

        Ok(Token {
            kind,
            text: &self.source[start..self.pos],
            location: start_loc,
        })
    }

    /// Tokenize the entire source into a vector of tokens
    pub fn tokenize(&mut self) -> Result<Vec<Token<'src>>> {
        let mut tokens = Vec::new();
        let mut template_depth: i32 = 0; // Track nested template literals
        let mut brace_depth_stack: Vec<i32> = Vec::new(); // Track brace depth for each template level

        loop {
            let token = self.next_token()?;
            let is_eof = token.kind == TokenKind::Eof;

            match token.kind {
                TokenKind::TemplateHead => {
                    // Starting a template with substitutions
                    template_depth += 1;
                    brace_depth_stack.push(0);
                    tokens.push(token);
                }
                TokenKind::LeftBrace if template_depth > 0 => {
                    // Track brace depth within template substitution
                    if let Some(depth) = brace_depth_stack.last_mut() {
                        *depth += 1;
                    }
                    tokens.push(token);
                }
                TokenKind::RightBrace if template_depth > 0 => {
                    if let Some(depth) = brace_depth_stack.last_mut() {
                        if *depth == 0 {
                            // This closes the template substitution
                            tokens.push(token);

                            // Continue scanning the template
                            let continuation = self.scan_template_continuation()?;
                            match continuation.kind {
                                TokenKind::TemplateTail => {
                                    template_depth -= 1;
                                    brace_depth_stack.pop();
                                    tokens.push(continuation);
                                }
                                TokenKind::TemplateMiddle => {
                                    // More substitutions coming
                                    tokens.push(continuation);
                                }
                                _ => {
                                    tokens.push(continuation);
                                }
                            }
                        } else {
                            *depth -= 1;
                            tokens.push(token);
                        }
                    } else {
                        tokens.push(token);
                    }
                }
                _ => {
                    tokens.push(token);
                }
            }

            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_source() {
        let mut lexer = Lexer::new("");
        let token = lexer.next_token().unwrap();
        assert_eq!(token.kind, TokenKind::Eof);
    }

    #[test]
    fn test_identifiers() {
        let mut lexer = Lexer::new("foo bar _private $jquery");
        assert_eq!(lexer.next_token().unwrap().text, "foo");
        assert_eq!(lexer.next_token().unwrap().text, "bar");
        assert_eq!(lexer.next_token().unwrap().text, "_private");
        assert_eq!(lexer.next_token().unwrap().text, "$jquery");
    }

    #[test]
    fn test_keywords() {
        let mut lexer = Lexer::new("let const function if else");
        assert_eq!(
            lexer.next_token().unwrap().kind,
            TokenKind::Keyword(Keyword::Let)
        );
        assert_eq!(
            lexer.next_token().unwrap().kind,
            TokenKind::Keyword(Keyword::Const)
        );
        assert_eq!(
            lexer.next_token().unwrap().kind,
            TokenKind::Keyword(Keyword::Function)
        );
        assert_eq!(
            lexer.next_token().unwrap().kind,
            TokenKind::Keyword(Keyword::If)
        );
        assert_eq!(
            lexer.next_token().unwrap().kind,
            TokenKind::Keyword(Keyword::Else)
        );
    }

    #[test]
    fn test_numbers() {
        let mut lexer = Lexer::new("42 3.14 0xFF 0b1010 0o777 1e10 123n");
        assert_eq!(lexer.next_token().unwrap().text, "42");
        assert_eq!(lexer.next_token().unwrap().text, "3.14");
        assert_eq!(lexer.next_token().unwrap().text, "0xFF");
        assert_eq!(lexer.next_token().unwrap().text, "0b1010");
        assert_eq!(lexer.next_token().unwrap().text, "0o777");
        assert_eq!(lexer.next_token().unwrap().text, "1e10");
        let bigint = lexer.next_token().unwrap();
        assert_eq!(bigint.kind, TokenKind::BigIntLiteral);
    }

    #[test]
    fn test_strings() {
        let mut lexer = Lexer::new(r#""hello" 'world' "with \"escape""#);
        assert_eq!(lexer.next_token().unwrap().text, r#""hello""#);
        assert_eq!(lexer.next_token().unwrap().text, "'world'");
        assert_eq!(lexer.next_token().unwrap().text, r#""with \"escape""#);
    }

    #[test]
    fn test_operators() {
        let mut lexer = Lexer::new("+ - * / === !== ?? ?.");
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Plus);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Minus);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Star);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Slash);
        assert_eq!(
            lexer.next_token().unwrap().kind,
            TokenKind::EqualsEqualsEquals
        );
        assert_eq!(
            lexer.next_token().unwrap().kind,
            TokenKind::BangEqualsEquals
        );
        assert_eq!(
            lexer.next_token().unwrap().kind,
            TokenKind::QuestionQuestion
        );
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::QuestionDot);
    }

    #[test]
    fn test_comments() {
        let mut lexer = Lexer::new("foo // comment\nbar /* block */ baz");
        assert_eq!(lexer.next_token().unwrap().text, "foo");
        assert_eq!(lexer.next_token().unwrap().text, "bar");
        assert_eq!(lexer.next_token().unwrap().text, "baz");
    }
}
