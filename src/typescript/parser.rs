//! TypeScript Parser
//!
//! This module provides a parser for TypeScript-specific syntax. It extends
//! the JavaScript parser to handle type annotations, interfaces, type aliases,
//! and other TypeScript constructs.

use super::types::*;
use crate::error::{Error, Result};

/// TypeScript parser for type-related constructs
pub struct TypeScriptParser<'src> {
    /// Source code
    source: &'src str,
    /// Current position
    pos: usize,
    /// Current line
    line: u32,
    /// Current column
    column: u32,
}

impl<'src> TypeScriptParser<'src> {
    /// Create a new TypeScript parser
    pub fn new(source: &'src str) -> Self {
        Self {
            source,
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    /// Check if at end of input
    fn is_eof(&self) -> bool {
        self.pos >= self.source.len()
    }

    /// Peek current character
    fn peek(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    /// Peek ahead n characters
    fn peek_ahead(&self, n: usize) -> Option<char> {
        self.source[self.pos..].chars().nth(n)
    }

    /// Advance one character
    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(c)
    }

    /// Skip whitespace
    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Skip whitespace and comments
    fn skip_trivia(&mut self) {
        loop {
            self.skip_whitespace();

            // Check for comments
            if self.peek() == Some('/') {
                if self.peek_ahead(1) == Some('/') {
                    // Single-line comment
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                    continue;
                } else if self.peek_ahead(1) == Some('*') {
                    // Multi-line comment
                    self.advance(); // /
                    self.advance(); // *
                    loop {
                        if self.is_eof() {
                            break;
                        }
                        if self.peek() == Some('*') && self.peek_ahead(1) == Some('/') {
                            self.advance(); // *
                            self.advance(); // /
                            break;
                        }
                        self.advance();
                    }
                    continue;
                }
            }
            break;
        }
    }

    /// Check if the upcoming text matches a string
    fn check(&self, s: &str) -> bool {
        self.source[self.pos..].starts_with(s)
    }

    /// Consume a string if it matches
    fn consume(&mut self, s: &str) -> bool {
        if self.check(s) {
            for _ in s.chars() {
                self.advance();
            }
            true
        } else {
            false
        }
    }

    /// Read an identifier
    fn read_identifier(&mut self) -> Option<String> {
        let start = self.pos;
        if let Some(c) = self.peek() {
            if !c.is_alphabetic() && c != '_' && c != '$' {
                return None;
            }
            self.advance();
        } else {
            return None;
        }

        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '$' {
                self.advance();
            } else {
                break;
            }
        }

        Some(self.source[start..self.pos].to_string())
    }

    /// Check if next token is an identifier matching a keyword
    fn check_keyword(&self, keyword: &str) -> bool {
        if !self.check(keyword) {
            return false;
        }
        // Make sure it's not a prefix of a longer identifier
        let next_pos = self.pos + keyword.len();
        if next_pos >= self.source.len() {
            return true;
        }
        let next_char = self.source[next_pos..].chars().next();
        !matches!(next_char, Some(c) if c.is_alphanumeric() || c == '_' || c == '$')
    }

    /// Parse a type annotation (after the colon)
    pub fn parse_type_annotation(&mut self) -> Result<TypeAnnotation> {
        self.skip_trivia();
        self.parse_union_type()
    }

    /// Parse union types: T | U | V
    fn parse_union_type(&mut self) -> Result<TypeAnnotation> {
        let mut types = vec![self.parse_intersection_type()?];

        self.skip_trivia();
        while self.consume("|") {
            self.skip_trivia();
            types.push(self.parse_intersection_type()?);
            self.skip_trivia();
        }

        if types.len() == 1 {
            Ok(types.pop().unwrap())
        } else {
            Ok(TypeAnnotation::Union(types))
        }
    }

    /// Parse intersection types: T & U & V
    fn parse_intersection_type(&mut self) -> Result<TypeAnnotation> {
        let mut types = vec![self.parse_primary_type()?];

        self.skip_trivia();
        while self.consume("&") {
            self.skip_trivia();
            types.push(self.parse_primary_type()?);
            self.skip_trivia();
        }

        if types.len() == 1 {
            Ok(types.pop().unwrap())
        } else {
            Ok(TypeAnnotation::Intersection(types))
        }
    }

    /// Parse primary type
    fn parse_primary_type(&mut self) -> Result<TypeAnnotation> {
        self.skip_trivia();

        // Check for primitives and keywords
        let mut base_type: Option<TypeAnnotation> = None;

        if self.check_keyword("string") {
            self.consume("string");
            base_type = Some(TypeAnnotation::Primitive(PrimitiveType::String));
        } else if self.check_keyword("number") {
            self.consume("number");
            base_type = Some(TypeAnnotation::Primitive(PrimitiveType::Number));
        } else if self.check_keyword("boolean") {
            self.consume("boolean");
            base_type = Some(TypeAnnotation::Primitive(PrimitiveType::Boolean));
        } else if self.check_keyword("bigint") {
            self.consume("bigint");
            base_type = Some(TypeAnnotation::Primitive(PrimitiveType::BigInt));
        } else if self.check_keyword("symbol") {
            self.consume("symbol");
            base_type = Some(TypeAnnotation::Primitive(PrimitiveType::Symbol));
        } else if self.check_keyword("object") {
            self.consume("object");
            base_type = Some(TypeAnnotation::Primitive(PrimitiveType::Object));
        } else if self.check_keyword("any") {
            self.consume("any");
            base_type = Some(TypeAnnotation::Any);
        } else if self.check_keyword("unknown") {
            self.consume("unknown");
            base_type = Some(TypeAnnotation::Unknown);
        } else if self.check_keyword("void") {
            self.consume("void");
            base_type = Some(TypeAnnotation::Void);
        } else if self.check_keyword("never") {
            self.consume("never");
            base_type = Some(TypeAnnotation::Never);
        } else if self.check_keyword("null") {
            self.consume("null");
            base_type = Some(TypeAnnotation::Null);
        } else if self.check_keyword("undefined") {
            self.consume("undefined");
            base_type = Some(TypeAnnotation::Undefined);
        } else if self.check_keyword("this") {
            self.consume("this");
            base_type = Some(TypeAnnotation::This);
        }

        // If we matched a primitive/keyword type, check for array suffix
        if let Some(mut result_type) = base_type {
            self.skip_trivia();
            while self.consume("[") {
                self.skip_trivia();
                if !self.consume("]") {
                    return Err(Error::InternalError("Expected ']' for array type".to_string()));
                }
                result_type = TypeAnnotation::Array(Box::new(result_type));
                self.skip_trivia();
            }
            return Ok(result_type);
        }

        // Array type with Array<T> syntax
        if self.check_keyword("Array") {
            self.consume("Array");
            self.skip_trivia();
            if self.consume("<") {
                self.skip_trivia();
                let element_type = self.parse_type_annotation()?;
                self.skip_trivia();
                if !self.consume(">") {
                    return Err(Error::InternalError("Expected '>' in Array type".to_string()));
                }
                return Ok(TypeAnnotation::GenericArray(Box::new(element_type)));
            }
            // Just Array without type arg
            return Ok(TypeAnnotation::Reference(TypeReference {
                name: vec!["Array".to_string()],
                type_args: vec![],
            }));
        }

        // Tuple type: [T, U, V]
        if self.consume("[") {
            let mut types = Vec::new();
            self.skip_trivia();

            if !self.check("]") {
                types.push(self.parse_type_annotation()?);
                self.skip_trivia();

                while self.consume(",") {
                    self.skip_trivia();
                    if self.check("]") {
                        break;
                    }
                    types.push(self.parse_type_annotation()?);
                    self.skip_trivia();
                }
            }

            if !self.consume("]") {
                return Err(Error::InternalError("Expected ']' in tuple type".to_string()));
            }
            return Ok(TypeAnnotation::Tuple(types));
        }

        // Object type: { key: Type }
        if self.consume("{") {
            let mut members = Vec::new();
            self.skip_trivia();

            while !self.check("}") && !self.is_eof() {
                // Check for index signature
                if self.check("[") {
                    self.advance();
                    self.skip_trivia();
                    let _key_name = self.read_identifier();
                    self.skip_trivia();
                    self.consume(":");
                    self.skip_trivia();
                    let key_type = self.parse_type_annotation()?;
                    self.skip_trivia();
                    self.consume("]");
                    self.skip_trivia();
                    self.consume(":");
                    self.skip_trivia();
                    let value_type = self.parse_type_annotation()?;

                    members.push(TypeMember {
                        name: TypeMemberName::IndexSignature(key_type),
                        optional: false,
                        readonly: false,
                        type_annotation: Some(value_type),
                    });
                } else {
                    // Regular property
                    let readonly = self.check_keyword("readonly");
                    if readonly {
                        self.consume("readonly");
                        self.skip_trivia();
                    }

                    let name = self.read_identifier()
                        .ok_or_else(|| Error::InternalError("Expected property name".to_string()))?;

                    self.skip_trivia();
                    let optional = self.consume("?");
                    self.skip_trivia();

                    let type_annotation = if self.consume(":") {
                        self.skip_trivia();
                        Some(self.parse_type_annotation()?)
                    } else {
                        None
                    };

                    members.push(TypeMember {
                        name: TypeMemberName::Identifier(name),
                        optional,
                        readonly,
                        type_annotation,
                    });
                }

                self.skip_trivia();
                // Allow ; or , or nothing as separator
                let _ = self.consume(";") || self.consume(",");
                self.skip_trivia();
            }

            if !self.consume("}") {
                return Err(Error::InternalError("Expected '}' in object type".to_string()));
            }
            return Ok(TypeAnnotation::Object(members));
        }

        // Parenthesized or function type
        if self.consume("(") {
            self.skip_trivia();

            // Could be parenthesized type or function type
            // Try to parse as function type
            let mut params = Vec::new();

            while !self.check(")") && !self.is_eof() {
                let rest = self.consume("...");
                self.skip_trivia();

                let name = self.read_identifier().unwrap_or_default();
                self.skip_trivia();

                let optional = self.consume("?");
                self.skip_trivia();

                let type_annotation = if self.consume(":") {
                    self.skip_trivia();
                    Some(self.parse_type_annotation()?)
                } else {
                    None
                };

                params.push(FunctionParam {
                    name,
                    optional,
                    rest,
                    type_annotation,
                });

                self.skip_trivia();
                if !self.consume(",") {
                    break;
                }
                self.skip_trivia();
            }

            if !self.consume(")") {
                return Err(Error::InternalError("Expected ')' in function type".to_string()));
            }

            self.skip_trivia();

            // Check for arrow
            if self.consume("=>") {
                self.skip_trivia();
                let return_type = self.parse_type_annotation()?;
                return Ok(TypeAnnotation::Function(FunctionType {
                    type_params: vec![],
                    params,
                    return_type: Box::new(return_type),
                }));
            }

            // If no arrow and we had a single type with no name, it's parenthesized
            if params.len() == 1 && params[0].name.is_empty() {
                if let Some(t) = params.pop().unwrap().type_annotation {
                    return Ok(TypeAnnotation::Parenthesized(Box::new(t)));
                }
            }

            // Otherwise treat as unknown function returning unknown
            return Ok(TypeAnnotation::Function(FunctionType {
                type_params: vec![],
                params,
                return_type: Box::new(TypeAnnotation::Unknown),
            }));
        }

        // String literal type
        if self.peek() == Some('"') || self.peek() == Some('\'') {
            let quote = self.advance().unwrap();
            let start = self.pos;
            while let Some(c) = self.peek() {
                if c == quote {
                    break;
                }
                if c == '\\' {
                    self.advance();
                }
                self.advance();
            }
            let value = self.source[start..self.pos].to_string();
            self.advance(); // closing quote
            return Ok(TypeAnnotation::Literal(LiteralType::String(value)));
        }

        // Number literal type
        if let Some(c) = self.peek() {
            if c.is_ascii_digit() || (c == '-' && self.peek_ahead(1).map(|c| c.is_ascii_digit()).unwrap_or(false)) {
                let start = self.pos;
                if c == '-' {
                    self.advance();
                }
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-' {
                        self.advance();
                    } else {
                        break;
                    }
                }
                let num_str = &self.source[start..self.pos];
                if let Ok(n) = num_str.parse::<f64>() {
                    return Ok(TypeAnnotation::Literal(LiteralType::Number(n)));
                }
            }
        }

