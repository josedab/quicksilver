//! JavaScript parser
//!
//! This module implements a recursive descent parser for JavaScript.
//! It produces an AST from a stream of tokens.

use crate::ast::*;
use crate::error::{Error, Result, SourceLocation};
use crate::lexer::{Keyword, Lexer, Token, TokenKind};

/// Parser state flags
#[derive(Debug, Clone, Copy, Default)]
struct ParserFlags {
    /// Inside a function
    in_function: bool,
    /// Inside an async function
    in_async: bool,
    /// Inside a generator function
    in_generator: bool,
    /// Inside a loop (for, while, etc.)
    in_loop: bool,
    /// Inside a switch statement
    in_switch: bool,
    /// Parsing strict mode code (reserved for future use)
    #[allow(dead_code)]
    strict: bool,
}

/// A recursive descent parser for JavaScript
pub struct Parser<'src> {
    /// Source code (kept for error messages and source maps)
    #[allow(dead_code)]
    source: &'src str,
    /// Tokens to parse
    tokens: Vec<Token<'src>>,
    /// Current position in tokens
    pos: usize,
    /// Parser state flags
    flags: ParserFlags,
    /// Collected parse errors (for error recovery mode)
    errors: Vec<Error>,
    /// Maximum errors to collect before giving up
    max_errors: usize,
}

impl<'src> Parser<'src> {
    /// Create a new parser from source code
    pub fn new(source: &'src str) -> Result<Self> {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize()?;
        Ok(Self {
            source,
            tokens,
            pos: 0,
            flags: ParserFlags::default(),
            errors: Vec::new(),
            max_errors: 10,
        })
    }

    /// Synchronize parser state after an error by skipping to next statement boundary
    fn synchronize(&mut self) {
        self.advance();
        while !self.is_eof() {
            // Stop at statement terminators
            if self.previous().kind == TokenKind::Semicolon {
                return;
            }
            // Stop at statement-starting keywords
            match self.current().kind {
                TokenKind::Keyword(Keyword::Function)
                | TokenKind::Keyword(Keyword::Var)
                | TokenKind::Keyword(Keyword::Let)
                | TokenKind::Keyword(Keyword::Const)
                | TokenKind::Keyword(Keyword::Class)
                | TokenKind::Keyword(Keyword::If)
                | TokenKind::Keyword(Keyword::While)
                | TokenKind::Keyword(Keyword::For)
                | TokenKind::Keyword(Keyword::Return)
                | TokenKind::Keyword(Keyword::Try) => return,
                _ => {}
            }
            self.advance();
        }
    }

