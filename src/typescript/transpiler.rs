//! TypeScript Transpiler
//!
//! This module provides the main transpiler that converts TypeScript to JavaScript
//! by stripping types and transforming TypeScript-specific constructs.

use super::TranspileOptions;
use crate::error::{Error, Result};

/// TypeScript to JavaScript transpiler
pub struct TypeScriptTranspiler {
    options: TranspileOptions,
}

impl Default for TypeScriptTranspiler {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeScriptTranspiler {
    /// Create a new transpiler with default options
    pub fn new() -> Self {
        Self {
            options: TranspileOptions::default(),
        }
    }

    /// Create a new transpiler with custom options
    pub fn with_options(options: TranspileOptions) -> Self {
        Self { options }
    }

    /// Transpile TypeScript source code to JavaScript
    pub fn transpile(&self, source: &str) -> Result<String> {
        let mut output = String::with_capacity(source.len());
        let mut pos = 0;
        let bytes = source.as_bytes();

        while pos < bytes.len() {
            // Skip whitespace and copy to output
            let ws_start = pos;
            while pos < bytes.len() && (bytes[pos] as char).is_whitespace() {
                pos += 1;
            }
            output.push_str(&source[ws_start..pos]);

            if pos >= bytes.len() {
                break;
            }

            // Check for comments
            if pos + 1 < bytes.len() && bytes[pos] == b'/' {
                if bytes[pos + 1] == b'/' {
                    // Single-line comment - copy to output
                    let start = pos;
                    while pos < bytes.len() && bytes[pos] != b'\n' {
                        pos += 1;
                    }
                    if !self.options.remove_comments {
                        output.push_str(&source[start..pos]);
                    }
                    continue;
                } else if bytes[pos + 1] == b'*' {
                    // Multi-line comment
                    let start = pos;
                    pos += 2;
                    while pos + 1 < bytes.len() {
                        if bytes[pos] == b'*' && bytes[pos + 1] == b'/' {
                            pos += 2;
                            break;
                        }
                        pos += 1;
                    }
                    if !self.options.remove_comments {
                        output.push_str(&source[start..pos]);
                    }
                    continue;
                }
            }

            // Check for string literals - copy them unchanged
            if bytes[pos] == b'"' || bytes[pos] == b'\'' {
                let quote = bytes[pos];
                let start = pos;
                pos += 1;
                while pos < bytes.len() {
                    if bytes[pos] == b'\\' && pos + 1 < bytes.len() {
                        pos += 2;
                        continue;
                    }
                    if bytes[pos] == quote {
                        pos += 1;
                        break;
                    }
                    pos += 1;
                }
                output.push_str(&source[start..pos]);
                continue;
            }

            // Check for template literals
            if bytes[pos] == b'`' {
                let start = pos;
                pos += 1;
                while pos < bytes.len() {
                    if bytes[pos] == b'\\' && pos + 1 < bytes.len() {
                        pos += 2;
                        continue;
                    }
                    if bytes[pos] == b'`' {
                        pos += 1;
                        break;
                    }
                    if bytes[pos] == b'$' && pos + 1 < bytes.len() && bytes[pos + 1] == b'{' {
                        // Template expression
                        pos += 2;
                        let mut depth = 1;
                        while pos < bytes.len() && depth > 0 {
                            if bytes[pos] == b'{' {
                                depth += 1;
                            } else if bytes[pos] == b'}' {
                                depth -= 1;
                            }
                            pos += 1;
                        }
                        continue;
                    }
                    pos += 1;
                }
                output.push_str(&source[start..pos]);
                continue;
            }

            // Check for interface declaration
            if self.check_keyword(source, pos, "interface") {
                pos = self.skip_interface(source, pos)?;
                continue;
            }

            // Check for type alias declaration
            if self.check_keyword(source, pos, "type") && self.is_type_alias(source, pos) {
                pos = self.skip_type_alias(source, pos)?;
                continue;
            }

            // Check for enum declaration
            if self.check_keyword(source, pos, "enum") {
                pos = self.transpile_enum(&mut output, source, pos, false)?;
                continue;
            }

            // Check for const enum
            if self.check_keyword(source, pos, "const") {
                let after_const = self.skip_whitespace(source, pos + 5);
                if self.check_keyword(source, after_const, "enum") {
                    pos = self.transpile_enum(&mut output, source, after_const, true)?;
                    continue;
                }
            }

            // Check for declare statement (remove entirely)
            if self.check_keyword(source, pos, "declare") {
                pos = self.skip_declare(source, pos)?;
                continue;
            }

            // Check for namespace/module (TypeScript modules)
            if self.check_keyword(source, pos, "namespace") || self.check_keyword(source, pos, "module") {
                pos = self.skip_namespace(source, pos)?;
                continue;
            }

            // Check for access modifiers before class members
            if self.check_keyword(source, pos, "public") ||
               self.check_keyword(source, pos, "private") ||
               self.check_keyword(source, pos, "protected") ||
               self.check_keyword(source, pos, "readonly") {
                // Skip the modifier
                pos = self.skip_identifier(source, pos);
                continue;
            }

            // Check for abstract keyword
            if self.check_keyword(source, pos, "abstract") {
                let after_abstract = self.skip_whitespace(source, pos + 8);
                if self.check_keyword(source, after_abstract, "class") {
                    // Keep abstract class as just class
                    pos = after_abstract;
                    continue;
                }
                // Skip abstract for methods
                pos = self.skip_identifier(source, pos);
                continue;
            }

            // Check for override keyword
            if self.check_keyword(source, pos, "override") {
                pos = self.skip_identifier(source, pos);
                continue;
            }

            // Check for type annotations (: Type)
            if bytes[pos] == b':' && self.is_type_annotation_context(source, pos) {
                pos = self.skip_type_annotation(source, pos)?;
                // Add a space if the next character is not a space or punctuation
                let bytes = source.as_bytes();
                if pos < bytes.len() {
                    let next = bytes[pos] as char;
                    if next.is_alphanumeric() || next == '=' || next == '{' {
                        output.push(' ');
                    }
                }
                continue;
            }

            // Check for type parameters (<T, U>)
            if bytes[pos] == b'<' && self.is_type_parameter_context(source, pos) {
                pos = self.skip_type_parameters(source, pos)?;
                continue;
            }

            // Check for type assertion (as Type)
            if self.check_keyword(source, pos, "as") && self.is_type_assertion_context(source, pos) {
                pos = self.skip_type_assertion(source, pos)?;
                continue;
            }

            // Check for non-null assertion (!)
            if bytes[pos] == b'!' && self.is_non_null_assertion(source, pos) {
                // Skip the !
                pos += 1;
                continue;
            }

            // Check for definite assignment assertion (!)
            if bytes[pos] == b'!' && pos + 1 < bytes.len() && bytes[pos + 1] == b':' {
                // Skip the ! before type annotation
                pos += 1;
                continue;
            }

            // Check for satisfies keyword
            if self.check_keyword(source, pos, "satisfies") {
                pos = self.skip_satisfies(source, pos)?;
                continue;
            }

            // Check for import type { ... }
            if self.check_keyword(source, pos, "import") {
                let after_import = self.skip_whitespace(source, pos + 6);
                if self.check_keyword(source, after_import, "type") {
                    // import type statement - skip entirely
                    pos = self.skip_to_next_statement(source, pos)?;
                    continue;
                }
            }

            // Check for export type { ... }
            if self.check_keyword(source, pos, "export") {
                let after_export = self.skip_whitespace(source, pos + 6);
                if self.check_keyword(source, after_export, "type") {
                    // export type statement - skip entirely
                    pos = self.skip_to_next_statement(source, pos)?;
                    continue;
                }
            }

            // Default: copy character to output
            output.push(source[pos..].chars().next().unwrap());
            pos += source[pos..].chars().next().unwrap().len_utf8();
        }

        Ok(self.cleanup_output(&output))
    }

