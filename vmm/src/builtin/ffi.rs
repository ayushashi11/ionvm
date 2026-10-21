//! FFI Integration module for IonVM
//!
//! Bridges IonVM's internal `Value` representation with `vm-ffi`'s `FfiValue`
//! and provides seamless access to the modular standard library.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use vm_ffi::bridge::{FromFfiValue, ToFfiValue};
use vm_ffi::{FfiContext, FfiError, FfiRegistry, FfiReturn, FfiValue};

use crate::lang::types::{Object, Property, Protection, Value};

impl ToFfiValue for Value {
    fn to_ffi(&self) -> FfiValue {
        match self {
            Value::Nil => FfiValue::Unit,
            Value::Bool(b) => FfiValue::Boolean(*b),
            Value::Int(i) => FfiValue::Integer(*i),
            Value::Float(f) => FfiValue::Float(*f),
            Value::String(s) => FfiValue::String(s.clone()),
            Value::Array(arr) => {
                if let Ok(b) = arr.read() {
                    FfiValue::Array(b.iter().map(|v| v.to_ffi()).collect())
                } else {
                    FfiValue::Array(vec![])
                }
            }
            Value::IntArray(arr) => {
                if let Ok(b) = arr.read() {
                    FfiValue::Array(b.iter().map(|&i| FfiValue::Integer(i)).collect())
                } else {
                    FfiValue::Array(vec![])
                }
            }
            Value::FloatArray(arr) => {
                if let Ok(b) = arr.read() {
                    FfiValue::Array(b.iter().map(|&f| FfiValue::Float(f)).collect())
                } else {
                    FfiValue::Array(vec![])
                }
            }
            Value::Object(obj) => {
                let mut map = HashMap::new();
                if let Ok(b) = obj.read() {
                    for (k, v) in &b.properties {
                        map.insert(k.clone(), v.value.to_ffi());
                    }
                    for (idx, prop) in b.indexed.iter().enumerate() {
                        if let Some(p) = prop {
                            map.insert(idx.to_string(), p.value.to_ffi());
                        }
                    }
                }
                FfiValue::Object(map)
            }
            Value::Function(_) => FfiValue::Undefined,
        }
    }
}

impl FromFfiValue for Value {
    fn from_ffi(value: FfiValue) -> Result<Self, FfiError> {
        Ok(match value {
            FfiValue::Unit | FfiValue::Undefined => Value::Nil,
            FfiValue::Boolean(b) => Value::Bool(b),
            FfiValue::Integer(i) => Value::Int(i),
            FfiValue::Float(f) => Value::Float(f),
            FfiValue::Number(n) => {
                if n.fract() == 0.0 && n.is_finite() && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                    Value::Int(n as i64)
                } else {
                    Value::Float(n)
                }
            }
            FfiValue::String(s) => Value::String(s),
            FfiValue::Atom(a) => Value::String(a),
            FfiValue::Complex(_) => Value::Nil,
            FfiValue::Tuple(t) => {
                let mut items = Vec::with_capacity(t.len());
                for item in t {
                    items.push(Value::from_ffi(item)?);
                }
                Value::Array(Arc::new(RwLock::new(items)))
            }
            FfiValue::Array(arr) => {
                let mut items = Vec::with_capacity(arr.len());
                for item in arr {
                    items.push(Value::from_ffi(item)?);
                }
                Value::Array(Arc::new(RwLock::new(items)))
            }
            FfiValue::Object(map) => {
                let mut obj = Object::new(None);
                for (k, v) in map {
                    obj.properties.insert(
                        k,
                        Property {
                            value: Value::from_ffi(v)?,
                            protection: Protection::Writeable,
                        },
                    );
                }
                Value::Object(Arc::new(RwLock::new(obj)))
            }
            FfiValue::Native(_) => Value::Nil,
        })
    }
}

pub struct VmFfiContext;

impl FfiContext for VmFfiContext {
    fn get_this(&self) -> Option<FfiValue> {
        None
    }
}

