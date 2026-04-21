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
