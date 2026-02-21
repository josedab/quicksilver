//! Declaration File Generation
//!
//! This module generates `.d.ts` declaration files from TypeScript source code.
//! It extracts public API surface (exported types, functions, classes) and
//! produces type declaration output.

/// Generates .d.ts declaration files from TypeScript source
pub struct DeclarationGenerator {
    declarations: Vec<Declaration>,
}

/// A single declaration entry in a .d.ts file
#[derive(Debug, Clone)]
pub enum Declaration {
    Variable {
        name: String,
        type_annotation: String,
        is_exported: bool,
    },
    Function {
        name: String,
        params: Vec<(String, String)>,
        return_type: String,
        is_exported: bool,
    },
    Class {
        name: String,
        members: Vec<ClassMember>,
        is_exported: bool,
    },
    Interface {
        name: String,
        members: Vec<(String, String)>,
        is_exported: bool,
    },
    TypeAlias {
        name: String,
        definition: String,
        is_exported: bool,
    },
    Enum {
        name: String,
        members: Vec<String>,
        is_exported: bool,
    },
}

/// A member of a class declaration
#[derive(Debug, Clone)]
pub struct ClassMember {
    pub name: String,
    pub type_annotation: String,
    pub is_method: bool,
    pub is_static: bool,
    pub visibility: Visibility,
}

/// Visibility of a class member
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Public,
    Protected,
    Private,
}

impl DeclarationGenerator {
    /// Create a new empty declaration generator
    pub fn new() -> Self {
        Self {
            declarations: Vec::new(),
        }
    }

    /// Add a declaration
    pub fn add_declaration(&mut self, decl: Declaration) {
        self.declarations.push(decl);
    }

    /// Generate .d.ts content from all added declarations
    pub fn generate(&self) -> String {
        let mut output = String::new();

        for decl in &self.declarations {
            match decl {
                Declaration::Variable {
                    name,
                    type_annotation,
                    is_exported,
                } => {
                    if *is_exported {
                        output.push_str("export ");
                    }
                    output.push_str("declare const ");
                    output.push_str(name);
                    output.push_str(": ");
                    output.push_str(type_annotation);
                    output.push_str(";\n");
                }
                Declaration::Function {
                    name,
                    params,
                    return_type,
                    is_exported,
                } => {
                    if *is_exported {
                        output.push_str("export ");
                    }
                    output.push_str("declare function ");
                    output.push_str(name);
                    output.push('(');
                    for (i, (pname, ptype)) in params.iter().enumerate() {
                        if i > 0 {
                            output.push_str(", ");
                        }
                        output.push_str(pname);
                        output.push_str(": ");
                        output.push_str(ptype);
                    }
                    output.push_str("): ");
                    output.push_str(return_type);
                    output.push_str(";\n");
                }
                Declaration::Class {
                    name,
                    members,
                    is_exported,
                } => {
                    if *is_exported {
                        output.push_str("export ");
                    }
                    output.push_str("declare class ");
                    output.push_str(name);
                    output.push_str(" {\n");
                    for member in members {
                        output.push_str("    ");
                        match member.visibility {
                            Visibility::Protected => output.push_str("protected "),
                            Visibility::Private => output.push_str("private "),
                            Visibility::Public => {}
                        }
                        if member.is_static {
                            output.push_str("static ");
                        }
                        output.push_str(&member.name);
                        if member.is_method {
                            // type_annotation holds the signature like "(x: number): void"
                            output.push_str(&member.type_annotation);
                        } else {
                            output.push_str(": ");
                            output.push_str(&member.type_annotation);
                        }
                        output.push_str(";\n");
                    }
                    output.push_str("}\n");
                }
                Declaration::Interface {
                    name,
                    members,
                    is_exported,
                } => {
                    if *is_exported {
                        output.push_str("export ");
                    }
                    output.push_str("interface ");
                    output.push_str(name);
                    output.push_str(" {\n");
                    for (mname, mtype) in members {
                        output.push_str("    ");
                        output.push_str(mname);
                        output.push_str(": ");
                        output.push_str(mtype);
                        output.push_str(";\n");
                    }
                    output.push_str("}\n");
                }
                Declaration::TypeAlias {
                    name,
                    definition,
                    is_exported,
                } => {
                    if *is_exported {
                        output.push_str("export ");
                    }
                    output.push_str("type ");
                    output.push_str(name);
                    output.push_str(" = ");
                    output.push_str(definition);
                    output.push_str(";\n");
                }
                Declaration::Enum {
                    name,
                    members,
                    is_exported,
                } => {
                    if *is_exported {
                        output.push_str("export ");
                    }
                    output.push_str("declare enum ");
                    output.push_str(name);
                    output.push_str(" {\n");
                    for member in members {
                        output.push_str("    ");
                        output.push_str(member);
                        output.push_str(",\n");
                    }
                    output.push_str("}\n");
                }
            }
        }

        output
    }

