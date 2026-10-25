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
//! use vm_ffi::{FfiRegistry, FfiFunction, RawFfiFunction, FfiValue, FfiResult, FfiContext};
//!
//! struct AddFunction;
//!
//! impl RawFfiFunction for AddFunction {
//!     fn call(&self, _ctx: &mut dyn FfiContext, args: Vec<FfiValue>) -> FfiResult {
//!         if args.len() != 2 {
//!             return Err(vm_ffi::FfiError::ArgumentCount { expected: 2, got: args.len() });
//!         }
//!
//!         match (&args[0], &args[1]) {
//!             (FfiValue::Number(a), FfiValue::Number(b)) => {
//!                 Ok(FfiValue::Number(a + b).into())
//!             }
//!             _ => Err(vm_ffi::FfiError::RuntimeError("Invalid arguments".to_string()))
//!         }
//!     }
//!
//!     fn name(&self) -> &str { "add" }
//!     fn arity(&self) -> usize { 2 }
//! }
//!
//! impl FfiFunction for AddFunction {}
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
#![feature(coroutines, coroutine_trait)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::{Coroutine, CoroutineState};
use std::pin::Pin;
use std::rc::Rc;

use num_complex::Complex64;

pub mod bridge;
pub mod stdlib;

/// Represents a value that can be passed between the VM and external functions
#[repr(C, u8)]
#[derive(Debug, Clone)]
pub enum FfiValue {
    Integer(i64) = 0,
    Float(f64) = 1,
    Number(f64) = 2,
    Boolean(bool) = 3,
    Atom(String) = 4,
    String(String) = 5,
    Complex(Complex64) = 6,
    Unit = 7,
    Undefined = 8,
    Tuple(Vec<FfiValue>) = 9,
    Array(Vec<FfiValue>) = 10,
    Object(HashMap<String, FfiValue>) = 11,
    Native(Rc<dyn std::any::Any>) = 12,
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

    /// Builds a tagged enum value; VM contexts can qualify its type name.
    fn make_enum(&self, enum_name: &str, variant_name: &str, payload: Option<FfiValue>) -> FfiValue {
        let mut properties = HashMap::from([
            ("@".to_string(), FfiValue::String(enum_name.to_string())),
            ("@@".to_string(), FfiValue::String(variant_name.to_string())),
        ]);
        if let Some(payload) = payload {
            properties.insert("@payload_0".to_string(), payload.clone());
            properties.insert("0".to_string(), payload);
        }
        FfiValue::Object(properties)
    }
}

/// Return value from an FFI function call:
/// - `Value`: Standard completion with an `FfiValue`
/// - `Callback`: Request IonVM to invoke a callback function with arguments
/// - `WaitAndCheck`: Request IonVM to yield the green thread/process until a condition/poll target is ready
#[derive(Debug, Clone, PartialEq)]
pub enum FfiReturn {
    Value(FfiValue),
    Callback {
        name: String,
        args: Vec<FfiValue>,
    },
    WaitAndCheck {
        target: String,
        args: Vec<FfiValue>,
        timeout_ms: Option<u64>,
    },
}

impl FfiReturn {
    pub fn value(val: FfiValue) -> Self {
        FfiReturn::Value(val)
    }

    pub fn callback(name: impl Into<String>, args: Vec<FfiValue>) -> Self {
        FfiReturn::Callback {
            name: name.into(),
            args,
        }
    }

    pub fn wait_and_check(
        target: impl Into<String>,
        args: Vec<FfiValue>,
        timeout_ms: Option<u64>,
    ) -> Self {
        FfiReturn::WaitAndCheck {
            target: target.into(),
            args,
            timeout_ms,
        }
    }

    pub fn is_value(&self) -> bool {
        matches!(self, FfiReturn::Value(_))
    }

    pub fn is_callback(&self) -> bool {
        matches!(self, FfiReturn::Callback { .. })
    }

    pub fn is_wait_and_check(&self) -> bool {
        matches!(self, FfiReturn::WaitAndCheck { .. })
    }

