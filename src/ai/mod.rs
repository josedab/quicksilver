//! AI-Native Runtime
//!
//! Built-in support for LLM tool calls with automatic schema generation
//! from JSDoc annotations. Seamless integration with AI function calling.
//!
//! # Example
//! ```text
//! /**
//!  * Search for products matching a query
//!  * @param query - The search query
//!  * @param limit - Maximum number of results (default: 10)
//!  * @returns Array of matching products
//!  */
//! function searchProducts(query, limit = 10) {
//!   return db.products.search(query).limit(limit);
//! }
//!
//! // Auto-generates:
//! // {
//! //   "name": "searchProducts",
//! //   "description": "Search for products matching a query",
//! //   "parameters": {
//! //     "type": "object",
//! //     "properties": {
//! //       "query": {"type": "string", "description": "The search query"},
//! //       "limit": {"type": "number", "description": "Maximum number of results (default: 10)"}
//! //     },
//! //     "required": ["query"]
//! //   }
//! // }
//! ```

//! **Status:** ✅ Complete — JSDoc to LLM tool schema generation (OpenAI/Anthropic)

use rustc_hash::FxHashMap as HashMap;

/// JSON Schema types
#[derive(Debug, Clone, PartialEq)]
pub enum JsonSchemaType {
    String,
    Number,
    Integer,
    Boolean,
    Array(Box<JsonSchemaType>),
    Object(HashMap<String, JsonSchemaProperty>),
    Null,
    Any,
}

impl JsonSchemaType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Integer => "integer",
            Self::Boolean => "boolean",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
            Self::Null => "null",
            Self::Any => "any",
        }
    }
}

/// A property in a JSON Schema object
#[derive(Debug, Clone, PartialEq)]
pub struct JsonSchemaProperty {
    pub schema_type: JsonSchemaType,
    pub description: Option<String>,
    pub default: Option<String>,
    pub required: bool,
    pub enum_values: Option<Vec<String>>,
}

impl JsonSchemaProperty {
    pub fn new(schema_type: JsonSchemaType) -> Self {
        Self {
            schema_type,
            description: None,
            default: None,
            required: true,
            enum_values: None,
        }
    }

    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }

    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    pub fn with_default(mut self, default: &str) -> Self {
        self.default = Some(default.to_string());
        self.required = false;
        self
    }
}

/// A tool definition that can be called by an LLM
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ToolParameter>,
    pub returns: Option<ToolReturn>,
}

/// A parameter for a tool
#[derive(Debug, Clone)]
pub struct ToolParameter {
    pub name: String,
    pub param_type: JsonSchemaType,
    pub description: String,
    pub required: bool,
    pub default: Option<String>,
}

/// Return type description
#[derive(Debug, Clone)]
pub struct ToolReturn {
    pub return_type: JsonSchemaType,
    pub description: String,
}