    /// Extract declarations from TypeScript source code.
    ///
    /// This performs a lightweight parse to identify exported declarations
    /// (variables, functions, classes, interfaces, type aliases, enums).
    pub fn from_source(source: &str) -> Self {
        let mut gen = Self::new();
        let mut pos = 0;
        let bytes = source.as_bytes();

        while pos < bytes.len() {
            // skip whitespace
            while pos < bytes.len() && (bytes[pos] as char).is_whitespace() {
                pos += 1;
            }
            if pos >= bytes.len() {
                break;
            }

            let is_exported = starts_with_keyword(source, pos, "export");
            let scan_pos = if is_exported {
                skip_ws(source, pos + 6)
            } else {
                pos
            };

            if starts_with_keyword(source, scan_pos, "interface") {
                let after_kw = skip_ws(source, scan_pos + 9);
                if let Some((name, end)) = read_ident(source, after_kw) {
                    let members = extract_brace_members(source, end);
                    gen.add_declaration(Declaration::Interface {
                        name,
                        members,
                        is_exported,
                    });
                    pos = skip_past_braces(source, end);
                    continue;
                }
            }

            if starts_with_keyword(source, scan_pos, "type") {
                let after_kw = skip_ws(source, scan_pos + 4);
                if let Some((name, end)) = read_ident(source, after_kw) {
                    let eq_pos = skip_ws(source, end);
                    if eq_pos < bytes.len() && bytes[eq_pos] == b'=' {
                        let def_start = skip_ws(source, eq_pos + 1);
                        let def_end = find_statement_end(source, def_start);
                        let definition = source[def_start..def_end].trim().to_string();
                        gen.add_declaration(Declaration::TypeAlias {
                            name,
                            definition,
                            is_exported,
                        });
                        pos = def_end;
                        if pos < bytes.len() && bytes[pos] == b';' {
                            pos += 1;
                        }
                        continue;
                    }
                }
            }

            if starts_with_keyword(source, scan_pos, "enum") {
                let after_kw = skip_ws(source, scan_pos + 4);
                if let Some((name, end)) = read_ident(source, after_kw) {
                    let members = extract_enum_members(source, end);
                    gen.add_declaration(Declaration::Enum {
                        name,
                        members,
                        is_exported,
                    });
                    pos = skip_past_braces(source, end);
                    continue;
                }
            }

            if starts_with_keyword(source, scan_pos, "function") {
                let after_kw = skip_ws(source, scan_pos + 8);
                if let Some((name, end)) = read_ident(source, after_kw) {
                    let (params, return_type, after_sig) = extract_function_sig(source, end);
                    gen.add_declaration(Declaration::Function {
                        name,
                        params,
                        return_type,
                        is_exported,
                    });
                    pos = skip_past_braces(source, after_sig);
                    continue;
                }
            }

            if starts_with_keyword(source, scan_pos, "class") {
                let after_kw = skip_ws(source, scan_pos + 5);
                if let Some((name, end)) = read_ident(source, after_kw) {
                    gen.add_declaration(Declaration::Class {
                        name,
                        members: Vec::new(),
                        is_exported,
                    });
                    pos = skip_past_braces(source, end);
                    continue;
                }
            }

            if starts_with_keyword(source, scan_pos, "const")
                || starts_with_keyword(source, scan_pos, "let")
                || starts_with_keyword(source, scan_pos, "var")
            {
                let kw_len = if starts_with_keyword(source, scan_pos, "const") {
                    5
                } else {
                    3 // "let" or "var"
                };
                let after_kw = skip_ws(source, scan_pos + kw_len);
                if let Some((name, end)) = read_ident(source, after_kw) {
                    let type_ann = extract_var_type(source, end);
                    gen.add_declaration(Declaration::Variable {
                        name,
                        type_annotation: type_ann,
                        is_exported,
                    });
                    pos = find_statement_end(source, end);
                    if pos < bytes.len() && bytes[pos] == b';' {
                        pos += 1;
                    }
                    continue;
                }
            }

            // Skip to next line
            while pos < bytes.len() && bytes[pos] != b'\n' {
                pos += 1;
            }
            if pos < bytes.len() {
                pos += 1;
            }
        }

        gen
    }
}

impl Default for DeclarationGenerator {
    fn default() -> Self {
        Self::new()
    }
}

