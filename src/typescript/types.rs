//! TypeScript AST types and structures
//!
//! This module defines the AST nodes specific to TypeScript that need to be
//! handled during transpilation.

/// TypeScript type annotation
#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnnotation {
    /// Primitive types: string, number, boolean, etc.
    Primitive(PrimitiveType),
    /// Array type: T[]
    Array(Box<TypeAnnotation>),
    /// Generic array: Array<T>
    GenericArray(Box<TypeAnnotation>),
    /// Tuple type: [T, U, V]
    Tuple(Vec<TypeAnnotation>),
    /// Union type: T | U
    Union(Vec<TypeAnnotation>),
    /// Intersection type: T & U
    Intersection(Vec<TypeAnnotation>),
    /// Object type: { key: Type }
    Object(Vec<TypeMember>),
    /// Function type: (args) => ReturnType
    Function(FunctionType),
    /// Type reference: SomeType or Generic<T>
    Reference(TypeReference),
    /// Literal type: "hello" | 42 | true
    Literal(LiteralType),
    /// Conditional type: T extends U ? X : Y
    Conditional(Box<ConditionalType>),
    /// Indexed access: T[K]
    IndexedAccess(Box<IndexedAccessType>),
    /// Mapped type: { [K in keyof T]: T[K] }
    Mapped(Box<MappedType>),
    /// Template literal type: `${T}`
    TemplateLiteral(Vec<TemplateLiteralSpan>),
    /// Infer type: infer U
    Infer(String),
    /// Keyof type: keyof T
    Keyof(Box<TypeAnnotation>),
    /// Typeof type: typeof x
    Typeof(String),
    /// Type predicate: x is T
    TypePredicate(Box<TypePredicate>),
    /// Parenthesized: (T)
    Parenthesized(Box<TypeAnnotation>),
    /// Optional type: T?
    Optional(Box<TypeAnnotation>),
    /// Rest type: ...T
    Rest(Box<TypeAnnotation>),
    /// Any
    Any,
    /// Unknown
    Unknown,
    /// Void
    Void,
    /// Never
    Never,
    /// Null
    Null,
    /// Undefined
    Undefined,
    /// This type
    This,
}

/// Primitive TypeScript types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveType {
    String,
    Number,
    Boolean,
    BigInt,
    Symbol,
    Object,
}

/// A member of a type object
#[derive(Debug, Clone, PartialEq)]
pub struct TypeMember {
    /// Property name (or index signature)
    pub name: TypeMemberName,
    /// Optional?
    pub optional: bool,
    /// Readonly?
    pub readonly: bool,
    /// Type annotation
    pub type_annotation: Option<TypeAnnotation>,
}

/// Name of a type member
#[derive(Debug, Clone, PartialEq)]
pub enum TypeMemberName {
    /// Named property
    Identifier(String),
    /// Computed property [expr]
    Computed(String),
    /// Index signature [key: Type]
    IndexSignature(TypeAnnotation),
}

/// Function type signature
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionType {
    /// Type parameters <T, U>
    pub type_params: Vec<TypeParameter>,
    /// Parameters
    pub params: Vec<FunctionParam>,
    /// Return type
    pub return_type: Box<TypeAnnotation>,
}

/// Function parameter in a type
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionParam {
    /// Parameter name
    pub name: String,
    /// Optional?
    pub optional: bool,
    /// Rest parameter?
    pub rest: bool,
    /// Type annotation
    pub type_annotation: Option<TypeAnnotation>,
}

/// Type reference (named type with optional generics)
#[derive(Debug, Clone, PartialEq)]
pub struct TypeReference {
    /// Type name (may be qualified: A.B.C)
    pub name: Vec<String>,
    /// Type arguments
    pub type_args: Vec<TypeAnnotation>,
}

/// Literal type (string, number, or boolean literal used as a type)
#[derive(Debug, Clone, PartialEq)]
pub enum LiteralType {
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
    Undefined,
}

