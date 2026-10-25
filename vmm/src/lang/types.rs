use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use crate::bytecode::instruction::Instruction;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Protection {
    Writeable,
    Readonly,
    ThisWriteable,
    ThisReadonly,
    Private,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Property {
    pub value: Value,
    pub protection: Protection,
}

#[derive(Clone, Debug)]
pub struct Object {
    pub prototype: Option<Arc<RwLock<Object>>>,
    pub properties: HashMap<String, Property>,
    pub indexed: Vec<Option<Property>>,
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        let proto_eq = match (&self.prototype, &other.prototype) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        proto_eq && self.properties == other.properties && self.indexed == other.indexed
    }
}

impl Object {
    pub fn new(prototype: Option<Arc<RwLock<Object>>>) -> Self {
        Object {
            prototype,
            properties: HashMap::new(),
            indexed: Vec::new(),
        }
    }

    pub fn get_property(&self, name: &str) -> Option<Value> {
        if let Some(idx) = parse_numeric_key(name) {
            if idx < self.indexed.len() {
                if let Some(Some(prop)) = self.indexed.get(idx) {
                    return Some(prop.value.clone());
                }
            }
        }
        if let Some(prop) = self.properties.get(name) {
            return Some(prop.value.clone());
        }
        if let Some(proto) = &self.prototype {
            return proto.read().unwrap().get_property(name);
        }
        None
    }

    pub fn set_property(&mut self, name: &str, value: Value, protection: Protection) {
        if let Some(idx) = parse_numeric_key(name) {
            if idx >= self.indexed.len() {
                self.indexed.resize(idx + 1, None);
            }
            self.indexed[idx] = Some(Property { value, protection });
            return;
        }
        self.properties.insert(name.to_string(), Property { value, protection });
    }
}

pub fn parse_numeric_key(key: &str) -> Option<usize> {
    if key.is_empty() {
        return None;
    }
    let mut idx: usize = 0;
    for b in key.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        idx = idx.checked_mul(10)?.checked_add((b - b'0') as usize)?;
    }
    Some(idx)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ValType {
    Unknown,
    Int,
    Float,
    Bool,
    String,
    Object,
    Array,
    IntArray,
    FloatArray,
}

#[derive(Clone, PartialEq, Debug)]
pub enum FunctionType {
    Regular,
    Native,
    Ffi(String),
    Closure,
}

pub type NativeJitFn = unsafe extern "C" fn(registers: *mut Value, self_value: *mut Value, args: *const Value, env: *mut Value, out_res: *mut Value);

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub instructions: Vec<Instruction>,
    pub register_count: usize,
    pub argument_count: usize,
    pub return_type: Option<ValType>,
    pub bound_this: Option<Value>,
    pub fn_type: FunctionType,
    pub call_counter: usize,
    pub jit_compiled_fn: Option<NativeJitFn>,
    pub closure_env: Option<Arc<RwLock<Vec<Value>>>>,
}

impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.instructions == other.instructions
            && self.register_count == other.register_count
            && self.argument_count == other.argument_count
            && self.return_type == other.return_type
            && self.bound_this == other.bound_this
            && self.fn_type == other.fn_type
            && match (&self.closure_env, &other.closure_env) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b) || *a.read().unwrap() == *b.read().unwrap(),
                _ => false,
            }
    }
}

impl Function {
    pub fn new(
        name: String,
        instructions: Vec<Instruction>,
        register_count: usize,
        argument_count: usize,
        fn_type: FunctionType,
    ) -> Self {
        Function {
            name,
            instructions,
            register_count,
            argument_count,
            return_type: None,
            bound_this: None,
            fn_type,
            call_counter: 0,
            jit_compiled_fn: None,
            closure_env: None,
        }
    }

    pub fn with_return_type(mut self, ret: ValType) -> Self {
        self.return_type = Some(ret);
        self
    }
}


pub use crate::lang::matcher::{ArrayPattern, ListSegment, Pattern};

#[repr(C, u8)]
#[derive(Clone, Debug)]
pub enum Value {
    Int(i64) = 0,
    Float(f64) = 1,
    Bool(bool) = 2,
    String(String) = 3,
    Object(Arc<RwLock<Object>>) = 4,
    Function(Arc<RwLock<Function>>) = 5,
    Nil = 6,
    Array(Arc<RwLock<Vec<Value>>>) = 7,
    IntArray(Arc<RwLock<Vec<i64>>>) = 8,
    FloatArray(Arc<RwLock<Vec<f64>>>) = 9,
}


impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.eql(other)
    }
}

impl Value {
    pub fn eql(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => Arc::ptr_eq(a, b),
            (Value::Function(a), Value::Function(b)) => Arc::ptr_eq(a, b),
            (Value::Array(a), Value::Array(b)) => Arc::ptr_eq(a, b) || *a.read().unwrap() == *b.read().unwrap(),
            (Value::IntArray(a), Value::IntArray(b)) => Arc::ptr_eq(a, b) || *a.read().unwrap() == *b.read().unwrap(),
            (Value::FloatArray(a), Value::FloatArray(b)) => Arc::ptr_eq(a, b) || *a.read().unwrap() == *b.read().unwrap(),
            (Value::Nil, Value::Nil) => true,
            _ => false,
        }
    }

    pub fn to_str(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => if *b { "true".to_string() } else { "false".to_string() },
            Value::Nil => "nil".to_string(),
            Value::Object(_) => "[Object]".to_string(),
            Value::Function(f) => format!("[Function {}]", f.read().unwrap().name),
            Value::Array(a) => format!("[Array len={}]", a.read().unwrap().len()),
            Value::IntArray(a) => format!("[IntArray len={}]", a.read().unwrap().len()),
            Value::FloatArray(a) => format!("[FloatArray len={}]", a.read().unwrap().len()),
        }
    }
}