// --- helper functions for from_source ---

fn starts_with_keyword(source: &str, pos: usize, keyword: &str) -> bool {
    if !source[pos..].starts_with(keyword) {
        return false;
    }
    let next_pos = pos + keyword.len();
    if next_pos >= source.len() {
        return true;
    }
    let next = source.as_bytes()[next_pos] as char;
    !next.is_alphanumeric() && next != '_' && next != '$'
}

fn skip_ws(source: &str, mut pos: usize) -> usize {
    let bytes = source.as_bytes();
    while pos < bytes.len() && (bytes[pos] as char).is_whitespace() {
        pos += 1;
    }
    pos
}

fn read_ident(source: &str, pos: usize) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    if pos >= bytes.len() {
        return None;
    }
    let c = bytes[pos] as char;
    if !c.is_alphabetic() && c != '_' && c != '$' {
        return None;
    }
    let start = pos;
    let mut end = pos;
    while end < bytes.len() {
        let ch = bytes[end] as char;
        if ch.is_alphanumeric() || ch == '_' || ch == '$' {
            end += 1;
        } else {
            break;
        }
    }
    Some((source[start..end].to_string(), end))
}

fn skip_past_braces(source: &str, mut pos: usize) -> usize {
    let bytes = source.as_bytes();
    // Find the opening brace
    while pos < bytes.len() && bytes[pos] != b'{' {
        pos += 1;
    }
    if pos >= bytes.len() {
        return pos;
    }
    pos += 1;
    let mut depth = 1;
    while pos < bytes.len() && depth > 0 {
        match bytes[pos] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'"' | b'\'' => {
                let q = bytes[pos];
                pos += 1;
                while pos < bytes.len() {
                    if bytes[pos] == b'\\' {
                        pos += 1;
                    } else if bytes[pos] == q {
                        break;
                    }
                    pos += 1;
                }
            }
            _ => {}
        }
        pos += 1;
    }
    pos
}

fn find_statement_end(source: &str, mut pos: usize) -> usize {
    let bytes = source.as_bytes();
    let mut depth = 0;
    while pos < bytes.len() {
        match bytes[pos] {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            b';' | b'\n' if depth == 0 => return pos,
            _ => {}
        }
        pos += 1;
    }
    pos
}

fn extract_brace_members(source: &str, start: usize) -> Vec<(String, String)> {
    let bytes = source.as_bytes();
    let mut pos = start;
    // Find opening brace
    while pos < bytes.len() && bytes[pos] != b'{' {
        pos += 1;
    }
    if pos >= bytes.len() {
        return Vec::new();
    }
    pos += 1; // skip {

    let mut members = Vec::new();
    loop {
        pos = skip_ws(source, pos);
        if pos >= bytes.len() || bytes[pos] == b'}' {
            break;
        }
        // skip readonly
        if starts_with_keyword(source, pos, "readonly") {
            pos = skip_ws(source, pos + 8);
        }
        if let Some((name, end)) = read_ident(source, pos) {
            pos = end;
            // skip optional ?
            if pos < bytes.len() && bytes[pos] == b'?' {
                pos += 1;
            }
            pos = skip_ws(source, pos);
            let type_str = if pos < bytes.len() && bytes[pos] == b':' {
                pos += 1;
                pos = skip_ws(source, pos);
                let tstart = pos;
                while pos < bytes.len()
                    && bytes[pos] != b';'
                    && bytes[pos] != b','
                    && bytes[pos] != b'}'
                    && bytes[pos] != b'\n'
                {
                    pos += 1;
                }
                source[tstart..pos].trim().to_string()
            } else {
                "any".to_string()
            };
            members.push((name, type_str));
            // skip separator
            if pos < bytes.len() && (bytes[pos] == b';' || bytes[pos] == b',') {
                pos += 1;
            }
        } else {
            // Skip unknown token
            while pos < bytes.len() && bytes[pos] != b';' && bytes[pos] != b'}' && bytes[pos] != b'\n' {
                pos += 1;
            }
            if pos < bytes.len() && bytes[pos] != b'}' {
                pos += 1;
            }
        }
    }
    members
}

