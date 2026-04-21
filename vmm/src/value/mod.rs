// Core value and object model types for the prototype-based VM
use num_complex::Complex64;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub mod object;
pub mod process;
pub mod property;

pub use object::Object;
pub use process::{Frame, Process, ProcessStatus};
pub use property::{PropertyAccess, PropertyDescriptor};

pub type Atom = String;

#[derive(Debug, Clone, PartialEq)]
pub enum Primitive {
    Number(f64),
    Boolean(bool),
    Complex(Complex64),
    String(String),
    Atom(Atom),
    Unit,
    Undefined,
}

#[derive(Debug, Clone)]
pub enum Value {
    Primitive(Primitive),
    Tuple(Rc<Vec<Value>>),
    Array(Rc<RefCell<Vec<Value>>>),
    Object(Rc<RefCell<Object>>),
    // No RefCell here: functions are immutable. Use with_bound_this() to get a bound copy.
    Function(Rc<RefCell<Function>>),
    Closure(Rc<Closure>),
    Process(Rc<RefCell<Process>>),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use Value::*;
        match (self, other) {
            (Primitive(a), Primitive(b)) => a == b,
            (Tuple(a), Tuple(b)) => a == b,
            (Array(a), Array(b)) => *a.borrow() == *b.borrow(),
            (Object(a), Object(b)) => *a.borrow() == *b.borrow(),
            (Function(a), Function(b)) => Rc::ptr_eq(a, b),
            (Closure(a), Closure(b)) => Rc::ptr_eq(a, b),
            (Process(a), Process(b)) => a.borrow().pid == b.borrow().pid,
            _ => false,
        }
    }
}

// Argument descriptor for the ObjectInit instruction.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectInitArg {
    Register(usize),
    Value(Value),
    // Register source with explicit property access level
    RegisterWithAccess(usize, PropertyAccess),
    // Inline value with explicit property access level
    ValueWithAccess(Value, PropertyAccess),
}

// Designed to accommodate future expansion:
//   Closure { bytecode, env_slot: usize } — env stored on the process (Process::environments)
//   Iterator { frame: Box<Frame> }        — generator/coroutine with a saved execution frame
#[derive(Debug, PartialEq, Clone)]
pub enum FunctionType {
    // Regular bytecode function
    Bytecode {
        bytecode: Vec<crate::instruction::Instruction>,
    },
    // Native Rust function registered in the FFI registry
    Ffi {
        function_name: String,
    },
}

#[derive(Debug, PartialEq, Clone)]
pub struct Function {
    pub name: Option<String>,
    pub arity: usize,
    pub extra_regs: usize,
    pub function_type: FunctionType,
    pub bound_this: Option<Value>,
    pub closure_env: Option<Rc<RefCell<HashMap<String, Value>>>>,
    pub capture_order: Vec<String>,
}

impl Function {
    pub fn new_bytecode(
        name: Option<String>,
        arity: usize,
        extra_regs: usize,
        bytecode: Vec<crate::instruction::Instruction>,
    ) -> Self {
        Function {
            name,
            arity,
            extra_regs,
            function_type: FunctionType::Bytecode { bytecode },
            bound_this: None,
            closure_env: None,
            capture_order: Vec::new(),
        }
    }

    pub fn new_ffi(name: Option<String>, arity: usize, function_name: String) -> Self {
        Function {
            name,
            arity,
            extra_regs: 0,
            function_type: FunctionType::Ffi { function_name },
            bound_this: None,
            closure_env: None,
            capture_order: Vec::new(),
        }
    }

    pub fn total_registers(&self) -> usize {
        self.arity + self.extra_regs
    }

    // Returns a new Function with bound_this set. Does not mutate in place.
    // This is how method binding works: each GetProp call produces a fresh bound copy.
    pub fn with_bound_this(&self, this: Value) -> Self {
        Function {
            bound_this: Some(this),
            ..self.clone()
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Closure {
    pub function: Rc<Function>,
    pub environment: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct TypeAlias {
    pub name: String,
    pub structure: Rc<Object>,
}
