//! Pattern AST node types for destructuring

use super::*;

/// A binding pattern (used in variable declarations, function parameters, etc.)
#[derive(Debug, Clone)]
pub enum Pattern {
    /// Simple identifier binding
    Identifier(Identifier),

    /// Array destructuring pattern [a, b, c]
    Array(ArrayPattern),

    /// Object destructuring pattern {a, b, c}
    Object(ObjectPattern),

    /// Assignment pattern with default value a = 1
    Assignment(Box<AssignmentPattern>),

    /// Rest pattern ...rest
    Rest(Box<RestPattern>),

    /// Member expression (for assignment targets, not declarations)
    Member(Box<MemberExpression>),
}

impl Pattern {
    /// Get the span of this pattern
    pub fn span(&self) -> Span {
        match self {
            Pattern::Identifier(id) => id.span,
            Pattern::Array(a) => a.span,
            Pattern::Object(o) => o.span,
            Pattern::Assignment(a) => a.span,
            Pattern::Rest(r) => r.span,
            Pattern::Member(m) => m.span,
        }
    }

    /// Get all bound identifiers in this pattern
    pub fn bound_names(&self) -> Vec<&Identifier> {
        let mut names = Vec::new();
        self.collect_bound_names(&mut names);
        names
    }

    fn collect_bound_names<'a>(&'a self, names: &mut Vec<&'a Identifier>) {
        match self {
            Pattern::Identifier(id) => names.push(id),
            Pattern::Array(arr) => {
                for elem in arr.elements.iter().flatten() {
                    elem.collect_bound_names(names);
                }
                if let Some(rest) = &arr.rest {
                    rest.collect_bound_names(names);
                }
            }
            Pattern::Object(obj) => {
                for prop in &obj.properties {
                    match prop {
                        ObjectPatternProperty::Property { value, .. } => {
                            value.collect_bound_names(names);
                        }
                        ObjectPatternProperty::Rest { argument, .. } => {
                            argument.collect_bound_names(names);
                        }
                    }
                }
            }
            Pattern::Assignment(a) => {
                a.left.collect_bound_names(names);
            }
            Pattern::Rest(r) => {
                r.argument.collect_bound_names(names);
            }
            Pattern::Member(_) => {
                // Member expressions don't bind names in declarations
            }
        }
    }
}

/// Array destructuring pattern
#[derive(Debug, Clone)]
pub struct ArrayPattern {
    /// Pattern elements (None for holes)
    pub elements: Vec<Option<Pattern>>,
    /// Rest element
    pub rest: Option<Box<Pattern>>,
    /// Span in source
    pub span: Span,
}

/// Object destructuring pattern
#[derive(Debug, Clone)]
pub struct ObjectPattern {
    /// Pattern properties
    pub properties: Vec<ObjectPatternProperty>,
    /// Span in source
    pub span: Span,
}

/// Object pattern property
#[derive(Debug, Clone)]
pub enum ObjectPatternProperty {
    /// Regular property {key: value} or shorthand {key}
    Property {
        /// Property key
        key: PropertyKey,
        /// Value pattern
        value: Pattern,
        /// Is this a shorthand property?
        shorthand: bool,
        /// Is this a computed property?
        computed: bool,
        /// Span in source
        span: Span,
    },
    /// Rest property {...rest}
    Rest {
        /// Rest argument
        argument: Pattern,
        /// Span in source
        span: Span,
    },
}

/// Assignment pattern with default value
#[derive(Debug, Clone)]
pub struct AssignmentPattern {
    /// Left-hand side pattern
    pub left: Pattern,
    /// Default value expression
    pub right: Expression,
    /// Span in source
    pub span: Span,
}

/// Rest pattern
#[derive(Debug, Clone)]
pub struct RestPattern {
    /// Rest argument pattern
    pub argument: Pattern,
    /// Span in source
    pub span: Span,
}
