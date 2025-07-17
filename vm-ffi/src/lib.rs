//! VM FFI - Foreign Function Interface for the VM
//!
//! This library provides a bridge between the VM and external functions written in Rust
//! and eventually Python. It's designed to be the foundation for the standard library.

use std::collections::HashMap;
use std::sync::Arc;

pub mod bridge;
pub mod stdlib;
// TODO: pub mod python_ffi;

/// Represents a value that can be passed between the VM and external functions
#[derive(Debug, Clone, PartialEq)]
pub enum FfiValue {
    Number(f64),
    Boolean(bool),
    String(String),
    Unit,
    Undefined,
    Tuple(Vec<FfiValue>),
    Array(Vec<FfiValue>),
    Object(HashMap<String, FfiValue>),
}

/// Result type for FFI function calls
pub type FfiResult = Result<FfiValue, FfiError>;

/// Errors that can occur during FFI calls
#[derive(Debug, Clone, PartialEq)]
pub enum FfiError {
    ArgumentCount { expected: usize, got: usize },
    ArgumentType { expected: String, got: String },
    RuntimeError(String),
    FunctionNotFound(String),
}

impl std::fmt::Display for FfiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FfiError::ArgumentCount { expected, got } => {
                write!(f, "Expected {} arguments, got {}", expected, got)
            }
            FfiError::ArgumentType { expected, got } => {
                write!(f, "Expected {} argument, got {}", expected, got)
            }
            FfiError::RuntimeError(msg) => write!(f, "Runtime error: {}", msg),
            FfiError::FunctionNotFound(name) => write!(f, "Function not found: {}", name),
        }
    }
}

impl std::error::Error for FfiError {}

/// Trait for functions that can be called from the VM
pub trait FfiFunction: Send + Sync {
    fn call(&self, args: Vec<FfiValue>) -> FfiResult;
    fn name(&self) -> &str;
    fn arity(&self) -> usize;
    fn description(&self) -> Option<&str> {
        None
    }
}

/// A registry of external functions that can be called from the VM
pub struct FfiRegistry {
    functions: HashMap<String, Arc<dyn FfiFunction>>,
}

impl FfiRegistry {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    /// Register a new FFI function
    pub fn register<F: FfiFunction + 'static>(&mut self, function: F) {
        let name = function.name().to_string();
        self.functions.insert(name, Arc::new(function));
    }

    /// Call an FFI function by name
    pub fn call(&self, name: &str, args: Vec<FfiValue>) -> FfiResult {
        match self.functions.get(name) {
            Some(function) => function.call(args),
            None => Err(FfiError::FunctionNotFound(name.to_string())),
        }
    }

    /// Get information about a registered function
    pub fn get_function_info(&self, name: &str) -> Option<(&str, usize, Option<&str>)> {
        self.functions
            .get(name)
            .map(|f| (f.name(), f.arity(), f.description()))
    }

    /// List all registered function names
    pub fn list_functions(&self) -> Vec<&str> {
        self.functions.keys().map(|s| s.as_str()).collect()
    }

    /// Create a registry with standard library functions
    pub fn with_stdlib() -> Self {
        let mut registry = Self::new();
        stdlib::register_all(&mut registry);
        registry
    }

    /// Create a registry with only specific stdlib modules
    pub fn with_modules(modules: &[&str]) -> Self {
        let mut registry = Self::new();
        stdlib::register_modules(&mut registry, modules);
        registry
    }
}

impl Default for FfiRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Conversion helpers
impl FfiValue {
    /// Convert to a number if possible
    pub fn as_number(&self) -> Option<f64> {
        match self {
            FfiValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Convert to a boolean if possible
    pub fn as_boolean(&self) -> Option<bool> {
        match self {
            FfiValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Convert to a string if possible
    pub fn as_string(&self) -> Option<&str> {
        match self {
            FfiValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Convert to an array if possible
    pub fn as_array(&self) -> Option<&Vec<FfiValue>> {
        match self {
            FfiValue::Array(arr) => Some(arr),
            _ => None,
        }
    }

    /// Convert to an object if possible
    pub fn as_object(&self) -> Option<&HashMap<String, FfiValue>> {
        match self {
            FfiValue::Object(obj) => Some(obj),
            _ => None,
        }
    }

    /// Get the type name as a string
    pub fn type_name(&self) -> &'static str {
        match self {
            FfiValue::Number(_) => "Number",
            FfiValue::Boolean(_) => "Boolean",
            FfiValue::String(_) => "String",
            FfiValue::Unit => "Unit",
            FfiValue::Undefined => "Undefined",
            FfiValue::Tuple(_) => "Tuple",
            FfiValue::Array(_) => "Array",
            FfiValue::Object(_) => "Object",
        }
    }

    /// Check if the value is truthy (following JS-like semantics)
    pub fn is_truthy(&self) -> bool {
        match self {
            FfiValue::Boolean(b) => *b,
            FfiValue::Number(n) => *n != 0.0,
            FfiValue::String(s) => !s.is_empty(),
            FfiValue::Tuple(arr) => !arr.is_empty() && arr.iter().all(|v| v.is_truthy()),
            FfiValue::Array(arr) => !arr.is_empty(),
            FfiValue::Object(obj) => !obj.is_empty(),
            FfiValue::Unit | FfiValue::Undefined => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_value_conversions() {
        let num = FfiValue::Number(42.0);
        assert_eq!(num.as_number(), Some(42.0));
        assert_eq!(num.type_name(), "Number");
        assert!(num.is_truthy());

        let bool_val = FfiValue::Boolean(false);
        assert_eq!(bool_val.as_boolean(), Some(false));
        assert!(!bool_val.is_truthy());

        let str_val = FfiValue::String("hello".to_string());
        assert_eq!(str_val.as_string(), Some("hello"));
        assert!(str_val.is_truthy());

        let empty_str = FfiValue::String("".to_string());
        assert!(!empty_str.is_truthy());
    }

    #[test]
    fn test_ffi_registry() {
        let mut registry = FfiRegistry::new();

        // Test that we can list functions (should be empty)
        assert!(registry.list_functions().is_empty());

        // Test function not found
        let result = registry.call("nonexistent", vec![]);
        assert!(matches!(result, Err(FfiError::FunctionNotFound(_))));
    }

    #[test]
    fn test_ffi_error_display() {
        let err = FfiError::ArgumentCount {
            expected: 2,
            got: 1,
        };
        assert_eq!(err.to_string(), "Expected 2 arguments, got 1");

        let err = FfiError::FunctionNotFound("test".to_string());
        assert_eq!(err.to_string(), "Function not found: test");
    }
}
