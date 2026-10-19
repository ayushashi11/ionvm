use std::sync::{Arc, RwLock};
use crate::bytecode::instruction::OperandType;
use crate::lang::types::{Function, Property, Value};

#[derive(Clone, Debug)]
pub struct Frame {
    pub function_index: usize,
    pub bound_function: Option<Arc<RwLock<Function>>>,
    pub registers: Vec<Value>,
    pub arguments: Vec<Value>,
    pub pc: usize,
    pub ret_store: OperandType,
    pub cached_indexed_items: [Option<Vec<Option<Property>>>; 8],
    pub cached_int_arrays: [Option<Vec<i64>>; 8],
}

impl Frame {
    pub fn new(
        function_index: usize,
        reg_count: usize,
        arguments: Vec<Value>,
        ret_store: OperandType,
    ) -> Self {
        let mut cached = [const { None }; 8];
        let mut cached_ints = [const { None }; 8];
        for (i, arg) in arguments.iter().enumerate().take(8) {
            match arg {
                Value::IntArray(arr) => {
                    cached_ints[i] = Some(arr.read().unwrap().clone());
                }
                Value::Object(obj) => {
                    cached[i] = Some(obj.read().unwrap().indexed.clone());
                }
                _ => {}
            }
        }

        Frame {
            function_index,
            bound_function: None,
            registers: vec![Value::Nil; reg_count],
            arguments,
            pc: 0,
            ret_store,
            cached_indexed_items: cached,
            cached_int_arrays: cached_ints,
        }
    }
}