    pub fn into_value(self) -> Option<FfiValue> {
        match self {
            FfiReturn::Value(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_value(&self) -> Option<&FfiValue> {
        match self {
            FfiReturn::Value(v) => Some(v),
            _ => None,
        }
    }
}

impl From<FfiValue> for FfiReturn {
    fn from(val: FfiValue) -> Self {
        FfiReturn::Value(val)
    }
}

impl PartialEq<FfiValue> for FfiReturn {
    fn eq(&self, other: &FfiValue) -> bool {
        match self {
            FfiReturn::Value(v) => v == other,
            _ => false,
        }
    }
}

impl PartialEq<FfiReturn> for FfiValue {
    fn eq(&self, other: &FfiReturn) -> bool {
        other == self
    }
}

/// Result type for FFI function calls
pub type FfiResult = Result<FfiReturn, FfiError>;

/// Errors that can occur during FFI calls
#[repr(C, u8)]
#[derive(Debug, Clone, PartialEq)]
pub enum FfiError {
    ArgumentCount { expected: usize, got: usize } = 0,
    ArgumentType { expected: String, got: String } = 1,
    RuntimeError(String) = 2,
    FunctionNotFound(String) = 3,
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
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FfiType {
    Any = 0,
    Void = 1,
    Int = 2,
    Float = 3,
    Boolean = 4,
    String = 5,
    Tuple = 6,
    Array = 7,
    Object = 8,
}

/// Raw, low-level FFI function trait matching C++ vtable layout (9 slots: drop, size, align, call, name, arity, return_type, is_variadic, description).
pub trait RawFfiFunction {
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

/// High-level FFI function trait extending `RawFfiFunction` with generator / coroutine yield & resume support.
pub trait FfiFunction: RawFfiFunction {
    /// Resume an active generator function with the result from a previously yielded Callback or WaitAndCheck
    fn resume(&self, _ctx: &mut dyn FfiContext, _input: FfiValue) -> FfiResult {
        Err(FfiError::RuntimeError(format!(
            "Function '{}' is not a resumable generator",
            self.name()
        )))
    }

    /// Whether this function is currently suspended at a yield point awaiting input
    fn is_resumable(&self) -> bool {
        false
    }
}

pub type BoxedFfiCoroutine =
    Pin<Box<dyn Coroutine<FfiValue, Yield = FfiReturn, Return = FfiReturn>>>;

/// FfiCoroutineFunction: A generator-based FFI function powered by Rust's native `#[coroutine]`.
/// Allows writing multi-step functions with `yield` statements that pause and resume seamlessly.
pub struct FfiCoroutineFunction<F> {
    name: String,
    arity: usize,
    factory: F,
    active: RefCell<Option<BoxedFfiCoroutine>>,
}

impl<F, C> FfiCoroutineFunction<F>
where
    F: Fn(Vec<FfiValue>) -> C,
    C: Coroutine<FfiValue, Yield = FfiReturn, Return = FfiReturn> + 'static,
{
    pub fn new(name: impl Into<String>, arity: usize, factory: F) -> Self {
        Self {
            name: name.into(),
            arity,
            factory,
            active: RefCell::new(None),
        }
    }
}

impl<F, C> RawFfiFunction for FfiCoroutineFunction<F>
where
    F: Fn(Vec<FfiValue>) -> C,
    C: Coroutine<FfiValue, Yield = FfiReturn, Return = FfiReturn> + 'static,
{
    fn call(&self, _ctx: &mut dyn FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() < self.arity {
            return Err(FfiError::ArgumentCount {
                expected: self.arity,
                got: args.len(),
            });
        }
        let coroutine = (self.factory)(args);
        let mut pinned = Box::pin(coroutine);
        match pinned.as_mut().resume(FfiValue::Unit) {
            CoroutineState::Yielded(ret) => {
                *self.active.borrow_mut() = Some(pinned);
                Ok(ret)
            }
            CoroutineState::Complete(ret) => {
                *self.active.borrow_mut() = None;
                Ok(ret)
            }
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn arity(&self) -> usize {
        self.arity
    }
}

impl<F, C> FfiFunction for FfiCoroutineFunction<F>
where
    F: Fn(Vec<FfiValue>) -> C,
    C: Coroutine<FfiValue, Yield = FfiReturn, Return = FfiReturn> + 'static,
{
    fn resume(&self, _ctx: &mut dyn FfiContext, input: FfiValue) -> FfiResult {
        let mut active_cell = self.active.borrow_mut();
        if let Some(mut coroutine) = active_cell.take() {
            match coroutine.as_mut().resume(input) {
                CoroutineState::Yielded(ret) => {
                    *active_cell = Some(coroutine);
                    Ok(ret)
                }
                CoroutineState::Complete(ret) => {
                    *active_cell = None;
                    Ok(ret)
                }
            }
        } else {
            Err(FfiError::RuntimeError(format!(
                "Coroutine '{}' is not currently active",
                self.name
            )))
        }
    }

    fn is_resumable(&self) -> bool {
        self.active.borrow().is_some()
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

    /// Resume an active generator function by name
    pub fn resume(&self, ctx: &mut dyn FfiContext, name: &str, input: FfiValue) -> FfiResult {
        match self.functions.get(name) {
            Some(function) => function.resume(ctx, input),
            None => Err(FfiError::FunctionNotFound(name.to_string())),
        }
    }

    /// Get a registered function reference by name
    pub fn get_function(&self, name: &str) -> Option<Rc<dyn FfiFunction>> {
        self.functions.get(name).cloned()
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

    #[test]
    fn test_cpp_compatible_vtable() {
        // Test that a manual vtable matching C++ layout works seamlessly as dyn FfiFunction
        #[repr(C)]
        struct RustStr {
            ptr: *const u8,
            len: usize,
        }

        static NAME_STR: &str = "test_cpp_fn";
        unsafe extern "C" fn manual_name(_self: *const ()) -> RustStr {
            RustStr { ptr: NAME_STR.as_ptr(), len: NAME_STR.len() }
        }

        unsafe extern "C" fn manual_arity(_self: *const ()) -> usize {
            2
        }

        unsafe extern "C" fn manual_return_type(_self: *const ()) -> FfiType {
            FfiType::Int
        }

        unsafe extern "C" fn manual_is_variadic(_self: *const ()) -> bool {
            false
        }

        unsafe extern "C" fn manual_description(_self: *const ()) -> RustStr {
            RustStr { ptr: std::ptr::null(), len: 0 }
        }

        unsafe extern "C" fn manual_drop(_self: *mut ()) {}

        #[repr(C)]
        struct ManualData {
            dummy: i32,
        }

        static MANUAL_DATA: ManualData = ManualData { dummy: 123 };

        // In SysV AMD64: sret pointer in rdi, self in rsi, ctx in rdx+rcx, args passed by value
        // If we use extern "C" / extern "Rust":
        unsafe extern "Rust" fn manual_call(
            _self: &ManualData,
            _ctx: &mut dyn FfiContext,
            args: Vec<FfiValue>,
        ) -> FfiResult {
            let sum = match (&args[0], &args[1]) {
                (FfiValue::Integer(a), FfiValue::Integer(b)) => a + b,
                _ => 0,
            };
            Ok(FfiValue::Integer(sum).into())
        }

        let vtable: [usize; 9] = [
            manual_drop as *const () as usize,
            std::mem::size_of::<ManualData>(),
            std::mem::align_of::<ManualData>(),
            manual_call as *const () as usize,
            manual_name as *const () as usize,
            manual_arity as *const () as usize,
            manual_return_type as *const () as usize,
            manual_is_variadic as *const () as usize,
            manual_description as *const () as usize,
        ];

        let fat_ptr: (*const (), *const ()) = (&MANUAL_DATA as *const _ as *const (), vtable.as_ptr() as *const ());
        let trait_ref: &dyn RawFfiFunction = unsafe { std::mem::transmute(fat_ptr) };

        assert_eq!(trait_ref.name(), "test_cpp_fn");
        assert_eq!(trait_ref.arity(), 2);
        assert_eq!(trait_ref.return_type(), FfiType::Int);
        assert_eq!(trait_ref.is_variadic(), false);
        assert_eq!(trait_ref.description(), None);

        struct LocalMockContext;
        impl FfiContext for LocalMockContext {
            fn get_this(&self) -> Option<FfiValue> { None }
        }
        let mut mock_ctx = LocalMockContext;
        let res = trait_ref.call(&mut mock_ctx, vec![FfiValue::Integer(10), FfiValue::Integer(20)]);
        assert_eq!(res, Ok(FfiValue::Integer(30).into()));
    }

    #[test]
    fn test_ffi_return_variants() {
        let val_ret = FfiReturn::value(FfiValue::Integer(42));
        assert!(val_ret.is_value());
        assert_eq!(val_ret.as_value(), Some(&FfiValue::Integer(42)));
        assert_eq!(val_ret.into_value(), Some(FfiValue::Integer(42)));

        let cb_ret = FfiReturn::callback("on_click", vec![FfiValue::String("btn".to_string())]);
        assert!(cb_ret.is_callback());
        if let FfiReturn::Callback { name, args } = cb_ret {
            assert_eq!(name, "on_click");
            assert_eq!(args.len(), 1);
        } else {
            panic!("expected Callback");
        }

        let wait_ret = FfiReturn::wait_and_check("poll_ready", vec![FfiValue::Integer(1)], Some(500));
        assert!(wait_ret.is_wait_and_check());
        if let FfiReturn::WaitAndCheck { target, args, timeout_ms } = wait_ret {
            assert_eq!(target, "poll_ready");
            assert_eq!(args.len(), 1);
            assert_eq!(timeout_ms, Some(500));
        } else {
            panic!("expected WaitAndCheck");
        }
    }

    #[test]
    fn test_coroutine_generator_function() {
        struct MockContext;
        impl FfiContext for MockContext {
            fn get_this(&self) -> Option<FfiValue> { None }
        }
        let mut ctx = MockContext;

        let coroutine_fn = FfiCoroutineFunction::new("test.calc_chain", 1, |args| {
            #[coroutine] move |_input: FfiValue| {
                let x = args[0].as_number().unwrap();

                // Yield 1: Ask host to compute sqrt
                let sqrt_res = yield FfiReturn::callback("sqrt", vec![FfiValue::Float(100.0)]);
                let s = sqrt_res.as_number().unwrap();

                // Yield 2: Ask host to compute sin
                let sin_res = yield FfiReturn::callback("sin", vec![FfiValue::Float(s)]);
                let sn = sin_res.as_number().unwrap();

                // Complete
                FfiReturn::Value(FfiValue::Float(x + s + sn))
            }
        });

        // 1. Initial call
        let res1 = coroutine_fn.call(&mut ctx, vec![FfiValue::Float(50.0)]).unwrap();
        assert!(coroutine_fn.is_resumable());
        assert_eq!(res1, FfiReturn::callback("sqrt", vec![FfiValue::Float(100.0)]));

        // 2. Resume with sqrt result (10.0)
        let res2 = coroutine_fn.resume(&mut ctx, FfiValue::Float(10.0)).unwrap();
        assert!(coroutine_fn.is_resumable());
        assert_eq!(res2, FfiReturn::callback("sin", vec![FfiValue::Float(10.0)]));

        // 3. Resume with sin result (-0.544)
        let res3 = coroutine_fn.resume(&mut ctx, FfiValue::Float(-0.544)).unwrap();
        assert!(!coroutine_fn.is_resumable());
        assert_eq!(res3, FfiReturn::value(FfiValue::Float(50.0 + 10.0 - 0.544)));
    }
}
