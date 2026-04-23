use super::Value;
use super::property::{PropertyAccess, PropertyDescriptor};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, PartialEq)]
pub struct Object {
    pub properties: HashMap<String, PropertyDescriptor>,
    pub prototype: Option<Rc<RefCell<Object>>>,
    // Hooks for __getattr__ / __setattr__ overrides
    pub magic_methods: HashMap<String, Value>,
    pub type_name: Option<String>,
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

    // External read: only Public and Readonly properties are accessible
    pub fn get_property(&self, key: &str) -> Option<Value> {
        if let Some(desc) = self.properties.get(key) {
            if desc.readable_externally() {
                return Some(desc.value.clone());
            } else {
                let kind = match desc.access {
                    PropertyAccess::Protected => "protected",
                    PropertyAccess::ProtectedReadonly => "protected readonly",
                    _ => "restricted",
                };
                panic!("cannot read {} property '{}'", kind, key);
            }
        }
        if let Some(proto) = &self.prototype {
            return proto.borrow().get_property(key);
        }
        None
    }

    // this.prop read: all access levels are visible
    pub fn get_this_property(&self, key: &str) -> Option<Value> {
        if let Some(desc) = self.properties.get(key) {
            return Some(desc.value.clone());
        }
        if let Some(proto) = &self.prototype {
            return proto.borrow().get_property(key);
        }
        None
    }

    // External write: only Public properties
    pub fn set_property(&mut self, key: &str, value: Value) {
        if self.magic_methods.contains_key("__setattr__") {
            return;
        }
        let desc = self
            .properties
            .entry(key.to_string())
            .or_insert_with(|| PropertyDescriptor::public(value.clone()));
        if desc.writable_externally() {
            desc.value = value;
        } else {
            eprintln!("Warning: '{}' is not externally writable", key);
        }
    }

    // this.prop write: Public and Protected
    pub fn set_this_property(&mut self, key: &str, value: Value) {
        if self.magic_methods.contains_key("__setattr__") {
            return;
        }
        let desc = self
            .properties
            .entry(key.to_string())
            .or_insert_with(|| PropertyDescriptor::public(value.clone()));
        if desc.writable_by_this() {
            desc.value = value;
        } else {
            eprintln!("Warning: '{}' is not writable even via this", key);
        }
    }
}

#[cfg(test)]
mod native_ffi_tests {
    use super::*;
    use crate::value::{Primitive, Function, NativeInterface};
    use crate::vm::{IonVM, ExecutionResult, Instruction};
    use crate::value::process::Process;

    #[derive(Debug)]
    struct MockNative {
        pub last_this: Option<Value>,
        pub bytecode_to_call: Option<Rc<RefCell<Function>>>,
    }

    impl NativeInterface for MockNative {
        fn tag(&self) -> String { "mock".to_string() }
        fn inner_value(&self) -> Value { Value::Primitive(Primitive::Unit) }
        fn get_property(&self, key: &str) -> Value {
            if key == "last_this" {
                return self.last_this.clone().unwrap_or(Value::Primitive(Primitive::Undefined));
            }
            if key == "set_this" {
                return Value::Function(Rc::new(RefCell::new(Function::new_ffi(Some("set_this".to_string()), 0, "set_this".to_string()))));
            }
            if key == "call_bytecode" {
                return Value::Function(Rc::new(RefCell::new(Function::new_ffi(Some("call_bytecode".to_string()), 1, "call_bytecode".to_string()))));
            }
            Value::Primitive(Primitive::Undefined)
        }
        fn set_property(&mut self, _key: &str, _value: Value) {}
        fn call_method(
            &mut self,
            _vm: &mut IonVM,
            proc: &mut Process,
            bound_this: Option<Value>,
            name: &str,
            args: Vec<Value>,
            dst_reg: usize,
        ) -> ExecutionResult {
            if name == "set_this" {
                self.last_this = bound_this;
                return ExecutionResult::Exited(Value::Primitive(Primitive::Unit));
            }
            if name == "call_bytecode" {
                if let Some(f) = &self.bytecode_to_call {
                    let total_regs = f.borrow().total_registers();
                    let mut registers = args;
                    registers.resize(total_regs, Value::Primitive(Primitive::Undefined));

                    proc.frames.push(crate::value::Frame {
                        registers,
                        ip: 0,
                        function: f.clone(),
                        return_value: None,
                        caller_return_reg: Some(dst_reg),
                        scope_environments: std::collections::HashMap::new(),
                    });
                    return ExecutionResult::Continue;
                }
            }
            ExecutionResult::Exited(Value::Primitive(Primitive::Unit))
        }
    }

    #[test]
    fn test_native_object_bound_this() {
        let mut vm = IonVM::new();
        let mock = Rc::new(RefCell::new(MockNative { last_this: None, bytecode_to_call: None }));
        let native_val = Value::Native(mock.clone());

        let bytecode = vec![
            Instruction::LoadConst(1, Value::Primitive(Primitive::Atom("set_this".to_string()))),
            Instruction::GetProp(2, 0, 1),
            Instruction::Call(3, 2, vec![]),
            Instruction::Return(3),
        ];

        let func = Function::new_bytecode(Some("test".to_string()), 1, 10, bytecode);
        let _pid = vm.spawn_process(Rc::new(RefCell::new(func)), vec![native_val.clone()]);

        vm.run();

        let last_this = mock.borrow().last_this.clone().expect("set_this should have been called");
        assert_eq!(last_this, native_val);
    }

    #[test]
    fn test_native_calls_bytecode() {
        let mut vm = IonVM::new();

        let target_bytecode = vec![
            Instruction::LoadConst(1, Value::Primitive(Primitive::Number(10.0))),
            Instruction::Add(2, 0, 1),
            Instruction::Return(2),
        ];
        let target_func = Rc::new(RefCell::new(Function::new_bytecode(Some("target".to_string()), 1, 5, target_bytecode)));

        let mock = Rc::new(RefCell::new(MockNative {
            last_this: None,
            bytecode_to_call: Some(target_func.clone())
        }));
        let native_val = Value::Native(mock.clone());

        let main_bytecode = vec![
            Instruction::LoadConst(1, Value::Primitive(Primitive::Atom("call_bytecode".to_string()))),
            Instruction::GetProp(2, 0, 1),
            Instruction::LoadConst(3, Value::Primitive(Primitive::Number(5.0))),
            Instruction::Call(4, 2, vec![3]),
            Instruction::Return(4),
        ];

        let main_func = Function::new_bytecode(Some("main".to_string()), 1, 10, main_bytecode);
        let pid = vm.spawn_process(Rc::new(RefCell::new(main_func)), vec![native_val.clone()]);

        vm.run();

        let proc = vm.processes.get(&pid).unwrap().borrow();
        assert_eq!(proc.last_result, Some(Value::Primitive(Primitive::Number(15.0))));
    }
}