    /// Check if the text at pos is a keyword
    fn check_keyword(&self, source: &str, pos: usize, keyword: &str) -> bool {
        if !source[pos..].starts_with(keyword) {
            return false;
        }
        let next_pos = pos + keyword.len();
        if next_pos >= source.len() {
            return true;
        }
        let next_char = source[next_pos..].chars().next().unwrap();
        !next_char.is_alphanumeric() && next_char != '_' && next_char != '$'
    }

    /// Skip whitespace and return new position
    fn skip_whitespace(&self, source: &str, mut pos: usize) -> usize {
        let bytes = source.as_bytes();
        while pos < bytes.len() && (bytes[pos] as char).is_whitespace() {
            pos += 1;
        }
        pos
    }

    /// Skip an identifier and return new position
    fn skip_identifier(&self, source: &str, mut pos: usize) -> usize {
        let bytes = source.as_bytes();
        while pos < bytes.len() {
            let c = bytes[pos] as char;
            if c.is_alphanumeric() || c == '_' || c == '$' {
                pos += 1;
            } else {
                break;
            }
        }
        pos
    }

    /// Check if this is a type alias (not a destructuring with 'type' identifier)
    fn is_type_alias(&self, source: &str, pos: usize) -> bool {
        // Check that 'type' is followed by an identifier and then either < or =
        let after_type = self.skip_identifier(source, pos);
        let after_ws = self.skip_whitespace(source, after_type);

        // Read the potential type name
        let name_start = after_ws;
        let name_end = self.skip_identifier(source, name_start);

        if name_end == name_start {
            return false;
        }

        let after_name = self.skip_whitespace(source, name_end);
        if after_name >= source.len() {
            return false;
        }

        let next_char = source.as_bytes()[after_name] as char;
        next_char == '=' || next_char == '<'
    }

