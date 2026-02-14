//! Bytecode compiler
//!
//! This module compiles the AST into bytecode that can be executed
//! by the interpreter.

use super::{Chunk, CompiledFunction, Opcode};
use crate::ast::*;
use crate::error::{Error, Result};
use crate::runtime::{JsFunction, Value};
use crate::ast::{ClassElement, MethodKind};
use rustc_hash::FxHashMap as HashMap;

/// Local variable information
#[derive(Debug, Clone)]
struct Local {
    name: String,
    depth: u32,
    is_captured: bool,
}

/// Upvalue information during compilation (reserved for closures)
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct CompilerUpvalue {
    index: u8,
    is_local: bool,
}

/// Loop information for break/continue
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct LoopInfo {
    start: usize,
    break_jumps: Vec<usize>,
    continue_jumps: Vec<usize>,
    depth: u32,
}

/// Compiler state
pub struct Compiler {
    /// Current chunk being compiled
    chunk: Chunk,
    /// Compiled functions (for nested functions)
    functions: Vec<CompiledFunction>,
    /// Local variables
    locals: Vec<Local>,
    /// Upvalues (reserved for closures)
    #[allow(dead_code)]
    upvalues: Vec<CompilerUpvalue>,
    /// Current scope depth
    scope_depth: u32,
    /// Loop stack
    loop_stack: Vec<LoopInfo>,
    /// Current line number
    current_line: u32,
    /// Current column number (for source maps)
    current_column: u32,
    /// Is this a function body?
    in_function: bool,
    /// Source file name
    source_file: Option<String>,
}

impl Compiler {
    /// Create a new compiler
    pub fn new() -> Self {
        Self {
            chunk: Chunk::new(),
            functions: Vec::new(),
            locals: Vec::new(),
            upvalues: Vec::new(),
            scope_depth: 0,
            loop_stack: Vec::new(),
            current_line: 1,
            current_column: 1,
            in_function: false,
            source_file: None,
        }
    }

    /// Create a new compiler with a source file name
    pub fn with_source_file(source_file: &str) -> Self {
        let mut compiler = Self::new();
        compiler.source_file = Some(source_file.to_string());
        compiler.chunk.source_file = Some(source_file.to_string());
        compiler
    }

    /// Compile a program to bytecode
    pub fn compile_program(&mut self, program: &Program) -> Result<Chunk> {
        // Set strict mode on the chunk
        self.chunk.is_strict = program.strict;

        // Phase 1: Hoist function declarations (they take precedence)
        let hoisted_funcs = self.collect_function_declarations(&program.body);
        self.hoist_function_declarations(&hoisted_funcs)?;

        // Phase 2: Hoist var declarations (initialize with undefined)
        let hoisted_vars = self.collect_var_declarations(&program.body);
        self.hoist_var_declarations(&hoisted_vars)?;

        // Phase 3: Compile statements (skip already-hoisted function declarations)
        let len = program.body.len();
        for (i, stmt) in program.body.iter().enumerate() {
            // Skip function declarations - they were already hoisted
            if matches!(stmt, Statement::FunctionDeclaration(_)) {
                continue;
            }

            let is_last = i == len - 1;

            // For the last statement, if it's an expression statement, don't pop
            if is_last {
                if let Statement::Expression(expr_stmt) = stmt {
                    self.compile_expr(&expr_stmt.expression)?;
                    // Don't pop - leave value on stack for eval result
                } else {
                    self.compile_statement(stmt)?;
                    // Other statements don't leave a value, push undefined
                    self.emit(Opcode::Undefined);
                }
            } else {
                self.compile_statement(stmt)?;
            }
        }

        // If program is empty, push undefined
        if program.body.is_empty() {
            self.emit(Opcode::Undefined);
        }

        Ok(std::mem::take(&mut self.chunk))
    }

    /// Compile an expression and return a chunk
    pub fn compile_expression(&mut self, expr: &Expression) -> Result<Chunk> {
        self.compile_expr(expr)?;
        self.emit(Opcode::Return);
        Ok(std::mem::take(&mut self.chunk))
    }

    // ========== Helpers ==========

    fn emit(&mut self, opcode: Opcode) {
        self.chunk.write_opcode_with_location(opcode, self.current_line, self.current_column);
    }

    fn emit_byte(&mut self, byte: u8) {
        self.chunk.write_with_location(byte, self.current_line, self.current_column);
    }

    /// Update current source location from a span
    fn set_location(&mut self, span: &crate::ast::Span) {
        self.current_line = span.start.line;
        self.current_column = span.start.column;
    }

    fn emit_u16(&mut self, value: u16) {
        let bytes = value.to_le_bytes();
        self.emit_byte(bytes[0]);
        self.emit_byte(bytes[1]);
    }

    /// Emit the opcode for a compound assignment operator (+=, -=, etc.)
    /// Does nothing for simple assignment (=)
    fn emit_compound_operator(&mut self, op: &AssignmentOperator) {
        match op {
            AssignmentOperator::Assign => {}
            AssignmentOperator::AddAssign => self.emit(Opcode::Add),
            AssignmentOperator::SubAssign => self.emit(Opcode::Sub),
            AssignmentOperator::MulAssign => self.emit(Opcode::Mul),
            AssignmentOperator::DivAssign => self.emit(Opcode::Div),
            AssignmentOperator::ModAssign => self.emit(Opcode::Mod),
            AssignmentOperator::PowAssign => self.emit(Opcode::Pow),
            AssignmentOperator::ShlAssign => self.emit(Opcode::Shl),
            AssignmentOperator::ShrAssign => self.emit(Opcode::Shr),
            AssignmentOperator::UShrAssign => self.emit(Opcode::UShr),
            AssignmentOperator::BitwiseAndAssign => self.emit(Opcode::BitwiseAnd),
            AssignmentOperator::BitwiseOrAssign => self.emit(Opcode::BitwiseOr),
            AssignmentOperator::BitwiseXorAssign => self.emit(Opcode::BitwiseXor),
            _ => {}
        }
    }

    fn emit_constant(&mut self, value: Value) {
        let index = self.chunk.add_constant(value);
        self.emit(Opcode::Constant);
        self.emit_u16(index);
    }

    fn emit_jump(&mut self, opcode: Opcode) -> usize {
        self.emit(opcode);
        let jump_addr = self.chunk.code.len();
        self.emit_u16(0xFFFF); // Placeholder
        jump_addr
    }

    fn patch_jump(&mut self, addr: usize) {
        let offset = (self.chunk.code.len() as isize - addr as isize - 2) as i16;
        let bytes = offset.to_le_bytes();
        self.chunk.code[addr] = bytes[0];
        self.chunk.code[addr + 1] = bytes[1];
    }

    fn emit_loop(&mut self, loop_start: usize) {
        self.emit(Opcode::Jump);
        let offset = (loop_start as isize - self.chunk.code.len() as isize - 2) as i16;
        self.emit_u16(offset as u16);
    }

    /// Collect hoisted var declarations from statements (for function-scoped hoisting)
    fn collect_var_declarations(&self, stmts: &[Statement]) -> Vec<String> {
        let mut var_names = Vec::new();
        for stmt in stmts {
            self.collect_vars_from_statement(stmt, &mut var_names);
        }
        var_names
    }

