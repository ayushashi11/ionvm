//! # VM FFI - Foreign Function Interface
//!
//! This library provides a bridge between the IonVM and external functions written in Rust.
//! It enables the VM to call native functions for performance-critical operations and
//! standard library functionality.
//!
//! # Overview
//!
//! The FFI system consists of:
//! - [`FfiValue`] - Value types that cross the VM/native boundary
//! - [`FfiFunction`] - Trait for implementing native functions
//! - [`FfiRegistry`] - Registry for registering and calling functions
//! - [`bridge`] - Conversion utilities between VM and FFI values
//! - [`stdlib`] - Standard library function implementations
//!
//! # Example
//!
//! ```rust
//! use vm_ffi::{FfiRegistry, FfiFunction, FfiValue, FfiResult, FfiContext};
//!
//! struct AddFunction;
//!
//! impl FfiFunction for AddFunction {
//!     fn call(&self, _ctx: &mut dyn FfiContext, args: Vec<FfiValue>) -> FfiResult {
//!         if args.len() != 2 {
//!             return Err(vm_ffi::FfiError::ArgumentCount { expected: 2, got: args.len() });
//!         }
//!
//!         match (&args[0], &args[1]) {
//!             (FfiValue::Number(a), FfiValue::Number(b)) => {
//!                 Ok(FfiValue::Number(a + b))
//!             }
//!             _ => Err(vm_ffi::FfiError::RuntimeError("Invalid arguments".to_string()))
//!         }
//!     }
//!
//!     fn name(&self) -> &str { "add" }
//!     fn arity(&self) -> usize { 2 }
//! }
//!
//! struct MockContext;
//! impl FfiContext for MockContext {
//!     fn get_this(&self) -> Option<FfiValue> { None }
//! }
//!
//! let mut registry = FfiRegistry::new();
//! registry.register(AddFunction);
//!
//! let mut ctx = MockContext;
//! let result = registry.call(&mut ctx, "add", vec![
//!     FfiValue::Number(1.0),
//!     FfiValue::Number(2.0)
//! ]);
//! assert_eq!(result.unwrap(), FfiValue::Number(3.0));
//! ```
use std::collections::HashMap;
use std::rc::Rc;

use num_complex::Complex64;

pub mod bridge;
pub mod stdlib;

/// Represents a value that can be passed between the VM and external functions
#[derive(Debug, Clone)]
pub enum FfiValue {
    Integer(i64),
    Float(f64),
    Number(f64),
    Boolean(bool),
    Atom(String),
    String(String),
    Complex(Complex64),
    Unit,
    Undefined,
    Tuple(Vec<FfiValue>),
    Array(Vec<FfiValue>),
    Object(HashMap<String, FfiValue>),
    Native(Rc<dyn std::any::Any>),
}