    /// Skip an interface declaration
    fn skip_interface(&self, source: &str, mut pos: usize) -> Result<usize> {
        // Skip "interface"
        pos = self.skip_identifier(source, pos);
        pos = self.skip_whitespace(source, pos);

        // Skip name
        pos = self.skip_identifier(source, pos);
        pos = self.skip_whitespace(source, pos);

        // Skip type parameters
        let bytes = source.as_bytes();
        if pos < bytes.len() && bytes[pos] == b'<' {
            pos = self.skip_balanced(source, pos, b'<', b'>')?;
        }
        pos = self.skip_whitespace(source, pos);

        // Skip extends clause
        if self.check_keyword(source, pos, "extends") {
            pos = self.skip_identifier(source, pos);
            pos = self.skip_whitespace(source, pos);
            // Skip extended types (comma-separated)
            while pos < bytes.len() && bytes[pos] != b'{' {
                pos = self.skip_identifier(source, pos);
                pos = self.skip_whitespace(source, pos);
                if pos < bytes.len() && bytes[pos] == b'<' {
                    pos = self.skip_balanced(source, pos, b'<', b'>')?;
                }
                pos = self.skip_whitespace(source, pos);
                if pos < bytes.len() && bytes[pos] == b',' {
                    pos += 1;
                    pos = self.skip_whitespace(source, pos);
                }
            }
        }

        // Skip body
        if pos < bytes.len() && bytes[pos] == b'{' {
            pos = self.skip_balanced(source, pos, b'{', b'}')?;
        }

        Ok(pos)
    }

    /// Skip a type alias declaration
    fn skip_type_alias(&self, source: &str, mut pos: usize) -> Result<usize> {
        // Skip "type"
        pos = self.skip_identifier(source, pos);
        pos = self.skip_whitespace(source, pos);

        // Skip name
        pos = self.skip_identifier(source, pos);
        pos = self.skip_whitespace(source, pos);

        // Skip type parameters
        let bytes = source.as_bytes();
        if pos < bytes.len() && bytes[pos] == b'<' {
            pos = self.skip_balanced(source, pos, b'<', b'>')?;
        }
        pos = self.skip_whitespace(source, pos);

        // Skip =
        if pos < bytes.len() && bytes[pos] == b'=' {
            pos += 1;
        }
        pos = self.skip_whitespace(source, pos);

        // Skip the type (until ; or newline at same level)
        pos = self.skip_type_expression(source, pos)?;

        // Skip optional semicolon
        if pos < bytes.len() && bytes[pos] == b';' {
            pos += 1;
        }

        Ok(pos)
    }

    /// Skip a declare statement
    fn skip_declare(&self, source: &str, mut pos: usize) -> Result<usize> {
        // Skip until we find a matching } or ;
        let bytes = source.as_bytes();

        // Skip "declare"
        pos = self.skip_identifier(source, pos);
        pos = self.skip_whitespace(source, pos);

        // Check what follows
        if self.check_keyword(source, pos, "global") {
            pos = self.skip_identifier(source, pos);
            pos = self.skip_whitespace(source, pos);
        }

        if self.check_keyword(source, pos, "module") || self.check_keyword(source, pos, "namespace") {
            pos = self.skip_namespace(source, pos)?;
        } else if pos < bytes.len() && bytes[pos] == b'{' {
            pos = self.skip_balanced(source, pos, b'{', b'}')?;
        } else {
            pos = self.skip_to_next_statement(source, pos)?;
        }

        Ok(pos)
    }