        // Boolean literal type
        if self.check_keyword("true") {
            self.consume("true");
            return Ok(TypeAnnotation::Literal(LiteralType::Boolean(true)));
        }
        if self.check_keyword("false") {
            self.consume("false");
            return Ok(TypeAnnotation::Literal(LiteralType::Boolean(false)));
        }

        // Type reference (named type)
        if let Some(name) = self.read_identifier() {
            let mut names = vec![name];

            // Check for qualified names (A.B.C)
            self.skip_trivia();
            while self.consume(".") {
                self.skip_trivia();
                if let Some(n) = self.read_identifier() {
                    names.push(n);
                } else {
                    break;
                }
                self.skip_trivia();
            }

            // Check for type arguments
            let mut type_args = Vec::new();
            self.skip_trivia();
            if self.consume("<") {
                self.skip_trivia();
                if !self.check(">") {
                    type_args.push(self.parse_type_annotation()?);
                    self.skip_trivia();
                    while self.consume(",") {
                        self.skip_trivia();
                        type_args.push(self.parse_type_annotation()?);
                        self.skip_trivia();
                    }
                }
                if !self.consume(">") {
                    return Err(Error::InternalError("Expected '>' in type arguments".to_string()));
                }
            }

            // Check for array suffix T[]
            let mut result_type = TypeAnnotation::Reference(TypeReference {
                name: names,
                type_args,
            });

            self.skip_trivia();
            while self.consume("[") {
                self.skip_trivia();
                if !self.consume("]") {
                    return Err(Error::InternalError("Expected ']' for array type".to_string()));
                }
                result_type = TypeAnnotation::Array(Box::new(result_type));
                self.skip_trivia();
            }

            return Ok(result_type);
        }