    /// Recursively collect var declarations from a statement
    fn collect_vars_from_statement(&self, stmt: &Statement, var_names: &mut Vec<String>) {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                if decl.kind == VariableKind::Var {
                    for declarator in &decl.declarations {
                        self.collect_vars_from_pattern(&declarator.id, var_names);
                    }
                }
            }
            Statement::Block(block) => {
                for s in &block.body {
                    self.collect_vars_from_statement(s, var_names);
                }
            }
            Statement::If(if_stmt) => {
                self.collect_vars_from_statement(&if_stmt.consequent, var_names);
                if let Some(alt) = &if_stmt.alternate {
                    self.collect_vars_from_statement(alt, var_names);
                }
            }
            Statement::While(while_stmt) => {
                self.collect_vars_from_statement(&while_stmt.body, var_names);
            }
            Statement::DoWhile(do_while) => {
                self.collect_vars_from_statement(&do_while.body, var_names);
            }
            Statement::For(for_stmt) => {
                if let Some(ForInit::Declaration(decl)) = &for_stmt.init {
                    if decl.kind == VariableKind::Var {
                        for declarator in &decl.declarations {
                            self.collect_vars_from_pattern(&declarator.id, var_names);
                        }
                    }
                }
                self.collect_vars_from_statement(&for_stmt.body, var_names);
            }
            Statement::ForIn(for_in) => {
                if let ForInLeft::Declaration(decl) = &for_in.left {
                    if decl.kind == VariableKind::Var {
                        for declarator in &decl.declarations {
                            self.collect_vars_from_pattern(&declarator.id, var_names);
                        }
                    }
                }
                self.collect_vars_from_statement(&for_in.body, var_names);
            }
            Statement::ForOf(for_of) => {
                if let ForInLeft::Declaration(decl) = &for_of.left {
                    if decl.kind == VariableKind::Var {
                        for declarator in &decl.declarations {
                            self.collect_vars_from_pattern(&declarator.id, var_names);
                        }
                    }
                }
                self.collect_vars_from_statement(&for_of.body, var_names);
            }
            Statement::Try(try_stmt) => {
                for s in &try_stmt.block.body {
                    self.collect_vars_from_statement(s, var_names);
                }
                if let Some(catch) = &try_stmt.handler {
                    for s in &catch.body.body {
                        self.collect_vars_from_statement(s, var_names);
                    }
                }
                if let Some(finally) = &try_stmt.finalizer {
                    for s in &finally.body {
                        self.collect_vars_from_statement(s, var_names);
                    }
                }
            }
            Statement::Switch(switch) => {
                for case in &switch.cases {
                    for s in &case.consequent {
                        self.collect_vars_from_statement(s, var_names);
                    }
                }
            }
            Statement::Labeled(labeled) => {
                self.collect_vars_from_statement(&labeled.body, var_names);
            }
            Statement::With(with) => {
                self.collect_vars_from_statement(&with.body, var_names);
            }
            _ => {}
        }
    }

    /// Extract variable names from a pattern
    fn collect_vars_from_pattern(&self, pattern: &Pattern, var_names: &mut Vec<String>) {
        match pattern {
            Pattern::Identifier(id) => {
                if !var_names.contains(&id.name) {
                    var_names.push(id.name.clone());
                }
            }
            Pattern::Array(arr) => {
                for p in arr.elements.iter().flatten() {
                    self.collect_vars_from_pattern(p, var_names);
                }
            }
            Pattern::Object(obj) => {
                for prop in &obj.properties {
                    if let ObjectPatternProperty::Property { value, .. } = prop {
                        self.collect_vars_from_pattern(value, var_names);
                    }
                }
            }
            Pattern::Assignment(assign) => {
                self.collect_vars_from_pattern(&assign.left, var_names);
            }
            Pattern::Rest(rest) => {
                self.collect_vars_from_pattern(&rest.argument, var_names);
            }
            Pattern::Member(_) => {}
        }
    }

    /// Collect function declarations for hoisting
    fn collect_function_declarations<'a>(&self, stmts: &'a [Statement]) -> Vec<&'a Function> {
        stmts.iter()
            .filter_map(|stmt| {
                if let Statement::FunctionDeclaration(func) = stmt {
                    Some(func.as_ref())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Hoist var declarations by declaring them with undefined
    fn hoist_var_declarations(&mut self, var_names: &[String]) -> Result<()> {
        for name in var_names {
            if self.scope_depth > 0 {
                // Local variable - check if already declared
                let already_declared = self.locals.iter().any(|l| l.name == *name);
                if !already_declared {
                    self.add_local(name)?;
                    // Initialize with undefined (the slot is implicitly undefined)
                }
            } else {
                // Global variable - define with undefined
                self.emit(Opcode::Undefined);
                let name_idx = self.chunk.add_constant(Value::String(name.clone()));
                self.emit(Opcode::DefineGlobal);
                self.emit_u16(name_idx);
            }
        }
        Ok(())
    }

    /// Hoist function declarations by compiling and defining them
    fn hoist_function_declarations(&mut self, funcs: &[&Function]) -> Result<()> {
        for func in funcs {
            self.compile_function_decl(func)?;
        }
        Ok(())
    }

    fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    fn end_scope(&mut self) {
        self.scope_depth -= 1;

        // Pop locals going out of scope
        while !self.locals.is_empty() && self.locals.last().unwrap().depth > self.scope_depth {
            let local = self.locals.pop().unwrap();
            if local.is_captured {
                // Close upvalue
                self.emit(Opcode::CloseUpvalue);
                self.emit_u16((self.locals.len()) as u16);
            } else {
                self.emit(Opcode::Pop);
            }
        }
    }

    fn add_local(&mut self, name: &str) -> Result<u8> {
        if self.locals.len() >= 256 {
            return Err(Error::InternalError("Too many local variables".to_string()));
        }

        let local = Local {
            name: name.to_string(),
            depth: self.scope_depth,
            is_captured: false,
        };

        self.locals.push(local);
        self.chunk.locals.push(name.to_string());
        Ok((self.locals.len() - 1) as u8)
    }

    fn resolve_local(&self, name: &str) -> Option<u8> {
        for (i, local) in self.locals.iter().enumerate().rev() {
            if local.name == name {
                return Some(i as u8);
            }
        }
        None
    }

    fn resolve_upvalue(&mut self, _name: &str) -> Option<u8> {
        // For simplicity, we don't implement full upvalue resolution here
        // This would require tracking enclosing compilers
        None
    }

    // ========== Statements ==========

    fn compile_statement(&mut self, stmt: &Statement) -> Result<()> {
        self.set_location(&stmt.span());

        match stmt {
            Statement::Block(block) => self.compile_block(block),
            Statement::Empty(_) => Ok(()),
            Statement::Expression(expr_stmt) => {
                self.compile_expr(&expr_stmt.expression)?;
                self.emit(Opcode::Pop);
                Ok(())
            }
            Statement::VariableDeclaration(decl) => self.compile_var_decl(decl),
            Statement::FunctionDeclaration(func) => self.compile_function_decl(func),
            Statement::If(if_stmt) => self.compile_if(if_stmt),
            Statement::While(while_stmt) => self.compile_while(while_stmt),
            Statement::DoWhile(do_while) => self.compile_do_while(do_while),
            Statement::For(for_stmt) => self.compile_for(for_stmt),
            Statement::ForIn(for_in) => self.compile_for_in(for_in),
            Statement::ForOf(for_of) => self.compile_for_of(for_of),
            Statement::Break(break_stmt) => self.compile_break(break_stmt),
            Statement::Continue(continue_stmt) => self.compile_continue(continue_stmt),
            Statement::Return(return_stmt) => self.compile_return(return_stmt),
            Statement::Throw(throw_stmt) => self.compile_throw(throw_stmt),
            Statement::Try(try_stmt) => self.compile_try(try_stmt),
            Statement::Switch(switch_stmt) => self.compile_switch(switch_stmt),
            Statement::Debugger(_) => Ok(()), // No-op for now
            Statement::ClassDeclaration(class) => self.compile_class_decl(class),
            Statement::Import(import) => self.compile_import(import),
            Statement::Export(export) => self.compile_export(export),
            _ => Ok(()), // Labeled, With statements not yet implemented
        }
    }

    /// Compile import declaration
    /// Imports are resolved at runtime by the module loader
    fn compile_import(&mut self, import: &crate::ast::ImportDeclaration) -> Result<()> {
        // Emit module load instruction
        let source_idx = self.chunk.add_constant(Value::String(import.source.clone()));
        self.emit(Opcode::LoadModule);
        self.emit_u16(source_idx);

        // For each specifier, emit binding instructions
        for spec in &import.specifiers {
            match spec {
                crate::ast::ImportSpecifier::Default { local, .. } => {
                    // Get default export from module on stack
                    self.emit(Opcode::Dup); // Keep module on stack
                    let default_idx = self.chunk.add_constant(Value::String("default".to_string()));
                    self.emit(Opcode::GetProperty);
                    self.emit_u16(default_idx);

                    // Bind to local name
                    if self.scope_depth > 0 {
                        self.add_local(&local.name)?;
                    } else {
                        let name_idx = self.chunk.add_constant(Value::String(local.name.clone()));
                        self.emit(Opcode::DefineGlobal);
                        self.emit_u16(name_idx);
                    }
                }
                crate::ast::ImportSpecifier::Named { local, imported, .. } => {
                    // Get named export from module on stack
                    self.emit(Opcode::Dup); // Keep module on stack
                    let export_idx = self.chunk.add_constant(Value::String(imported.name.clone()));
                    self.emit(Opcode::GetProperty);
                    self.emit_u16(export_idx);

                    // Bind to local name
                    if self.scope_depth > 0 {
                        self.add_local(&local.name)?;
                    } else {
                        let name_idx = self.chunk.add_constant(Value::String(local.name.clone()));
                        self.emit(Opcode::DefineGlobal);
                        self.emit_u16(name_idx);
                    }
                }
                crate::ast::ImportSpecifier::Namespace { local, .. } => {
                    // Namespace import - keep entire module object
                    self.emit(Opcode::Dup);

                    // Bind to local name
                    if self.scope_depth > 0 {
                        self.add_local(&local.name)?;
                    } else {
                        let name_idx = self.chunk.add_constant(Value::String(local.name.clone()));
                        self.emit(Opcode::DefineGlobal);
                        self.emit_u16(name_idx);
                    }
                }
            }
        }

        // Pop the module object
        self.emit(Opcode::Pop);
        Ok(())
    }

    /// Compile export declaration
    fn compile_export(&mut self, export: &crate::ast::ExportDeclaration) -> Result<()> {
        match &export.kind {
            crate::ast::ExportKind::Declaration(decl) => {
                // Compile the declaration normally
                self.compile_statement(decl)?;

                // Extract exported names from the declaration and emit export instructions
                let names = self.extract_declaration_names(decl);
                for name in names {
                    // Get the value from the global scope
                    let name_idx = self.chunk.add_constant(Value::String(name.clone()));
                    self.emit(Opcode::GetGlobal);
                    self.emit_u16(name_idx);

                    // Export it
                    self.emit(Opcode::ExportValue);
                    self.emit_u16(name_idx);
                }
            }
            crate::ast::ExportKind::Default(expr) => {
                // Compile the expression
                self.compile_expr(expr)?;

                // Store as default export
                let default_idx = self.chunk.add_constant(Value::String("default".to_string()));
                self.emit(Opcode::ExportValue);
                self.emit_u16(default_idx);
            }
            crate::ast::ExportKind::DefaultDeclaration(decl) => {
                // Compile the declaration
                self.compile_statement(decl)?;

                // Get the name of the declaration and push its value
                let names = self.extract_declaration_names(decl);
                if let Some(name) = names.first() {
                    let name_idx = self.chunk.add_constant(Value::String(name.clone()));
                    self.emit(Opcode::GetGlobal);
                    self.emit_u16(name_idx);
                } else {
                    // Anonymous declaration - push undefined
                    self.emit(Opcode::Undefined);
                }

                // Export it as default
                let default_idx = self.chunk.add_constant(Value::String("default".to_string()));
                self.emit(Opcode::ExportValue);
                self.emit_u16(default_idx);
            }
            crate::ast::ExportKind::Named { specifiers, source } => {
                if source.is_some() {
                    // Re-export from another module
                    let source_idx = self.chunk.add_constant(Value::String(source.clone().unwrap()));
                    self.emit(Opcode::LoadModule);
                    self.emit_u16(source_idx);

                    for spec in specifiers {
                        if let crate::ast::ExportSpecifier::Named { local, exported, .. } = spec {
                            self.emit(Opcode::Dup);
                            let local_idx = self.chunk.add_constant(Value::String(local.name.clone()));
                            self.emit(Opcode::GetProperty);
                            self.emit_u16(local_idx);

                            let exported_idx = self.chunk.add_constant(Value::String(exported.name.clone()));
                            self.emit(Opcode::ExportValue);
                            self.emit_u16(exported_idx);
                        }
                    }
                    self.emit(Opcode::Pop);
                } else {
                    // Export local bindings
                    for spec in specifiers {
                        if let crate::ast::ExportSpecifier::Named { local, exported, .. } = spec {
                            // Get the local value
                            let local_idx = self.chunk.add_constant(Value::String(local.name.clone()));
                            self.emit(Opcode::GetGlobal);
                            self.emit_u16(local_idx);

                            // Export it
                            let exported_idx = self.chunk.add_constant(Value::String(exported.name.clone()));
                            self.emit(Opcode::ExportValue);
                            self.emit_u16(exported_idx);
                        }
                    }
                }
            }
            crate::ast::ExportKind::All { source } => {
                // export * from 'module'
                let source_idx = self.chunk.add_constant(Value::String(source.clone()));
                self.emit(Opcode::LoadModule);
                self.emit_u16(source_idx);
                self.emit(Opcode::ExportAll);
            }
            crate::ast::ExportKind::AllAs { exported, source } => {
                // export * as name from 'module'
                let source_idx = self.chunk.add_constant(Value::String(source.clone()));
                self.emit(Opcode::LoadModule);
                self.emit_u16(source_idx);

                let exported_idx = self.chunk.add_constant(Value::String(exported.name.clone()));
                self.emit(Opcode::ExportValue);
                self.emit_u16(exported_idx);
            }
        }
        Ok(())
    }

    fn compile_block(&mut self, block: &BlockStatement) -> Result<()> {
        self.begin_scope();
        for stmt in &block.body {
            self.compile_statement(stmt)?;
        }
        self.end_scope();
        Ok(())
    }

    fn compile_var_decl(&mut self, decl: &VariableDeclaration) -> Result<()> {
        let is_var = decl.kind == VariableKind::Var;

        for declarator in &decl.declarations {
            match &declarator.id {
                Pattern::Identifier(id) => {
                    // For var declarations that were hoisted, only compile if there's an initializer
                    if is_var {
                        if let Some(init) = &declarator.init {
                            self.compile_expr(init)?;

                            // Find the already-declared local or set global
                            if self.scope_depth > 0 {
                                // Look for existing local
                                if let Some(idx) = self.resolve_local(&id.name) {
                                    self.emit(Opcode::SetLocal);
                                    self.emit_u16(idx as u16);
                                    self.emit(Opcode::Pop);
                                } else {
                                    // Shouldn't happen if hoisting worked, but handle it
                                    self.add_local(&id.name)?;
                                }
                            } else {
                                // Global variable - set the already-defined global
                                let name_idx = self.chunk.add_constant(Value::String(id.name.clone()));
                                self.emit(Opcode::SetGlobal);
                                self.emit_u16(name_idx);
                                self.emit(Opcode::Pop);
                            }
                        }
                        // If no initializer for var, skip - it was hoisted with undefined
                    } else {
                        // let/const - original behavior
                        if let Some(init) = &declarator.init {
                            self.compile_expr(init)?;
                        } else {
                            self.emit(Opcode::Undefined);
                        }

                        if self.scope_depth > 0 {
                            // Local variable
                            self.add_local(&id.name)?;
                        } else {
                            // Global variable
                            let name_idx = self.chunk.add_constant(Value::String(id.name.clone()));
                            self.emit(Opcode::DefineGlobal);
                            self.emit_u16(name_idx);
                        }
                    }
                }
                Pattern::Array(arr_pat) => {
                    // Compile initializer - pushes array on stack
                    if let Some(init) = &declarator.init {
                        self.compile_expr(init)?;
                    } else {
                        self.emit(Opcode::Undefined);
                    }

                    // Destructure each element
                    for (idx, elem) in arr_pat.elements.iter().enumerate() {
                        if let Some(pattern) = elem {
                            // Dup the array for the element access
                            self.emit(Opcode::Dup);
                            // Push the index
                            let idx_const = self.chunk.add_constant(Value::Number(idx as f64));
                            self.emit(Opcode::Constant);
                            self.emit_u16(idx_const);
                            // Get element at index
                            self.emit(Opcode::GetElement);

                            // Bind the value to the pattern
                            self.compile_pattern_binding(pattern)?;
                        }
                    }

                    // Pop the original array
                    self.emit(Opcode::Pop);
                }
                Pattern::Object(obj_pat) => {
                    // Compile initializer - pushes object on stack
                    if let Some(init) = &declarator.init {
                        self.compile_expr(init)?;
                    } else {
                        self.emit(Opcode::Undefined);
                    }

                    // Destructure each property
                    for prop in &obj_pat.properties {
                        if let ObjectPatternProperty::Property { key, value, .. } = prop {
                            // Dup the object for the property access
                            self.emit(Opcode::Dup);
                            // Get property name
                            let prop_name = match key {
                                PropertyKey::Identifier(id) => id.name.clone(),
                                PropertyKey::String(s) => s.clone(),
                                PropertyKey::Number(n) => n.to_string(),
                                PropertyKey::Computed(_) => continue, // Skip computed keys for now
                                PropertyKey::PrivateName(name) => name.clone(),
                            };
                            let name_idx = self.chunk.add_constant(Value::String(prop_name));
                            self.emit(Opcode::GetProperty);
                            self.emit_u16(name_idx);

                            // Bind the value to the pattern
                            self.compile_pattern_binding(value)?;
                        }
                    }

                    // Pop the original object
                    self.emit(Opcode::Pop);
                }
                _ => {
                    // Other patterns (rest, assignment) - fallback
                    if let Some(init) = &declarator.init {
                        self.compile_expr(init)?;
                    } else {
                        self.emit(Opcode::Undefined);
                    }
                    self.emit(Opcode::Pop);
                }
            }
        }
        Ok(())
    }

    fn compile_pattern_binding(&mut self, pattern: &Pattern) -> Result<()> {
        match pattern {
            Pattern::Identifier(id) => {
                if self.scope_depth > 0 {
                    // Local variable
                    self.add_local(&id.name)?;
                } else {
                    // Global variable
                    let name_idx = self.chunk.add_constant(Value::String(id.name.clone()));
                    self.emit(Opcode::DefineGlobal);
                    self.emit_u16(name_idx);
                }
            }
            Pattern::Array(arr_pat) => {
                // Nested array destructuring
                for (idx, elem) in arr_pat.elements.iter().enumerate() {
                    if let Some(inner_pattern) = elem {
                        self.emit(Opcode::Dup);
                        let idx_const = self.chunk.add_constant(Value::Number(idx as f64));
                        self.emit(Opcode::Constant);
                        self.emit_u16(idx_const);
                        self.emit(Opcode::GetElement);
                        self.compile_pattern_binding(inner_pattern)?;
                    }
                }
                self.emit(Opcode::Pop);
            }
            Pattern::Object(obj_pat) => {
                // Nested object destructuring
                for prop in &obj_pat.properties {
                    if let ObjectPatternProperty::Property { key, value, .. } = prop {
                        self.emit(Opcode::Dup);
                        let prop_name = match key {
                            PropertyKey::Identifier(id) => id.name.clone(),
                            PropertyKey::String(s) => s.clone(),
                            PropertyKey::Number(n) => n.to_string(),
                            PropertyKey::Computed(_) => continue,
                            PropertyKey::PrivateName(name) => name.clone(),
                        };
                        let name_idx = self.chunk.add_constant(Value::String(prop_name));
                        self.emit(Opcode::GetProperty);
                        self.emit_u16(name_idx);
                        self.compile_pattern_binding(value)?;
                    }
                }
                self.emit(Opcode::Pop);
            }
            Pattern::Assignment(assign_pat) => {
                // Pattern with default value: a = defaultValue
                // Check if value is undefined and use default
                self.emit(Opcode::Dup); // Keep value on stack for check
                self.emit(Opcode::Undefined);
                self.emit(Opcode::StrictEq);
                let use_value_jump = self.emit_jump(Opcode::JumpIfFalse);

                // Value is undefined, use default
                self.emit(Opcode::Pop); // Pop the undefined value
                self.compile_expr(&assign_pat.right)?;
                let end_jump = self.emit_jump(Opcode::Jump);

                // Value is not undefined, use it
                self.patch_jump(use_value_jump);
                self.emit(Opcode::Pop); // Pop the comparison result

                self.patch_jump(end_jump);
                self.compile_pattern_binding(&assign_pat.left)?;
            }
            _ => {
                // Rest patterns, etc. - pop value
                self.emit(Opcode::Pop);
            }
        }
        Ok(())
    }

    /// Compile pattern assignment - assigns to existing variables (not declarations)
    fn compile_pattern_assignment(&mut self, pattern: &Pattern) -> Result<()> {
        match pattern {
            Pattern::Identifier(id) => {
                // Assign to existing variable
                if let Some(slot) = self.resolve_local(&id.name) {
                    self.emit(Opcode::SetLocal);
                    self.emit_byte(slot);
                } else {
                    let name_idx = self.chunk.add_constant(Value::String(id.name.clone()));
                    self.emit(Opcode::SetGlobal);
                    self.emit_u16(name_idx);
                }
                // Pop the value since SetLocal/SetGlobal peek rather than pop
                self.emit(Opcode::Pop);
            }
            Pattern::Array(arr_pat) => {
                // Array destructuring assignment: [a, b] = arr
                for (idx, elem) in arr_pat.elements.iter().enumerate() {
                    if let Some(inner_pattern) = elem {
                        self.emit(Opcode::Dup); // Keep array on stack
                        let idx_const = self.chunk.add_constant(Value::Number(idx as f64));
                        self.emit(Opcode::Constant);
                        self.emit_u16(idx_const);
                        self.emit(Opcode::GetElement);
                        self.compile_pattern_assignment(inner_pattern)?;
                    }
                }
                self.emit(Opcode::Pop); // Pop the array
            }
            Pattern::Object(obj_pat) => {
                // Object destructuring assignment: {a, b} = obj
                for prop in &obj_pat.properties {
                    if let ObjectPatternProperty::Property { key, value, .. } = prop {
                        self.emit(Opcode::Dup); // Keep object on stack
                        let prop_name = match key {
                            PropertyKey::Identifier(id) => id.name.clone(),
                            PropertyKey::String(s) => s.clone(),
                            PropertyKey::Number(n) => n.to_string(),
                            PropertyKey::Computed(_) => continue, // Skip computed for now
                            PropertyKey::PrivateName(name) => name.clone(),
                        };
                        let name_idx = self.chunk.add_constant(Value::String(prop_name));
                        self.emit(Opcode::GetProperty);
                        self.emit_u16(name_idx);
                        self.compile_pattern_assignment(value)?;
                    }
                }
                self.emit(Opcode::Pop); // Pop the object
            }
            Pattern::Assignment(assign_pat) => {
                // Default value in destructuring: a = defaultValue
                // If value is undefined, use default
                self.emit(Opcode::Dup);
                self.emit(Opcode::Undefined);
                self.emit(Opcode::StrictEq);
                let use_value_jump = self.emit_jump(Opcode::JumpIfFalse);

                // Value is undefined, use default
                self.emit(Opcode::Pop);
                self.compile_expr(&assign_pat.right)?;
                let end_jump = self.emit_jump(Opcode::Jump);

                // Value is not undefined, use it
                self.patch_jump(use_value_jump);
                self.emit(Opcode::Pop);

                self.patch_jump(end_jump);
                self.compile_pattern_assignment(&assign_pat.left)?;
            }
            Pattern::Member(member) => {
                // Assignment to member expression: obj.prop = value or obj[key] = value
                self.compile_expr(&member.object)?;
                match &member.property {
                    MemberProperty::Identifier(id) => {
                        let name_idx = self.chunk.add_constant(Value::String(id.name.clone()));
                        self.emit(Opcode::Swap); // [obj, value] -> [value, obj]
                        self.emit(Opcode::Swap); // [value, obj] -> [obj, value]
                        self.emit(Opcode::SetProperty);
                        self.emit_u16(name_idx);
                    }
                    MemberProperty::Expression(key_expr) => {
                        self.compile_expr(key_expr)?;
                        self.emit(Opcode::Swap); // [obj, key, value] -> [obj, value, key]
                        self.emit(Opcode::SetElement);
                    }
                    MemberProperty::PrivateName(name) => {
                        let name_idx = self.chunk.add_constant(Value::String(name.clone()));
                        self.emit(Opcode::Swap);
                        self.emit(Opcode::Swap);
                        self.emit(Opcode::SetPrivateField);
                        self.emit_u16(name_idx);
                    }
                }
            }
            Pattern::Rest(_) => {
                // Rest patterns in assignment - not yet supported
                self.emit(Opcode::Pop);
            }
        }
        Ok(())
    }

    fn compile_function_decl(&mut self, func: &Function) -> Result<()> {
        let name = func.id.as_ref().map(|id| id.name.clone());

        // Compile function body
        let compiled = self.compile_function_body(func)?;

        // Store the compiled function as a Value in the constants pool
        let mut runtime_func = crate::runtime::JsFunction::new(compiled.name.clone(), compiled.chunk);
        runtime_func.is_async = func.is_async;
        runtime_func.is_generator = func.is_generator;
        let func_value = Value::new_function(runtime_func);
        let func_idx = self.chunk.add_constant(func_value);
        self.emit(Opcode::CreateFunction);
        self.emit_u16(func_idx);

        // Bind to name
        if let Some(name) = name {
            if self.scope_depth > 0 {
                self.add_local(&name)?;
            } else {
                let name_idx = self.chunk.add_constant(Value::String(name));
                self.emit(Opcode::DefineGlobal);
                self.emit_u16(name_idx);
            }
        }

        Ok(())
    }

    fn compile_function_body(&mut self, func: &Function) -> Result<CompiledFunction> {
        let mut compiler = Compiler::new();
        compiler.in_function = true;
        compiler.scope_depth = 1;

        // Collect default values while adding parameters as locals
        let mut default_params: Vec<(usize, &Expression)> = Vec::new();

        // Add parameters as locals
        for (idx, param) in func.params.params.iter().enumerate() {
            match param {
                Pattern::Identifier(id) => {
                    compiler.add_local(&id.name)?;
                }
                Pattern::Assignment(assign_pat) => {
                    // Extract the identifier from the assignment pattern
                    if let Pattern::Identifier(id) = &assign_pat.left {
                        compiler.add_local(&id.name)?;
                        default_params.push((idx, &assign_pat.right));
                    }
                }
                _ => {}
            }
        }

        // Handle rest parameter (...args)
        if let Some(rest) = &func.params.rest {
            if let Pattern::Identifier(id) = rest.as_ref() {
                compiler.add_local(&id.name)?;
                compiler.chunk.has_rest_param = true;
            }
        }

        compiler.chunk.param_count = func.params.params.len() as u8;
        compiler.chunk.is_async = func.is_async;
        compiler.chunk.is_generator = func.is_generator;

        // Emit code for default parameter values
        // Check if param is undefined, and if so, assign the default value
        for (local_idx, default_expr) in default_params {
            // Get the local (parameter)
            compiler.emit(Opcode::GetLocal);
            compiler.emit_u16(local_idx as u16);

            // Check if undefined
            compiler.emit(Opcode::Undefined);
            compiler.emit(Opcode::StrictEq);

            // Jump if not undefined (parameter was provided)
            let skip_jump = compiler.emit_jump(Opcode::JumpIfFalse);
            compiler.emit(Opcode::Pop); // Pop the comparison result

            // Compile and assign the default value
            compiler.compile_expr(default_expr)?;
            compiler.emit(Opcode::SetLocal);
            compiler.emit_u16(local_idx as u16);
            compiler.emit(Opcode::Pop); // Pop the set result

            let end_jump = compiler.emit_jump(Opcode::Jump);
            compiler.patch_jump(skip_jump);
            compiler.emit(Opcode::Pop); // Pop the comparison result when skipping
            compiler.patch_jump(end_jump);
        }

        // Compile body with hoisting
        match &func.body {
            FunctionBody::Block(block) => {
                // Hoist function declarations first
                let hoisted_funcs = compiler.collect_function_declarations(&block.body);
                compiler.hoist_function_declarations(&hoisted_funcs)?;

                // Hoist var declarations
                let hoisted_vars = compiler.collect_var_declarations(&block.body);
                compiler.hoist_var_declarations(&hoisted_vars)?;

                // Compile statements (skip already-hoisted function declarations)
                for stmt in &block.body {
                    if matches!(stmt, Statement::FunctionDeclaration(_)) {
                        continue;
                    }
                    compiler.compile_statement(stmt)?;
                }
            }
            FunctionBody::Expression(expr) => {
                compiler.compile_expr(expr)?;
                compiler.emit(Opcode::Return);
            }
        }

        // Ensure we return undefined at the end
        compiler.emit(Opcode::ReturnUndefined);

        let name = func.id.as_ref().map(|id| id.name.clone());
        Ok(CompiledFunction::new(name, compiler.chunk))
    }

    fn compile_if(&mut self, if_stmt: &IfStatement) -> Result<()> {
        self.compile_expr(&if_stmt.test)?;

        let then_jump = self.emit_jump(Opcode::JumpIfFalse);
        self.emit(Opcode::Pop);

        self.compile_statement(&if_stmt.consequent)?;

        if let Some(alternate) = &if_stmt.alternate {
            let else_jump = self.emit_jump(Opcode::Jump);
            self.patch_jump(then_jump);
            self.emit(Opcode::Pop);
            self.compile_statement(alternate)?;
            self.patch_jump(else_jump);
        } else {
            self.patch_jump(then_jump);
            self.emit(Opcode::Pop);
        }

        Ok(())
    }

    fn compile_while(&mut self, while_stmt: &WhileStatement) -> Result<()> {
        let loop_start = self.chunk.code.len();

        self.loop_stack.push(LoopInfo {
            start: loop_start,
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            depth: self.scope_depth,
        });

        self.compile_expr(&while_stmt.test)?;
        let exit_jump = self.emit_jump(Opcode::JumpIfFalse);
        self.emit(Opcode::Pop);

        self.compile_statement(&while_stmt.body)?;

        self.emit_loop(loop_start);

        self.patch_jump(exit_jump);
        self.emit(Opcode::Pop);

        // Patch break jumps
        let loop_info = self.loop_stack.pop().unwrap();
        for jump in loop_info.break_jumps {
            self.patch_jump(jump);
        }

        Ok(())
    }

    fn compile_do_while(&mut self, do_while: &DoWhileStatement) -> Result<()> {
        let loop_start = self.chunk.code.len();

        self.loop_stack.push(LoopInfo {
            start: loop_start,
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            depth: self.scope_depth,
        });

        self.compile_statement(&do_while.body)?;

        // Continue point (saved for reference, patching done via loop_info)
        let _continue_point = self.chunk.code.len();
        let loop_info = self.loop_stack.last_mut().unwrap();
        for jump in std::mem::take(&mut loop_info.continue_jumps) {
            self.patch_jump(jump);
        }

        self.compile_expr(&do_while.test)?;
        let exit_jump = self.emit_jump(Opcode::JumpIfFalse);
        self.emit(Opcode::Pop);
        self.emit_loop(loop_start);
        self.patch_jump(exit_jump);
        self.emit(Opcode::Pop);

        // Patch break jumps
        let loop_info = self.loop_stack.pop().unwrap();
        for jump in loop_info.break_jumps {
            self.patch_jump(jump);
        }

        Ok(())
    }

    fn compile_for(&mut self, for_stmt: &ForStatement) -> Result<()> {
        self.begin_scope();

        // Init
        if let Some(init) = &for_stmt.init {
            match init {
                ForInit::Declaration(decl) => self.compile_var_decl(decl)?,
                ForInit::Expression(expr) => {
                    self.compile_expr(expr)?;
                    self.emit(Opcode::Pop);
                }
            }
        }

        let loop_start = self.chunk.code.len();

        self.loop_stack.push(LoopInfo {
            start: loop_start,
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            depth: self.scope_depth,
        });

        // Test
        let mut exit_jump = None;
        if let Some(test) = &for_stmt.test {
            self.compile_expr(test)?;
            exit_jump = Some(self.emit_jump(Opcode::JumpIfFalse));
            self.emit(Opcode::Pop);
        }

        // Body
        self.compile_statement(&for_stmt.body)?;

        // Continue point
        let loop_info = self.loop_stack.last_mut().unwrap();
        for jump in std::mem::take(&mut loop_info.continue_jumps) {
            self.patch_jump(jump);
        }

        // Update
        if let Some(update) = &for_stmt.update {
            self.compile_expr(update)?;
            self.emit(Opcode::Pop);
        }

        self.emit_loop(loop_start);

        if let Some(exit_jump) = exit_jump {
            self.patch_jump(exit_jump);
            self.emit(Opcode::Pop);
        }

        // Patch break jumps
        let loop_info = self.loop_stack.pop().unwrap();
        for jump in loop_info.break_jumps {
            self.patch_jump(jump);
        }

        self.end_scope();
        Ok(())
    }

    fn compile_for_in(&mut self, for_in: &ForInStatement) -> Result<()> {
        // Simplified for-in: iterate over Object.keys
        self.begin_scope();

        // Get iterator
        self.compile_expr(&for_in.right)?;
        self.emit(Opcode::GetIterator);

        let loop_start = self.chunk.code.len();

        self.loop_stack.push(LoopInfo {
            start: loop_start,
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            depth: self.scope_depth,
        });

        // Check if done
        self.emit(Opcode::Dup);
        self.emit(Opcode::IteratorNext);
        self.emit(Opcode::Dup);
        self.emit(Opcode::IteratorDone);
        let exit_jump = self.emit_jump(Opcode::JumpIfTrue);
        self.emit(Opcode::Pop);

        // Get value and bind
        self.emit(Opcode::IteratorValue);

        match &for_in.left {
            ForInLeft::Declaration(decl) => {
                if let Some(decl) = decl.declarations.first() {
                    if let Pattern::Identifier(id) = &decl.id {
                        self.add_local(&id.name)?;
                    }
                }
            }
            _ => {
                self.emit(Opcode::Pop);
            }
        }

        self.compile_statement(&for_in.body)?;

        // Continue point
        let loop_info = self.loop_stack.last_mut().unwrap();
        for jump in std::mem::take(&mut loop_info.continue_jumps) {
            self.patch_jump(jump);
        }

        self.emit_loop(loop_start);

        self.patch_jump(exit_jump);
        self.emit(Opcode::Pop);
        self.emit(Opcode::Pop); // Pop iterator

        // Patch break jumps
        let loop_info = self.loop_stack.pop().unwrap();
        for jump in loop_info.break_jumps {
            self.patch_jump(jump);
        }

        self.end_scope();
        Ok(())
    }

    fn compile_for_of(&mut self, for_of: &ForOfStatement) -> Result<()> {
        // Similar to for-in but for iterables
        // For for-await-of, we await each value from the iterator
        self.begin_scope();

        // Pre-declare the loop variable so slot is reserved
        let loop_var_name = match &for_of.left {
            ForInLeft::Declaration(decl) => {
                if let Some(d) = decl.declarations.first() {
                    if let Pattern::Identifier(id) = &d.id {
                        Some(id.name.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        };

        // Push undefined placeholder for the loop variable
        let loop_var_slot = if let Some(ref name) = loop_var_name {
            self.emit(Opcode::Undefined);
            self.add_local(name)?
        } else {
            0
        };

        // Get iterator
        self.compile_expr(&for_of.right)?;
        self.emit(Opcode::GetIterator);

        let loop_start = self.chunk.code.len();

        self.loop_stack.push(LoopInfo {
            start: loop_start,
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            depth: self.scope_depth,
        });

        // Check if done - IteratorNext peeks iterator, pushes {value, done}
        self.emit(Opcode::IteratorNext);

        // For for-await-of, await the iterator result (which may be a promise)
        if for_of.is_await {
            self.emit(Opcode::Await);
        }

        self.emit(Opcode::Dup);
        self.emit(Opcode::IteratorDone);
        let exit_jump = self.emit_jump(Opcode::JumpIfTrue);
        self.emit(Opcode::Pop); // Pop done boolean

        // Get value - IteratorValue pops result, pushes value
        self.emit(Opcode::IteratorValue);

        // For for-await-of, also await the value itself (in case it's a promise)
        if for_of.is_await {
            self.emit(Opcode::Await);
        }

        // Store value in the loop variable slot
        if loop_var_name.is_some() {
            self.emit(Opcode::SetLocal);
            self.emit_u16(loop_var_slot as u16);
            self.emit(Opcode::Pop); // Pop the value after storing
        }

        // Pop the duplicate result from Dup
        self.emit(Opcode::Pop);

        self.compile_statement(&for_of.body)?;

        // Continue point
        let loop_info = self.loop_stack.last_mut().unwrap();
        for jump in std::mem::take(&mut loop_info.continue_jumps) {
            self.patch_jump(jump);
        }

        self.emit_loop(loop_start);

        self.patch_jump(exit_jump);
        self.emit(Opcode::Pop); // Pop done boolean (from the Dup+IteratorDone that was left)
        self.emit(Opcode::Pop); // Pop iterator

        // Patch break jumps
        let loop_info = self.loop_stack.pop().unwrap();
        for jump in loop_info.break_jumps {
            self.patch_jump(jump);
        }

        self.end_scope();
        Ok(())
    }

    fn compile_break(&mut self, _break_stmt: &BreakStatement) -> Result<()> {
        if self.loop_stack.is_empty() {
            return Err(Error::syntax_error("break outside of loop"));
        }

        let jump = self.emit_jump(Opcode::Jump);
        self.loop_stack.last_mut().unwrap().break_jumps.push(jump);
        Ok(())
    }

    fn compile_continue(&mut self, _continue_stmt: &ContinueStatement) -> Result<()> {
        if self.loop_stack.is_empty() {
            return Err(Error::syntax_error("continue outside of loop"));
        }

        let jump = self.emit_jump(Opcode::Jump);
        self.loop_stack
            .last_mut()
            .unwrap()
            .continue_jumps
            .push(jump);
        Ok(())
    }

    fn compile_return(&mut self, return_stmt: &ReturnStatement) -> Result<()> {
        if let Some(arg) = &return_stmt.argument {
            // Check if the return argument is a simple call expression (for TCO)
            if let Expression::Call(call) = arg {
                // Don't apply TCO to super() calls or method calls
                let is_simple_call = !matches!(&call.callee, Expression::Super(_))
                    && !matches!(&call.callee, Expression::Member(_));

                if is_simple_call {
                    // Emit tail call - compile callee and args, then TailCall
                    self.compile_expr(&call.callee)?;

                    for call_arg in &call.arguments {
                        match call_arg {
                            Expression::Spread(spread) => {
                                self.compile_expr(&spread.argument)?;
                                self.emit(Opcode::Spread);
                            }
                            _ => self.compile_expr(call_arg)?,
                        }
                    }

                    let count = call.arguments.len().min(255) as u8;
                    self.emit(Opcode::TailCall);
                    self.emit_byte(count);
                    self.emit(Opcode::Return);
                    return Ok(());
                }
            }

            // Regular return
            self.compile_expr(arg)?;
            self.emit(Opcode::Return);
        } else {
            self.emit(Opcode::ReturnUndefined);
        }
        Ok(())
    }

    fn compile_throw(&mut self, throw_stmt: &ThrowStatement) -> Result<()> {
        self.compile_expr(&throw_stmt.argument)?;
        self.emit(Opcode::Throw);
        Ok(())
    }

    fn compile_try(&mut self, try_stmt: &TryStatement) -> Result<()> {
        let try_start = self.emit_jump(Opcode::EnterTry);

        // Try block
        self.compile_block(&try_stmt.block)?;
        self.emit(Opcode::LeaveTry);
        let try_end = self.emit_jump(Opcode::Jump);

        // Catch block
        self.patch_jump(try_start);
        if let Some(handler) = &try_stmt.handler {
            self.begin_scope();
            if let Some(param) = &handler.param {
                if let Pattern::Identifier(id) = param {
                    self.add_local(&id.name)?;
                }
            } else {
                self.emit(Opcode::Pop);
            }
            self.compile_block(&handler.body)?;
            self.end_scope();
        }

        self.patch_jump(try_end);

        // Finally block
        if let Some(finalizer) = &try_stmt.finalizer {
            self.compile_block(finalizer)?;
        }

        Ok(())
    }

    fn compile_switch(&mut self, switch_stmt: &SwitchStatement) -> Result<()> {
        self.compile_expr(&switch_stmt.discriminant)?;

        let mut case_jumps = Vec::new();
        #[allow(unused_assignments)]
        let mut _default_jump = None;
        #[allow(unused_variables)]
        let _body_jumps: Vec<usize> = Vec::new();

        // Compile case tests
        for case in &switch_stmt.cases {
            if let Some(test) = &case.test {
                self.emit(Opcode::Dup);
                self.compile_expr(test)?;
                self.emit(Opcode::StrictEq);
                case_jumps.push(self.emit_jump(Opcode::JumpIfTrue));
                self.emit(Opcode::Pop);
            } else {
                _default_jump = Some(case_jumps.len());
            }
        }

        // Jump to default or end
        let end_jump = self.emit_jump(Opcode::Jump);

        // Compile case bodies
        for (i, case) in switch_stmt.cases.iter().enumerate() {
            if i < case_jumps.len() {
                self.patch_jump(case_jumps[i]);
                self.emit(Opcode::Pop);
            }

            for stmt in &case.consequent {
                self.compile_statement(stmt)?;
            }
        }

        self.patch_jump(end_jump);
        self.emit(Opcode::Pop); // Pop discriminant

        Ok(())
    }

    fn compile_match_expr(&mut self, match_expr: &crate::ast::MatchExpression) -> Result<()> {
        use crate::ast::MatchPattern;

        // Compile discriminant - leave on stack
        self.compile_expr(&match_expr.discriminant)?;

        let mut end_jumps = Vec::new();

        for arm in &match_expr.arms {
            match &arm.pattern {
                MatchPattern::Wildcard(_) => {
                    // Default arm: pop discriminant, compile body
                    self.emit(Opcode::MatchEnd);
                    self.compile_expr(&arm.body)?;
                    end_jumps.push(self.emit_jump(Opcode::Jump));
                }
                MatchPattern::Literal(lit) => {
                    self.emit(Opcode::Dup);
                    self.compile_literal(lit)?;
                    self.emit(Opcode::MatchPattern);
                    let skip = self.emit_jump(Opcode::JumpIfFalse);
                    self.emit(Opcode::Pop); // pop true

                    if let Some(guard) = &arm.guard {
                        self.compile_expr(guard)?;
                        let guard_skip = self.emit_jump(Opcode::JumpIfFalse);
                        self.emit(Opcode::Pop); // pop true
                        self.emit(Opcode::MatchEnd); // pop discriminant
                        self.compile_expr(&arm.body)?;
                        end_jumps.push(self.emit_jump(Opcode::Jump));
                        self.patch_jump(guard_skip);
                        self.emit(Opcode::Pop); // pop false
                    } else {
                        self.emit(Opcode::MatchEnd); // pop discriminant
                        self.compile_expr(&arm.body)?;
                        end_jumps.push(self.emit_jump(Opcode::Jump));
                    }

                    self.patch_jump(skip);
                    self.emit(Opcode::Pop); // pop false
                }
                MatchPattern::Identifier(id) => {
                    // Identifier pattern: bind discriminant to a local
                    let _slot = self.add_local(&id.name)?;

                    if let Some(guard) = &arm.guard {
                        // Guard can reference the bound variable
                        self.compile_expr(guard)?;
                        let guard_skip = self.emit_jump(Opcode::JumpIfFalse);
                        self.emit(Opcode::Pop); // pop true

                        // Compile body (can reference bound variable via GetLocal)
                        self.compile_expr(&arm.body)?;
                        // Stack: [..., disc(=x), body_result]
                        // Swap result past disc, pop disc
                        self.emit(Opcode::Swap);
                        self.emit(Opcode::Pop);
                        self.locals.pop();
                        end_jumps.push(self.emit_jump(Opcode::Jump));

                        self.patch_jump(guard_skip);
                        self.emit(Opcode::Pop); // pop false
                        self.locals.pop();
                        // disc remains on stack for next arm
                    } else {
                        // Compile body (can reference bound variable via GetLocal)
                        self.compile_expr(&arm.body)?;
                        // Stack: [..., disc(=x), body_result]
                        self.emit(Opcode::Swap);
                        self.emit(Opcode::Pop);
                        self.locals.pop();
                        end_jumps.push(self.emit_jump(Opcode::Jump));
                    }
                }
                MatchPattern::Or(alternatives, _) => {
                    // Try each alternative
                    let mut or_end_jumps = Vec::new();
                    for (i, alt) in alternatives.iter().enumerate() {
                        if let MatchPattern::Literal(lit) = alt {
                            self.emit(Opcode::Dup);
                            self.compile_literal(lit)?;
                            self.emit(Opcode::MatchPattern);
                            if i < alternatives.len() - 1 {
                                or_end_jumps.push(self.emit_jump(Opcode::JumpIfTrue));
                                self.emit(Opcode::Pop); // pop false
                            }
                        } else if let MatchPattern::Wildcard(_) = alt {
                            self.emit(Opcode::True);
                            if i < alternatives.len() - 1 {
                                or_end_jumps.push(self.emit_jump(Opcode::JumpIfTrue));
                                self.emit(Opcode::Pop);
                            }
                        }
                    }
                    // Patch or-end jumps to here
                    let skip = self.emit_jump(Opcode::JumpIfFalse);
                    self.emit(Opcode::Pop); // pop true
                    for j in or_end_jumps {
                        self.patch_jump(j);
                        // The JumpIfTrue doesn't pop, so we need to pop the true
                    }
                    // Pop any remaining true values from or-short-circuit
                    // Actually JumpIfTrue leaves the value on stack, so pop it
                    self.emit(Opcode::Pop);

                    if let Some(guard) = &arm.guard {
                        self.compile_expr(guard)?;
                        let guard_skip = self.emit_jump(Opcode::JumpIfFalse);
                        self.emit(Opcode::Pop);
                        self.emit(Opcode::MatchEnd);
                        self.compile_expr(&arm.body)?;
                        end_jumps.push(self.emit_jump(Opcode::Jump));
                        self.patch_jump(guard_skip);
                        self.emit(Opcode::Pop);
                    } else {
                        self.emit(Opcode::MatchEnd);
                        self.compile_expr(&arm.body)?;
                        end_jumps.push(self.emit_jump(Opcode::Jump));
                    }

                    self.patch_jump(skip);
                    self.emit(Opcode::Pop); // pop false
                }
                MatchPattern::Array(elements, _) => {
                    // Test array length
                    self.emit(Opcode::Dup);
                    let length_idx = self.chunk.add_constant(Value::String("length".to_string()));
                    self.emit(Opcode::GetProperty);
                    self.emit_u16(length_idx);
                    let non_rest = elements.iter().filter(|e| !matches!(e, MatchPattern::Rest(_, _))).count();
                    self.emit_constant(Value::Number(non_rest as f64));
                    self.emit(Opcode::Ge);
                    let length_fail = self.emit_jump(Opcode::JumpIfFalse);
                    self.emit(Opcode::Pop); // pop true

                    // Test each literal element
                    let mut elem_fails = Vec::new();
                    for (i, elem) in elements.iter().enumerate() {
                        if let MatchPattern::Literal(lit) = elem {
                            self.emit(Opcode::Dup);
                            self.emit_constant(Value::Number(i as f64));
                            self.emit(Opcode::GetElement);
                            self.compile_literal(lit)?;
                            self.emit(Opcode::MatchPattern);
                            elem_fails.push(self.emit_jump(Opcode::JumpIfFalse));
                            self.emit(Opcode::Pop); // pop true
                        }
                    }

                    // All tests passed — bind variables
                    let mut bindings_count = 0;
                    for (i, elem) in elements.iter().enumerate() {
                        if let MatchPattern::Identifier(id) = elem {
                            self.emit(Opcode::Dup); // dup array
                            self.emit_constant(Value::Number(i as f64));
                            self.emit(Opcode::GetElement);
                            let _slot = self.add_local(&id.name)?;
                            bindings_count += 1;
                        }
                    }

                    if let Some(guard) = &arm.guard {
                        self.compile_expr(guard)?;
                        let guard_skip = self.emit_jump(Opcode::JumpIfFalse);
                        self.emit(Opcode::Pop);

                        self.compile_expr(&arm.body)?;
                        // Clean up: swap result past bindings and disc
                        for _ in 0..bindings_count {
                            self.emit(Opcode::Swap);
                            self.emit(Opcode::Pop);
                            self.locals.pop();
                        }
                        self.emit(Opcode::Swap);
                        self.emit(Opcode::Pop); // pop disc
                        end_jumps.push(self.emit_jump(Opcode::Jump));

                        self.patch_jump(guard_skip);
                        self.emit(Opcode::Pop);
                        for _ in 0..bindings_count {
                            self.emit(Opcode::Pop);
                            self.locals.pop();
                        }
                    } else {
                        self.compile_expr(&arm.body)?;
                        for _ in 0..bindings_count {
                            self.emit(Opcode::Swap);
                            self.emit(Opcode::Pop);
                            self.locals.pop();
                        }
                        self.emit(Opcode::Swap);
                        self.emit(Opcode::Pop); // pop disc
                        end_jumps.push(self.emit_jump(Opcode::Jump));
                    }

                    // Jump target for failed element checks
                    let skip_to = self.chunk.code.len();
                    self.patch_jump(length_fail);
                    for j in elem_fails {
                        self.patch_jump(j);
                    }
                    // Pop false from the failed check
                    self.emit(Opcode::Pop);
                    let _ = skip_to;
                }
                MatchPattern::Object(props, _) => {
                    // Test each property
                    let mut prop_fails = Vec::new();
                    for (key, value_pattern) in props {
                        if let MatchPattern::Literal(lit) = value_pattern {
                            self.emit(Opcode::Dup);
                            let key_idx = self.chunk.add_constant(Value::String(key.clone()));
                            self.emit(Opcode::GetProperty);
                            self.emit_u16(key_idx);
                            self.compile_literal(lit)?;
                            self.emit(Opcode::MatchPattern);
                            prop_fails.push(self.emit_jump(Opcode::JumpIfFalse));
                            self.emit(Opcode::Pop); // pop true
                        }
                    }

                    // All checks passed — bind variables
                    let mut bindings_count = 0;
                    for (key, value_pattern) in props {
                        if let MatchPattern::Identifier(_id) = value_pattern {
                            self.emit(Opcode::Dup); // dup object
                            let key_idx = self.chunk.add_constant(Value::String(key.clone()));
                            self.emit(Opcode::GetProperty);
                            self.emit_u16(key_idx);
                            let _slot = self.add_local(&_id.name)?;
                            bindings_count += 1;
                        }
                    }

                    if let Some(guard) = &arm.guard {
                        self.compile_expr(guard)?;
                        let guard_skip = self.emit_jump(Opcode::JumpIfFalse);
                        self.emit(Opcode::Pop);

                        self.compile_expr(&arm.body)?;
                        for _ in 0..bindings_count {
                            self.emit(Opcode::Swap);
                            self.emit(Opcode::Pop);
                            self.locals.pop();
                        }
                        self.emit(Opcode::Swap);
                        self.emit(Opcode::Pop); // pop disc
                        end_jumps.push(self.emit_jump(Opcode::Jump));

                        self.patch_jump(guard_skip);
                        self.emit(Opcode::Pop);
                        for _ in 0..bindings_count {
                            self.emit(Opcode::Pop);
                            self.locals.pop();
                        }
                    } else {
                        self.compile_expr(&arm.body)?;
                        for _ in 0..bindings_count {
                            self.emit(Opcode::Swap);
                            self.emit(Opcode::Pop);
                            self.locals.pop();
                        }
                        self.emit(Opcode::Swap);
                        self.emit(Opcode::Pop); // pop disc
                        end_jumps.push(self.emit_jump(Opcode::Jump));
                    }

                    // Failed property checks
                    for j in prop_fails {
                        self.patch_jump(j);
                    }
                    self.emit(Opcode::Pop); // pop false
                }
                MatchPattern::Binding { .. } => {
                    // Test inner pattern first
                    self.compile_match_arm_pattern_test(&arm.pattern, &mut end_jumps, &arm.guard, &arm.body)?;
                }
                MatchPattern::Rest(_, _) => {
                    // Rest pattern always matches (like wildcard)
                    self.emit(Opcode::MatchEnd);
                    self.compile_expr(&arm.body)?;
                    end_jumps.push(self.emit_jump(Opcode::Jump));
                }
            }
        }

        // If no arm matched, result is undefined
        self.emit(Opcode::MatchEnd); // pop discriminant
        self.emit(Opcode::Undefined);

        for jump in end_jumps {
            self.patch_jump(jump);
        }

        Ok(())
    }

    /// Helper: compile a Binding pattern arm (name @ inner_pattern)
    fn compile_match_arm_pattern_test(
        &mut self,
        pattern: &crate::ast::MatchPattern,
        end_jumps: &mut Vec<usize>,
        _guard: &Option<Box<Expression>>,
        body: &Expression,
    ) -> Result<()> {
        use crate::ast::MatchPattern;

        if let MatchPattern::Binding { name, pattern: inner, .. } = pattern {
            // Register the binding
            let slot = self.add_local(name)?;
            let _ = slot;

            match inner.as_ref() {
                MatchPattern::Literal(lit) => {
                    self.emit(Opcode::Dup);
                    self.compile_literal(lit)?;
                    self.emit(Opcode::MatchPattern);
                    let skip = self.emit_jump(Opcode::JumpIfFalse);
                    self.emit(Opcode::Pop); // pop true

                    self.compile_expr(body)?;
                    self.emit(Opcode::Swap);
                    self.emit(Opcode::Pop);
                    self.locals.pop();
                    end_jumps.push(self.emit_jump(Opcode::Jump));

                    self.patch_jump(skip);
                    self.emit(Opcode::Pop); // pop false
                    self.locals.pop();
                }
                MatchPattern::Wildcard(_) => {
                    self.compile_expr(body)?;
                    self.emit(Opcode::Swap);
                    self.emit(Opcode::Pop);
                    self.locals.pop();
                    end_jumps.push(self.emit_jump(Opcode::Jump));
                }
                _ => {
                    // For other inner patterns, treat as wildcard binding
                    self.compile_expr(body)?;
                    self.emit(Opcode::Swap);
                    self.emit(Opcode::Pop);
                    self.locals.pop();
                    end_jumps.push(self.emit_jump(Opcode::Jump));
                }
            }
        }

        Ok(())
    }

    fn compile_class_decl(&mut self, class: &Class) -> Result<()> {
        let name = class.id.as_ref().map(|id| id.name.clone());

        // Find and compile the constructor
        let mut constructor_value = None;
        for element in &class.body.body {
            if let ClassElement::Method(method) = element {
                if method.kind == MethodKind::Constructor {
                    let compiled = self.compile_function_body(&method.value)?;
                    let mut runtime_func = JsFunction::new(compiled.name, compiled.chunk);
                    runtime_func.is_async = method.value.is_async;
                    runtime_func.is_generator = method.value.is_generator;
                    constructor_value = Some(Value::new_function(runtime_func));
                    break;
                }
            }
        }

        // Compile all methods, getters, setters, and static members
        let mut prototype_methods: HashMap<String, Value> = HashMap::default();
        let mut getters: HashMap<String, Value> = HashMap::default();
        let mut setters: HashMap<String, Value> = HashMap::default();
        let mut static_methods: HashMap<String, Value> = HashMap::default();
        let mut static_getters: HashMap<String, Value> = HashMap::default();
        let mut static_setters: HashMap<String, Value> = HashMap::default();

        // Track static fields with complex initializers (need runtime initialization)
        let mut complex_static_fields: Vec<(String, Box<Expression>)> = Vec::new();
        // Track instance fields with their default values (for private fields, key includes # prefix)
        let mut instance_fields: HashMap<String, Value> = HashMap::default();

        for element in &class.body.body {
            match element {
                ClassElement::Method(method) => {
                    if method.kind == MethodKind::Constructor {
                        continue;
                    }

                    // Compile the method
                    let compiled = self.compile_function_body(&method.value)?;
                    let mut runtime_func = JsFunction::new(compiled.name.clone(), compiled.chunk);
                    runtime_func.is_async = method.value.is_async;
                    runtime_func.is_generator = method.value.is_generator;
                    let method_value = Value::new_function(runtime_func);

                    // Get method name
                    let method_name = match &method.key {
                        crate::ast::PropertyKey::Identifier(id) => id.name.clone(),
                        crate::ast::PropertyKey::String(s) => s.clone(),
                        crate::ast::PropertyKey::Number(n) => n.to_string(),
                        crate::ast::PropertyKey::Computed(_) => continue, // Skip computed for now
                        crate::ast::PropertyKey::PrivateName(name) => format!("#{}", name),
                    };

                    // Store in appropriate map based on method kind and static modifier
                    if method.is_static {
                        match method.kind {
                            MethodKind::Get => {
                                static_getters.insert(method_name, method_value);
                            }
                            MethodKind::Set => {
                                static_setters.insert(method_name, method_value);
                            }
                            MethodKind::Method => {
                                static_methods.insert(method_name, method_value);
                            }
                            MethodKind::Constructor => unreachable!(),
                        }
                    } else {
                        match method.kind {
                            MethodKind::Get => {
                                getters.insert(method_name, method_value);
                            }
                            MethodKind::Set => {
                                setters.insert(method_name, method_value);
                            }
                            MethodKind::Method => {
                                prototype_methods.insert(method_name, method_value);
                            }
                            MethodKind::Constructor => unreachable!(),
                        }
                    }
                }
                ClassElement::Property(prop) => {
                    // Get property name (for private fields, include the # prefix internally)
                    let (prop_name, is_private) = match &prop.key {
                        crate::ast::PropertyKey::Identifier(id) => (id.name.clone(), false),
                        crate::ast::PropertyKey::String(s) => (s.clone(), false),
                        crate::ast::PropertyKey::Number(n) => (n.to_string(), false),
                        crate::ast::PropertyKey::Computed(_) => continue, // Skip computed for now
                        crate::ast::PropertyKey::PrivateName(name) => (name.clone(), true),
                    };

                    // For private fields, store with # prefix to distinguish from public fields
                    let storage_name = if is_private {
                        format!("#{}", prop_name)
                    } else {
                        prop_name.clone()
                    };

                    if prop.is_static {
                        // Static field
                        if let Some(init_expr) = &prop.value {
                            // Try to evaluate as a literal at compile time
                            if let Some(value) = self.try_eval_literal(init_expr) {
                                static_methods.insert(storage_name, value);
                            } else {
                                // Complex expression - needs runtime initialization
                                complex_static_fields.push((storage_name, init_expr.clone()));
                            }
                        } else {
                            // No initializer - default to undefined
                            static_methods.insert(storage_name, Value::Undefined);
                        }
                    } else {
                        // Instance field - evaluate at compile time if possible
                        let value = if let Some(init_expr) = &prop.value {
                            self.try_eval_literal(init_expr).unwrap_or(Value::Undefined)
                        } else {
                            Value::Undefined
                        };
                        instance_fields.insert(storage_name, value);
                    }
                }
                ClassElement::StaticBlock(_) => {
                    // Static blocks not yet supported
                    continue;
                }
            }
        }

        // Create class value with constructor, prototype methods, getters, setters, static members, and instance fields
        let class_name = name.clone().unwrap_or_default();
        let class_value = Value::new_class_with_prototype_and_fields(
            class_name.clone(),
            constructor_value,
            prototype_methods,
            getters,
            setters,
            static_methods,
            static_getters,
            static_setters,
            instance_fields,
        );

        let class_idx = self.chunk.add_constant(class_value);
        self.emit(Opcode::Constant);
        self.emit_u16(class_idx);

        // Handle superclass if present
        if let Some(super_expr) = &class.super_class {
            // Compile the superclass expression
            self.compile_expr(super_expr)?;
            // Set superclass on the class
            self.emit(Opcode::SetSuperClass);
        }

        // Bind to name
        if let Some(ref name) = name {
            if self.scope_depth > 0 {
                self.add_local(name)?;
            } else {
                let name_idx = self.chunk.add_constant(Value::String(name.clone()));
                self.emit(Opcode::DefineGlobal);
                self.emit_u16(name_idx);
            }
        }

        // Initialize complex static fields at runtime
        for (field_name, init_expr) in complex_static_fields {
            // Get the class onto the stack
            if let Some(ref name) = name {
                if let Some(slot) = self.resolve_local(name) {
                    self.emit(Opcode::GetLocal);
                    self.emit_byte(slot);
                } else {
                    let name_idx = self.chunk.add_constant(Value::String(name.clone()));
                    self.emit(Opcode::GetGlobal);
                    self.emit_u16(name_idx);
                }
            }
            // Compile the initializer expression
            self.compile_expr(&init_expr)?;
            // Set the property on the class
            let prop_idx = self.chunk.add_constant(Value::String(field_name));
            self.emit(Opcode::SetProperty);
            self.emit_u16(prop_idx);
            // Pop the result of SetProperty
            self.emit(Opcode::Pop);
        }

        Ok(())
    }

    // ========== Expressions ==========

    fn compile_expr(&mut self, expr: &Expression) -> Result<()> {
        self.set_location(&expr.span());

        match expr {
            Expression::Literal(lit) => self.compile_literal(lit),
            Expression::Identifier(id) => self.compile_identifier(id),
            Expression::This(_) => {
                self.emit(Opcode::This);
                Ok(())
            }
            Expression::Super(_) => {
                self.emit(Opcode::Super);
                Ok(())
            }
            Expression::Array(arr) => self.compile_array(arr),
            Expression::Object(obj) => self.compile_object(obj),
            Expression::Function(func) => self.compile_function_expr(func),
            Expression::Arrow(func) => self.compile_function_expr(func),
            Expression::Member(member) => self.compile_member(member, false),
            Expression::OptionalMember(member) => self.compile_member(member, true),
            Expression::Call(call) => self.compile_call(call, false),
            Expression::OptionalCall(call) => self.compile_call(call, true),
            Expression::New(new) => self.compile_new(new),
            Expression::Unary(unary) => self.compile_unary(unary),
            Expression::Update(update) => self.compile_update(update),
            Expression::Binary(binary) => self.compile_binary(binary),
            Expression::Logical(logical) => self.compile_logical(logical),
            Expression::Assignment(assignment) => self.compile_assignment(assignment),
            Expression::Conditional(cond) => self.compile_conditional(cond),
            Expression::Sequence(seq) => self.compile_sequence(seq),
            Expression::Parenthesized(inner) => self.compile_expr(inner),
            Expression::Await(await_expr) => {
                self.compile_expr(&await_expr.argument)?;
                self.emit(Opcode::Await);
                Ok(())
            }
            Expression::Yield(yield_expr) => {
                if let Some(arg) = &yield_expr.argument {
                    self.compile_expr(arg)?;
                } else {
                    self.emit(Opcode::Undefined);
                }
                self.emit(Opcode::Yield);
                Ok(())
            }
            Expression::TemplateLiteral(template) => self.compile_template_literal(template),
            Expression::Import(import_expr) => {
                // Dynamic import: import(source) returns Promise<Module>
                self.compile_expr(&import_expr.source)?;
                self.emit(Opcode::DynamicImport);
                Ok(())
            }
            Expression::Perform(perform_expr) => {
                // Compile arguments
                for arg in &perform_expr.arguments {
                    self.compile_expr(arg)?;
                }
                // Emit perform opcode with effect type, operation, and arg count
                let effect_type_index = self.chunk.add_constant(Value::String(perform_expr.effect_type.clone()));
                let operation_index = self.chunk.add_constant(Value::String(perform_expr.operation.clone()));
                self.emit(Opcode::Perform);
                self.emit_u16(effect_type_index);
                self.emit_u16(operation_index);
                self.emit_byte(perform_expr.arguments.len() as u8);
                Ok(())
            }
            Expression::Match(match_expr) => {
                self.compile_match_expr(match_expr)
            }
            Expression::Pipeline { left, right } => {
                // Desugar: expr |> func  →  func(expr)
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                self.emit(Opcode::Swap);
                self.emit(Opcode::Call);
                self.emit_byte(1);
                Ok(())
            }
            _ => {
                // Fallback for unimplemented expression types (e.g., MetaProperty, JSX)
                // These are parsed but not yet supported in bytecode generation
                self.emit(Opcode::Undefined);
                Ok(())
            }
        }
    }

    fn compile_literal(&mut self, lit: &Literal) -> Result<()> {
        match &lit.value {
            LiteralValue::Null => self.emit(Opcode::Null),
            LiteralValue::Boolean(true) => self.emit(Opcode::True),
            LiteralValue::Boolean(false) => self.emit(Opcode::False),
            LiteralValue::Number(n) => {
                self.emit_constant(Value::Number(*n));
            }
            LiteralValue::String(s) => {
                self.emit_constant(Value::String(s.clone()));
            }
            LiteralValue::BigInt(s) => {
                // Parse BigInt value
                if let Some(bigint_val) = Value::new_bigint(s) {
                    self.emit_constant(bigint_val);
                } else {
                    // Fallback to 0n if parsing fails
                    self.emit_constant(Value::bigint_from_i64(0));
                }
            }
            LiteralValue::Regex { pattern, flags } => {
                // Store as object with pattern and flags
                self.emit_constant(Value::String(format!("/{}/{}", pattern, flags)));
            }
        }
        Ok(())
    }

    fn compile_identifier(&mut self, id: &Identifier) -> Result<()> {
        // Check for local variable
        if let Some(slot) = self.resolve_local(&id.name) {
            self.emit(Opcode::GetLocal);
            self.emit_byte(slot);
            return Ok(());
        }

        // Check for upvalue
        if let Some(slot) = self.resolve_upvalue(&id.name) {
            self.emit(Opcode::GetUpvalue);
            self.emit_u16(slot as u16);
            return Ok(());
        }

        // Global variable
        let name_idx = self.chunk.add_constant(Value::String(id.name.clone()));
        self.emit(Opcode::GetGlobal);
        self.emit_u16(name_idx);
        Ok(())
    }

    fn compile_array(&mut self, arr: &ArrayExpression) -> Result<()> {
        for elem in &arr.elements {
            if let Some(elem) = elem {
                match elem {
                    Expression::Spread(spread) => {
                        self.compile_expr(&spread.argument)?;
                        self.emit(Opcode::Spread);
                    }
                    _ => self.compile_expr(elem)?,
                }
            } else {
                self.emit(Opcode::Undefined);
            }
        }

        let count = arr.elements.len().min(255) as u8;
        self.emit(Opcode::CreateArray);
        self.emit_byte(count);
        Ok(())
    }

    fn compile_object(&mut self, obj: &ObjectExpression) -> Result<()> {
        let count = obj.properties.len().min(255) as u8;
        self.emit(Opcode::CreateObject);
        self.emit_byte(count);

        for prop in &obj.properties {
            match prop {
                ObjectProperty::Property {
                    key,
                    value,
                    shorthand: _,
                    computed: _,
                    method: _,
                    ..
                } => {
                    // Push key
                    match key {
                        PropertyKey::Identifier(id) => {
                            self.emit_constant(Value::String(id.name.clone()));
                        }
                        PropertyKey::String(s) => {
                            self.emit_constant(Value::String(s.clone()));
                        }
                        PropertyKey::Number(n) => {
                            self.emit_constant(Value::Number(*n));
                        }
                        PropertyKey::Computed(expr) => {
                            self.compile_expr(expr)?;
                        }
                        PropertyKey::PrivateName(name) => {
                            self.emit_constant(Value::String(name.clone()));
                        }
                    }

                    // Push value
                    self.compile_expr(value)?;

                    self.emit(Opcode::DefineProperty);
                    self.emit_u16(0); // Property index (unused)
                }
                ObjectProperty::Spread { argument, .. } => {
                    self.compile_expr(argument)?;
                    self.emit(Opcode::Spread);
                }
                ObjectProperty::Method(method) => {
                    // Compile as property with function value
                    match &method.key {
                        PropertyKey::Identifier(id) => {
                            self.emit_constant(Value::String(id.name.clone()));
                        }
                        PropertyKey::String(s) => {
                            self.emit_constant(Value::String(s.clone()));
                        }
                        _ => {}
                    }

                    let compiled = self.compile_function_body(&method.value)?;
                    let func_idx = self.functions.len();
                    self.functions.push(compiled);

                    let idx = self.chunk.add_constant(Value::Number(func_idx as f64));
                    self.emit(Opcode::CreateFunction);
                    self.emit_u16(idx);

                    self.emit(Opcode::DefineProperty);
                    self.emit_u16(0);
                }
            }
        }

        Ok(())
    }

    fn compile_function_expr(&mut self, func: &Function) -> Result<()> {
        let compiled = self.compile_function_body(func)?;

        // Convert compiled function to runtime function value
        let mut runtime_func = JsFunction::new(compiled.name, compiled.chunk);
        runtime_func.is_async = func.is_async;
        runtime_func.is_generator = func.is_generator;
        let func_value = Value::new_function(runtime_func);

        let idx = self.chunk.add_constant(func_value);
        self.emit(Opcode::CreateFunction);
        self.emit_u16(idx);
        Ok(())
    }

    fn compile_member(&mut self, member: &MemberExpression, optional: bool) -> Result<()> {
        self.compile_expr(&member.object)?;

        if optional {
            let skip_jump = self.emit_jump(Opcode::JumpIfNull);
            self.compile_member_access(member)?;
            self.patch_jump(skip_jump);
        } else {
            self.compile_member_access(member)?;
        }

        Ok(())
    }

    fn compile_member_access(&mut self, member: &MemberExpression) -> Result<()> {
        match &member.property {
            MemberProperty::Identifier(id) => {
                let name_idx = self.chunk.add_constant(Value::String(id.name.clone()));
                self.emit(Opcode::GetProperty);
                self.emit_u16(name_idx);
            }
            MemberProperty::Expression(expr) => {
                self.compile_expr(expr)?;
                self.emit(Opcode::GetElement);
            }
            MemberProperty::PrivateName(name) => {
                let name_idx = self.chunk.add_constant(Value::String(name.clone()));
                self.emit(Opcode::GetPrivateField);
                self.emit_u16(name_idx);
            }
        }
        Ok(())
    }

    fn compile_call(&mut self, call: &CallExpression, optional: bool) -> Result<()> {
        // Check if this is a super() call
        if let Expression::Super(_) = &call.callee {
            // super() call - compile args and emit SuperCall
            for arg in &call.arguments {
                match arg {
                    Expression::Spread(spread) => {
                        self.compile_expr(&spread.argument)?;
                        self.emit(Opcode::Spread);
                    }
                    _ => self.compile_expr(arg)?,
                }
            }
            let arg_count = call.arguments.len().min(255) as u8;
            self.emit(Opcode::SuperCall);
            self.emit_byte(arg_count);
            return Ok(());
        }

        // Check if this is a method call (callee is a member expression)
        if let Expression::Member(member) = &call.callee {
            // Method call: emit receiver, args, then CallMethod
            self.compile_expr(&member.object)?;

            if optional {
                let skip_jump = self.emit_jump(Opcode::JumpIfNull);
                self.compile_method_call(member, call)?;
                self.patch_jump(skip_jump);
            } else {
                self.compile_method_call(member, call)?;
            }
        } else {
            // Regular function call
            self.compile_expr(&call.callee)?;

            if optional {
                let skip_jump = self.emit_jump(Opcode::JumpIfNull);
                self.compile_call_args(call)?;
                self.patch_jump(skip_jump);
            } else {
                self.compile_call_args(call)?;
            }
        }

        Ok(())
    }

    fn compile_method_call(&mut self, member: &MemberExpression, call: &CallExpression) -> Result<()> {
        // Compile arguments
        for arg in &call.arguments {
            match arg {
                Expression::Spread(spread) => {
                    self.compile_expr(&spread.argument)?;
                    self.emit(Opcode::Spread);
                }
                _ => self.compile_expr(arg)?,
            }
        }

        let arg_count = call.arguments.len().min(255) as u8;

        // Emit CallMethod with method name
        match &member.property {
            MemberProperty::Identifier(id) => {
                let name_idx = self.chunk.add_constant(Value::String(id.name.clone()));
                self.emit(Opcode::CallMethod);
                self.emit_u16(name_idx);
                self.emit_byte(arg_count);
            }
            MemberProperty::Expression(expr) => {
                // For computed properties like arr["map"], fall back to GetProperty + Call
                self.compile_expr(expr)?;
                self.emit(Opcode::GetElement);
                self.emit(Opcode::Call);
                self.emit_byte(arg_count);
            }
            MemberProperty::PrivateName(name) => {
                // Private method call - get the private field and call it
                let name_idx = self.chunk.add_constant(Value::String(name.clone()));
                self.emit(Opcode::GetPrivateField);
                self.emit_u16(name_idx);
                self.emit(Opcode::Call);
                self.emit_byte(arg_count);
            }
        }

        Ok(())
    }

    fn compile_call_args(&mut self, call: &CallExpression) -> Result<()> {
        for arg in &call.arguments {
            match arg {
                Expression::Spread(spread) => {
                    self.compile_expr(&spread.argument)?;
                    self.emit(Opcode::Spread);
                }
                _ => self.compile_expr(arg)?,
            }
        }

        let count = call.arguments.len().min(255) as u8;
        self.emit(Opcode::Call);
        self.emit_byte(count);
        Ok(())
    }

    fn compile_new(&mut self, new: &NewExpression) -> Result<()> {
        self.compile_expr(&new.callee)?;

        for arg in &new.arguments {
            self.compile_expr(arg)?;
        }

        let count = new.arguments.len().min(255) as u8;
        self.emit(Opcode::New);
        self.emit_byte(count);
        Ok(())
    }

    fn compile_unary(&mut self, unary: &UnaryExpression) -> Result<()> {
        use crate::ast::MemberProperty;

        // Handle delete specially for member expressions
        if matches!(unary.operator, UnaryOperator::Delete) {
            if let Expression::Member(member) = &unary.argument {
                // Compile the object
                self.compile_expr(&member.object)?;

                // Get the property name
                match &member.property {
                    MemberProperty::Identifier(id) => {
                        let idx = self.chunk.add_constant(Value::String(id.name.clone()));
                        self.emit(Opcode::DeleteProperty);
                        self.emit_u16(idx);
                    }
                    MemberProperty::Expression(expr) if member.computed => {
                        // For computed properties like delete obj[expr], evaluate the expression
                        // and fall back to simple delete
                        self.compile_expr(expr)?;
                        self.emit(Opcode::Delete);
                    }
                    _ => {
                        // Fallback for other cases
                        self.emit(Opcode::Delete);
                    }
                }
                return Ok(());
            }
        }

        // typeof <identifier> uses TryGetGlobal to avoid ReferenceError on undefined vars
        if matches!(unary.operator, UnaryOperator::Typeof) {
            if let Expression::Identifier(id) = &unary.argument {
                if self.resolve_local(&id.name).is_none() {
                    let idx = self.chunk.add_constant(Value::String(id.name.clone()));
                    self.emit(Opcode::TryGetGlobal);
                    self.emit_u16(idx);
                    self.emit(Opcode::Typeof);
                    return Ok(());
                }
            }
        }

        self.compile_expr(&unary.argument)?;

        match unary.operator {
            UnaryOperator::Minus => self.emit(Opcode::Neg),
            UnaryOperator::Plus => {} // No-op, converts to number
            UnaryOperator::Not => self.emit(Opcode::Not),
            UnaryOperator::BitwiseNot => self.emit(Opcode::BitwiseNot),
            UnaryOperator::Typeof => self.emit(Opcode::Typeof),
            UnaryOperator::Void => self.emit(Opcode::Void),
            UnaryOperator::Delete => self.emit(Opcode::Delete),
        }
        Ok(())
    }

    fn compile_update(&mut self, update: &UpdateExpression) -> Result<()> {
        // Simplified: only handle identifiers
        if let Expression::Identifier(id) = &update.argument {
            if let Some(slot) = self.resolve_local(&id.name) {
                self.emit(Opcode::GetLocal);
                self.emit_byte(slot);

                if !update.prefix {
                    self.emit(Opcode::Dup);
                }

                match update.operator {
                    UpdateOperator::Increment => self.emit(Opcode::Increment),
                    UpdateOperator::Decrement => self.emit(Opcode::Decrement),
                }

                if update.prefix {
                    self.emit(Opcode::Dup);
                }

                self.emit(Opcode::SetLocal);
                self.emit_byte(slot);

                if update.prefix {
                    self.emit(Opcode::Pop);
                }
            } else {
                let name_idx = self.chunk.add_constant(Value::String(id.name.clone()));
                self.emit(Opcode::GetGlobal);
                self.emit_u16(name_idx);

                if !update.prefix {
                    self.emit(Opcode::Dup);
                }

                match update.operator {
                    UpdateOperator::Increment => self.emit(Opcode::Increment),
                    UpdateOperator::Decrement => self.emit(Opcode::Decrement),
                }

                if update.prefix {
                    self.emit(Opcode::Dup);
                }

                self.emit(Opcode::SetGlobal);
                self.emit_u16(name_idx);

                if update.prefix {
                    self.emit(Opcode::Pop);
                }
            }
        } else if let Expression::Member(member) = &update.argument {
            // Handle member expressions like obj.prop++ or obj[key]++
            match &member.property {
                MemberProperty::Identifier(id) => {
                    let name_idx = self.chunk.add_constant(Value::String(id.name.clone()));

                    if update.prefix {
                        // Prefix: ++obj.x
                        // 1. Compile obj: [obj]
                        // 2. Dup: [obj, obj]
                        // 3. GetProperty: [obj, value]
                        // 4. Increment: [obj, new_value]
                        // 5. SetProperty: [new_value] (returns the new value)
                        self.compile_expr(&member.object)?;
                        self.emit(Opcode::Dup);
                        self.emit(Opcode::GetProperty);
                        self.emit_u16(name_idx);
                        match update.operator {
                            UpdateOperator::Increment => self.emit(Opcode::Increment),
                            UpdateOperator::Decrement => self.emit(Opcode::Decrement),
                        }
                        self.emit(Opcode::SetProperty);
                        self.emit_u16(name_idx);
                    } else {
                        // Postfix: obj.x++
                        // 1. Compile obj: [obj]
                        // 2. GetProperty: [value]
                        // 3. Dup: [value, value] (save original for return)
                        // 4. Increment: [orig_value, new_value]
                        // 5. Compile obj: [orig_value, new_value, obj]
                        // 6. Swap: [orig_value, obj, new_value]
                        // 7. SetProperty: [orig_value, new_value] (SetProperty returns the value)
                        // 8. Pop: [orig_value]
                        self.compile_expr(&member.object)?;
                        self.emit(Opcode::GetProperty);
                        self.emit_u16(name_idx);
                        self.emit(Opcode::Dup);
                        match update.operator {
                            UpdateOperator::Increment => self.emit(Opcode::Increment),
                            UpdateOperator::Decrement => self.emit(Opcode::Decrement),
                        }
                        self.compile_expr(&member.object)?;
                        self.emit(Opcode::Swap);
                        self.emit(Opcode::SetProperty);
                        self.emit_u16(name_idx);
                        self.emit(Opcode::Pop);
                    }
                }
                MemberProperty::Expression(key_expr) => {
                    // Handle obj[expr]++ - similar approach but with SetElement
                    if update.prefix {
                        self.compile_expr(&member.object)?;
                        self.emit(Opcode::Dup);
                        self.compile_expr(key_expr)?;
                        self.emit(Opcode::GetElement);
                        match update.operator {
                            UpdateOperator::Increment => self.emit(Opcode::Increment),
                            UpdateOperator::Decrement => self.emit(Opcode::Decrement),
                        }
                        // Now we have [obj, new_value], need to set element
                        // But SetElement expects [obj, key, value]
                        // We need to re-get the key
                        self.compile_expr(key_expr)?;
                        self.emit(Opcode::Swap);
                        self.emit(Opcode::SetElement);
                    } else {
                        // Postfix: obj[key]++ returns original, stores incremented
                        // Approach: evaluate twice for correctness (key may have side effects)
                        // 1. Get original value to return
                        self.compile_expr(&member.object)?;
                        self.compile_expr(key_expr)?;
                        self.emit(Opcode::GetElement);

                        // 2. Store incremented value back
                        self.compile_expr(&member.object)?;
                        self.compile_expr(key_expr)?;
                        self.compile_expr(&member.object)?;
                        self.compile_expr(key_expr)?;
                        self.emit(Opcode::GetElement);
                        match update.operator {
                            UpdateOperator::Increment => self.emit(Opcode::Increment),
                            UpdateOperator::Decrement => self.emit(Opcode::Decrement),
                        }
                        self.emit(Opcode::SetElement);
                        self.emit(Opcode::Pop); // Pop SetElement result, leaving original
                    }
                }
                MemberProperty::PrivateName(name) => {
                    // Handle this.#field++ - similar to identifier but with private field opcodes
                    let name_idx = self.chunk.add_constant(Value::String(name.clone()));

                    if update.prefix {
                        self.compile_expr(&member.object)?;
                        self.emit(Opcode::Dup);
                        self.emit(Opcode::GetPrivateField);
                        self.emit_u16(name_idx);
                        match update.operator {
                            UpdateOperator::Increment => self.emit(Opcode::Increment),
                            UpdateOperator::Decrement => self.emit(Opcode::Decrement),
                        }
                        self.emit(Opcode::SetPrivateField);
                        self.emit_u16(name_idx);
                    } else {
                        self.compile_expr(&member.object)?;
                        self.emit(Opcode::GetPrivateField);
                        self.emit_u16(name_idx);
                        self.emit(Opcode::Dup);
                        match update.operator {
                            UpdateOperator::Increment => self.emit(Opcode::Increment),
                            UpdateOperator::Decrement => self.emit(Opcode::Decrement),
                        }
                        self.compile_expr(&member.object)?;
                        self.emit(Opcode::Swap);
                        self.emit(Opcode::SetPrivateField);
                        self.emit_u16(name_idx);
                        self.emit(Opcode::Pop);
                    }
                }
            }
        } else {
            // For other expressions, just evaluate and apply increment/decrement
            // (This doesn't properly store the result, but is a fallback)
            self.compile_expr(&update.argument)?;
            match update.operator {
                UpdateOperator::Increment => self.emit(Opcode::Increment),
                UpdateOperator::Decrement => self.emit(Opcode::Decrement),
            }
        }
        Ok(())
    }

    fn compile_binary(&mut self, binary: &BinaryExpression) -> Result<()> {
        self.compile_expr(&binary.left)?;
        self.compile_expr(&binary.right)?;

        match binary.operator {
            BinaryOperator::Add => self.emit(Opcode::Add),
            BinaryOperator::Sub => self.emit(Opcode::Sub),
            BinaryOperator::Mul => self.emit(Opcode::Mul),
            BinaryOperator::Div => self.emit(Opcode::Div),
            BinaryOperator::Mod => self.emit(Opcode::Mod),
            BinaryOperator::Pow => self.emit(Opcode::Pow),
            BinaryOperator::Eq => self.emit(Opcode::Eq),
            BinaryOperator::Ne => self.emit(Opcode::Ne),
            BinaryOperator::StrictEq => self.emit(Opcode::StrictEq),
            BinaryOperator::StrictNe => self.emit(Opcode::StrictNe),
            BinaryOperator::Lt => self.emit(Opcode::Lt),
            BinaryOperator::Le => self.emit(Opcode::Le),
            BinaryOperator::Gt => self.emit(Opcode::Gt),
            BinaryOperator::Ge => self.emit(Opcode::Ge),
            BinaryOperator::Shl => self.emit(Opcode::Shl),
            BinaryOperator::Shr => self.emit(Opcode::Shr),
            BinaryOperator::UShr => self.emit(Opcode::UShr),
            BinaryOperator::BitwiseAnd => self.emit(Opcode::BitwiseAnd),
            BinaryOperator::BitwiseOr => self.emit(Opcode::BitwiseOr),
            BinaryOperator::BitwiseXor => self.emit(Opcode::BitwiseXor),
            BinaryOperator::In => self.emit(Opcode::In),
            BinaryOperator::Instanceof => self.emit(Opcode::Instanceof),
        }
        Ok(())
    }

    fn compile_logical(&mut self, logical: &LogicalExpression) -> Result<()> {
        self.compile_expr(&logical.left)?;

        match logical.operator {
            LogicalOperator::And => {
                let jump = self.emit_jump(Opcode::JumpIfFalse);
                self.emit(Opcode::Pop);
                self.compile_expr(&logical.right)?;
                self.patch_jump(jump);
            }
            LogicalOperator::Or => {
                let jump = self.emit_jump(Opcode::JumpIfTrue);
                self.emit(Opcode::Pop);
                self.compile_expr(&logical.right)?;
                self.patch_jump(jump);
            }
            LogicalOperator::NullishCoalescing => {
                let jump = self.emit_jump(Opcode::JumpIfNotNull);
                self.emit(Opcode::Pop);
                self.compile_expr(&logical.right)?;
                self.patch_jump(jump);
            }
        }
        Ok(())
    }

    fn compile_assignment(&mut self, assignment: &AssignmentExpression) -> Result<()> {
        match &assignment.left {
            AssignmentTarget::Simple(expr) => {
                if let Expression::Identifier(id) = expr {
                    // Compile right-hand side
                    if assignment.operator != AssignmentOperator::Assign {
                        self.compile_identifier(id)?;
                    }

                    self.compile_expr(&assignment.right)?;

                    // Apply compound operator
                    self.emit_compound_operator(&assignment.operator);

                    // Store
                    self.emit(Opcode::Dup); // Keep value on stack as result

                    if let Some(slot) = self.resolve_local(&id.name) {
                        self.emit(Opcode::SetLocal);
                        self.emit_byte(slot);
                    } else {
                        let name_idx = self.chunk.add_constant(Value::String(id.name.clone()));
                        self.emit(Opcode::SetGlobal);
                        self.emit_u16(name_idx);
                    }
                } else if let Expression::Member(member) = expr {
                    // Compile object and property
                    self.compile_expr(&member.object)?;

                    let is_compound = assignment.operator != AssignmentOperator::Assign;

                    match &member.property {
                        MemberProperty::Identifier(id) => {
                            let name_idx = self.chunk.add_constant(Value::String(id.name.clone()));

                            if is_compound {
                                // For compound assignment: get current value first
                                self.emit(Opcode::Dup); // Duplicate object for later SetProperty
                                self.emit(Opcode::GetProperty);
                                self.emit_u16(name_idx);
                            }

                            self.compile_expr(&assignment.right)?;

                            if is_compound {
                                // Apply compound operator
                                self.emit_compound_operator(&assignment.operator);
                            }

                            self.emit(Opcode::SetProperty);
                            self.emit_u16(name_idx);
                        }
                        MemberProperty::Expression(key_expr) => {
                            if is_compound {
                                // For compound assignment (obj[key] += value):
                                // Stack starts with [obj] from line 2275
                                self.emit(Opcode::Dup); // [obj, obj]
                                self.compile_expr(key_expr)?; // [obj, obj, key]
                                self.emit(Opcode::GetElement); // [obj, currentValue]

                                // Compile RHS and apply operator
                                self.compile_expr(&assignment.right)?; // [obj, currentValue, rhs]
                                self.emit_compound_operator(&assignment.operator); // [obj, newValue]

                                // Store back: SetElement expects [obj, key, value]
                                self.compile_expr(key_expr)?; // [obj, newValue, key]
                                self.emit(Opcode::Swap); // [obj, key, newValue]
                                self.emit(Opcode::SetElement); // [newValue]
                            } else {
                                // Simple assignment: obj[key] = value
                                self.compile_expr(key_expr)?;
                                self.compile_expr(&assignment.right)?;
                                self.emit(Opcode::SetElement);
                            }
                        }
                        MemberProperty::PrivateName(name) => {
                            let name_idx = self.chunk.add_constant(Value::String(name.clone()));

                            if is_compound {
                                // For compound assignment: get current value first
                                self.emit(Opcode::Dup); // Duplicate object for later SetPrivateField
                                self.emit(Opcode::GetPrivateField);
                                self.emit_u16(name_idx);
                            }

                            self.compile_expr(&assignment.right)?;

                            if is_compound {
                                // Apply compound operator
                                self.emit_compound_operator(&assignment.operator);
                            }

                            self.emit(Opcode::SetPrivateField);
                            self.emit_u16(name_idx);
                        }
                    }
                } else {
                    self.compile_expr(&assignment.right)?;
                }
            }
            AssignmentTarget::Pattern(pat) => {
                // Destructuring assignment: [a, b] = arr, {x, y} = obj
                // Compile the RHS, then assign to each variable in the pattern
                self.compile_expr(&assignment.right)?;
                self.emit(Opcode::Dup); // Keep value on stack as result of assignment
                self.compile_pattern_assignment(pat)?;
            }
        }
        Ok(())
    }

    fn compile_conditional(&mut self, cond: &ConditionalExpression) -> Result<()> {
        self.compile_expr(&cond.test)?;

        let else_jump = self.emit_jump(Opcode::JumpIfFalse);
        self.emit(Opcode::Pop);
        self.compile_expr(&cond.consequent)?;
        let end_jump = self.emit_jump(Opcode::Jump);

        self.patch_jump(else_jump);
        self.emit(Opcode::Pop);
        self.compile_expr(&cond.alternate)?;

        self.patch_jump(end_jump);
        Ok(())
    }

    fn compile_sequence(&mut self, seq: &SequenceExpression) -> Result<()> {
        for (i, expr) in seq.expressions.iter().enumerate() {
            self.compile_expr(expr)?;
            if i < seq.expressions.len() - 1 {
                self.emit(Opcode::Pop);
            }
        }
        Ok(())
    }

    fn compile_template_literal(&mut self, template: &TemplateLiteral) -> Result<()> {
        // Template literals are compiled as string concatenations
        // For `Hello ${name}!`, we generate:
        //   push "Hello "
        //   push name
        //   concat
        //   push "!"
        //   concat

        let mut first = true;

        for (i, quasi) in template.quasis.iter().enumerate() {
            // Get the cooked string value
            let text = quasi.cooked.clone().unwrap_or_default();

            // Only push non-empty strings
            if !text.is_empty() {
                self.emit_constant(Value::String(text));
                if !first {
                    self.emit(Opcode::Add);
                }
                first = false;
            }

            // If there's a corresponding expression, compile it
            if i < template.expressions.len() {
                self.compile_expr(&template.expressions[i])?;
                if !first {
                    self.emit(Opcode::Add);
                }
                first = false;
            }
        }

        // If template was empty, push empty string
        if first {
            self.emit_constant(Value::String(String::new()));
        }

        Ok(())
    }

    /// Extract declared names from a statement (for exports)
    fn extract_declaration_names(&self, stmt: &Statement) -> Vec<String> {
        let mut names = Vec::new();
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    self.extract_pattern_names(&declarator.id, &mut names);
                }
            }
            Statement::FunctionDeclaration(func) => {
                if let Some(id) = &func.id {
                    names.push(id.name.clone());
                }
            }
            Statement::ClassDeclaration(class) => {
                if let Some(id) = &class.id {
                    names.push(id.name.clone());
                }
            }
            _ => {}
        }
        names
    }

    /// Extract variable names from a pattern (for destructuring)
    fn extract_pattern_names(&self, pattern: &crate::ast::Pattern, names: &mut Vec<String>) {
        match pattern {
            crate::ast::Pattern::Identifier(id) => {
                names.push(id.name.clone());
            }
            crate::ast::Pattern::Object(obj) => {
                for prop in &obj.properties {
                    match prop {
                        crate::ast::ObjectPatternProperty::Property { value, .. } => {
                            self.extract_pattern_names(value, names);
                        }
                        crate::ast::ObjectPatternProperty::Rest { argument, .. } => {
                            self.extract_pattern_names(argument, names);
                        }
                    }
                }
            }
            crate::ast::Pattern::Array(arr) => {
                for elem in arr.elements.iter().flatten() {
                    self.extract_pattern_names(elem, names);
                }
            }
            crate::ast::Pattern::Assignment(assignment) => {
                self.extract_pattern_names(&assignment.left, names);
            }
            crate::ast::Pattern::Rest(rest) => {
                self.extract_pattern_names(&rest.argument, names);
            }
            crate::ast::Pattern::Member(_) => {
                // Member expressions don't bind names in declarations
            }
        }
    }

    /// Try to evaluate an expression to a Value at compile time.
    /// Returns Some(Value) for literals and simple expressions, None for complex expressions.
    fn try_eval_literal(&self, expr: &Expression) -> Option<Value> {
        match expr {
            Expression::Literal(lit) => {
                match &lit.value {
                    LiteralValue::Null => Some(Value::Null),
                    LiteralValue::Boolean(b) => Some(Value::Boolean(*b)),
                    LiteralValue::Number(n) => Some(Value::Number(*n)),
                    LiteralValue::String(s) => Some(Value::String(s.clone())),
                    LiteralValue::BigInt(_) => None, // BigInt needs runtime handling
                    LiteralValue::Regex { .. } => None, // RegExp needs runtime handling
                }
            }
            Expression::Identifier(id) if id.name == "undefined" => Some(Value::Undefined),
            Expression::Unary(unary) => {
                // Handle simple unary expressions like -1
                use crate::ast::UnaryOperator;
                if let Some(operand) = self.try_eval_literal(&unary.argument) {
                    match unary.operator {
                        UnaryOperator::Minus => {
                            if let Value::Number(n) = operand {
                                return Some(Value::Number(-n));
                            }
                        }
                        UnaryOperator::Plus => {
                            if let Value::Number(n) = operand {
                                return Some(Value::Number(n));
                            }
                        }
                        UnaryOperator::Not => {
                            return Some(Value::Boolean(!operand.to_boolean()));
                        }
                        _ => {}
                    }
                }
                None
            }
            _ => None, // Complex expressions need runtime evaluation
        }
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Compile source code to bytecode
pub fn compile(source: &str) -> Result<Chunk> {
    let program = crate::parser::parse(source)?;
    let mut compiler = Compiler::new();
    compiler.compile_program(&program)
}

/// Compile source code to bytecode with source file information for source maps
pub fn compile_with_source_file(source: &str, source_file: &str) -> Result<Chunk> {
    let program = crate::parser::parse(source)?;
    let mut compiler = Compiler::with_source_file(source_file);
    compiler.compile_program(&program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_literals() {
        let chunk = compile("42;").unwrap();
        assert!(!chunk.code.is_empty());
    }

    #[test]
    fn test_compile_binary() {
        let chunk = compile("1 + 2;").unwrap();
        let disasm = chunk.disassemble("test");
        assert!(disasm.contains("Add"));
    }

    #[test]
    fn test_compile_variable() {
        let chunk = compile("let x = 10; x;").unwrap();
        let disasm = chunk.disassemble("test");
        assert!(disasm.contains("Constant") || disasm.contains("GetLocal"));
    }

    #[test]
    fn test_compile_if() {
        let chunk = compile("if (true) { 1; } else { 2; }").unwrap();
        let disasm = chunk.disassemble("test");
        assert!(disasm.contains("JumpIfFalse"));
    }

    #[test]
    fn test_compile_while() {
        let chunk = compile("while (true) { break; }").unwrap();
        let disasm = chunk.disassemble("test");
        assert!(disasm.contains("Jump"));
    }
}