fn extract_enum_members(source: &str, start: usize) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut pos = start;
    while pos < bytes.len() && bytes[pos] != b'{' {
        pos += 1;
    }
    if pos >= bytes.len() {
        return Vec::new();
    }
    pos += 1;

    let mut members = Vec::new();
    loop {
        pos = skip_ws(source, pos);
        if pos >= bytes.len() || bytes[pos] == b'}' {
            break;
        }
        if let Some((name, end)) = read_ident(source, pos) {
            members.push(name);
            pos = end;
            // skip optional = value
            pos = skip_ws(source, pos);
            if pos < bytes.len() && bytes[pos] == b'=' {
                pos += 1;
                while pos < bytes.len() && bytes[pos] != b',' && bytes[pos] != b'}' {
                    pos += 1;
                }
            }
            if pos < bytes.len() && bytes[pos] == b',' {
                pos += 1;
            }
        } else {
            pos += 1;
        }
    }
    members
}

fn extract_function_sig(source: &str, start: usize) -> (Vec<(String, String)>, String, usize) {
    let bytes = source.as_bytes();
    let mut pos = start;

    // skip type params <T>
    pos = skip_ws(source, pos);
    if pos < bytes.len() && bytes[pos] == b'<' {
        let mut depth = 1;
        pos += 1;
        while pos < bytes.len() && depth > 0 {
            if bytes[pos] == b'<' {
                depth += 1;
            } else if bytes[pos] == b'>' {
                depth -= 1;
            }
            pos += 1;
        }
    }

    // find (
    while pos < bytes.len() && bytes[pos] != b'(' {
        pos += 1;
    }
    if pos >= bytes.len() {
        return (Vec::new(), "void".to_string(), pos);
    }
    pos += 1; // skip (

    let mut params = Vec::new();
    loop {
        pos = skip_ws(source, pos);
        if pos >= bytes.len() || bytes[pos] == b')' {
            break;
        }
        if let Some((name, end)) = read_ident(source, pos) {
            pos = end;
            // skip ?
            if pos < bytes.len() && bytes[pos] == b'?' {
                pos += 1;
            }
            pos = skip_ws(source, pos);
            let ptype = if pos < bytes.len() && bytes[pos] == b':' {
                pos += 1;
                pos = skip_ws(source, pos);
                let ts = pos;
                let mut depth = 0;
                while pos < bytes.len() {
                    match bytes[pos] {
                        b'(' | b'[' | b'<' => depth += 1,
                        b')' | b']' | b'>' => {
                            if depth == 0 {
                                break;
                            }
                            depth -= 1;
                        }
                        b',' if depth == 0 => break,
                        _ => {}
                    }
                    pos += 1;
                }
                source[ts..pos].trim().to_string()
            } else {
                "any".to_string()
            };
            params.push((name, ptype));
            pos = skip_ws(source, pos);
            if pos < bytes.len() && bytes[pos] == b',' {
                pos += 1;
            }
        } else {
            pos += 1;
        }
    }
    if pos < bytes.len() && bytes[pos] == b')' {
        pos += 1;
    }

    // return type
    pos = skip_ws(source, pos);
    let return_type = if pos < bytes.len() && bytes[pos] == b':' {
        pos += 1;
        pos = skip_ws(source, pos);
        let ts = pos;
        while pos < bytes.len() && bytes[pos] != b'{' && bytes[pos] != b'\n' && bytes[pos] != b';' {
            pos += 1;
        }
        source[ts..pos].trim().to_string()
    } else {
        "void".to_string()
    };

    (params, return_type, pos)
}