        Err(Error::InternalError(format!(
            "Unexpected character in type: {:?}",
            self.peek()
        )))
    }

    /// Parse type parameters: <T, U extends Constraint = Default>
    pub fn parse_type_parameters(&mut self) -> Result<Vec<TypeParameter>> {
        let mut params = Vec::new();

        self.skip_trivia();
        if !self.consume("<") {
            return Ok(params);
        }

        self.skip_trivia();
        while !self.check(">") && !self.is_eof() {
            let name = self.read_identifier()
                .ok_or_else(|| Error::InternalError("Expected type parameter name".to_string()))?;

            self.skip_trivia();

            // Check for extends constraint
            let constraint = if self.check_keyword("extends") {
                self.consume("extends");
                self.skip_trivia();
                Some(Box::new(self.parse_type_annotation()?))
            } else {
                None
            };

            self.skip_trivia();

            // Check for default type
            let default = if self.consume("=") {
                self.skip_trivia();
                Some(Box::new(self.parse_type_annotation()?))
            } else {
                None
            };

            params.push(TypeParameter {
                name,
                constraint,
                default,
            });

            self.skip_trivia();
            if !self.consume(",") {
                break;
            }
            self.skip_trivia();
        }

        if !self.consume(">") {
            return Err(Error::InternalError("Expected '>' in type parameters".to_string()));
        }

        Ok(params)
    }

    /// Parse an interface declaration
    pub fn parse_interface(&mut self) -> Result<InterfaceDeclaration> {
        self.skip_trivia();

        // interface keyword already consumed
        let name = self.read_identifier()
            .ok_or_else(|| Error::InternalError("Expected interface name".to_string()))?;

        self.skip_trivia();
        let type_params = self.parse_type_parameters()?;

        // Parse extends clause
        self.skip_trivia();
        let mut extends = Vec::new();
        if self.check_keyword("extends") {
            self.consume("extends");
            self.skip_trivia();

            loop {
                let ext_name = self.read_identifier()
                    .ok_or_else(|| Error::InternalError("Expected type name in extends".to_string()))?;

                self.skip_trivia();
                let mut type_args = Vec::new();
                if self.consume("<") {
                    self.skip_trivia();
                    while !self.check(">") && !self.is_eof() {
                        type_args.push(self.parse_type_annotation()?);
                        self.skip_trivia();
                        if !self.consume(",") {
                            break;
                        }
                        self.skip_trivia();
                    }
                    self.consume(">");
                }

                extends.push(TypeReference {
                    name: vec![ext_name],
                    type_args,
                });

                self.skip_trivia();
                if !self.consume(",") {
                    break;
                }
                self.skip_trivia();
            }
        }

        // Parse body
        self.skip_trivia();
        if !self.consume("{") {
            return Err(Error::InternalError("Expected '{' in interface".to_string()));
        }

        let mut members = Vec::new();
        self.skip_trivia();

        while !self.check("}") && !self.is_eof() {
            let readonly = self.check_keyword("readonly");
            if readonly {
                self.consume("readonly");
                self.skip_trivia();
            }

            let name = self.read_identifier()
                .ok_or_else(|| Error::InternalError("Expected property name".to_string()))?;

            self.skip_trivia();
            let optional = self.consume("?");
            self.skip_trivia();

            let type_annotation = if self.consume(":") {
                self.skip_trivia();
                Some(self.parse_type_annotation()?)
            } else {
                None
            };

            members.push(TypeMember {
                name: TypeMemberName::Identifier(name),
                optional,
                readonly,
                type_annotation,
            });

            self.skip_trivia();
            let _ = self.consume(";") || self.consume(",");
            self.skip_trivia();
        }

        if !self.consume("}") {
            return Err(Error::InternalError("Expected '}' in interface".to_string()));
        }

        Ok(InterfaceDeclaration {
            name,
            type_params,
            extends,
            members,
        })
    }

    /// Parse a type alias declaration
    pub fn parse_type_alias(&mut self) -> Result<TypeAliasDeclaration> {
        self.skip_trivia();

        // type keyword already consumed
        let name = self.read_identifier()
            .ok_or_else(|| Error::InternalError("Expected type alias name".to_string()))?;

        self.skip_trivia();
        let type_params = self.parse_type_parameters()?;

        self.skip_trivia();
        if !self.consume("=") {
            return Err(Error::InternalError("Expected '=' in type alias".to_string()));
        }

        self.skip_trivia();
        let type_annotation = self.parse_type_annotation()?;

        Ok(TypeAliasDeclaration {
            name,
            type_params,
            type_annotation,
        })
    }

    /// Parse an enum declaration
    pub fn parse_enum(&mut self, is_const: bool) -> Result<EnumDeclaration> {
        self.skip_trivia();

        // enum keyword already consumed
        let name = self.read_identifier()
            .ok_or_else(|| Error::InternalError("Expected enum name".to_string()))?;

        self.skip_trivia();
        if !self.consume("{") {
            return Err(Error::InternalError("Expected '{' in enum".to_string()));
        }

        let mut members = Vec::new();
        self.skip_trivia();

        while !self.check("}") && !self.is_eof() {
            let member_name = self.read_identifier()
                .ok_or_else(|| Error::InternalError("Expected enum member name".to_string()))?;

            self.skip_trivia();

            let initializer = if self.consume("=") {
                self.skip_trivia();
                // Read until , or }
                let start = self.pos;
                let mut depth = 0;
                while let Some(c) = self.peek() {
                    if c == '(' || c == '[' || c == '{' {
                        depth += 1;
                    } else if c == ')' || c == ']' || c == '}' {
                        if depth == 0 && c == '}' {
                            break;
                        }
                        depth -= 1;
                    } else if c == ',' && depth == 0 {
                        break;
                    }
                    self.advance();
                }
                Some(self.source[start..self.pos].trim().to_string())
            } else {
                None
            };

            members.push(EnumMember {
                name: member_name,
                initializer,
            });

            self.skip_trivia();
            self.consume(",");
            self.skip_trivia();
        }

        if !self.consume("}") {
            return Err(Error::InternalError("Expected '}' in enum".to_string()));
        }

        Ok(EnumDeclaration {
            name,
            is_const,
            members,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_primitive_types() {
        let mut parser = TypeScriptParser::new("string");
        let result = parser.parse_type_annotation().unwrap();
        assert_eq!(result, TypeAnnotation::Primitive(PrimitiveType::String));

        let mut parser = TypeScriptParser::new("number");
        let result = parser.parse_type_annotation().unwrap();
        assert_eq!(result, TypeAnnotation::Primitive(PrimitiveType::Number));
    }

    #[test]
    fn test_parse_union_type() {
        let mut parser = TypeScriptParser::new("string | number");
        let result = parser.parse_type_annotation().unwrap();
        assert!(matches!(result, TypeAnnotation::Union(_)));
    }

    #[test]
    fn test_parse_array_type() {
        let mut parser = TypeScriptParser::new("string[]");
        let result = parser.parse_type_annotation().unwrap();
        assert!(matches!(result, TypeAnnotation::Array(_)));
    }

    #[test]
    fn test_parse_generic_array() {
        let mut parser = TypeScriptParser::new("Array<string>");
        let result = parser.parse_type_annotation().unwrap();
        assert!(matches!(result, TypeAnnotation::GenericArray(_)));
    }

    #[test]
    fn test_parse_type_reference() {
        let mut parser = TypeScriptParser::new("MyType<T, U>");
        let result = parser.parse_type_annotation().unwrap();
        if let TypeAnnotation::Reference(ref_type) = result {
            assert_eq!(ref_type.name, vec!["MyType"]);
            assert_eq!(ref_type.type_args.len(), 2);
        } else {
            panic!("Expected TypeReference");
        }
    }
}