impl PartialEq for FfiValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (FfiValue::Integer(a), FfiValue::Integer(b)) => a == b,
            (FfiValue::Float(a), FfiValue::Float(b)) => a == b,
            (FfiValue::Number(a), FfiValue::Number(b)) => a == b,
            (FfiValue::Integer(a), FfiValue::Float(b)) => *a as f64 == *b,
            (FfiValue::Float(a), FfiValue::Integer(b)) => *a == *b as f64,
            (FfiValue::Integer(a), FfiValue::Number(b)) => *a as f64 == *b,
            (FfiValue::Number(a), FfiValue::Integer(b)) => *a == *b as f64,
            (FfiValue::Float(a), FfiValue::Number(b)) => a == b,
            (FfiValue::Number(a), FfiValue::Float(b)) => a == b,
            (FfiValue::Boolean(a), FfiValue::Boolean(b)) => a == b,
            (FfiValue::Atom(a), FfiValue::Atom(b)) => a == b,
            (FfiValue::String(a), FfiValue::String(b)) => a == b,
            (FfiValue::Complex(a), FfiValue::Complex(b)) => a == b,
            (FfiValue::Unit, FfiValue::Unit) => true,
            (FfiValue::Undefined, FfiValue::Undefined) => true,
            (FfiValue::Tuple(a), FfiValue::Tuple(b)) => a == b,
            (FfiValue::Array(a), FfiValue::Array(b)) => a == b,
            (FfiValue::Object(a), FfiValue::Object(b)) => a == b,
            (FfiValue::Native(a), FfiValue::Native(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// Context passed to FFI functions to allow interaction with the VM
pub trait FfiContext {
    fn get_this(&self) -> Option<FfiValue>;
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

/// Represents the return or value type across the FFI boundary
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FfiType {
    Any,
    Void,
    Int,
    Float,
    Boolean,
    String,
    Tuple,
    Array,
    Object,
}

/// Trait for functions that can be called from the VM
pub trait FfiFunction {
    fn call(&self, ctx: &mut dyn FfiContext, args: Vec<FfiValue>) -> FfiResult;
    fn name(&self) -> &str;
    fn arity(&self) -> usize;
    fn return_type(&self) -> FfiType {
        FfiType::Any
    }
    fn is_variadic(&self) -> bool {
        false
    }
    fn description(&self) -> Option<&str> {
        None
    }
}

/// A registry of external functions that can be called from the VM
pub struct FfiRegistry {
    functions: HashMap<String, Rc<dyn FfiFunction>>,
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
        self.functions.insert(name, Rc::new(function));
    }

    /// Call an FFI function by name
    pub fn call(&self, ctx: &mut dyn FfiContext, name: &str, args: Vec<FfiValue>) -> FfiResult {
        match self.functions.get(name) {
            Some(function) => function.call(ctx, args),
            None => Err(FfiError::FunctionNotFound(name.to_string())),
        }
    }

    /// Get information about a registered function
    pub fn get_function_info(&self, name: &str) -> Option<(&str, usize, Option<&str>)> {
        self.functions
            .get(name)
            .map(|f| (f.name(), f.arity(), f.description()))
    }

    /// Get return type of a registered function
    pub fn get_return_type(&self, name: &str) -> Option<FfiType> {
        self.functions.get(name).map(|f| f.return_type())
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
            FfiValue::Integer(i) => Some(*i as f64),
            FfiValue::Float(f) => Some(*f),
            FfiValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Convert to an integer if possible
    pub fn as_int(&self) -> Option<i64> {
        match self {
            FfiValue::Integer(i) => Some(*i),
            FfiValue::Float(f) => if f.fract() == 0.0 { Some(*f as i64) } else { None },
            FfiValue::Number(n) => if n.fract() == 0.0 { Some(*n as i64) } else { None },
            _ => None,
        }
    }

    /// Convert to a float if possible
    pub fn as_float(&self) -> Option<f64> {
        match self {
            FfiValue::Float(f) => Some(*f),
            FfiValue::Integer(i) => Some(*i as f64),
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
            FfiValue::Integer(_) => "Integer",
            FfiValue::Float(_) => "Float",
            FfiValue::Number(_) => "Number",
            FfiValue::Boolean(_) => "Boolean",
            FfiValue::Atom(_) => "Atom",
            FfiValue::String(_) => "String",
            FfiValue::Unit => "Unit",
            FfiValue::Complex(_) => "Complex",
            FfiValue::Undefined => "Undefined",
            FfiValue::Tuple(_) => "Tuple",
            FfiValue::Array(_) => "Array",
            FfiValue::Object(_) => "Object",
            FfiValue::Native(_) => "Native",
        }
    }

    /// Check if the value is truthy (following JS-like semantics)
    pub fn is_truthy(&self) -> bool {
        match self {
            FfiValue::Boolean(b) => *b,
            FfiValue::Integer(i) => *i != 0,
            FfiValue::Float(f) => *f != 0.0,
            FfiValue::Number(n) => *n != 0.0,
            FfiValue::Atom(s) => !s.is_empty(),
            FfiValue::String(s) => !s.is_empty(),
            FfiValue::Tuple(arr) => !arr.is_empty() && arr.iter().all(|v| v.is_truthy()),
            FfiValue::Array(arr) => !arr.is_empty(),
            FfiValue::Object(obj) => !obj.is_empty(),
            FfiValue::Unit | FfiValue::Undefined => false,
            FfiValue::Complex(c) => c.re != 0.0 && c.im != 0.0,
            FfiValue::Native(_) => true,
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
        let registry = FfiRegistry::new();

        struct MockContext;
        impl FfiContext for MockContext {
            fn get_this(&self) -> Option<FfiValue> {
                None
            }
        }
        let mut ctx = MockContext;

        // Test that we can list functions (should be empty)
        assert!(registry.list_functions().is_empty());

        // Test function not found
        let result = registry.call(&mut ctx, "nonexistent", vec![]);
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