impl ToolDefinition {
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            parameters: Vec::new(),
            returns: None,
        }
    }

    pub fn add_param(mut self, param: ToolParameter) -> Self {
        self.parameters.push(param);
        self
    }

    pub fn with_return(mut self, returns: ToolReturn) -> Self {
        self.returns = Some(returns);
        self
    }

    /// Convert to JSON schema format (OpenAI function calling compatible)
    pub fn to_json_schema(&self) -> String {
        let mut json = String::from("{\n");
        json.push_str(&format!("  \"name\": \"{}\",\n", self.name));
        json.push_str(&format!("  \"description\": \"{}\",\n", self.description));
        json.push_str("  \"parameters\": {\n");
        json.push_str("    \"type\": \"object\",\n");
        json.push_str("    \"properties\": {\n");

        for (i, param) in self.parameters.iter().enumerate() {
            json.push_str(&format!("      \"{}\": {{\n", param.name));
            json.push_str(&format!("        \"type\": \"{}\",\n", param.param_type.as_str()));
            json.push_str(&format!("        \"description\": \"{}\"", param.description));
            if let Some(default) = &param.default {
                json.push_str(&format!(",\n        \"default\": {}", default));
            }
            json.push_str("\n      }");
            if i < self.parameters.len() - 1 {
                json.push(',');
            }
            json.push('\n');
        }

        json.push_str("    },\n");

        // Required parameters
        let required: Vec<_> = self.parameters.iter()
            .filter(|p| p.required)
            .map(|p| format!("\"{}\"", p.name))
            .collect();
        json.push_str(&format!("    \"required\": [{}]\n", required.join(", ")));

        json.push_str("  }\n");
        json.push('}');
        json
    }

    /// Convert to Anthropic Claude tool format
    pub fn to_anthropic_format(&self) -> String {
        let mut json = String::from("{\n");
        json.push_str(&format!("  \"name\": \"{}\",\n", self.name));
        json.push_str(&format!("  \"description\": \"{}\",\n", self.description));
        json.push_str("  \"input_schema\": {\n");
        json.push_str("    \"type\": \"object\",\n");
        json.push_str("    \"properties\": {\n");

        for (i, param) in self.parameters.iter().enumerate() {
            json.push_str(&format!("      \"{}\": {{\n", param.name));
            json.push_str(&format!("        \"type\": \"{}\",\n", param.param_type.as_str()));
            json.push_str(&format!("        \"description\": \"{}\"", param.description));
            json.push_str("\n      }");
            if i < self.parameters.len() - 1 {
                json.push(',');
            }
            json.push('\n');
        }

        json.push_str("    },\n");

        let required: Vec<_> = self.parameters.iter()
            .filter(|p| p.required)
            .map(|p| format!("\"{}\"", p.name))
            .collect();
        json.push_str(&format!("    \"required\": [{}]\n", required.join(", ")));

        json.push_str("  }\n");
        json.push('}');
        json
    }
}

/// Parser for JSDoc comments
pub struct JsDocParser;

impl JsDocParser {
    /// Parse a JSDoc comment and extract tool definition
    pub fn parse(jsdoc: &str, function_name: &str) -> Option<ToolDefinition> {
        let mut description = String::new();
        let mut params = Vec::new();
        let mut returns = None;

        for line in jsdoc.lines() {
            let line = line.trim().trim_start_matches('*').trim();

            if line.starts_with("@param") {
                if let Some(param) = Self::parse_param(line) {
                    params.push(param);
                }
            } else if line.starts_with("@returns") || line.starts_with("@return") {
                returns = Self::parse_returns(line);
            } else if !line.is_empty() && !line.starts_with('@') && !line.starts_with('/') {
                if !description.is_empty() {
                    description.push(' ');
                }
                description.push_str(line);
            }
        }

        if description.is_empty() {
            return None;
        }

        let mut tool = ToolDefinition::new(function_name, &description);
        for param in params {
            tool = tool.add_param(param);
        }
        if let Some(ret) = returns {
            tool = tool.with_return(ret);
        }

        Some(tool)
    }

    fn parse_param(line: &str) -> Option<ToolParameter> {
        // Parse: @param {type} name - description
        // or: @param name - description
        let content = line.strip_prefix("@param")?.trim();

        let (param_type, rest) = if content.starts_with('{') {
            let end = content.find('}')?;
            let type_str = &content[1..end];
            (Self::parse_type(type_str), content[end + 1..].trim())
        } else {
            (JsonSchemaType::Any, content)
        };

        let (name, description) = if let Some(dash_pos) = rest.find(" - ") {
            (rest[..dash_pos].trim(), rest[dash_pos + 3..].trim())
        } else {
            let parts: Vec<_> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                (parts[0], parts[1])
            } else {
                (rest, "")
            }
        };

        // Check for default value in description
        let (description, default, required) = if description.contains("default:") || description.contains("(default") {
            let default_regex = regex::Regex::new(r"\(default[:\s]+([^)]+)\)").ok();
            if let Some(captures) = default_regex.and_then(|r| r.captures(description)) {
                let default_val = captures.get(1).map(|m| m.as_str().to_string());
                (description.to_string(), default_val, false)
            } else {
                (description.to_string(), None, true)
            }
        } else {
            (description.to_string(), None, true)
        };