/// Conditional type: T extends U ? X : Y
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionalType {
    pub check_type: TypeAnnotation,
    pub extends_type: TypeAnnotation,
    pub true_type: TypeAnnotation,
    pub false_type: TypeAnnotation,
}

/// Indexed access type: T[K]
#[derive(Debug, Clone, PartialEq)]
pub struct IndexedAccessType {
    pub object_type: TypeAnnotation,
    pub index_type: TypeAnnotation,
}

/// Mapped type: { [K in keyof T]: T[K] }
#[derive(Debug, Clone, PartialEq)]
pub struct MappedType {
    pub type_param: TypeParameter,
    pub constraint: Option<TypeAnnotation>,
    pub template_type: TypeAnnotation,
    pub readonly: MappedTypeModifier,
    pub optional: MappedTypeModifier,
}

/// Modifier for mapped types (+, -, or none)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedTypeModifier {
    None,
    Add,
    Remove,
}

/// Template literal span
#[derive(Debug, Clone, PartialEq)]
pub enum TemplateLiteralSpan {
    Text(String),
    Type(TypeAnnotation),
}

/// Type predicate: x is T
#[derive(Debug, Clone, PartialEq)]
pub struct TypePredicate {
    pub param_name: String,
    pub type_annotation: Box<TypeAnnotation>,
    pub asserts: bool,
}

/// Type parameter (generic parameter)
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParameter {
    /// Parameter name (T, U, etc.)
    pub name: String,
    /// Constraint: T extends Constraint
    pub constraint: Option<Box<TypeAnnotation>>,
    /// Default type: T = Default
    pub default: Option<Box<TypeAnnotation>>,
}

/// Interface declaration
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceDeclaration {
    /// Interface name
    pub name: String,
    /// Type parameters
    pub type_params: Vec<TypeParameter>,
    /// Extended interfaces
    pub extends: Vec<TypeReference>,
    /// Interface members
    pub members: Vec<TypeMember>,
}

/// Type alias declaration
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAliasDeclaration {
    /// Type alias name
    pub name: String,
    /// Type parameters
    pub type_params: Vec<TypeParameter>,
    /// The aliased type
    pub type_annotation: TypeAnnotation,
}

/// Enum declaration
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDeclaration {
    /// Enum name
    pub name: String,
    /// Is this a const enum?
    pub is_const: bool,
    /// Enum members
    pub members: Vec<EnumMember>,
}

/// Enum member
#[derive(Debug, Clone, PartialEq)]
pub struct EnumMember {
    /// Member name
    pub name: String,
    /// Optional initializer expression
    pub initializer: Option<String>,
}

/// Access modifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessModifier {
    Public,
    Private,
    Protected,
}

/// Class member modifiers
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberModifiers {
    pub access: Option<AccessModifier>,
    pub readonly: bool,
    pub abstract_: bool,
    pub static_: bool,
    pub override_: bool,
}

/// Namespace declaration
#[derive(Debug, Clone, PartialEq)]
pub struct NamespaceDeclaration {
    /// Namespace name
    pub name: String,
    /// Body (statements inside)
    pub body: String,
}

/// Import type: import("./module")
#[derive(Debug, Clone, PartialEq)]
pub struct ImportType {
    /// Module specifier
    pub module: String,
    /// Optional qualifier
    pub qualifier: Option<Vec<String>>,
    /// Type arguments
    pub type_args: Vec<TypeAnnotation>,
}

/// Declare statement kind
#[derive(Debug, Clone, PartialEq)]
pub enum DeclareKind {
    Variable,
    Function,
    Class,
    Namespace,
    Module,
    Global,
}

impl std::fmt::Display for PrimitiveType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrimitiveType::String => write!(f, "string"),
            PrimitiveType::Number => write!(f, "number"),
            PrimitiveType::Boolean => write!(f, "boolean"),
            PrimitiveType::BigInt => write!(f, "bigint"),
            PrimitiveType::Symbol => write!(f, "symbol"),
            PrimitiveType::Object => write!(f, "object"),
        }
    }
}