thread_local! {
    static FFI_REGISTRY: RefCell<FfiRegistry> = RefCell::new(FfiRegistry::with_stdlib());
    static LOCAL_SYNC_VERSION: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub fn with_ffi_registry<R>(f: impl FnOnce(&mut FfiRegistry) -> R) -> R {
    FFI_REGISTRY.with(|reg_cell| {
        let mut reg = reg_cell.borrow_mut();
        f(&mut *reg)
    })
}

fn sync_registered_natives(reg: &mut FfiRegistry) {
    crate::builtin::native::sync_registered_natives(reg);
}

fn sync_if_needed(reg_cell: &RefCell<FfiRegistry>) {
    let global_ver = crate::builtin::native::get_native_version();
    let local_ver = LOCAL_SYNC_VERSION.get();
    if local_ver < global_ver {
        let mut reg = reg_cell.borrow_mut();
        sync_registered_natives(&mut *reg);
        LOCAL_SYNC_VERSION.set(global_ver);
    }
}

fn snake_to_pascal(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut c = word.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

fn handle_ffi_return(res: FfiReturn) -> Option<Value> {
    match res {
        FfiReturn::Value(v) => Value::from_ffi(v).ok(),
        FfiReturn::Callback { name, args } => {
            let cb_vm_args: Vec<Value> = args
                .into_iter()
                .map(|a| Value::from_ffi(a).unwrap_or(Value::Nil))
                .collect();
            crate::builtin::call_native(&name, &cb_vm_args)
        }
        FfiReturn::WaitAndCheck {
            target,
            args,
            timeout_ms,
        } => {
            let check_vm_args: Vec<Value> = args
                .into_iter()
                .map(|a| Value::from_ffi(a).unwrap_or(Value::Nil))
                .collect();
            let start = std::time::Instant::now();
            loop {
                if let Some(val) = crate::builtin::call_native(&target, &check_vm_args) {
                    if val != Value::Nil && val != Value::Bool(false) {
                        return Some(val);
                    }
                }
                if let Some(timeout) = timeout_ms {
                    if start.elapsed().as_millis() as u64 >= timeout {
                        return None;
                    }
                }
                std::thread::yield_now();
            }
        }
    }
}

/// Call a registered FFI function by name
pub fn call_ffi(name: &str, args: &[Value]) -> Option<Value> {
    FFI_REGISTRY.with(|reg_cell| {
        sync_if_needed(reg_cell);
        let registry = reg_cell.borrow();
        let ffi_args: Vec<FfiValue> = args.iter().map(|v| v.to_ffi()).collect();
        let mut ctx = VmFfiContext;

        // 1. Direct name lookup (e.g. "Sqrt", "ReadFile", "GetTime", "gui.show_message")
        if let Ok(res) = registry.call(&mut ctx, name, ffi_args.clone()) {
            return handle_ffi_return(res);
        }

        // 2. Snake_case to PascalCase lookup (e.g. "sqrt" -> "Sqrt", "read_file" -> "ReadFile")
        let pascal_name = snake_to_pascal(name);
        if pascal_name != name {
            if let Ok(res) = registry.call(&mut ctx, &pascal_name, ffi_args.clone()) {
                return handle_ffi_return(res);
            }
        }

        // 3. Namespace stripped leaf lookup (e.g. "rust_math.square" -> "square")
        if let Some(leaf) = name.split('.').last() {
            if leaf != name {
                if let Ok(res) = registry.call(&mut ctx, leaf, ffi_args.clone()) {
                    return handle_ffi_return(res);
                }
                let pascal_leaf = snake_to_pascal(leaf);
                if pascal_leaf != leaf {
                    if let Ok(res) = registry.call(&mut ctx, &pascal_leaf, ffi_args) {
                        return handle_ffi_return(res);
                    }
                }
            }
        }

        None
    })
}

/// Call a registered FFI function by name and return the raw FfiReturn
pub fn call_ffi_raw(name: &str, args: &[Value]) -> Result<FfiReturn, FfiError> {
    FFI_REGISTRY.with(|reg_cell| {
        sync_if_needed(reg_cell);
        let registry = reg_cell.borrow();
        let ffi_args: Vec<FfiValue> = args.iter().map(|v| v.to_ffi()).collect();
        let mut ctx = VmFfiContext;

        if let Ok(res) = registry.call(&mut ctx, name, ffi_args.clone()) {
            return Ok(res);
        }

        let pascal_name = snake_to_pascal(name);
        if pascal_name != name {
            if let Ok(res) = registry.call(&mut ctx, &pascal_name, ffi_args) {
                return Ok(res);
            }
        }

        Err(FfiError::FunctionNotFound(name.to_string()))
    })
}

/// Check if a function name is registered in the FFI registry
pub fn is_ffi_function(name: &str) -> bool {
    FFI_REGISTRY.with(|reg_cell| {
        sync_if_needed(reg_cell);
        let registry = reg_cell.borrow();
        if registry.get_function_info(name).is_some() {
            return true;
        }
        let pascal_name = snake_to_pascal(name);
        registry.get_function_info(&pascal_name).is_some()
    })
}

/// Get the declared return type of an FFI function
pub fn get_ffi_return_type(name: &str) -> Option<vm_ffi::FfiType> {
    FFI_REGISTRY.with(|reg_cell| {
        sync_if_needed(reg_cell);
        let registry = reg_cell.borrow();
        if let Some(t) = registry.get_return_type(name) {
            return Some(t);
        }
        let pascal_name = snake_to_pascal(name);
        registry.get_return_type(&pascal_name)
    })
}
