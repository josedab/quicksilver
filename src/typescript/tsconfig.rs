//! TSConfig Support
//!
//! This module provides parsing and validation of TypeScript configuration
//! files (tsconfig.json), converting them to transpiler options.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{JsTarget, ModuleSystem, SourceMapOption, TranspileOptions};
use crate::error::{Error, Result};

/// TypeScript configuration parsed from tsconfig.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsConfig {
    /// Compiler options
    #[serde(default, rename = "compilerOptions")]
    pub compiler_options: CompilerOptions,
    /// Include glob patterns
    #[serde(default)]
    pub include: Vec<String>,
    /// Exclude glob patterns
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Path to a base tsconfig to extend
    #[serde(default)]
    pub extends: Option<String>,
}

/// TypeScript compiler options from tsconfig.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CompilerOptions {
    pub target: Option<String>,
    pub module: Option<String>,
    pub strict: Option<bool>,
    pub strict_null_checks: Option<bool>,
    pub no_implicit_any: Option<bool>,
    pub no_implicit_returns: Option<bool>,
    pub source_map: Option<bool>,
    pub declaration: Option<bool>,
    pub jsx: Option<String>,
    pub out_dir: Option<String>,
    pub root_dir: Option<String>,
    pub base_url: Option<String>,
    pub paths: Option<HashMap<String, Vec<String>>>,
    pub types: Option<Vec<String>>,
    pub lib: Option<Vec<String>>,
    pub es_module_interop: Option<bool>,
    pub allow_js: Option<bool>,
    pub skip_lib_check: Option<bool>,
}

impl TsConfig {
    /// Load a TsConfig from a JSON string (typically the contents of tsconfig.json)
    pub fn load(json: &str) -> Result<Self> {
        serde_json::from_str(json)
            .map_err(|e| Error::InternalError(format!("Failed to parse tsconfig.json: {}", e)))
    }

    /// Return a TsConfig with sensible defaults
    pub fn default_config() -> Self {
        Self {
            compiler_options: CompilerOptions {
                target: Some("ES2020".to_string()),
                module: Some("ESNext".to_string()),
                strict: Some(true),
                strict_null_checks: Some(true),
                no_implicit_any: Some(true),
                no_implicit_returns: Some(false),
                source_map: Some(false),
                declaration: Some(false),
                jsx: None,
                out_dir: Some("./dist".to_string()),
                root_dir: Some("./src".to_string()),
                base_url: None,
                paths: None,
                types: None,
                lib: None,
                es_module_interop: Some(true),
                allow_js: Some(false),
                skip_lib_check: Some(true),
            },
            include: vec!["src/**/*.ts".to_string()],
            exclude: vec![
                "node_modules".to_string(),
                "dist".to_string(),
            ],
            extends: None,
        }
    }

    /// Convert this TsConfig into TranspileOptions for the transpiler
    pub fn to_transpile_options(&self) -> TranspileOptions {
        let target = self
            .compiler_options
            .target
            .as_deref()
            .map(parse_js_target)
            .unwrap_or(JsTarget::ES2020);

        let module = self
            .compiler_options
            .module
            .as_deref()
            .map(parse_module_system)
            .unwrap_or(ModuleSystem::ESNext);

        let source_map = if self.compiler_options.source_map == Some(true) {
            SourceMapOption::External
        } else {
            SourceMapOption::None
        };

        TranspileOptions {
            target,
            preserve_const_enums: false,
            emit_decorator_metadata: false,
            use_define_for_class_fields: true,
            module,
            source_map,
            remove_comments: false,
        }
    }

    /// Validate the configuration and return a list of warnings
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if let Some(ref target) = self.compiler_options.target {
            let normalized = target.to_uppercase();
            let valid = [
                "ES3", "ES5", "ES2015", "ES2016", "ES2017", "ES2018", "ES2019",
                "ES2020", "ES2021", "ES2022", "ESNEXT",
            ];
            if !valid.contains(&normalized.as_str()) {
                warnings.push(format!("Unknown target: '{}'. Expected one of: ES5, ES2015-ES2022, ESNext", target));
            }
        }

        if let Some(ref module) = self.compiler_options.module {
            let normalized = module.to_uppercase();
            let valid = ["COMMONJS", "AMD", "UMD", "ESNEXT", "ES2015", "ES2020", "ES2022", "NONE", "NODE16", "NODENEXT"];
            if !valid.contains(&normalized.as_str()) {
                warnings.push(format!("Unknown module system: '{}'. Expected one of: CommonJS, AMD, UMD, ESNext, None", module));
            }
        }

        if let Some(ref jsx) = self.compiler_options.jsx {
            let normalized = jsx.to_lowercase();
            let valid = ["preserve", "react", "react-jsx", "react-jsxdev", "react-native"];
            if !valid.contains(&normalized.as_str()) {
                warnings.push(format!("Unknown jsx option: '{}'. Expected one of: preserve, react, react-jsx, react-jsxdev, react-native", jsx));
            }
        }

        if self.compiler_options.strict == Some(true) {
            if self.compiler_options.strict_null_checks == Some(false) {
                warnings.push("strictNullChecks is false but strict is true; strict implies strictNullChecks".to_string());
            }
            if self.compiler_options.no_implicit_any == Some(false) {
                warnings.push("noImplicitAny is false but strict is true; strict implies noImplicitAny".to_string());
            }
        }