    /// Skip a namespace/module declaration
    fn skip_namespace(&self, source: &str, mut pos: usize) -> Result<usize> {
        let bytes = source.as_bytes();

        // Skip "namespace" or "module"
        pos = self.skip_identifier(source, pos);
        pos = self.skip_whitespace(source, pos);

        // Skip name (possibly dotted)
        while pos < bytes.len() && bytes[pos] != b'{' {
            pos = self.skip_identifier(source, pos);
            pos = self.skip_whitespace(source, pos);
            if pos < bytes.len() && bytes[pos] == b'.' {
                pos += 1;
                pos = self.skip_whitespace(source, pos);
            }
        }

        // Skip body
        if pos < bytes.len() && bytes[pos] == b'{' {
            pos = self.skip_balanced(source, pos, b'{', b'}')?;
        }

        Ok(pos)
    }

    /// Transpile an enum to a JavaScript object
    fn transpile_enum(&self, output: &mut String, source: &str, mut pos: usize, is_const: bool) -> Result<usize> {
        let bytes = source.as_bytes();

        // Skip "enum"
        pos = self.skip_identifier(source, pos);
        pos = self.skip_whitespace(source, pos);

        // Get enum name
        let name_start = pos;
        pos = self.skip_identifier(source, pos);
        let name = &source[name_start..pos];

        // For const enums, we might inline them later
        // For now, we'll emit them as regular objects
        if is_const && !self.options.preserve_const_enums {
            // Skip the entire enum
            pos = self.skip_whitespace(source, pos);
            if pos < bytes.len() && bytes[pos] == b'{' {
                pos = self.skip_balanced(source, pos, b'{', b'}')?;
            }
            return Ok(pos);
        }

        pos = self.skip_whitespace(source, pos);

        // Skip {
        if pos >= bytes.len() || bytes[pos] != b'{' {
            return Err(Error::InternalError("Expected '{' in enum".to_string()));
        }
        pos += 1;
        pos = self.skip_whitespace(source, pos);

        // Parse members
        let mut members: Vec<(String, String)> = Vec::new();
        let mut next_value = 0i64;

        while pos < bytes.len() && bytes[pos] != b'}' {
            // Get member name
            let member_start = pos;
            pos = self.skip_identifier(source, pos);
            let member_name = source[member_start..pos].to_string();
            pos = self.skip_whitespace(source, pos);

            // Check for initializer
            let value = if pos < bytes.len() && bytes[pos] == b'=' {
                pos += 1;
                pos = self.skip_whitespace(source, pos);

                // Read the value expression
                let value_start = pos;
                while pos < bytes.len() {
                    let c = bytes[pos] as char;
                    if c == ',' || c == '}' {
                        break;
                    }
                    pos += 1;
                }
                let value_str = source[value_start..pos].trim();

                // Try to parse as number for auto-increment
                if let Ok(n) = value_str.parse::<i64>() {
                    next_value = n + 1;
                }

                value_str.to_string()
            } else {
                let v = next_value.to_string();
                next_value += 1;
                v
            };

            members.push((member_name, value));

            pos = self.skip_whitespace(source, pos);
            if pos < bytes.len() && bytes[pos] == b',' {
                pos += 1;
                pos = self.skip_whitespace(source, pos);
            }
        }

        // Skip }
        if pos < bytes.len() && bytes[pos] == b'}' {
            pos += 1;
        }

        // Emit JavaScript enum implementation
        output.push_str("var ");
        output.push_str(name);
        output.push_str(";\n(function (");
        output.push_str(name);
        output.push_str(") {\n");

        for (member_name, value) in &members {
            // Check if value is a string literal
            let is_string = value.starts_with('"') || value.starts_with('\'');

            if is_string {
                // String enum: only single mapping
                output.push_str("    ");
                output.push_str(name);
                output.push_str("[\"");
                output.push_str(member_name);
                output.push_str("\"] = ");
                output.push_str(value);
                output.push_str(";\n");
            } else {
                // Numeric enum: bi-directional mapping
                output.push_str("    ");
                output.push_str(name);
                output.push('[');
                output.push_str(name);
                output.push_str("[\"");
                output.push_str(member_name);
                output.push_str("\"] = ");
                output.push_str(value);
                output.push_str("] = \"");
                output.push_str(member_name);
                output.push_str("\";\n");
            }
        }

        output.push_str("})(");
        output.push_str(name);
        output.push_str(" || (");
        output.push_str(name);
        output.push_str(" = {}));");

        Ok(pos)
    }