    /// Get previous token (for synchronize)
    fn previous(&self) -> &Token<'src> {
        &self.tokens[self.pos.saturating_sub(1)]
    }

    /// Record an error and continue parsing (error recovery mode)
    fn record_error(&mut self, error: Error) {
        self.errors.push(error);
    }

    /// Check if we've hit the error limit
    fn too_many_errors(&self) -> bool {
        self.errors.len() >= self.max_errors
    }

    /// Get all collected errors
    pub fn get_errors(&self) -> &[Error] {
        &self.errors
    }

    /// Parse with error recovery, returning partial AST and collected errors
    pub fn parse_program_with_recovery(&mut self) -> (Program, Vec<Error>) {
        let start = self.location();
        let mut body = Vec::new();

        // Check for "use strict" directive at the start
        if self.check_use_strict_directive() {
            self.flags.strict = true;
            if let Ok(stmt) = self.parse_statement() {
                body.push(stmt);
            }
        }

        while !self.is_eof() && !self.too_many_errors() {
            match self.parse_statement() {
                Ok(stmt) => body.push(stmt),
                Err(e) => {
                    self.record_error(e);
                    self.synchronize();
                }
            }
        }

        let program = Program {
            body,
            source_type: SourceType::Script, // Default to script mode
            span: Span::new(start, self.location()),
            strict: self.flags.strict,
        };
        let errors = std::mem::take(&mut self.errors);
        (program, errors)
    }

    /// Check if the current position has a "use strict" directive
    fn check_use_strict_directive(&self) -> bool {
        let token = self.current();
        if token.kind == TokenKind::StringLiteral {
            // token.text includes the quotes, so check for both single and double quoted versions
            let text = token.text;
            text == "\"use strict\"" || text == "'use strict'"
        } else {
            false
        }
    }

    /// Parse the source as a complete program
    pub fn parse_program(&mut self) -> Result<Program> {
        let start = self.location();
        let mut body = Vec::new();

        // Check for "use strict" directive at the start
        if self.check_use_strict_directive() {
            self.flags.strict = true;
            // Parse and include the directive as an expression statement
            body.push(self.parse_statement()?);
        }

        while !self.is_eof() {
            body.push(self.parse_statement()?);
        }

        let end = self.location();
        Ok(Program {
            body,
            source_type: SourceType::Script,
            strict: self.flags.strict,
            span: Span::new(start, end),
        })
    }

    /// Parse a single expression
    pub fn parse_expression(&mut self) -> Result<Expression> {
        self.parse_assignment_expression()
    }

    // ========== Token Access ==========

    fn current(&self) -> &Token<'src> {
        &self.tokens[self.pos]
    }

    fn peek(&self) -> TokenKind {
        self.tokens[self.pos].kind
    }

    fn peek_at(&self, offset: usize) -> TokenKind {
        self.tokens
            .get(self.pos + offset)
            .map(|t| t.kind)
            .unwrap_or(TokenKind::Eof)
    }

    fn is_eof(&self) -> bool {
        self.peek() == TokenKind::Eof
    }

    fn location(&self) -> SourceLocation {
        self.current().location
    }

    /// Create a parse error with source context
    fn error(&self, message: impl Into<String>, location: SourceLocation) -> Error {
        Error::parse_error_with_context(message, location, self.source)
    }

    fn advance(&mut self) -> &Token<'src> {
        let token = &self.tokens[self.pos];
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        token
    }

    fn expect(&mut self, kind: TokenKind) -> Result<&Token<'src>> {
        if self.peek() == kind {
            Ok(self.advance())
        } else {
            let loc = self.location();
            Err(self.error(
                format!("Expected {:?}, found {:?}", kind, self.peek()),
                loc,
            ))
        }
    }

    fn expect_keyword(&mut self, keyword: Keyword) -> Result<&Token<'src>> {
        if self.peek() == TokenKind::Keyword(keyword) {
            Ok(self.advance())
        } else {
            let loc = self.location();
            Err(self.error(
                format!("Expected '{}', found {:?}", keyword, self.peek()),
                loc,
            ))
        }
    }

    fn consume(&mut self, kind: TokenKind) -> bool {
        if self.peek() == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    fn consume_semicolon(&mut self) -> bool {
        // Automatic semicolon insertion (ASI)
        if self.consume(TokenKind::Semicolon) {
            return true;
        }

        // ASI: newline, }, or EOF
        if self.peek() == TokenKind::RightBrace || self.peek() == TokenKind::Eof {
            return true;
        }

        // Check if there was a newline between tokens
        if self.pos > 0 {
            let prev = &self.tokens[self.pos - 1];
            let curr = self.current();
            if prev.location.line < curr.location.line {
                return true;
            }
        }

        false
    }

    // ========== Statements ==========

    fn parse_statement(&mut self) -> Result<Statement> {
        match self.peek() {
            TokenKind::LeftBrace => self.parse_block_statement().map(Statement::Block),
            TokenKind::Semicolon => {
                let loc = self.location();
                self.advance();
                Ok(Statement::Empty(Span::new(loc, self.location())))
            }
            TokenKind::Keyword(Keyword::Var) => self
                .parse_variable_declaration(VariableKind::Var)
                .map(Statement::VariableDeclaration),
            TokenKind::Keyword(Keyword::Let) => self
                .parse_variable_declaration(VariableKind::Let)
                .map(Statement::VariableDeclaration),
            TokenKind::Keyword(Keyword::Const) => self
                .parse_variable_declaration(VariableKind::Const)
                .map(Statement::VariableDeclaration),
            TokenKind::Keyword(Keyword::Function) => self.parse_function_declaration(),
            TokenKind::Keyword(Keyword::Async)
                if self.peek_at(1) == TokenKind::Keyword(Keyword::Function) =>
            {
                self.parse_function_declaration()
            }
            TokenKind::Keyword(Keyword::Class) => self.parse_class_declaration(),
            TokenKind::Keyword(Keyword::If) => self.parse_if_statement(),
            TokenKind::Keyword(Keyword::While) => self.parse_while_statement(),
            TokenKind::Keyword(Keyword::Do) => self.parse_do_while_statement(),
            TokenKind::Keyword(Keyword::For) => self.parse_for_statement(),
            TokenKind::Keyword(Keyword::Switch) => self.parse_switch_statement(),
            TokenKind::Keyword(Keyword::Break) => self.parse_break_statement(),
            TokenKind::Keyword(Keyword::Continue) => self.parse_continue_statement(),
            TokenKind::Keyword(Keyword::Return) => self.parse_return_statement(),
            TokenKind::Keyword(Keyword::Throw) => self.parse_throw_statement(),
            TokenKind::Keyword(Keyword::Try) => self.parse_try_statement(),
            TokenKind::Keyword(Keyword::Debugger) => {
                let start = self.location();
                self.advance();
                self.consume_semicolon();
                Ok(Statement::Debugger(Span::new(start, self.location())))
            }
            TokenKind::Keyword(Keyword::Import) => self.parse_import_declaration(),
            TokenKind::Keyword(Keyword::Export) => self.parse_export_declaration(),
            _ => self.parse_expression_statement(),
        }
    }

    fn parse_block_statement(&mut self) -> Result<BlockStatement> {
        let start = self.location();
        self.expect(TokenKind::LeftBrace)?;

        let mut body = Vec::new();
        while !self.consume(TokenKind::RightBrace) {
            if self.is_eof() {
                let loc = self.location();
                return Err(self.error("Unexpected end of input", loc));
            }
            body.push(self.parse_statement()?);
        }

        Ok(BlockStatement {
            body,
            span: Span::new(start, self.location()),
        })
    }

    /// Parse a function body with "use strict" directive detection
    /// This checks for "use strict" at the start of the body and enforces
    /// strict mode rules like duplicate parameter checking
    fn parse_function_block_statement(
        &mut self,
        params: &FunctionParams,
        func_start: SourceLocation,
    ) -> Result<BlockStatement> {
        let start = self.location();
        self.expect(TokenKind::LeftBrace)?;

        let mut body = Vec::new();

        // Check for "use strict" at the start of function body
        if self.check_use_strict_directive() {
            // Parse the "use strict" directive first
            body.push(self.parse_statement()?);

            // If not already in strict mode, enter it and re-check parameters
            if !self.flags.strict {
                self.flags.strict = true;
                // Re-check parameters for duplicates now that we're in strict mode
                self.check_duplicate_params(params, func_start)?;
            }
        }

        while !self.consume(TokenKind::RightBrace) {
            if self.is_eof() {
                let loc = self.location();
                return Err(self.error("Unexpected end of input", loc));
            }
            body.push(self.parse_statement()?);
        }

        Ok(BlockStatement {
            body,
            span: Span::new(start, self.location()),
        })
    }

    fn parse_variable_declaration(&mut self, kind: VariableKind) -> Result<VariableDeclaration> {
        let start = self.location();
        self.advance(); // var/let/const

        let mut declarations = Vec::new();
        loop {
            let decl = self.parse_variable_declarator(kind)?;
            declarations.push(decl);

            if !self.consume(TokenKind::Comma) {
                break;
            }
        }

        self.consume_semicolon();

        Ok(VariableDeclaration {
            kind,
            declarations,
            span: Span::new(start, self.location()),
        })
    }

    fn parse_variable_declarator(&mut self, kind: VariableKind) -> Result<VariableDeclarator> {
        let start = self.location();
        let id = self.parse_binding_pattern()?;

        let init = if self.consume(TokenKind::Equals) {
            Some(Box::new(self.parse_assignment_expression()?))
        } else if kind == VariableKind::Const {
            return Err(self.error("const declarations must be initialized", start));
        } else {
            None
        };

        Ok(VariableDeclarator {
            id,
            init,
            span: Span::new(start, self.location()),
        })
    }

    fn parse_binding_pattern(&mut self) -> Result<Pattern> {
        match self.peek() {
            TokenKind::LeftBracket => self.parse_array_pattern(),
            TokenKind::LeftBrace => self.parse_object_pattern(),
            TokenKind::Identifier => {
                let id = self.parse_identifier()?;
                // Don't consume '=' here - let the caller handle it
                // Assignment patterns are only valid inside destructuring contexts
                Ok(Pattern::Identifier(id))
            }
            _ => {
                let loc = self.location();
                Err(self.error("Expected identifier or destructuring pattern", loc))
            }
        }
    }

    fn parse_array_pattern(&mut self) -> Result<Pattern> {
        let start = self.location();
        self.expect(TokenKind::LeftBracket)?;

        let mut elements = Vec::new();
        let mut rest = None;

        while !self.consume(TokenKind::RightBracket) {
            if self.consume(TokenKind::Comma) {
                elements.push(None);
                continue;
            }

            if self.consume(TokenKind::DotDotDot) {
                let arg = self.parse_binding_pattern()?;
                rest = Some(Box::new(arg));
                self.consume(TokenKind::Comma);
                self.expect(TokenKind::RightBracket)?;
                break;
            }

            elements.push(Some(self.parse_binding_pattern()?));

            if !self.consume(TokenKind::Comma) {
                self.expect(TokenKind::RightBracket)?;
                break;
            }
        }

        Ok(Pattern::Array(ArrayPattern {
            elements,
            rest,
            span: Span::new(start, self.location()),
        }))
    }

    fn parse_object_pattern(&mut self) -> Result<Pattern> {
        let start = self.location();
        self.expect(TokenKind::LeftBrace)?;

        let mut properties = Vec::new();

        while !self.consume(TokenKind::RightBrace) {
            if self.consume(TokenKind::DotDotDot) {
                let prop_start = self.location();
                let arg = self.parse_binding_pattern()?;
                properties.push(ObjectPatternProperty::Rest {
                    argument: arg,
                    span: Span::new(prop_start, self.location()),
                });
                self.consume(TokenKind::Comma);
                self.expect(TokenKind::RightBrace)?;
                break;
            }

            let prop = self.parse_object_pattern_property()?;
            properties.push(prop);

            if !self.consume(TokenKind::Comma) {
                self.expect(TokenKind::RightBrace)?;
                break;
            }
        }

        Ok(Pattern::Object(ObjectPattern {
            properties,
            span: Span::new(start, self.location()),
        }))
    }

    fn parse_object_pattern_property(&mut self) -> Result<ObjectPatternProperty> {
        let start = self.location();
        let computed = self.peek() == TokenKind::LeftBracket;

        let key = self.parse_property_key()?;

        // Check for shorthand {foo} or {foo = default}
        if !computed && !self.consume(TokenKind::Colon) {
            let id = match &key {
                PropertyKey::Identifier(id) => id.clone(),
                _ => return Err(self.error("Shorthand property must be an identifier", start)),
            };

            let value = if self.consume(TokenKind::Equals) {
                let right = self.parse_assignment_expression()?;
                let span = id.span.merge(right.span());
                Pattern::Assignment(Box::new(AssignmentPattern {
                    left: Pattern::Identifier(id.clone()),
                    right,
                    span,
                }))
            } else {
                Pattern::Identifier(id)
            };

            return Ok(ObjectPatternProperty::Property {
                key,
                value,
                shorthand: true,
                computed: false,
                span: Span::new(start, self.location()),
            });
        }

        let value = self.parse_binding_pattern()?;

        Ok(ObjectPatternProperty::Property {
            key,
            value,
            shorthand: false,
            computed,
            span: Span::new(start, self.location()),
        })
    }

    fn parse_property_key(&mut self) -> Result<PropertyKey> {
        if self.consume(TokenKind::LeftBracket) {
            let expr = self.parse_assignment_expression()?;
            self.expect(TokenKind::RightBracket)?;
            return Ok(PropertyKey::Computed(Box::new(expr)));
        }

        match self.peek() {
            TokenKind::Identifier | TokenKind::Keyword(_) => {
                let id = self.parse_identifier_name()?;
                Ok(PropertyKey::Identifier(id))
            }
            TokenKind::StringLiteral => {
                let text = self.advance().text.to_string();
                let s = self.parse_string_value(&text)?;
                Ok(PropertyKey::String(s))
            }
            TokenKind::NumberLiteral => {
                let text = self.advance().text.to_string();
                let n = self.parse_number_value(&text)?;
                Ok(PropertyKey::Number(n))
            }
            TokenKind::PrivateName => {
                let text = self.advance().text.to_string();
                // Strip the leading # from the private name
                let name = text[1..].to_string();
                Ok(PropertyKey::PrivateName(name))
            }
            _ => {
                let loc = self.location();
                Err(self.error("Expected property name", loc))
            }
        }
    }

    fn parse_function_declaration(&mut self) -> Result<Statement> {
        let func = self.parse_function(true, false)?;
        Ok(Statement::FunctionDeclaration(Box::new(func)))
    }

    fn parse_function(&mut self, require_name: bool, _is_expression: bool) -> Result<Function> {
        let start = self.location();
        let is_async = self.consume(TokenKind::Keyword(Keyword::Async));
        self.expect_keyword(Keyword::Function)?;
        let is_generator = self.consume(TokenKind::Star);

        let id = if self.peek() == TokenKind::Identifier {
            Some(self.parse_identifier()?)
        } else if require_name {
            let loc = self.location();
            return Err(self.error("Function declaration requires a name", loc));
        } else {
            None
        };

        self.expect(TokenKind::LeftParen)?;
        let params = self.parse_function_params()?;
        self.expect(TokenKind::RightParen)?;

        // Check for duplicate parameters in strict mode
        self.check_duplicate_params(&params, start)?;

        let old_flags = self.flags;
        self.flags.in_function = true;
        self.flags.in_async = is_async;
        self.flags.in_generator = is_generator;

        // Parse function body with "use strict" directive detection
        let body = self.parse_function_block_statement(&params, start)?;

        self.flags = old_flags;

        Ok(Function {
            id,
            params,
            body: FunctionBody::Block(body),
            is_async,
            is_generator,
            span: Span::new(start, self.location()),
        })
    }

    fn parse_function_params(&mut self) -> Result<FunctionParams> {
        let mut params = Vec::new();
        let mut rest = None;

        while self.peek() != TokenKind::RightParen {
            if self.consume(TokenKind::DotDotDot) {
                let pattern = self.parse_binding_pattern()?;
                rest = Some(Box::new(pattern));
                break;
            }

            let start = self.location();
            let pattern = self.parse_binding_pattern()?;

            // Check for default parameter value
            let param = if self.consume(TokenKind::Equals) {
                let default_value = self.parse_assignment_expression()?;
                Pattern::Assignment(Box::new(AssignmentPattern {
                    left: pattern,
                    right: default_value,
                    span: Span::new(start, self.location()),
                }))
            } else {
                pattern
            };

            params.push(param);

            if !self.consume(TokenKind::Comma) {
                break;
            }
        }

        Ok(FunctionParams { params, rest })
    }

    /// Check for duplicate parameter names in strict mode
    fn check_duplicate_params(&self, params: &FunctionParams, start_loc: SourceLocation) -> Result<()> {
        if !self.flags.strict {
            return Ok(());
        }

        let mut seen = std::collections::HashSet::new();

        // Collect all parameter names
        for param in &params.params {
            for name in param.bound_names() {
                if !seen.insert(&name.name) {
                    return Err(self.error(
                        format!("Duplicate parameter name '{}' not allowed in strict mode", name.name),
                        start_loc,
                    ));
                }
            }
        }

        // Also check rest parameter
        if let Some(rest) = &params.rest {
            for name in rest.bound_names() {
                if !seen.insert(&name.name) {
                    return Err(self.error(
                        format!("Duplicate parameter name '{}' not allowed in strict mode", name.name),
                        start_loc,
                    ));
                }
            }
        }

        Ok(())
    }

    fn parse_class_declaration(&mut self) -> Result<Statement> {
        let class = self.parse_class(true)?;
        Ok(Statement::ClassDeclaration(Box::new(class)))
    }

    fn parse_class(&mut self, require_name: bool) -> Result<Class> {
        let start = self.location();
        self.expect_keyword(Keyword::Class)?;

        let id = if self.peek() == TokenKind::Identifier {
            Some(self.parse_identifier()?)
        } else if require_name {
            let loc = self.location();
            return Err(self.error("Class declaration requires a name", loc));
        } else {
            None
        };

        let super_class = if self.consume(TokenKind::Keyword(Keyword::Extends)) {
            Some(Box::new(self.parse_left_hand_side_expression()?))
        } else {
            None
        };

        let body = self.parse_class_body()?;

        Ok(Class {
            id,
            super_class,
            body,
            span: Span::new(start, self.location()),
        })
    }

    fn parse_class_body(&mut self) -> Result<ClassBody> {
        let start = self.location();
        self.expect(TokenKind::LeftBrace)?;

        let mut body = Vec::new();

        while !self.consume(TokenKind::RightBrace) {
            if self.consume(TokenKind::Semicolon) {
                continue;
            }

            let element = self.parse_class_element()?;
            body.push(element);
        }

        Ok(ClassBody {
            body,
            span: Span::new(start, self.location()),
        })
    }

    fn parse_class_element(&mut self) -> Result<ClassElement> {
        let start = self.location();
        let is_static = self.consume(TokenKind::Keyword(Keyword::Static));

        // Static block
        if is_static && self.peek() == TokenKind::LeftBrace {
            let block = self.parse_block_statement()?;
            return Ok(ClassElement::StaticBlock(block));
        }

        // Method or property
        let is_async = self.consume(TokenKind::Keyword(Keyword::Async));
        let is_generator = self.consume(TokenKind::Star);

        let kind = if self.peek() == TokenKind::Keyword(Keyword::Get)
            && self.peek_at(1) != TokenKind::LeftParen
        {
            self.advance();
            MethodKind::Get
        } else if self.peek() == TokenKind::Keyword(Keyword::Set)
            && self.peek_at(1) != TokenKind::LeftParen
        {
            self.advance();
            MethodKind::Set
        } else {
            MethodKind::Method
        };

        let computed = self.peek() == TokenKind::LeftBracket;
        let key = self.parse_property_key()?;

        // Check if this is constructor
        let kind = if !computed
            && matches!(&key, PropertyKey::Identifier(id) if id.name == "constructor")
        {
            MethodKind::Constructor
        } else {
            kind
        };

        // Method
        if self.peek() == TokenKind::LeftParen {
            self.expect(TokenKind::LeftParen)?;
            let params = self.parse_function_params()?;
            self.expect(TokenKind::RightParen)?;

            let old_flags = self.flags;
            self.flags.in_function = true;
            self.flags.in_async = is_async;
            self.flags.in_generator = is_generator;

            let body = self.parse_block_statement()?;

            self.flags = old_flags;

            let func = Function {
                id: None,
                params,
                body: FunctionBody::Block(body),
                is_async,
                is_generator,
                span: Span::new(start, self.location()),
            };

            return Ok(ClassElement::Method(MethodDefinition {
                key,
                value: func,
                kind,
                computed,
                is_static,
                span: Span::new(start, self.location()),
            }));
        }

        // Property
        let value = if self.consume(TokenKind::Equals) {
            Some(Box::new(self.parse_assignment_expression()?))
        } else {
            None
        };

        self.consume_semicolon();

        Ok(ClassElement::Property(PropertyDefinition {
            key,
            value,
            computed,
            is_static,
            span: Span::new(start, self.location()),
        }))
    }

    fn parse_if_statement(&mut self) -> Result<Statement> {
        let start = self.location();
        self.expect_keyword(Keyword::If)?;
        self.expect(TokenKind::LeftParen)?;
        let test = self.parse_expression()?;
        self.expect(TokenKind::RightParen)?;

        let consequent = self.parse_statement()?;

        let alternate = if self.consume(TokenKind::Keyword(Keyword::Else)) {
            Some(self.parse_statement()?)
        } else {
            None
        };

        Ok(Statement::If(Box::new(IfStatement {
            test,
            consequent,
            alternate,
            span: Span::new(start, self.location()),
        })))
    }

    fn parse_while_statement(&mut self) -> Result<Statement> {
        let start = self.location();
        self.expect_keyword(Keyword::While)?;
        self.expect(TokenKind::LeftParen)?;
        let test = self.parse_expression()?;
        self.expect(TokenKind::RightParen)?;

        let old_in_loop = self.flags.in_loop;
        self.flags.in_loop = true;
        let body = self.parse_statement()?;
        self.flags.in_loop = old_in_loop;

        Ok(Statement::While(Box::new(WhileStatement {
            test,
            body,
            span: Span::new(start, self.location()),
        })))
    }

    fn parse_do_while_statement(&mut self) -> Result<Statement> {
        let start = self.location();
        self.expect_keyword(Keyword::Do)?;

        let old_in_loop = self.flags.in_loop;
        self.flags.in_loop = true;
        let body = self.parse_statement()?;
        self.flags.in_loop = old_in_loop;

        self.expect_keyword(Keyword::While)?;
        self.expect(TokenKind::LeftParen)?;
        let test = self.parse_expression()?;
        self.expect(TokenKind::RightParen)?;
        self.consume_semicolon();

        Ok(Statement::DoWhile(Box::new(DoWhileStatement {
            body,
            test,
            span: Span::new(start, self.location()),
        })))
    }

    fn parse_for_statement(&mut self) -> Result<Statement> {
        let start = self.location();
        self.expect_keyword(Keyword::For)?;
        let is_await = self.consume(TokenKind::Keyword(Keyword::Await));
        self.expect(TokenKind::LeftParen)?;

        // Parse init
        let init = match self.peek() {
            TokenKind::Semicolon => None,
            TokenKind::Keyword(Keyword::Var) => {
                let decl = self.parse_variable_declaration_no_semi(VariableKind::Var)?;
                Some(ForInit::Declaration(decl))
            }
            TokenKind::Keyword(Keyword::Let) => {
                let decl = self.parse_variable_declaration_no_semi(VariableKind::Let)?;
                Some(ForInit::Declaration(decl))
            }
            TokenKind::Keyword(Keyword::Const) => {
                let decl = self.parse_variable_declaration_no_semi(VariableKind::Const)?;
                Some(ForInit::Declaration(decl))
            }
            _ => Some(ForInit::Expression(self.parse_expression_no_in()?)),
        };

        // Check for for-in or for-of
        if self.consume(TokenKind::Keyword(Keyword::In)) {
            let left = match init {
                Some(ForInit::Declaration(decl)) => ForInLeft::Declaration(decl),
                Some(ForInit::Expression(expr)) => ForInLeft::Expression(expr),
                None => {
                    let loc = self.location();
                    return Err(self.error("for-in statement requires a left-hand side", loc));
                }
            };

            let right = self.parse_expression()?;
            self.expect(TokenKind::RightParen)?;

            let old_in_loop = self.flags.in_loop;
            self.flags.in_loop = true;
            let body = self.parse_statement()?;
            self.flags.in_loop = old_in_loop;

            return Ok(Statement::ForIn(Box::new(ForInStatement {
                left,
                right,
                body,
                span: Span::new(start, self.location()),
            })));
        }

        if self.consume(TokenKind::Keyword(Keyword::Of)) {
            let left = match init {
                Some(ForInit::Declaration(decl)) => ForInLeft::Declaration(decl),
                Some(ForInit::Expression(expr)) => ForInLeft::Expression(expr),
                None => {
                    let loc = self.location();
                    return Err(self.error("for-of statement requires a left-hand side", loc));
                }
            };

            let right = self.parse_assignment_expression()?;
            self.expect(TokenKind::RightParen)?;

            let old_in_loop = self.flags.in_loop;
            self.flags.in_loop = true;
            let body = self.parse_statement()?;
            self.flags.in_loop = old_in_loop;

            return Ok(Statement::ForOf(Box::new(ForOfStatement {
                left,
                right,
                body,
                is_await,
                span: Span::new(start, self.location()),
            })));
        }

        // Regular for loop
        self.expect(TokenKind::Semicolon)?;

        let test = if self.peek() != TokenKind::Semicolon {
            Some(self.parse_expression()?)
        } else {
            None
        };
        self.expect(TokenKind::Semicolon)?;

        let update = if self.peek() != TokenKind::RightParen {
            Some(self.parse_expression()?)
        } else {
            None
        };
        self.expect(TokenKind::RightParen)?;

        let old_in_loop = self.flags.in_loop;
        self.flags.in_loop = true;
        let body = self.parse_statement()?;
        self.flags.in_loop = old_in_loop;

        Ok(Statement::For(Box::new(ForStatement {
            init,
            test,
            update,
            body,
            span: Span::new(start, self.location()),
        })))
    }

    fn parse_variable_declaration_no_semi(
        &mut self,
        kind: VariableKind,
    ) -> Result<VariableDeclaration> {
        let start = self.location();
        self.advance(); // var/let/const

        let mut declarations = Vec::new();
        loop {
            let decl_start = self.location();
            let id = self.parse_binding_pattern()?;

            // For for-in/of, don't require initializer
            let init = if self.consume(TokenKind::Equals) {
                Some(Box::new(self.parse_assignment_expression_no_in()?))
            } else {
                None
            };

            declarations.push(VariableDeclarator {
                id,
                init,
                span: Span::new(decl_start, self.location()),
            });

            if !self.consume(TokenKind::Comma) {
                break;
            }
        }

        Ok(VariableDeclaration {
            kind,
            declarations,
            span: Span::new(start, self.location()),
        })
    }

    fn parse_switch_statement(&mut self) -> Result<Statement> {
        let start = self.location();
        self.expect_keyword(Keyword::Switch)?;
        self.expect(TokenKind::LeftParen)?;
        let discriminant = self.parse_expression()?;
        self.expect(TokenKind::RightParen)?;

        self.expect(TokenKind::LeftBrace)?;

        let old_in_switch = self.flags.in_switch;
        self.flags.in_switch = true;

        let mut cases = Vec::new();
        while !self.consume(TokenKind::RightBrace) {
            let case = self.parse_switch_case()?;
            cases.push(case);
        }

        self.flags.in_switch = old_in_switch;

        Ok(Statement::Switch(Box::new(SwitchStatement {
            discriminant,
            cases,
            span: Span::new(start, self.location()),
        })))
    }

    fn parse_switch_case(&mut self) -> Result<SwitchCase> {
        let start = self.location();

        let test = if self.consume(TokenKind::Keyword(Keyword::Case)) {
            Some(self.parse_expression()?)
        } else {
            self.expect_keyword(Keyword::Default)?;
            None
        };

        self.expect(TokenKind::Colon)?;

        let mut consequent = Vec::new();
        while !matches!(
            self.peek(),
            TokenKind::Keyword(Keyword::Case)
                | TokenKind::Keyword(Keyword::Default)
                | TokenKind::RightBrace
        ) {
            consequent.push(self.parse_statement()?);
        }

        Ok(SwitchCase {
            test,
            consequent,
            span: Span::new(start, self.location()),
        })
    }

    fn parse_break_statement(&mut self) -> Result<Statement> {
        let start = self.location();
        self.expect_keyword(Keyword::Break)?;

        let label = if !self.consume_semicolon() && self.peek() == TokenKind::Identifier {
            Some(self.parse_identifier()?)
        } else {
            None
        };

        self.consume_semicolon();

        if label.is_none() && !self.flags.in_loop && !self.flags.in_switch {
            return Err(self.error("Illegal break statement", start));
        }

        Ok(Statement::Break(BreakStatement {
            label,
            span: Span::new(start, self.location()),
        }))
    }

    fn parse_continue_statement(&mut self) -> Result<Statement> {
        let start = self.location();
        self.expect_keyword(Keyword::Continue)?;

        let label = if !self.consume_semicolon() && self.peek() == TokenKind::Identifier {
            Some(self.parse_identifier()?)
        } else {
            None
        };

        self.consume_semicolon();

        if !self.flags.in_loop {
            return Err(self.error("Illegal continue statement", start));
        }

        Ok(Statement::Continue(ContinueStatement {
            label,
            span: Span::new(start, self.location()),
        }))
    }

    fn parse_return_statement(&mut self) -> Result<Statement> {
        let start = self.location();
        self.expect_keyword(Keyword::Return)?;

        if !self.flags.in_function {
            return Err(self.error("Illegal return statement", start));
        }

        let argument = if self.consume_semicolon() {
            None
        } else {
            let expr = self.parse_expression()?;
            self.consume_semicolon();
            Some(expr)
        };

        Ok(Statement::Return(ReturnStatement {
            argument,
            span: Span::new(start, self.location()),
        }))
    }

    fn parse_throw_statement(&mut self) -> Result<Statement> {
        let start = self.location();
        self.expect_keyword(Keyword::Throw)?;

        // No newline allowed between throw and argument
        let argument = self.parse_expression()?;
        self.consume_semicolon();

        Ok(Statement::Throw(Box::new(ThrowStatement {
            argument,
            span: Span::new(start, self.location()),
        })))
    }

    fn parse_try_statement(&mut self) -> Result<Statement> {
        let start = self.location();
        self.expect_keyword(Keyword::Try)?;

        let block = self.parse_block_statement()?;

        let handler = if self.consume(TokenKind::Keyword(Keyword::Catch)) {
            let catch_start = self.location();

            let param = if self.consume(TokenKind::LeftParen) {
                let p = self.parse_binding_pattern()?;
                self.expect(TokenKind::RightParen)?;
                Some(p)
            } else {
                None
            };

            let body = self.parse_block_statement()?;

            Some(CatchClause {
                param,
                body,
                span: Span::new(catch_start, self.location()),
            })
        } else {
            None
        };

        let finalizer = if self.consume(TokenKind::Keyword(Keyword::Finally)) {
            Some(self.parse_block_statement()?)
        } else {
            None
        };

        if handler.is_none() && finalizer.is_none() {
            return Err(self.error("try statement must have catch or finally", start));
        }

        Ok(Statement::Try(Box::new(TryStatement {
            block,
            handler,
            finalizer,
            span: Span::new(start, self.location()),
        })))
    }

    fn parse_expression_statement(&mut self) -> Result<Statement> {
        let start = self.location();
        let expression = self.parse_expression()?;
        self.consume_semicolon();

        Ok(Statement::Expression(ExpressionStatement {
            expression,
            span: Span::new(start, self.location()),
        }))
    }

    // ========== Expressions ==========

    fn parse_assignment_expression(&mut self) -> Result<Expression> {
        self.parse_assignment_expression_impl(true)
    }

    fn parse_assignment_expression_no_in(&mut self) -> Result<Expression> {
        self.parse_assignment_expression_impl(false)
    }

    fn parse_assignment_expression_impl(&mut self, allow_in: bool) -> Result<Expression> {
        // Try arrow function
        if let Some(arrow) = self.try_parse_arrow_function()? {
            return Ok(arrow);
        }

        let start = self.location();
        let left = if allow_in {
            self.parse_conditional_expression()?
        } else {
            self.parse_conditional_expression_no_in()?
        };

        if self.peek().is_assignment_operator() {
            let op = self.parse_assignment_operator()?;
            let right = self.parse_assignment_expression_impl(allow_in)?;

            let target = if left.is_valid_assignment_target() {
                AssignmentTarget::Simple(left)
            } else {
                return Err(self.error("Invalid left-hand side in assignment", start));
            };

            return Ok(Expression::Assignment(Box::new(AssignmentExpression {
                operator: op,
                left: target,
                right,
                span: Span::new(start, self.location()),
            })));
        }

        Ok(left)
    }

    fn try_parse_arrow_function(&mut self) -> Result<Option<Expression>> {
        let start_pos = self.pos;
        let start = self.location();

        // Check for async
        let is_async = self.peek() == TokenKind::Keyword(Keyword::Async);
        if is_async {
            self.advance();
        }

        // Check for parameter pattern
        let params = if self.peek() == TokenKind::Identifier {
            // Single parameter without parens
            let id = self.parse_identifier()?;
            if self.peek() != TokenKind::Arrow {
                self.pos = start_pos;
                return Ok(None);
            }
            FunctionParams {
                params: vec![Pattern::Identifier(id)],
                rest: None,
            }
        } else if self.peek() == TokenKind::LeftParen {
            self.advance();
            let params = self.parse_function_params()?;
            if self.expect(TokenKind::RightParen).is_err() || self.peek() != TokenKind::Arrow {
                self.pos = start_pos;
                return Ok(None);
            }
            params
        } else {
            self.pos = start_pos;
            return Ok(None);
        };

        // Check for duplicate parameters in strict mode
        self.check_duplicate_params(&params, start)?;

        self.expect(TokenKind::Arrow)?;

        let old_flags = self.flags;
        self.flags.in_function = true;
        self.flags.in_async = is_async;

        let body = if self.peek() == TokenKind::LeftBrace {
            FunctionBody::Block(self.parse_block_statement()?)
        } else {
            FunctionBody::Expression(Box::new(self.parse_assignment_expression()?))
        };

        self.flags = old_flags;

        Ok(Some(Expression::Arrow(Box::new(Function {
            id: None,
            params,
            body,
            is_async,
            is_generator: false,
            span: Span::new(start, self.location()),
        }))))
    }

    fn parse_assignment_operator(&mut self) -> Result<AssignmentOperator> {
        let op = match self.peek() {
            TokenKind::Equals => AssignmentOperator::Assign,
            TokenKind::PlusEquals => AssignmentOperator::AddAssign,
            TokenKind::MinusEquals => AssignmentOperator::SubAssign,
            TokenKind::StarEquals => AssignmentOperator::MulAssign,
            TokenKind::SlashEquals => AssignmentOperator::DivAssign,
            TokenKind::PercentEquals => AssignmentOperator::ModAssign,
            TokenKind::StarStarEquals => AssignmentOperator::PowAssign,
            TokenKind::LessLessEquals => AssignmentOperator::ShlAssign,
            TokenKind::GreaterGreaterEquals => AssignmentOperator::ShrAssign,
            TokenKind::GreaterGreaterGreaterEquals => AssignmentOperator::UShrAssign,
            TokenKind::AmpersandEquals => AssignmentOperator::BitwiseAndAssign,
            TokenKind::PipeEquals => AssignmentOperator::BitwiseOrAssign,
            TokenKind::CaretEquals => AssignmentOperator::BitwiseXorAssign,
            TokenKind::AmpersandAmpersandEquals => AssignmentOperator::AndAssign,
            TokenKind::PipePipeEquals => AssignmentOperator::OrAssign,
            TokenKind::QuestionQuestionEquals => AssignmentOperator::NullishAssign,
            _ => {
                let loc = self.location();
                return Err(self.error("Expected assignment operator", loc));
            }
        };
        self.advance();
        Ok(op)
    }

    fn parse_conditional_expression(&mut self) -> Result<Expression> {
        self.parse_conditional_expression_impl(true)
    }

    fn parse_conditional_expression_no_in(&mut self) -> Result<Expression> {
        self.parse_conditional_expression_impl(false)
    }

    fn parse_conditional_expression_impl(&mut self, allow_in: bool) -> Result<Expression> {
        let start = self.location();
        let test = self.parse_binary_expression_impl(allow_in, 0)?;

        if self.consume(TokenKind::Question) {
            let consequent = self.parse_assignment_expression()?;
            self.expect(TokenKind::Colon)?;
            let alternate = self.parse_assignment_expression_impl(allow_in)?;

            return Ok(Expression::Conditional(Box::new(ConditionalExpression {
                test,
                consequent,
                alternate,
                span: Span::new(start, self.location()),
            })));
        }

        Ok(test)
    }

    fn parse_expression_no_in(&mut self) -> Result<Expression> {
        self.parse_sequence_expression_impl(false)
    }

    fn parse_sequence_expression_impl(&mut self, allow_in: bool) -> Result<Expression> {
        let start = self.location();
        let mut expr = self.parse_assignment_expression_impl(allow_in)?;

        if self.peek() == TokenKind::Comma {
            let mut expressions = vec![expr];
            while self.consume(TokenKind::Comma) {
                expressions.push(self.parse_assignment_expression_impl(allow_in)?);
            }
            expr = Expression::Sequence(Box::new(SequenceExpression {
                expressions,
                span: Span::new(start, self.location()),
            }));
        }

        Ok(expr)
    }

    fn parse_binary_expression_impl(&mut self, allow_in: bool, min_prec: u8) -> Result<Expression> {
        let start = self.location();
        let mut left = self.parse_unary_expression()?;

        loop {
            let prec = self.binary_precedence(allow_in);
            if prec == 0 || prec < min_prec {
                break;
            }

            // Check for logical operators (they have different semantics)
            let is_logical = matches!(
                self.peek(),
                TokenKind::AmpersandAmpersand | TokenKind::PipePipe | TokenKind::QuestionQuestion
            );

            if is_logical {
                let op = self.parse_logical_operator()?;
                let right = self.parse_binary_expression_impl(allow_in, prec + 1)?;
                left = Expression::Logical(Box::new(LogicalExpression {
                    operator: op,
                    left,
                    right,
                    span: Span::new(start, self.location()),
                }));
            } else {
                let op = self.parse_binary_operator()?;
                let right = self.parse_binary_expression_impl(allow_in, prec + 1)?;
                left = Expression::Binary(Box::new(BinaryExpression {
                    operator: op,
                    left,
                    right,
                    span: Span::new(start, self.location()),
                }));
            }
        }

        Ok(left)
    }

    fn binary_precedence(&self, allow_in: bool) -> u8 {
        match self.peek() {
            TokenKind::PipePipe => 4,
            TokenKind::AmpersandAmpersand => 5,
            TokenKind::QuestionQuestion => 4,
            TokenKind::Pipe => 6,
            TokenKind::Caret => 7,
            TokenKind::Ampersand => 8,
            TokenKind::EqualsEquals
            | TokenKind::BangEquals
            | TokenKind::EqualsEqualsEquals
            | TokenKind::BangEqualsEquals => 9,
            TokenKind::Less
            | TokenKind::Greater
            | TokenKind::LessEquals
            | TokenKind::GreaterEquals
            | TokenKind::Keyword(Keyword::Instanceof) => 10,
            TokenKind::Keyword(Keyword::In) if allow_in => 10,
            TokenKind::LessLess | TokenKind::GreaterGreater | TokenKind::GreaterGreaterGreater => {
                11
            }
            TokenKind::Plus | TokenKind::Minus => 12,
            TokenKind::Star | TokenKind::Slash | TokenKind::Percent => 13,
            TokenKind::StarStar => 14,
            _ => 0,
        }
    }

    fn parse_binary_operator(&mut self) -> Result<BinaryOperator> {
        let op = match self.peek() {
            TokenKind::Plus => BinaryOperator::Add,
            TokenKind::Minus => BinaryOperator::Sub,
            TokenKind::Star => BinaryOperator::Mul,
            TokenKind::Slash => BinaryOperator::Div,
            TokenKind::Percent => BinaryOperator::Mod,
            TokenKind::StarStar => BinaryOperator::Pow,
            TokenKind::EqualsEquals => BinaryOperator::Eq,
            TokenKind::BangEquals => BinaryOperator::Ne,
            TokenKind::EqualsEqualsEquals => BinaryOperator::StrictEq,
            TokenKind::BangEqualsEquals => BinaryOperator::StrictNe,
            TokenKind::Less => BinaryOperator::Lt,
            TokenKind::LessEquals => BinaryOperator::Le,
            TokenKind::Greater => BinaryOperator::Gt,
            TokenKind::GreaterEquals => BinaryOperator::Ge,
            TokenKind::LessLess => BinaryOperator::Shl,
            TokenKind::GreaterGreater => BinaryOperator::Shr,
            TokenKind::GreaterGreaterGreater => BinaryOperator::UShr,
            TokenKind::Ampersand => BinaryOperator::BitwiseAnd,
            TokenKind::Pipe => BinaryOperator::BitwiseOr,
            TokenKind::Caret => BinaryOperator::BitwiseXor,
            TokenKind::Keyword(Keyword::In) => BinaryOperator::In,
            TokenKind::Keyword(Keyword::Instanceof) => BinaryOperator::Instanceof,
            _ => {
                let loc = self.location();
                return Err(self.error("Expected binary operator", loc));
            }
        };
        self.advance();
        Ok(op)
    }

    fn parse_logical_operator(&mut self) -> Result<LogicalOperator> {
        let op = match self.peek() {
            TokenKind::AmpersandAmpersand => LogicalOperator::And,
            TokenKind::PipePipe => LogicalOperator::Or,
            TokenKind::QuestionQuestion => LogicalOperator::NullishCoalescing,
            _ => {
                let loc = self.location();
                return Err(self.error("Expected logical operator", loc));
            }
        };
        self.advance();
        Ok(op)
    }

    fn parse_unary_expression(&mut self) -> Result<Expression> {
        let start = self.location();

        // Prefix increment/decrement
        if matches!(self.peek(), TokenKind::PlusPlus | TokenKind::MinusMinus) {
            let op = if self.peek() == TokenKind::PlusPlus {
                UpdateOperator::Increment
            } else {
                UpdateOperator::Decrement
            };
            self.advance();
            let argument = self.parse_unary_expression()?;

            return Ok(Expression::Update(Box::new(UpdateExpression {
                operator: op,
                argument,
                prefix: true,
                span: Span::new(start, self.location()),
            })));
        }

        // Unary operators
        let unary_op = match self.peek() {
            TokenKind::Plus => Some(UnaryOperator::Plus),
            TokenKind::Minus => Some(UnaryOperator::Minus),
            TokenKind::Bang => Some(UnaryOperator::Not),
            TokenKind::Tilde => Some(UnaryOperator::BitwiseNot),
            TokenKind::Keyword(Keyword::Typeof) => Some(UnaryOperator::Typeof),
            TokenKind::Keyword(Keyword::Void) => Some(UnaryOperator::Void),
            TokenKind::Keyword(Keyword::Delete) => Some(UnaryOperator::Delete),
            _ => None,
        };

        if let Some(op) = unary_op {
            self.advance();
            let argument = self.parse_unary_expression()?;

            return Ok(Expression::Unary(Box::new(UnaryExpression {
                operator: op,
                argument,
                span: Span::new(start, self.location()),
            })));
        }

        // Await expression
        if self.peek() == TokenKind::Keyword(Keyword::Await) && self.flags.in_async {
            self.advance();
            let argument = self.parse_unary_expression()?;

            return Ok(Expression::Await(Box::new(AwaitExpression {
                argument,
                span: Span::new(start, self.location()),
            })));
        }

        // Yield expression (only in generator functions)
        if self.peek() == TokenKind::Keyword(Keyword::Yield) && self.flags.in_generator {
            self.advance();

            // Check for yield* (delegate)
            let delegate = self.consume(TokenKind::Star);

            // Parse optional argument
            let argument = if self.peek().can_start_expression() {
                Some(self.parse_assignment_expression()?)
            } else {
                None
            };

            return Ok(Expression::Yield(Box::new(YieldExpression {
                argument,
                delegate,
                span: Span::new(start, self.location()),
            })));
        }

        self.parse_update_expression()
    }

    fn parse_update_expression(&mut self) -> Result<Expression> {
        let start = self.location();
        let argument = self.parse_left_hand_side_expression()?;

        // Postfix increment/decrement
        if matches!(self.peek(), TokenKind::PlusPlus | TokenKind::MinusMinus) {
            let op = if self.peek() == TokenKind::PlusPlus {
                UpdateOperator::Increment
            } else {
                UpdateOperator::Decrement
            };
            self.advance();

            return Ok(Expression::Update(Box::new(UpdateExpression {
                operator: op,
                argument,
                prefix: false,
                span: Span::new(start, self.location()),
            })));
        }

        Ok(argument)
    }

    fn parse_left_hand_side_expression(&mut self) -> Result<Expression> {
        let start = self.location();

        let mut expr = if self.consume(TokenKind::Keyword(Keyword::New)) {
            if self.peek() == TokenKind::Dot {
                // new.target
                self.advance();
                self.expect_keyword(Keyword::Target)?;
                Expression::MetaProperty(MetaProperty {
                    meta: Identifier::new("new", Span::new(start, self.location())),
                    property: Identifier::new("target", Span::new(start, self.location())),
                    span: Span::new(start, self.location()),
                })
            } else {
                let callee = self.parse_member_expression()?;
                let arguments = if self.consume(TokenKind::LeftParen) {
                    self.parse_arguments()?
                } else {
                    Vec::new()
                };

                Expression::New(Box::new(NewExpression {
                    callee,
                    arguments,
                    span: Span::new(start, self.location()),
                }))
            }
        } else {
            self.parse_member_expression()?
        };

        // Call expressions and member accesses
        loop {
            match self.peek() {
                TokenKind::LeftParen => {
                    self.advance();
                    let arguments = self.parse_arguments()?;
                    expr = Expression::Call(Box::new(CallExpression {
                        callee: expr,
                        arguments,
                        span: Span::new(start, self.location()),
                    }));
                }
                TokenKind::Dot => {
                    self.advance();
                    if self.peek() == TokenKind::PrivateName {
                        let text = self.advance().text.to_string();
                        let name = text[1..].to_string();
                        expr = Expression::Member(Box::new(MemberExpression {
                            object: expr,
                            property: MemberProperty::PrivateName(name),
                            computed: false,
                            span: Span::new(start, self.location()),
                        }));
                    } else {
                        let property = self.parse_identifier_name()?;
                        expr = Expression::Member(Box::new(MemberExpression {
                            object: expr,
                            property: MemberProperty::Identifier(property),
                            computed: false,
                            span: Span::new(start, self.location()),
                        }));
                    }
                }
                TokenKind::LeftBracket => {
                    self.advance();
                    let property = self.parse_expression()?;
                    self.expect(TokenKind::RightBracket)?;
                    expr = Expression::Member(Box::new(MemberExpression {
                        object: expr,
                        property: MemberProperty::Expression(Box::new(property)),
                        computed: true,
                        span: Span::new(start, self.location()),
                    }));
                }
                TokenKind::QuestionDot => {
                    self.advance();
                    if self.peek() == TokenKind::LeftParen {
                        self.advance();
                        let arguments = self.parse_arguments()?;
                        expr = Expression::OptionalCall(Box::new(CallExpression {
                            callee: expr,
                            arguments,
                            span: Span::new(start, self.location()),
                        }));
                    } else if self.peek() == TokenKind::LeftBracket {
                        self.advance();
                        let property = self.parse_expression()?;
                        self.expect(TokenKind::RightBracket)?;
                        expr = Expression::OptionalMember(Box::new(MemberExpression {
                            object: expr,
                            property: MemberProperty::Expression(Box::new(property)),
                            computed: true,
                            span: Span::new(start, self.location()),
                        }));
                    } else {
                        let property = self.parse_identifier_name()?;
                        expr = Expression::OptionalMember(Box::new(MemberExpression {
                            object: expr,
                            property: MemberProperty::Identifier(property),
                            computed: false,
                            span: Span::new(start, self.location()),
                        }));
                    }
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_member_expression(&mut self) -> Result<Expression> {
        let start = self.location();
        let mut expr = self.parse_primary_expression()?;

        loop {
            match self.peek() {
                TokenKind::Dot => {
                    self.advance();
                    if self.peek() == TokenKind::PrivateName {
                        let text = self.advance().text.to_string();
                        let name = text[1..].to_string();
                        expr = Expression::Member(Box::new(MemberExpression {
                            object: expr,
                            property: MemberProperty::PrivateName(name),
                            computed: false,
                            span: Span::new(start, self.location()),
                        }));
                    } else {
                        let property = self.parse_identifier_name()?;
                        expr = Expression::Member(Box::new(MemberExpression {
                            object: expr,
                            property: MemberProperty::Identifier(property),
                            computed: false,
                            span: Span::new(start, self.location()),
                        }));
                    }
                }
                TokenKind::LeftBracket => {
                    self.advance();
                    let property = self.parse_expression()?;
                    self.expect(TokenKind::RightBracket)?;
                    expr = Expression::Member(Box::new(MemberExpression {
                        object: expr,
                        property: MemberProperty::Expression(Box::new(property)),
                        computed: true,
                        span: Span::new(start, self.location()),
                    }));
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_primary_expression(&mut self) -> Result<Expression> {
        let start = self.location();

        match self.peek() {
            TokenKind::Identifier => {
                let id = self.parse_identifier()?;
                Ok(Expression::Identifier(id))
            }
            TokenKind::NumberLiteral => {
                let raw = self.advance().text.to_string();
                let value = self.parse_number_value(&raw)?;
                Ok(Expression::Literal(Literal {
                    value: LiteralValue::Number(value),
                    raw,
                    span: Span::new(start, self.location()),
                }))
            }
            TokenKind::BigIntLiteral => {
                let raw = self.advance().text.to_string();
                let text = raw.trim_end_matches('n');
                Ok(Expression::Literal(Literal {
                    value: LiteralValue::BigInt(text.to_string()),
                    raw,
                    span: Span::new(start, self.location()),
                }))
            }
            TokenKind::StringLiteral => {
                let raw = self.advance().text.to_string();
                let value = self.parse_string_value(&raw)?;
                Ok(Expression::Literal(Literal {
                    value: LiteralValue::String(value),
                    raw,
                    span: Span::new(start, self.location()),
                }))
            }
            TokenKind::Keyword(Keyword::True) => {
                self.advance();
                Ok(Expression::Literal(Literal {
                    value: LiteralValue::Boolean(true),
                    raw: "true".to_string(),
                    span: Span::new(start, self.location()),
                }))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.advance();
                Ok(Expression::Literal(Literal {
                    value: LiteralValue::Boolean(false),
                    raw: "false".to_string(),
                    span: Span::new(start, self.location()),
                }))
            }
            TokenKind::Keyword(Keyword::Null) => {
                self.advance();
                Ok(Expression::Literal(Literal {
                    value: LiteralValue::Null,
                    raw: "null".to_string(),
                    span: Span::new(start, self.location()),
                }))
            }
            TokenKind::Keyword(Keyword::This) => {
                self.advance();
                Ok(Expression::This(Span::new(start, self.location())))
            }
            TokenKind::Keyword(Keyword::Super) => {
                self.advance();
                Ok(Expression::Super(Span::new(start, self.location())))
            }
            TokenKind::LeftParen => {
                self.advance();
                let expr = self.parse_expression()?;
                self.expect(TokenKind::RightParen)?;
                Ok(Expression::Parenthesized(Box::new(expr)))
            }
            TokenKind::LeftBracket => self.parse_array_literal(),
            TokenKind::LeftBrace => self.parse_object_literal(),
            TokenKind::Keyword(Keyword::Function) => {
                let func = self.parse_function(false, true)?;
                Ok(Expression::Function(Box::new(func)))
            }
            TokenKind::Keyword(Keyword::Async)
                if self.peek_at(1) == TokenKind::Keyword(Keyword::Function) =>
            {
                let func = self.parse_function(false, true)?;
                Ok(Expression::Function(Box::new(func)))
            }
            TokenKind::Keyword(Keyword::Class) => {
                let class = self.parse_class(false)?;
                Ok(Expression::Class(Box::new(class)))
            }
            TokenKind::TemplateLiteral | TokenKind::TemplateHead => self.parse_template_literal(),
            _ => {
                let loc = self.location();
                Err(self.error(format!("Unexpected token {:?}", self.peek()), loc))
            }
        }
    }

    fn parse_array_literal(&mut self) -> Result<Expression> {
        let start = self.location();
        self.expect(TokenKind::LeftBracket)?;

        let mut elements = Vec::new();

        while !self.consume(TokenKind::RightBracket) {
            if self.consume(TokenKind::Comma) {
                elements.push(None);
                continue;
            }

            let elem = if self.consume(TokenKind::DotDotDot) {
                let arg = self.parse_assignment_expression()?;
                Expression::Spread(Box::new(SpreadElement {
                    argument: arg.clone(),
                    span: arg.span(),
                }))
            } else {
                self.parse_assignment_expression()?
            };

            elements.push(Some(elem));

            if !self.consume(TokenKind::Comma) {
                self.expect(TokenKind::RightBracket)?;
                break;
            }
        }

        Ok(Expression::Array(ArrayExpression {
            elements,
            span: Span::new(start, self.location()),
        }))
    }

    fn parse_object_literal(&mut self) -> Result<Expression> {
        let start = self.location();
        self.expect(TokenKind::LeftBrace)?;

        let mut properties = Vec::new();

        while !self.consume(TokenKind::RightBrace) {
            let prop = self.parse_object_property()?;
            properties.push(prop);

            if !self.consume(TokenKind::Comma) {
                self.expect(TokenKind::RightBrace)?;
                break;
            }
        }

        Ok(Expression::Object(ObjectExpression {
            properties,
            span: Span::new(start, self.location()),
        }))
    }

    fn parse_object_property(&mut self) -> Result<ObjectProperty> {
        let start = self.location();

        // Spread property
        if self.consume(TokenKind::DotDotDot) {
            let arg = self.parse_assignment_expression()?;
            return Ok(ObjectProperty::Spread {
                argument: arg,
                span: Span::new(start, self.location()),
            });
        }

        // Method with get/set
        if (self.peek() == TokenKind::Keyword(Keyword::Get)
            || self.peek() == TokenKind::Keyword(Keyword::Set))
            && self.peek_at(1) != TokenKind::Colon
            && self.peek_at(1) != TokenKind::LeftParen
        {
            let kind = if self.peek() == TokenKind::Keyword(Keyword::Get) {
                MethodKind::Get
            } else {
                MethodKind::Set
            };
            self.advance();

            let computed = self.peek() == TokenKind::LeftBracket;
            let key = self.parse_property_key()?;

            self.expect(TokenKind::LeftParen)?;
            let params = self.parse_function_params()?;
            self.expect(TokenKind::RightParen)?;

            let old_flags = self.flags;
            self.flags.in_function = true;
            let body = self.parse_block_statement()?;
            self.flags = old_flags;

            let func = Function {
                id: None,
                params,
                body: FunctionBody::Block(body),
                is_async: false,
                is_generator: false,
                span: Span::new(start, self.location()),
            };

            return Ok(ObjectProperty::Method(MethodDefinition {
                key,
                value: func,
                kind,
                computed,
                is_static: false,
                span: Span::new(start, self.location()),
            }));
        }

        // Check for async/generator methods
        let is_async = self.consume(TokenKind::Keyword(Keyword::Async));
        let is_generator = self.consume(TokenKind::Star);

        let computed = self.peek() == TokenKind::LeftBracket;
        let key = self.parse_property_key()?;

        // Method
        if self.peek() == TokenKind::LeftParen {
            self.expect(TokenKind::LeftParen)?;
            let params = self.parse_function_params()?;
            self.expect(TokenKind::RightParen)?;

            let old_flags = self.flags;
            self.flags.in_function = true;
            self.flags.in_async = is_async;
            self.flags.in_generator = is_generator;
            let body = self.parse_block_statement()?;
            self.flags = old_flags;

            let func = Function {
                id: None,
                params,
                body: FunctionBody::Block(body),
                is_async,
                is_generator,
                span: Span::new(start, self.location()),
            };

            return Ok(ObjectProperty::Method(MethodDefinition {
                key,
                value: func,
                kind: MethodKind::Method,
                computed,
                is_static: false,
                span: Span::new(start, self.location()),
            }));
        }

        // Shorthand property {foo} or {foo = default} (only for non-computed)
        if !computed && !self.consume(TokenKind::Colon) {
            let id = match &key {
                PropertyKey::Identifier(id) => id.clone(),
                _ => return Err(self.error("Shorthand property must be an identifier", start)),
            };

            return Ok(ObjectProperty::Property {
                key,
                value: Expression::Identifier(id),
                computed: false,
                shorthand: true,
                method: false,
                span: Span::new(start, self.location()),
            });
        }

        // For computed properties, expect the colon
        if computed {
            self.expect(TokenKind::Colon)?;
        }

        // Regular property {foo: bar} or {[expr]: bar}
        let value = self.parse_assignment_expression()?;

        Ok(ObjectProperty::Property {
            key,
            value,
            computed,
            shorthand: false,
            method: false,
            span: Span::new(start, self.location()),
        })
    }

    fn parse_template_literal(&mut self) -> Result<Expression> {
        let start = self.location();
        let mut quasis = Vec::new();
        let mut expressions = Vec::new();

        // Handle complete template literal with no substitutions
        if self.peek() == TokenKind::TemplateLiteral {
            let text = self.advance().text.to_string();
            let (raw, cooked) = self.parse_template_string(&text)?;
            quasis.push(TemplateElement {
                raw,
                cooked: Some(cooked),
                tail: true,
                span: Span::new(start, self.location()),
            });

            return Ok(Expression::TemplateLiteral(TemplateLiteral {
                quasis,
                expressions,
                span: Span::new(start, self.location()),
            }));
        }

        // Template with substitutions
        // Token sequence: TemplateHead, expr, RightBrace, (TemplateMiddle, expr, RightBrace)*, TemplateTail
        loop {
            match self.peek() {
                TokenKind::TemplateHead => {
                    let text = self.advance().text.to_string();
                    let (raw, cooked) = self.parse_template_string(&text)?;
                    quasis.push(TemplateElement {
                        raw,
                        cooked: Some(cooked),
                        tail: false,
                        span: Span::new(start, self.location()),
                    });
                    // ${ is included in the template head token, so expression comes next
                }
                TokenKind::TemplateMiddle => {
                    let text = self.advance().text.to_string();
                    let (raw, cooked) = self.parse_template_string(&text)?;
                    quasis.push(TemplateElement {
                        raw,
                        cooked: Some(cooked),
                        tail: false,
                        span: Span::new(start, self.location()),
                    });
                    // ${ is included in the template middle token, so expression comes next
                }
                TokenKind::TemplateTail => {
                    let text = self.advance().text.to_string();
                    let (raw, cooked) = self.parse_template_string(&text)?;
                    quasis.push(TemplateElement {
                        raw,
                        cooked: Some(cooked),
                        tail: true,
                        span: Span::new(start, self.location()),
                    });
                    break;
                }
                _ => {
                    // Parse expression inside ${}
                    expressions.push(self.parse_expression()?);
                    // The } closes the substitution and the tokenizer generates the next template token
                    self.expect(TokenKind::RightBrace)?;
                }
            }
        }

        Ok(Expression::TemplateLiteral(TemplateLiteral {
            quasis,
            expressions,
            span: Span::new(start, self.location()),
        }))
    }

    fn parse_arguments(&mut self) -> Result<Vec<Expression>> {
        let mut args = Vec::new();

        while !self.consume(TokenKind::RightParen) {
            let arg = if self.consume(TokenKind::DotDotDot) {
                let expr = self.parse_assignment_expression()?;
                Expression::Spread(Box::new(SpreadElement {
                    argument: expr.clone(),
                    span: expr.span(),
                }))
            } else {
                self.parse_assignment_expression()?
            };

            args.push(arg);

            if !self.consume(TokenKind::Comma) {
                self.expect(TokenKind::RightParen)?;
                break;
            }
        }

        Ok(args)
    }

    // ========== Helpers ==========

    fn parse_identifier(&mut self) -> Result<Identifier> {
        if self.peek() != TokenKind::Identifier {
            let loc = self.location();
            return Err(self.error(
                format!("Expected identifier, found {:?}", self.peek()),
                loc,
            ));
        }

        let token = self.advance();
        Ok(Identifier {
            name: token.text.to_string(),
            span: Span::new(token.location, self.location()),
        })
    }

    fn parse_identifier_name(&mut self) -> Result<Identifier> {
        // Allow keywords as property names
        match self.peek() {
            TokenKind::Identifier | TokenKind::Keyword(_) => {
                let token = self.advance();
                Ok(Identifier {
                    name: token.text.to_string(),
                    span: Span::new(token.location, self.location()),
                })
            }
            _ => {
                let loc = self.location();
                Err(self.error("Expected identifier", loc))
            }
        }
    }

    fn parse_string_value(&self, text: &str) -> Result<String> {
        // Remove quotes and process escapes
        let inner = &text[1..text.len() - 1];
        let mut result = String::new();
        let mut chars = inner.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => result.push('\n'),
                    Some('r') => result.push('\r'),
                    Some('t') => result.push('\t'),
                    Some('\\') => result.push('\\'),
                    Some('\'') => result.push('\''),
                    Some('"') => result.push('"'),
                    Some('0') => result.push('\0'),
                    Some('x') => {
                        let hex: String = chars.by_ref().take(2).collect();
                        if let Ok(code) = u8::from_str_radix(&hex, 16) {
                            result.push(code as char);
                        }
                    }
                    Some('u') => {
                        if chars.peek() == Some(&'{') {
                            chars.next();
                            let hex: String = chars.by_ref().take_while(|&c| c != '}').collect();
                            if let Ok(code) = u32::from_str_radix(&hex, 16) {
                                if let Some(c) = char::from_u32(code) {
                                    result.push(c);
                                }
                            }
                        } else {
                            let hex: String = chars.by_ref().take(4).collect();
                            if let Ok(code) = u16::from_str_radix(&hex, 16) {
                                if let Some(c) = char::from_u32(code as u32) {
                                    result.push(c);
                                }
                            }
                        }
                    }
                    Some(other) => result.push(other),
                    None => {}
                }
            } else {
                result.push(c);
            }
        }

        Ok(result)
    }

    fn parse_number_value(&self, text: &str) -> Result<f64> {
        // Remove numeric separators
        let clean: String = text.chars().filter(|&c| c != '_').collect();

        if clean.starts_with("0x") || clean.starts_with("0X") {
            let hex = &clean[2..];
            Ok(i64::from_str_radix(hex, 16).unwrap_or(0) as f64)
        } else if clean.starts_with("0b") || clean.starts_with("0B") {
            let bin = &clean[2..];
            Ok(i64::from_str_radix(bin, 2).unwrap_or(0) as f64)
        } else if clean.starts_with("0o") || clean.starts_with("0O") {
            let oct = &clean[2..];
            Ok(i64::from_str_radix(oct, 8).unwrap_or(0) as f64)
        } else {
            let loc = self.location();
            clean
                .parse::<f64>()
                .map_err(|_| self.error(format!("Invalid number: {}", text), loc))
        }
    }

    fn parse_template_string(&self, text: &str) -> Result<(String, String)> {
        // Remove ` and handle escapes
        let inner = text.trim_start_matches('`').trim_end_matches('`');
        let inner = inner.trim_end_matches("${");

        let mut raw = String::new();
        let mut cooked = String::new();
        let mut chars = inner.chars().peekable();

        while let Some(c) = chars.next() {
            raw.push(c);
            if c == '\\' {
                if let Some(&next) = chars.peek() {
                    raw.push(next);
                    match next {
                        'n' => {
                            cooked.push('\n');
                            chars.next();
                        }
                        'r' => {
                            cooked.push('\r');
                            chars.next();
                        }
                        't' => {
                            cooked.push('\t');
                            chars.next();
                        }
                        '\\' => {
                            cooked.push('\\');
                            chars.next();
                        }
                        '`' => {
                            cooked.push('`');
                            chars.next();
                        }
                        '$' => {
                            cooked.push('$');
                            chars.next();
                        }
                        _ => {
                            cooked.push(c);
                        }
                    }
                }
            } else {
                cooked.push(c);
            }
        }

        Ok((raw, cooked))
    }

    // ========== Module Declarations ==========

    /// Parse import declaration
    /// import defaultExport from "module-name";
    /// import * as name from "module-name";
    /// import { export1 } from "module-name";
    /// import { export1 as alias1 } from "module-name";
    /// import { default as alias } from "module-name";
    /// import { export1, export2 } from "module-name";
    /// import defaultExport, { export1 } from "module-name";
    /// import defaultExport, * as name from "module-name";
    /// import "module-name";
    fn parse_import_declaration(&mut self) -> Result<Statement> {
        let start = self.location();
        self.expect_keyword(Keyword::Import)?;

        // import "module-name"; (side-effect only import)
        if self.peek() == TokenKind::StringLiteral {
            let source = self.parse_module_source()?;
            self.consume_semicolon();
            return Ok(Statement::Import(Box::new(ImportDeclaration {
                specifiers: vec![],
                source,
                span: Span::new(start, self.location()),
            })));
        }

        let mut specifiers = Vec::new();

        // Check for default import
        if self.peek() == TokenKind::Identifier {
            let local = self.parse_identifier()?;
            specifiers.push(ImportSpecifier::Default {
                local: local.clone(),
                span: local.span,
            });

            // Check for additional imports after default
            if self.consume(TokenKind::Comma) {
                if self.peek() == TokenKind::Star {
                    // import default, * as name from "module"
                    self.advance();
                    self.expect_keyword(Keyword::As)?;
                    let local = self.parse_identifier()?;
                    specifiers.push(ImportSpecifier::Namespace {
                        local: local.clone(),
                        span: local.span,
                    });
                } else if self.peek() == TokenKind::LeftBrace {
                    // import default, { named } from "module"
                    self.parse_named_imports(&mut specifiers)?;
                }
            }
        } else if self.peek() == TokenKind::Star {
            // import * as name from "module"
            self.advance();
            self.expect_keyword(Keyword::As)?;
            let local = self.parse_identifier()?;
            specifiers.push(ImportSpecifier::Namespace {
                local: local.clone(),
                span: local.span,
            });
        } else if self.peek() == TokenKind::LeftBrace {
            // import { named } from "module"
            self.parse_named_imports(&mut specifiers)?;
        }

        // from "module-name"
        self.expect_keyword(Keyword::From)?;
        let source = self.parse_module_source()?;

        self.consume_semicolon();

        Ok(Statement::Import(Box::new(ImportDeclaration {
            specifiers,
            source,
            span: Span::new(start, self.location()),
        })))
    }

    /// Parse named imports { export1, export2 as alias2 }
    fn parse_named_imports(&mut self, specifiers: &mut Vec<ImportSpecifier>) -> Result<()> {
        self.expect(TokenKind::LeftBrace)?;

        while !self.consume(TokenKind::RightBrace) {
            let start = self.location();
            let imported = self.parse_identifier()?;

            let local = if self.consume_keyword(Keyword::As) {
                self.parse_identifier()?
            } else {
                imported.clone()
            };

            specifiers.push(ImportSpecifier::Named {
                local,
                imported,
                span: Span::new(start, self.location()),
            });

            if !self.consume(TokenKind::Comma) && self.peek() != TokenKind::RightBrace {
                let loc = self.location();
                return Err(self.error("Expected ',' or '}' in import specifiers", loc));
            }
        }

        Ok(())
    }

    /// Parse export declaration
    /// export { name1, name2 };
    /// export { variable1 as name1, variable2 as name2 };
    /// export let name1, name2;
    /// export function functionName() {}
    /// export class ClassName {}
    /// export default expression;
    /// export default function () {}
    /// export * from "module-name";
    /// export * as name from "module-name";
    /// export { name1, name2 } from "module-name";
    fn parse_export_declaration(&mut self) -> Result<Statement> {
        let start = self.location();
        self.expect_keyword(Keyword::Export)?;

        let kind = if self.consume_keyword(Keyword::Default) {
            // export default ...
            if self.peek() == TokenKind::Keyword(Keyword::Function) {
                // export default function ...
                let decl = self.parse_function_declaration()?;
                ExportKind::DefaultDeclaration(Box::new(decl))
            } else if self.peek() == TokenKind::Keyword(Keyword::Class) {
                // export default class ...
                let decl = self.parse_class_declaration()?;
                ExportKind::DefaultDeclaration(Box::new(decl))
            } else if self.peek() == TokenKind::Keyword(Keyword::Async)
                && self.peek_at(1) == TokenKind::Keyword(Keyword::Function)
            {
                // export default async function ...
                let decl = self.parse_function_declaration()?;
                ExportKind::DefaultDeclaration(Box::new(decl))
            } else {
                // export default expression
                let expr = self.parse_assignment_expression()?;
                self.consume_semicolon();
                ExportKind::Default(expr)
            }
        } else if self.peek() == TokenKind::Star {
            // export * from "module" or export * as name from "module"
            self.advance();

            if self.consume_keyword(Keyword::As) {
                // export * as name from "module"
                let exported = self.parse_identifier()?;
                self.expect_keyword(Keyword::From)?;
                let source = self.parse_module_source()?;
                self.consume_semicolon();
                ExportKind::AllAs { exported, source }
            } else {
                // export * from "module"
                self.expect_keyword(Keyword::From)?;
                let source = self.parse_module_source()?;
                self.consume_semicolon();
                ExportKind::All { source }
            }
        } else if self.peek() == TokenKind::LeftBrace {
            // export { name1, name2 } or export { name1 } from "module"
            let specifiers = self.parse_export_specifiers()?;

            let source = if self.consume_keyword(Keyword::From) {
                Some(self.parse_module_source()?)
            } else {
                None
            };

            self.consume_semicolon();
            ExportKind::Named { specifiers, source }
        } else if self.peek() == TokenKind::Keyword(Keyword::Var)
            || self.peek() == TokenKind::Keyword(Keyword::Let)
            || self.peek() == TokenKind::Keyword(Keyword::Const)
        {
            // export var/let/const
            let decl = match self.peek() {
                TokenKind::Keyword(Keyword::Var) => self
                    .parse_variable_declaration(VariableKind::Var)
                    .map(Statement::VariableDeclaration)?,
                TokenKind::Keyword(Keyword::Let) => self
                    .parse_variable_declaration(VariableKind::Let)
                    .map(Statement::VariableDeclaration)?,
                TokenKind::Keyword(Keyword::Const) => self
                    .parse_variable_declaration(VariableKind::Const)
                    .map(Statement::VariableDeclaration)?,
                _ => unreachable!(),
            };
            ExportKind::Declaration(Box::new(decl))
        } else if self.peek() == TokenKind::Keyword(Keyword::Function)
            || (self.peek() == TokenKind::Keyword(Keyword::Async)
                && self.peek_at(1) == TokenKind::Keyword(Keyword::Function))
        {
            // export function
            let decl = self.parse_function_declaration()?;
            ExportKind::Declaration(Box::new(decl))
        } else if self.peek() == TokenKind::Keyword(Keyword::Class) {
            // export class
            let decl = self.parse_class_declaration()?;
            ExportKind::Declaration(Box::new(decl))
        } else {
            let loc = self.location();
            return Err(self.error(
                format!("Unexpected token in export declaration: {:?}", self.peek()),
                loc,
            ));
        };

        Ok(Statement::Export(Box::new(ExportDeclaration {
            kind,
            span: Span::new(start, self.location()),
        })))
    }

    /// Parse export specifiers { name1, name2 as alias2 }
    fn parse_export_specifiers(&mut self) -> Result<Vec<ExportSpecifier>> {
        self.expect(TokenKind::LeftBrace)?;

        let mut specifiers = Vec::new();

        while !self.consume(TokenKind::RightBrace) {
            let start = self.location();
            let local = self.parse_identifier()?;

            let exported = if self.consume_keyword(Keyword::As) {
                self.parse_identifier()?
            } else {
                local.clone()
            };

            specifiers.push(ExportSpecifier::Named {
                local,
                exported,
                span: Span::new(start, self.location()),
            });

            if !self.consume(TokenKind::Comma) && self.peek() != TokenKind::RightBrace {
                let loc = self.location();
                return Err(self.error("Expected ',' or '}' in export specifiers", loc));
            }
        }

        Ok(specifiers)
    }

    /// Parse module source string
    fn parse_module_source(&mut self) -> Result<String> {
        if self.peek() == TokenKind::StringLiteral {
            let text = self.current().text;
            // Strip the quotes from the string literal
            let source = if (text.starts_with('"') && text.ends_with('"'))
                || (text.starts_with('\'') && text.ends_with('\''))
            {
                text[1..text.len() - 1].to_string()
            } else {
                text.to_string()
            };
            self.advance();
            Ok(source)
        } else {
            let loc = self.location();
            Err(self.error("Expected module specifier string", loc))
        }
    }

    /// Check for and consume a keyword
    fn consume_keyword(&mut self, keyword: Keyword) -> bool {
        if self.peek() == TokenKind::Keyword(keyword) {
            self.advance();
            true
        } else {
            false
        }
    }
}

/// Parse JavaScript source code into an AST
pub fn parse(source: &str) -> Result<Program> {
    let mut parser = Parser::new(source)?;
    parser.parse_program()
}

/// Parse a single JavaScript expression
pub fn parse_expression(source: &str) -> Result<Expression> {
    let mut parser = Parser::new(source)?;
    parser.parse_expression()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_literals() {
        let program = parse("42;").unwrap();
        assert_eq!(program.body.len(), 1);

        let program = parse("'hello';").unwrap();
        assert_eq!(program.body.len(), 1);

        let program = parse("true; false; null;").unwrap();
        assert_eq!(program.body.len(), 3);
    }

    #[test]
    fn test_parse_binary_expression() {
        let expr = parse_expression("1 + 2 * 3").unwrap();
        match expr {
            Expression::Binary(b) => {
                assert_eq!(b.operator, BinaryOperator::Add);
            }
            _ => panic!("Expected binary expression"),
        }
    }

    #[test]
    fn test_parse_variable_declaration() {
        let program = parse("let x = 1;").unwrap();
        match &program.body[0] {
            Statement::VariableDeclaration(decl) => {
                assert_eq!(decl.kind, VariableKind::Let);
                assert_eq!(decl.declarations.len(), 1);
            }
            _ => panic!("Expected variable declaration"),
        }
    }

    #[test]
    fn test_parse_function() {
        let program = parse("function foo(a, b) { return a + b; }").unwrap();
        match &program.body[0] {
            Statement::FunctionDeclaration(func) => {
                assert_eq!(func.id.as_ref().unwrap().name, "foo");
                assert_eq!(func.params.params.len(), 2);
            }
            _ => panic!("Expected function declaration"),
        }
    }

    #[test]
    fn test_parse_arrow_function() {
        let expr = parse_expression("(x) => x * 2").unwrap();
        match expr {
            Expression::Arrow(func) => {
                assert!(func.id.is_none());
                assert!(!func.is_async);
            }
            _ => panic!("Expected arrow function"),
        }
    }

    #[test]
    fn test_parse_class() {
        let program = parse("class Foo { constructor() {} method() {} }").unwrap();
        match &program.body[0] {
            Statement::ClassDeclaration(class) => {
                assert_eq!(class.id.as_ref().unwrap().name, "Foo");
                assert_eq!(class.body.body.len(), 2);
            }
            _ => panic!("Expected class declaration"),
        }
    }

    #[test]
    fn test_parse_if_statement() {
        let program = parse("if (x) { y; } else { z; }").unwrap();
        match &program.body[0] {
            Statement::If(stmt) => {
                assert!(stmt.alternate.is_some());
            }
            _ => panic!("Expected if statement"),
        }
    }

    #[test]
    fn test_parse_for_loop() {
        let program = parse("for (let i = 0; i < 10; i++) { console.log(i); }").unwrap();
        match &program.body[0] {
            Statement::For(_) => {}
            _ => panic!("Expected for statement"),
        }
    }
}