        if self.include.is_empty() && self.compiler_options.root_dir.is_none() {
            warnings.push("No 'include' patterns or 'rootDir' specified".to_string());
        }

        warnings
    }
}

/// Parse a target string into JsTarget
fn parse_js_target(s: &str) -> JsTarget {
    match s.to_uppercase().as_str() {
        "ES5" => JsTarget::ES5,
        "ES2015" | "ES6" => JsTarget::ES2015,
        "ES2016" => JsTarget::ES2016,
        "ES2017" => JsTarget::ES2017,
        "ES2018" => JsTarget::ES2018,
        "ES2019" => JsTarget::ES2019,
        "ES2020" => JsTarget::ES2020,
        "ES2021" => JsTarget::ES2021,
        "ES2022" => JsTarget::ES2022,
        "ESNEXT" => JsTarget::ESNext,
        _ => JsTarget::ES2020,
    }
}

/// Parse a module string into ModuleSystem
fn parse_module_system(s: &str) -> ModuleSystem {
    match s.to_uppercase().as_str() {
        "COMMONJS" => ModuleSystem::CommonJS,
        "AMD" => ModuleSystem::AMD,
        "UMD" => ModuleSystem::UMD,
        "ESNEXT" | "ES2015" | "ES2020" | "ES2022" | "NODE16" | "NODENEXT" => ModuleSystem::ESNext,
        "NONE" => ModuleSystem::None,
        _ => ModuleSystem::ESNext,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_tsconfig_from_json() {
        let json = r#"{
            "compilerOptions": {
                "target": "ES2020",
                "module": "ESNext",
                "strict": true,
                "outDir": "./dist"
            },
            "include": ["src/**/*.ts"],
            "exclude": ["node_modules"]
        }"#;

        let config = TsConfig::load(json).unwrap();
        assert_eq!(config.compiler_options.target.as_deref(), Some("ES2020"));
        assert_eq!(config.compiler_options.module.as_deref(), Some("ESNext"));
        assert_eq!(config.compiler_options.strict, Some(true));
        assert_eq!(config.compiler_options.out_dir.as_deref(), Some("./dist"));
        assert_eq!(config.include, vec!["src/**/*.ts"]);
        assert_eq!(config.exclude, vec!["node_modules"]);
    }

    #[test]
    fn test_default_config() {
        let config = TsConfig::default_config();
        assert_eq!(config.compiler_options.target.as_deref(), Some("ES2020"));
        assert_eq!(config.compiler_options.strict, Some(true));
        assert!(!config.include.is_empty());
        assert!(!config.exclude.is_empty());
    }

    #[test]
    fn test_to_transpile_options() {
        let json = r#"{
            "compilerOptions": {
                "target": "ES2021",
                "module": "CommonJS",
                "sourceMap": true
            }
        }"#;
        let config = TsConfig::load(json).unwrap();
        let opts = config.to_transpile_options();
        assert_eq!(opts.target, JsTarget::ES2021);
        assert_eq!(opts.module, ModuleSystem::CommonJS);
        assert_eq!(opts.source_map, SourceMapOption::External);
    }

    #[test]
    fn test_to_transpile_options_defaults() {
        let config = TsConfig::default_config();
        let opts = config.to_transpile_options();
        assert_eq!(opts.target, JsTarget::ES2020);
        assert_eq!(opts.module, ModuleSystem::ESNext);
        assert_eq!(opts.source_map, SourceMapOption::None);
    }

    #[test]
    fn test_validate_valid_config() {
        let config = TsConfig::default_config();
        let warnings = config.validate();
        assert!(warnings.is_empty(), "Expected no warnings, got: {:?}", warnings);
    }

    #[test]
    fn test_validate_unknown_target() {
        let json = r#"{ "compilerOptions": { "target": "ES2099" }, "include": ["src"] }"#;
        let config = TsConfig::load(json).unwrap();
        let warnings = config.validate();
        assert!(warnings.iter().any(|w| w.contains("Unknown target")));
    }

    #[test]
    fn test_validate_strict_conflicts() {
        let json = r#"{
            "compilerOptions": {
                "strict": true,
                "strictNullChecks": false,
                "noImplicitAny": false
            },
            "include": ["src"]
        }"#;
        let config = TsConfig::load(json).unwrap();
        let warnings = config.validate();
        assert!(warnings.iter().any(|w| w.contains("strictNullChecks")));
        assert!(warnings.iter().any(|w| w.contains("noImplicitAny")));
    }

    #[test]
    fn test_invalid_json() {
        let result = TsConfig::load("not valid json {{{");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_minimal_config() {
        let json = r#"{}"#;
        let config = TsConfig::load(json).unwrap();
        assert!(config.compiler_options.target.is_none());
        assert!(config.include.is_empty());
    }

    #[test]
    fn test_compiler_options_paths() {
        let json = r#"{
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@/*": ["src/*"],
                    "@utils/*": ["src/utils/*"]
                }
            },
            "include": ["src"]
        }"#;
        let config = TsConfig::load(json).unwrap();
        let paths = config.compiler_options.paths.as_ref().unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains_key("@/*"));
    }
}