    /// Check if this is a type annotation context
    fn is_type_annotation_context(&self, source: &str, pos: usize) -> bool {
        // Look back to see what precedes the colon
        if pos == 0 {
            return false;
        }

        // Skip back over whitespace
        let bytes = source.as_bytes();
        let mut back = pos - 1;
        while back > 0 && (bytes[back] as char).is_whitespace() {
            back -= 1;
        }

        let prev_char = bytes[back] as char;

        // First check: what comes after the colon?
        // If it's a string literal, number literal, or expression starting with [, {, (, etc.
        // then this is likely an object property value, not a type annotation
        let after_colon = self.skip_whitespace(source, pos + 1);
        if after_colon < bytes.len() {
            let next_char = bytes[after_colon] as char;
            // String literals are values, not types (in most contexts)
            if next_char == '"' || next_char == '\'' || next_char == '`' {
                return false;
            }
            // Numbers starting the "type" are likely values unless in specific type contexts
            if next_char.is_ascii_digit() {
                // Check if this looks like a type annotation context (after a variable declaration)
                // Look further back for 'let', 'const', 'var', function params, etc.
                if !self.looks_like_variable_declaration(source, pos) {
                    return false;
                }
            }
            // [ starting could be tuple type or array literal - need more context
            if next_char == '[' || next_char == '{' {
                // In object literals, these are values
                // Check if we're in an object literal context
                if self.is_in_object_literal(source, pos) {
                    return false;
                }
            }
        }

        // Type annotation follows:
        // - identifier (let x: Type)
        // - ) for return type (function(): Type)
        // - ? for optional (x?: Type)
        // - ] for array destructure ([a]: Type)
        // - } for object destructure ({a}: Type)
        // - > for generic params (<T>(): Type)
        prev_char.is_alphanumeric() ||
            prev_char == '_' ||
            prev_char == '$' ||
            prev_char == ')' ||
            prev_char == '?' ||
            prev_char == ']' ||
            prev_char == '}' ||
            prev_char == '>'
    }

