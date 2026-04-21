use crate::value::process::Frame;
use crate::value::{Object, ObjectInitArg, Primitive, PropertyAccess, PropertyDescriptor, Value};
use std::cell::RefCell;
use std::rc::Rc;

pub(super) fn exec_object_init(frame: &mut Frame, dst: usize, kvs: Vec<(String, ObjectInitArg)>) {
    let mut obj = Object::new(None);
    for (key, arg) in kvs {
        let (value, access) = match arg {
            ObjectInitArg::Register(reg) => (frame.registers[reg].clone(), PropertyAccess::Public),
            ObjectInitArg::Value(val) => (val, PropertyAccess::Public),
            ObjectInitArg::RegisterWithAccess(reg, acc) => (frame.registers[reg].clone(), acc),
            ObjectInitArg::ValueWithAccess(val, acc) => (val, acc),
        };
        obj.properties
            .insert(key, PropertyDescriptor::with_access(value, access));
    }
    frame.registers[dst] = Value::Object(Rc::new(RefCell::new(obj)));
}

pub(super) fn exec_get_prop(frame: &mut Frame, dst: usize, obj_reg: usize, key_reg: usize) {
    let obj_val = frame.registers[obj_reg].clone();
    let result = match (&obj_val, &frame.registers[key_reg]) {
        (Value::Object(obj_rc), Value::Primitive(Primitive::Atom(key))) => obj_rc
            .borrow()
            .get_property(key)
            .unwrap_or(Value::Primitive(Primitive::Undefined)),
        (Value::Array(arr_rc), Value::Primitive(Primitive::Atom(key))) => {
            key.parse::<usize>()
                .ok()
                .and_then(|idx| arr_rc.borrow().get(idx).cloned())
                .unwrap_or(Value::Primitive(Primitive::Undefined))
        }
        (Value::Tuple(tup_rc), Value::Primitive(Primitive::Atom(key))) => key
            .parse::<usize>()
            .ok()
            .and_then(|idx| tup_rc.get(idx).cloned())
            .unwrap_or(Value::Primitive(Primitive::Undefined)),
        // __vm:this is a sentinel that routes the read through `this.prop` visibility rules
        (Value::Primitive(Primitive::Atom(sentinel)), Value::Primitive(Primitive::Atom(key)))
            if sentinel == "__vm:this" =>
        {
            if let Some(Value::Object(this)) = &frame.function.borrow().bound_this {
                this.borrow()
                    .get_this_property(key)
                    .unwrap_or(Value::Primitive(Primitive::Undefined))
            } else {
                panic!("__vm:this used but the current function has no bound_this")
            }
        }
        _ => Value::Primitive(Primitive::Undefined),
    };

    // When a function is read from an object, bind `this` to the object on a fresh copy
    let result = if let Value::Function(func_rc) = &result {
        if func_rc.borrow().bound_this.is_none() {
            Value::Function(Rc::new(RefCell::new(
                func_rc.borrow().with_bound_this(obj_val),
            )))
        } else {
            result
        }
    } else {
        result
    };

    frame.registers[dst] = result;
}

pub(super) fn exec_set_prop(frame: &mut Frame, obj_reg: usize, key_reg: usize, val_reg: usize) {
    let obj_val = frame.registers[obj_reg].clone();
    let key_val = frame.registers[key_reg].clone();
    if let (Value::Object(obj_rc), Value::Primitive(Primitive::Atom(key))) = (obj_val, key_val) {
        let value = frame.registers[val_reg].clone();
        obj_rc.borrow_mut().set_property(&key, value);
    }
}