fn extract_var_type(source: &str, start: usize) -> String {
    let bytes = source.as_bytes();
    let mut pos = skip_ws(source, start);
    if pos < bytes.len() && bytes[pos] == b':' {
        pos += 1;
        pos = skip_ws(source, pos);
        let ts = pos;
        while pos < bytes.len() && bytes[pos] != b'=' && bytes[pos] != b';' && bytes[pos] != b'\n' {
            pos += 1;
        }
        let t = source[ts..pos].trim();
        if t.is_empty() {
            "any".to_string()
        } else {
            t.to_string()
        }
    } else {
        "any".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_variable() {
        let mut gen = DeclarationGenerator::new();
        gen.add_declaration(Declaration::Variable {
            name: "count".to_string(),
            type_annotation: "number".to_string(),
            is_exported: true,
        });
        let output = gen.generate();
        assert!(output.contains("export declare const count: number;"));
    }

    #[test]
    fn test_generate_function() {
        let mut gen = DeclarationGenerator::new();
        gen.add_declaration(Declaration::Function {
            name: "add".to_string(),
            params: vec![
                ("a".to_string(), "number".to_string()),
                ("b".to_string(), "number".to_string()),
            ],
            return_type: "number".to_string(),
            is_exported: true,
        });
        let output = gen.generate();
        assert!(output.contains("export declare function add(a: number, b: number): number;"));
    }

    #[test]
    fn test_generate_class() {
        let mut gen = DeclarationGenerator::new();
        gen.add_declaration(Declaration::Class {
            name: "MyClass".to_string(),
            members: vec![
                ClassMember {
                    name: "x".to_string(),
                    type_annotation: "number".to_string(),
                    is_method: false,
                    is_static: false,
                    visibility: Visibility::Public,
                },
                ClassMember {
                    name: "secret".to_string(),
                    type_annotation: "string".to_string(),
                    is_method: false,
                    is_static: false,
                    visibility: Visibility::Private,
                },
                ClassMember {
                    name: "doStuff".to_string(),
                    type_annotation: "(): void".to_string(),
                    is_method: true,
                    is_static: false,
                    visibility: Visibility::Public,
                },
                ClassMember {
                    name: "create".to_string(),
                    type_annotation: "(): MyClass".to_string(),
                    is_method: true,
                    is_static: true,
                    visibility: Visibility::Public,
                },
            ],
            is_exported: true,
        });
        let output = gen.generate();
        assert!(output.contains("export declare class MyClass {"));
        assert!(output.contains("    x: number;"));
        assert!(output.contains("    private secret: string;"));
        assert!(output.contains("    doStuff(): void;"));
        assert!(output.contains("    static create(): MyClass;"));
        assert!(output.contains("}"));
    }

    #[test]
    fn test_generate_interface() {
        let mut gen = DeclarationGenerator::new();
        gen.add_declaration(Declaration::Interface {
            name: "User".to_string(),
            members: vec![
                ("name".to_string(), "string".to_string()),
                ("age".to_string(), "number".to_string()),
            ],
            is_exported: true,
        });
        let output = gen.generate();
        assert!(output.contains("export interface User {"));
        assert!(output.contains("    name: string;"));
        assert!(output.contains("    age: number;"));
    }

    #[test]
    fn test_generate_type_alias() {
        let mut gen = DeclarationGenerator::new();
        gen.add_declaration(Declaration::TypeAlias {
            name: "ID".to_string(),
            definition: "string | number".to_string(),
            is_exported: true,
        });
        let output = gen.generate();
        assert!(output.contains("export type ID = string | number;"));
    }

    #[test]
    fn test_generate_enum() {
        let mut gen = DeclarationGenerator::new();
        gen.add_declaration(Declaration::Enum {
            name: "Color".to_string(),
            members: vec![
                "Red".to_string(),
                "Green".to_string(),
                "Blue".to_string(),
            ],
            is_exported: false,
        });
        let output = gen.generate();
        assert!(output.contains("declare enum Color {"));
        assert!(output.contains("    Red,"));
        assert!(output.contains("    Green,"));
        assert!(output.contains("    Blue,"));
    }

    #[test]
    fn test_generate_not_exported() {
        let mut gen = DeclarationGenerator::new();
        gen.add_declaration(Declaration::Variable {
            name: "internal".to_string(),
            type_annotation: "string".to_string(),
            is_exported: false,
        });
        let output = gen.generate();
        assert!(!output.contains("export"));
        assert!(output.contains("declare const internal: string;"));
    }

    #[test]
    fn test_from_source_interface() {
        let source = r#"
            export interface User {
                name: string;
                age: number;
            }
        "#;
        let gen = DeclarationGenerator::from_source(source);
        let output = gen.generate();
        assert!(output.contains("interface User {"), "Got: {}", output);
        assert!(output.contains("name: string"), "Got: {}", output);
        assert!(output.contains("age: number"), "Got: {}", output);
    }

    #[test]
    fn test_from_source_function() {
        let source = "export function greet(name: string): string { return name; }";
        let gen = DeclarationGenerator::from_source(source);
        let output = gen.generate();
        assert!(output.contains("export declare function greet(name: string): string;"), "Got: {}", output);
    }

    #[test]
    fn test_from_source_type_alias() {
        let source = "export type ID = string | number;";
        let gen = DeclarationGenerator::from_source(source);
        let output = gen.generate();
        assert!(output.contains("export type ID = string | number;"), "Got: {}", output);
    }

    #[test]
    fn test_from_source_enum() {
        let source = "export enum Direction { Up, Down, Left, Right }";
        let gen = DeclarationGenerator::from_source(source);
        let output = gen.generate();
        assert!(output.contains("export declare enum Direction {"), "Got: {}", output);
        assert!(output.contains("Up"), "Got: {}", output);
    }

    #[test]
    fn test_from_source_variable() {
        let source = "export const MAX: number = 100;";
        let gen = DeclarationGenerator::from_source(source);
        let output = gen.generate();
        assert!(output.contains("export declare const MAX: number;"), "Got: {}", output);
    }
}