        Some(ToolParameter {
            name: name.to_string(),
            param_type,
            description,
            required,
            default,
        })
    }

    fn parse_returns(line: &str) -> Option<ToolReturn> {
        let content = line.strip_prefix("@returns")
            .or_else(|| line.strip_prefix("@return"))?
            .trim();

        let (return_type, description) = if content.starts_with('{') {
            let end = content.find('}')?;
            let type_str = &content[1..end];
            (Self::parse_type(type_str), content[end + 1..].trim())
        } else {
            (JsonSchemaType::Any, content)
        };

        Some(ToolReturn {
            return_type,
            description: description.to_string(),
        })
    }

    fn parse_type(type_str: &str) -> JsonSchemaType {
        match type_str.to_lowercase().as_str() {
            "string" => JsonSchemaType::String,
            "number" => JsonSchemaType::Number,
            "integer" | "int" => JsonSchemaType::Integer,
            "boolean" | "bool" => JsonSchemaType::Boolean,
            "null" => JsonSchemaType::Null,
            "object" => JsonSchemaType::Object(HashMap::default()),
            s if s.ends_with("[]") => {
                let inner = &s[..s.len() - 2];
                JsonSchemaType::Array(Box::new(Self::parse_type(inner)))
            }
            s if s.starts_with("array<") && s.ends_with('>') => {
                let inner = &s[6..s.len() - 1];
                JsonSchemaType::Array(Box::new(Self::parse_type(inner)))
            }
            _ => JsonSchemaType::Any,
        }
    }
}

/// Registry of AI-callable tools
#[derive(Debug, Default)]
pub struct ToolRegistry {
    tools: HashMap<String, ToolDefinition>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::default(),
        }
    }

    pub fn register(&mut self, tool: ToolDefinition) {
        self.tools.insert(tool.name.clone(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.get(name)
    }

    pub fn list(&self) -> Vec<&ToolDefinition> {
        self.tools.values().collect()
    }

    /// Export all tools as JSON schema array
    pub fn export_json_schema(&self) -> String {
        let schemas: Vec<_> = self.tools.values()
            .map(|t| t.to_json_schema())
            .collect();
        format!("[\n{}\n]", schemas.join(",\n"))
    }

    /// Export all tools in Anthropic format
    pub fn export_anthropic(&self) -> String {
        let schemas: Vec<_> = self.tools.values()
            .map(|t| t.to_anthropic_format())
            .collect();
        format!("[\n{}\n]", schemas.join(",\n"))
    }
}

/// Tool call from an LLM
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
}

/// Tool call result
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub id: String,
    pub content: String,
    pub is_error: bool,
}

impl ToolResult {
    pub fn success(id: &str, content: &str) -> Self {
        Self {
            id: id.to_string(),
            content: content.to_string(),
            is_error: false,
        }
    }

    pub fn error(id: &str, error: &str) -> Self {
        Self {
            id: id.to_string(),
            content: error.to_string(),
            is_error: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jsdoc_parsing() {
        let jsdoc = r#"
        /**
         * Search for products matching a query
         * @param {string} query - The search query
         * @param {number} limit - Maximum number of results (default: 10)
         * @returns {Array<object>} Array of matching products
         */
        "#;

        let tool = JsDocParser::parse(jsdoc, "searchProducts").unwrap();
        assert_eq!(tool.name, "searchProducts");
        assert_eq!(tool.parameters.len(), 2);
        assert_eq!(tool.parameters[0].name, "query");
        assert!(matches!(tool.parameters[0].param_type, JsonSchemaType::String));
    }

    #[test]
    fn test_tool_definition_to_json() {
        let tool = ToolDefinition::new("getWeather", "Get current weather for a location")
            .add_param(ToolParameter {
                name: "location".to_string(),
                param_type: JsonSchemaType::String,
                description: "City name".to_string(),
                required: true,
                default: None,
            })
            .add_param(ToolParameter {
                name: "units".to_string(),
                param_type: JsonSchemaType::String,
                description: "Temperature units".to_string(),
                required: false,
                default: Some("\"celsius\"".to_string()),
            });

        let json = tool.to_json_schema();
        assert!(json.contains("getWeather"));
        assert!(json.contains("location"));
        assert!(json.contains("\"required\": [\"location\"]"));
    }

    #[test]
    fn test_tool_registry() {
        let mut registry = ToolRegistry::new();

        registry.register(ToolDefinition::new("tool1", "First tool"));
        registry.register(ToolDefinition::new("tool2", "Second tool"));

        assert_eq!(registry.list().len(), 2);
        assert!(registry.get("tool1").is_some());
        assert!(registry.get("nonexistent").is_none());
    }
}