    /// Check if we're inside an object literal
    fn is_in_object_literal(&self, source: &str, pos: usize) -> bool {
        let bytes = source.as_bytes();
        let mut depth = 0;
        let mut i = pos;

        // Look back for unmatched {
        while i > 0 {
            i -= 1;
            let c = bytes[i] as char;

            match c {
                '}' => depth += 1,
                '{' => {
                    if depth == 0 {
                        // Found an unmatched { - check if it's an object literal
                        // Look back to see what precedes it
                        let mut back = i;
                        back = back.saturating_sub(1);
                        while back > 0 && (bytes[back] as char).is_whitespace() {
                            back -= 1;
                        }
                        if back < bytes.len() {
                            let prev = bytes[back] as char;
                            // Object literal follows: =, (, [, ,, :, return, etc.
                            // Not object literal: class/interface body, function body
                            if prev == '=' || prev == '(' || prev == '[' || prev == ',' || prev == ':' {
                                return true;
                            }
                            // Check for 'return {'
                            if prev == 'n' && back >= 5 {
                                let start = back - 5;
                                if &source[start..=back] == "return" {
                                    return true;
                                }
                            }
                        }
                        return false;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }

        false
    }

    /// Check if position looks like it's in a variable declaration
    fn looks_like_variable_declaration(&self, source: &str, pos: usize) -> bool {
        let bytes = source.as_bytes();
        let mut i = pos;

        // Look back past the identifier and optional ?
        while i > 0 {
            i -= 1;
            let c = bytes[i] as char;
            if c.is_whitespace() {
                continue;
            }
            if c.is_alphanumeric() || c == '_' || c == '$' || c == '?' {
                continue;
            }
            // Found a non-identifier char
            break;
        }

        // Now check if we have let/const/var/function param before this
        // Skip back over whitespace
        while i > 0 && (bytes[i] as char).is_whitespace() {
            i -= 1;
        }

        // Check for keywords
        let start = i.saturating_sub(5);
        let slice = &source[start..=i.min(source.len() - 1)];

        slice.ends_with("let") || slice.ends_with("const") || slice.ends_with("var") ||
            slice.ends_with("(") || slice.ends_with(",")
    }

    /// Skip a type annotation (: Type)
    fn skip_type_annotation(&self, source: &str, mut pos: usize) -> Result<usize> {
        // Skip the colon
        pos += 1;
        pos = self.skip_whitespace(source, pos);

        // Skip the type
        pos = self.skip_type_expression(source, pos)?;

        Ok(pos)
    }

    /// Skip a type expression
    fn skip_type_expression(&self, source: &str, mut pos: usize) -> Result<usize> {
        let bytes = source.as_bytes();
        let mut depth = 0;
        let mut angle_depth = 0;

        while pos < bytes.len() {
            let c = bytes[pos] as char;

            match c {
                '(' | '[' => depth += 1,
                '{' => {
                    // For type annotations, { at depth 0 means start of function body
                    // Type expressions can contain { only inside nested structures
                    if depth == 0 && angle_depth == 0 {
                        break;
                    }
                    depth += 1;
                }
                ')' | ']' | '}' => {
                    if depth == 0 && angle_depth == 0 {
                        break;
                    }
                    if depth > 0 {
                        depth -= 1;
                    }
                }
                '<' => angle_depth += 1,
                '>' => {
                    if angle_depth > 0 {
                        angle_depth -= 1;
                    } else if depth == 0 {
                        break;
                    }
                }
                ',' | ';' | '=' => {
                    if depth == 0 && angle_depth == 0 {
                        break;
                    }
                }
                '\n' => {
                    // Check if we should continue on next line
                    if depth == 0 && angle_depth == 0 {
                        // Look ahead - if next non-whitespace is | or &, continue
                        let next_pos = self.skip_whitespace(source, pos + 1);
                        if next_pos < bytes.len() {
                            let next_char = bytes[next_pos] as char;
                            if next_char != '|' && next_char != '&' {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                _ => {}
            }

            pos += 1;
        }

        Ok(pos)
    }

    /// Check if this looks like a type parameter context
    fn is_type_parameter_context(&self, source: &str, pos: usize) -> bool {
        if pos == 0 {
            return false;
        }

        let bytes = source.as_bytes();
        let mut back = pos - 1;

        // Skip whitespace
        while back > 0 && (bytes[back] as char).is_whitespace() {
            back -= 1;
        }

        let prev_char = bytes[back] as char;

        // Type parameters follow:
        // - function name (function foo<T>)
        // - class name (class Foo<T>)
        // - interface name (interface Foo<T>)
        // - type alias name (type Foo<T>)
        // - method name (method<T>())
        // - arrow function start (const x = <T>)
        if prev_char.is_alphanumeric() || prev_char == '_' || prev_char == '$' {
            // Look for identifier before
            let id_end = back + 1;
            while back > 0 && {
                let c = bytes[back] as char;
                c.is_alphanumeric() || c == '_' || c == '$'
            } {
                back -= 1;
            }

            let id_start = if (bytes[back] as char).is_alphanumeric() ||
                           bytes[back] == b'_' || bytes[back] == b'$' {
                back
            } else {
                back + 1
            };

            let identifier = &source[id_start..id_end];

            // These are definitely type parameter contexts
            if identifier == "function" || identifier == "class" ||
               identifier == "interface" || identifier == "type" {
                return true;
            }

            // For other identifiers, check if this looks like a call with type args
            // vs a comparison (a < b > c)
            let after_angle = self.skip_balanced_angle_tentative(source, pos);
            if let Some(end_pos) = after_angle {
                let after = self.skip_whitespace(source, end_pos);
                if after < bytes.len() {
                    let next = bytes[after] as char;
                    // If followed by ( or =>, it's type params
                    return next == '(' || (after + 1 < bytes.len() &&
                        bytes[after] == b'=' && bytes[after + 1] == b'>');
                }
            }
        }

        false
    }

    /// Try to skip balanced angle brackets, return None if it doesn't look like type params
    fn skip_balanced_angle_tentative(&self, source: &str, mut pos: usize) -> Option<usize> {
        let bytes = source.as_bytes();
        if pos >= bytes.len() || bytes[pos] != b'<' {
            return None;
        }

        pos += 1;
        let mut depth = 1;

        while pos < bytes.len() && depth > 0 {
            let c = bytes[pos] as char;
            match c {
                '<' => depth += 1,
                '>' => depth -= 1,
                '(' | '[' | '{' => {
                    // Nested brackets in type params
                    pos = self.skip_balanced(source, pos,
                        c as u8,
                        match c {
                            '(' => b')',
                            '[' => b']',
                            _ => b'}',
                        }).ok()?;
                    continue;
                }
                // If we see these, probably not type params
                '+' | '-' | '*' | '/' | '%' => return None,
                _ => {}
            }
            pos += 1;
        }

        if depth == 0 {
            Some(pos)
        } else {
            None
        }
    }

    /// Skip type parameters
    fn skip_type_parameters(&self, source: &str, pos: usize) -> Result<usize> {
        self.skip_balanced(source, pos, b'<', b'>')
    }

    /// Check if this is a type assertion context (as Type)
    fn is_type_assertion_context(&self, source: &str, pos: usize) -> bool {
        if pos == 0 {
            return false;
        }

        let bytes = source.as_bytes();
        let mut back = pos - 1;

        // Skip whitespace
        while back > 0 && (bytes[back] as char).is_whitespace() {
            back -= 1;
        }

        let prev_char = bytes[back] as char;

        // Type assertion follows an expression:
        // - identifier (x as Type)
        // - ) (call() as Type)
        // - ] (arr[i] as Type)
        // - ' or " (literal "str" as Type)
        prev_char.is_alphanumeric() ||
            prev_char == '_' ||
            prev_char == '$' ||
            prev_char == ')' ||
            prev_char == ']' ||
            prev_char == '\'' ||
            prev_char == '"' ||
            prev_char == '`'
    }

    /// Skip type assertion (as Type)
    fn skip_type_assertion(&self, source: &str, mut pos: usize) -> Result<usize> {
        // Skip "as"
        pos += 2;
        pos = self.skip_whitespace(source, pos);

        // Check for "as const"
        if self.check_keyword(source, pos, "const") {
            pos = self.skip_identifier(source, pos);
            return Ok(pos);
        }

        // Skip the type
        pos = self.skip_type_expression(source, pos)?;

        Ok(pos)
    }

    /// Check if this is a non-null assertion
    fn is_non_null_assertion(&self, source: &str, pos: usize) -> bool {
        let bytes = source.as_bytes();

        // Non-null assertion is ! not followed by =
        if pos + 1 < bytes.len() && bytes[pos + 1] == b'=' {
            return false;
        }

        // Must follow an expression
        if pos == 0 {
            return false;
        }

        let mut back = pos - 1;
        while back > 0 && (bytes[back] as char).is_whitespace() {
            back -= 1;
        }

        let prev_char = bytes[back] as char;

        prev_char.is_alphanumeric() ||
            prev_char == '_' ||
            prev_char == '$' ||
            prev_char == ')' ||
            prev_char == ']'
    }

    /// Skip satisfies clause
    fn skip_satisfies(&self, source: &str, mut pos: usize) -> Result<usize> {
        // Skip "satisfies"
        pos = self.skip_identifier(source, pos);
        pos = self.skip_whitespace(source, pos);

        // Skip the type
        pos = self.skip_type_expression(source, pos)?;

        Ok(pos)
    }

    /// Skip to the next statement (after ; or newline)
    fn skip_to_next_statement(&self, source: &str, mut pos: usize) -> Result<usize> {
        let bytes = source.as_bytes();
        let mut depth = 0;

        while pos < bytes.len() {
            let c = bytes[pos] as char;

            match c {
                '{' | '(' | '[' => depth += 1,
                '}' | ')' | ']' => {
                    if depth > 0 {
                        depth -= 1;
                    }
                }
                ';' => {
                    if depth == 0 {
                        pos += 1;
                        break;
                    }
                }
                '\n' => {
                    if depth == 0 {
                        pos += 1;
                        break;
                    }
                }
                _ => {}
            }

            pos += 1;
        }

        Ok(pos)
    }

    /// Skip balanced delimiters
    fn skip_balanced(&self, source: &str, mut pos: usize, open: u8, close: u8) -> Result<usize> {
        let bytes = source.as_bytes();

        if pos >= bytes.len() || bytes[pos] != open {
            return Ok(pos);
        }

        pos += 1;
        let mut depth = 1;

        while pos < bytes.len() && depth > 0 {
            let c = bytes[pos];
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
            } else if c == b'"' || c == b'\'' {
                // Skip string
                let quote = c;
                pos += 1;
                while pos < bytes.len() {
                    if bytes[pos] == b'\\' {
                        pos += 2;
                        continue;
                    }
                    if bytes[pos] == quote {
                        break;
                    }
                    pos += 1;
                }
            } else if c == b'`' {
                // Skip template literal
                pos += 1;
                while pos < bytes.len() {
                    if bytes[pos] == b'\\' {
                        pos += 2;
                        continue;
                    }
                    if bytes[pos] == b'`' {
                        break;
                    }
                    if bytes[pos] == b'$' && pos + 1 < bytes.len() && bytes[pos + 1] == b'{' {
                        pos += 2;
                        let mut template_depth = 1;
                        while pos < bytes.len() && template_depth > 0 {
                            if bytes[pos] == b'{' {
                                template_depth += 1;
                            } else if bytes[pos] == b'}' {
                                template_depth -= 1;
                            }
                            pos += 1;
                        }
                        continue;
                    }
                    pos += 1;
                }
            }
            pos += 1;
        }

        Ok(pos)
    }

    /// Clean up the output (remove extra whitespace, fix formatting)
    fn cleanup_output(&self, source: &str) -> String {
        let mut result = String::with_capacity(source.len());
        let mut prev_was_space = false;
        let mut prev_was_newline = false;

        for c in source.chars() {
            if c == '\n' {
                if !prev_was_newline {
                    result.push(c);
                    prev_was_newline = true;
                }
                prev_was_space = true;
            } else if c.is_whitespace() {
                if !prev_was_space {
                    result.push(' ');
                    prev_was_space = true;
                }
                prev_was_newline = false;
            } else {
                result.push(c);
                prev_was_space = false;
                prev_was_newline = false;
            }
        }

        result.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_type_stripping() {
        let transpiler = TypeScriptTranspiler::new();
        let ts = "let x: number = 42;";
        let js = transpiler.transpile(ts).unwrap();
        eprintln!("INPUT: {}", ts);
        eprintln!("OUTPUT: {}", js);
        assert!(js.contains("let x = 42;"), "Expected 'let x = 42;' in output: {}", js);
        assert!(!js.contains(": number"));
    }

    #[test]
    fn test_function_types() {
        let transpiler = TypeScriptTranspiler::new();
        let ts = "function add(a: number, b: number): number { return a + b; }";
        let js = transpiler.transpile(ts).unwrap();
        assert!(js.contains("function add(a, b)"));
        assert!(!js.contains(": number"));
    }

    #[test]
    fn test_interface_removal() {
        let transpiler = TypeScriptTranspiler::new();
        let ts = "interface Foo { bar: string; } let x = 5;";
        let js = transpiler.transpile(ts).unwrap();
        assert!(!js.contains("interface"));
        assert!(js.contains("let x = 5;"));
    }

    #[test]
    fn test_type_alias_removal() {
        let transpiler = TypeScriptTranspiler::new();
        let ts = "type MyType = string | number; let x = 5;";
        let js = transpiler.transpile(ts).unwrap();
        assert!(!js.contains("type MyType"));
        assert!(js.contains("let x = 5;"));
    }

    #[test]
    fn test_enum_transpilation() {
        let transpiler = TypeScriptTranspiler::new();
        let ts = "enum Color { Red, Green, Blue }";
        let js = transpiler.transpile(ts).unwrap();
        assert!(js.contains("var Color"));
        assert!(js.contains("Color[\"Red\"]"));
    }

    #[test]
    fn test_type_assertion() {
        let transpiler = TypeScriptTranspiler::new();
        let ts = "let x = foo as string;";
        let js = transpiler.transpile(ts).unwrap();
        eprintln!("INPUT: {}", ts);
        eprintln!("OUTPUT: {}", js);
        assert!(js.contains("let x = foo"), "Expected 'let x = foo' in output: {}", js);
        assert!(!js.contains("as string"));
    }

    #[test]
    fn test_generic_type_params() {
        let transpiler = TypeScriptTranspiler::new();
        let ts = "function identity<T>(x: T): T { return x; }";
        let js = transpiler.transpile(ts).unwrap();
        assert!(js.contains("function identity(x)"));
        assert!(!js.contains("<T>"));
    }

    #[test]
    fn test_class_modifiers() {
        let transpiler = TypeScriptTranspiler::new();
        let ts = "class Foo { private x: number; public y: string; }";
        let js = transpiler.transpile(ts).unwrap();
        assert!(!js.contains("private"));
        assert!(!js.contains("public"));
        assert!(js.contains("class Foo"));
    }

    #[test]
    fn test_preserves_strings() {
        let transpiler = TypeScriptTranspiler::new();
        let ts = r#"let s = "hello: world";"#;
        let js = transpiler.transpile(ts).unwrap();
        assert!(js.contains(r#""hello: world""#));
    }
}
