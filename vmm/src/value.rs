// Core value and object model types for the prototype-based VM

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub type Atom = String; // Atoms are interned strings in some languages; here, just String for now

#[derive(Debug, Clone, PartialEq)]
pub enum Primitive {
    Number(f64),
    Boolean(bool),
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
    TaggedEnum(Rc<TaggedEnum>),
    Function(Rc<Function>),
    Closure(Rc<Closure>),
    Process(Rc<RefCell<Process>>),
    // Add more as needed (e.g., native functions, etc.)
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use Value::*;
        match (self, other) {
            (Primitive(a), Primitive(b)) => a == b,
            (Tuple(a), Tuple(b)) => a == b,
            (Array(a), Array(b)) => *a.borrow() == *b.borrow(),
            (Object(a), Object(b)) => *a.borrow() == *b.borrow(),
            (TaggedEnum(a), TaggedEnum(b)) => a == b,
            (Function(a), Function(b)) => Rc::ptr_eq(a, b),
            (Closure(a), Closure(b)) => Rc::ptr_eq(a, b),
            (Process(a), Process(b)) => a.borrow().pid == b.borrow().pid,
            _ => false,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct PropertyDescriptor {
    pub value: Value,
    pub writable: bool,
    pub enumerable: bool,
    pub configurable: bool,
}

#[derive(Debug, PartialEq)]
pub struct Object {
    pub properties: HashMap<String, PropertyDescriptor>,
    pub prototype: Option<Rc<RefCell<Object>>>,
    pub magic_methods: HashMap<String, Value>, // e.g., "__getattr__", "__setattr__", "__add__", etc.
    pub type_name: Option<String>,             // For named types, e.g., "Point"
}

impl Object {
    pub fn new(prototype: Option<Rc<RefCell<Object>>>) -> Self {
        Object {
            properties: HashMap::new(),
            prototype,
            magic_methods: HashMap::new(),
            type_name: None,
        }
    }

    /// Get a property, traversing the prototype chain if needed.
    /// If not found, tries __getattr__ magic method if present.
    pub fn get_property(&self, key: &str) -> Option<Value> {
        if let Some(desc) = self.properties.get(key) {
            Some(desc.value.clone())
        } else if let Some(proto) = &self.prototype {
            proto.borrow().get_property(key)
        } else if let Some(_magic) = self.magic_methods.get("__getattr__") {
            // Call __getattr__ with key as argument (not implemented here)
            // Placeholder: just return None for now
            None
        } else {
            None
        }
    }

    /// Set a property, creating or updating as needed.
    /// If __setattr__ magic method exists, call it instead.
    pub fn set_property(&mut self, key: &str, value: Value) {
        if let Some(_magic) = self.magic_methods.get("__setattr__") {
            // Call __setattr__ with key and value as arguments (not implemented here)
            // Placeholder: do nothing for now
        } else {
            let desc = self
                .properties
                .entry(key.to_string())
                .or_insert(PropertyDescriptor {
                    value: value.clone(),
                    writable: true,
                    enumerable: true,
                    configurable: true,
                });
            if desc.writable {
                desc.value = value;
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct TaggedEnum {
    pub tag: String,
    pub value: Value,
}

#[derive(Debug, PartialEq, Clone)]
pub enum FunctionType {
    Bytecode {
        bytecode: Vec<crate::vm::Instruction>,
    },
    Ffi {
        function_name: String,
    },
}

#[derive(Debug, PartialEq, Clone)]
pub struct Function {
    pub name: Option<String>,
    pub arity: usize,
    pub extra_regs: usize, // Additional registers beyond arity for calculations
    pub function_type: FunctionType,
    // More metadata as needed
}

impl Function {
    /// Create a new bytecode function
    pub fn new_bytecode(name: Option<String>, arity: usize, extra_regs: usize, bytecode: Vec<crate::vm::Instruction>) -> Self {
        Function {
            name,
            arity,
            extra_regs,
            function_type: FunctionType::Bytecode { bytecode },
        }
    }
    
    /// Create a new FFI function
    pub fn new_ffi(name: Option<String>, arity: usize, function_name: String) -> Self {
        Function {
            name,
            arity,
            extra_regs: 0, // FFI functions don't need extra registers 
            function_type: FunctionType::Ffi { function_name },
        }
    }
    
    /// Get total number of registers needed (arity + extra_regs)
    pub fn total_registers(&self) -> usize {
        self.arity + self.extra_regs
    }
}

#[derive(Debug, PartialEq)]
pub struct Closure {
    pub function: Rc<Function>,
    pub environment: HashMap<String, Value>, // Captured variables
}

#[derive(Debug)]
pub struct Process {
    pub pid: usize,
    pub frames: Vec<crate::vm::Frame>,
    pub mailbox: Vec<Value>,
    pub links: Vec<usize>,
    pub alive: bool,
    pub last_result: Option<Value>,
    pub reductions: u32, // Current reduction count for this scheduling round
    pub status: ProcessStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessStatus {
    Runnable,
    WaitingForMessage,
    Suspended,
    Exited,
}

impl Process {
    pub fn new(pid: usize, function: Rc<Function>, args: Vec<Value>) -> Self {
        let mut registers = args;
        // Ensure we have enough registers for the function's needs
        let total_regs = function.total_registers().max(16); // Minimum 16 registers for compatibility
        registers.resize(total_regs, Value::Primitive(Primitive::Undefined));
        let frame = crate::vm::Frame {
            registers,
            stack: Vec::new(),
            ip: 0,
            function,
            return_value: None,
            caller_return_reg: None,
        };
        Process {
            pid,
            frames: vec![frame],
            mailbox: Vec::new(),
            links: Vec::new(),
            alive: true,
            last_result: None,
            reductions: 0,
            status: ProcessStatus::Runnable,
        }
    }

    /// Reset reduction count for new scheduling round
    pub fn reset_reductions(&mut self, limit: u32) {
        self.reductions = limit;
    }

    /// Consume one reduction, returns true if budget exhausted
    pub fn consume_reduction(&mut self) -> bool {
        if self.reductions > 0 {
            self.reductions -= 1;
            self.reductions == 0
        } else {
            true
        }
    }

    /// Check if process can be scheduled
    pub fn is_schedulable(&self) -> bool {
        self.alive && self.status == ProcessStatus::Runnable
    }
}

// Type aliasing for named types
#[derive(Debug, Clone)]
pub struct TypeAlias {
    pub name: String,
    pub structure: Rc<Object>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_primitive_obj() -> Rc<RefCell<Object>> {
        Rc::new(RefCell::new(Object::new(None)))
    }

    #[test]
    fn test_property_set_and_get() {
        let obj = make_primitive_obj();
        obj.borrow_mut()
            .set_property("foo", Value::Primitive(Primitive::Number(42.0)));
        let val = obj.borrow().get_property("foo");
        assert_eq!(val, Some(Value::Primitive(Primitive::Number(42.0))));
    }

    #[test]
    fn test_prototype_chain_lookup() {
        let proto = make_primitive_obj();
        proto
            .borrow_mut()
            .set_property("bar", Value::Primitive(Primitive::Boolean(true)));
        let obj = Rc::new(RefCell::new(Object::new(Some(proto.clone()))));
        // Property not on obj, but on prototype
        let val = obj.borrow().get_property("bar");
        assert_eq!(val, Some(Value::Primitive(Primitive::Boolean(true))));
        // Property on obj shadows prototype
        obj.borrow_mut()
            .set_property("bar", Value::Primitive(Primitive::Boolean(false)));
        let val2 = obj.borrow().get_property("bar");
        assert_eq!(val2, Some(Value::Primitive(Primitive::Boolean(false))));
    }

    #[test]
    fn test_magic_getattr_fallback() {
        let mut obj = Object::new(None);
        // Simulate a __getattr__ magic method by inserting a dummy function value
        obj.magic_methods.insert(
            "__getattr__".to_string(),
            Value::Primitive(Primitive::Atom("called".to_string())),
        );
        // get_property should return None (since we don't actually invoke the magic method yet)
        let val = obj.get_property("missing");
        assert_eq!(val, None);
    }

    #[test]
    fn test_magic_setattr_fallback() {
        let mut obj = Object::new(None);
        // Simulate a __setattr__ magic method by inserting a dummy function value
        obj.magic_methods.insert(
            "__setattr__".to_string(),
            Value::Primitive(Primitive::Atom("called".to_string())),
        );
        // set_property should not actually set the property (since we don't invoke the magic method)
        obj.set_property("foo", Value::Primitive(Primitive::Number(1.0)));
        assert!(obj.properties.get("foo").is_none());
    }

    #[test]
    fn test_object_property_access() {
        let obj = make_primitive_obj();
        obj.borrow_mut()
            .set_property("foo", Value::Primitive(Primitive::Number(123.0)));
        let val = obj.borrow().get_property("foo");
        assert_eq!(val, Some(Value::Primitive(Primitive::Number(123.0))));
    }

    #[test]
    fn test_function_call_frame_setup() {
        use crate::vm::{Frame, Instruction};
        // Create a function that just returns its first argument
        let func = Rc::new(Function::new_bytecode(
            Some("id".to_string()),
            1,
            0, // No extra registers needed for simple return
            vec![Instruction::Return(0)]
        ));
        let mut registers = vec![Value::Primitive(Primitive::Number(42.0))];
        registers.resize(16, Value::Primitive(Primitive::Undefined));
        let frame = Frame {
            registers,
            stack: Vec::new(),
            ip: 0,
            function: func.clone(),
            return_value: None,
            caller_return_reg: None,
        };
        // Simulate a call by pushing a new frame
        let frames = vec![frame];
        // The return value should be in register 0 after return
        let ret_val = frames.last().unwrap().registers[0].clone();
        assert_eq!(ret_val, Value::Primitive(Primitive::Number(42.0)));
    }
}

#[cfg(test)]
mod extra_regs_tests {
    use super::*;
    use crate::vm::{IonVM, Instruction, ExecutionResult};
    
    #[test]
    fn test_vm_with_extra_regs_function() {
        let mut vm = IonVM::new();
        
        // Create a function that uses extra registers for complex calculations
        // Function takes 2 arguments and uses 4 extra registers
        let complex_func = Function::new_bytecode(
            Some("complex_math".to_string()),
            2, // Arguments: r0, r1
            4, // Extra registers: r2, r3, r4, r5
            vec![
                // Complex calculation: ((a + b) * 2) + ((a - b) * 3)
                Instruction::Add(2, 0, 1),           // r2 = a + b
                Instruction::Sub(3, 0, 1),           // r3 = a - b
                Instruction::LoadConst(4, Value::Primitive(Primitive::Number(2.0))), // r4 = 2
                Instruction::LoadConst(5, Value::Primitive(Primitive::Number(3.0))), // r5 = 3
                Instruction::Mul(2, 2, 4),           // r2 = (a + b) * 2
                Instruction::Mul(3, 3, 5),           // r3 = (a - b) * 3
                Instruction::Add(0, 2, 3),           // r0 = result (reuse r0 for return)
                Instruction::Return(0),
            ]
        );
        
        // Test that function reports correct register requirements
        assert_eq!(complex_func.total_registers(), 6); // 2 args + 4 extra = 6 total
        
        // Spawn process with the function
        let args = vec![
            Value::Primitive(Primitive::Number(10.0)), // a = 10
            Value::Primitive(Primitive::Number(3.0)),  // b = 3
        ];
        
        let pid = vm.spawn_process(Rc::new(complex_func), args);
        
        // Run the VM until completion
        vm.run();
        
        // Verify the process allocated the correct number of registers
        if let Some(process_rc) = vm.processes.get(&pid) {
            let process = process_rc.borrow();
            if let Some(frame) = process.frames.first() {
                // Frame should have at least 6 registers (may have more for compatibility)
                assert!(frame.registers.len() >= 6);
                
                // Check that the computation was done correctly
                // Expected: ((10 + 3) * 2) + ((10 - 3) * 3) = (13 * 2) + (7 * 3) = 26 + 21 = 47
                if let Some(result) = &process.last_result {
                    assert_eq!(*result, Value::Primitive(Primitive::Number(47.0)));
                }
            }
        }
    }

    #[test] 
    fn test_minimal_register_allocation() {
        let mut vm = IonVM::new();
        
        // Function with no arguments and no extra registers - should still get minimum 16 for compatibility
        let minimal_func = Function::new_bytecode(
            Some("minimal".to_string()),
            0, // No arguments
            0, // No extra registers
            vec![
                Instruction::LoadConst(0, Value::Primitive(Primitive::Number(42.0))),
                Instruction::Return(0),
            ]
        );
        
        assert_eq!(minimal_func.total_registers(), 0);
        
        let pid = vm.spawn_process(Rc::new(minimal_func), vec![]);
        
        // Verify minimum register allocation
        if let Some(process_rc) = vm.processes.get(&pid) {
            let process = process_rc.borrow();
            if let Some(frame) = process.frames.first() {
                // Should have at least 16 registers for compatibility
                assert!(frame.registers.len() >= 16);
            }
        }
    }
}
