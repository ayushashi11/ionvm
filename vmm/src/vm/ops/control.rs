use super::super::{ExecutionResult, IonVM};
use super::comparison::truthy;
use crate::ffi_integration::{FfiCallResult, call_ffi_function};
use crate::value::process::{Frame, Process, ProcessStatus};
use crate::value::{FunctionType, Primitive, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub(super) fn exec_jump(frame: &mut Frame, offset: isize) {
    // IP was already incremented before this call, so subtract 1 to get net offset
    frame.ip = (frame.ip as isize + offset - 1) as usize;
}

pub(super) fn exec_jump_if_true(frame: &mut Frame, cond_reg: usize, offset: isize) {
    if truthy(&frame.registers[cond_reg]) {
        frame.ip = (frame.ip as isize + offset - 1) as usize;
    }
}

pub(super) fn exec_jump_if_false(frame: &mut Frame, cond_reg: usize, offset: isize) {
    if !truthy(&frame.registers[cond_reg]) {
        frame.ip = (frame.ip as isize + offset - 1) as usize;
    }
}

pub(super) fn exec_make_closure(
    proc: &mut Process,
    dst_reg: usize,
    func_reg: usize,
    scope_id: String,
    captures: Vec<(String, usize)>,
) {
    let capture_order: Vec<String> = captures.iter().map(|(name, _)| name.clone()).collect();
    let (template_value, captured_values) = {
        let frame = proc.frames.last().expect("active frame required");
        let template = frame.registers[func_reg].clone();
        let values = captures
            .iter()
            .map(|(name, reg)| (name.clone(), frame.registers[*reg].clone()))
            .collect::<Vec<_>>();
        (template, values)
    };

    let frame = proc.frames.last_mut().expect("active frame required");
    let env_rc = frame
        .scope_environments
        .entry(scope_id)
        .or_insert_with(|| Rc::new(RefCell::new(HashMap::new())))
        .clone();

    {
        let mut env = env_rc.borrow_mut();
        for (name, value) in captured_values {
            env.entry(name).or_insert(value);
        }
    }

    match template_value {
        Value::Function(func_rc) => {
            let mut closure_fn = func_rc.borrow().clone();
            closure_fn.closure_env = Some(env_rc);
            closure_fn.capture_order = capture_order;
            frame.registers[dst_reg] = Value::Function(Rc::new(RefCell::new(closure_fn)));
        }
        _ => {
            frame.registers[dst_reg] = Value::Primitive(Primitive::Undefined);
        }
    }
}

pub(super) fn exec_return(proc: &mut Process, reg: usize) -> ExecutionResult {
    let return_val = proc
        .frames
        .last()
        .map(|f| f.registers[reg].clone())
        .unwrap_or(Value::Primitive(Primitive::Unit));

    if let Some(frame) = proc.frames.last_mut() {
        frame.return_value = Some(return_val.clone());
    }

    let is_main = proc.frames.len() == 1;
    if is_main {
        proc.alive = false;
        proc.status = ProcessStatus::Exited;
        proc.last_result = Some(return_val);
    } else {
        let caller_reg = proc.frames.last().unwrap().caller_return_reg;
        proc.frames.pop();
        if let (Some(r), Some(caller)) = (caller_reg, proc.frames.last_mut()) {
            caller.registers[r] = return_val;
        }
    }
    ExecutionResult::Continue
}

pub(super) fn exec_call(
    vm: &IonVM,
    proc: &mut Process,
    dst_reg: usize,
    func_reg: usize,
    arg_regs: Vec<usize>,
) -> ExecutionResult {
    let (func_val, args) = {
        let frame = proc.frames.last().expect("active frame required");
        //dbg!(frame.function.name.clone());
        let func_val = frame.registers[func_reg].clone();
        let args: Vec<Value> = arg_regs
            .iter()
            .map(|&r| frame.registers[r].clone())
            .collect();
        (func_val, args)
    };

    match func_val {
        Value::Function(func_rc) => {
            let function_type = func_rc.borrow().function_type.clone();
            match function_type {
                FunctionType::Bytecode { .. } => {
                    let (total_regs, prefix) = {
                        let func = func_rc.borrow();
                        let prefix = if let Some(env_rc) = &func.closure_env {
                            let env = env_rc.borrow();
                            func.capture_order
                                .iter()
                                .map(|name| {
                                    env.get(name)
                                        .cloned()
                                        .unwrap_or(Value::Primitive(Primitive::Undefined))
                                })
                                .collect::<Vec<_>>()
                        } else {
                            Vec::new()
                        };
                        (func.total_registers(), prefix)
                    };

                    let mut registers = prefix;
                    registers.extend(args);
                    registers.resize(total_regs, Value::Primitive(Primitive::Undefined));
                    proc.frames.push(Frame {
                        registers,
                        ip: 0,
                        function: func_rc.clone(),
                        return_value: None,
                        caller_return_reg: Some(dst_reg),
                        scope_environments: HashMap::new(),
                    });
                    ExecutionResult::Continue
                }
                FunctionType::Ffi { function_name } => {
                    let result = call_ffi_function(&vm.ffi_registry, &function_name, args);
                    let frame = proc.frames.last_mut().expect("active frame required");
                    match result {
                        FfiCallResult::Success(v) => frame.registers[dst_reg] = v,
                        FfiCallResult::Error(e) => {
                            frame.registers[dst_reg] =
                                Value::Primitive(Primitive::Atom(format!("error:{}", e)));
                        }
                        FfiCallResult::Yield(_) => todo!("FFI yield not yet implemented"),
                    }
                    ExecutionResult::Continue
                }
            }
        }

        Value::Closure(closure_rc) => {
            let total_regs = closure_rc.function.total_registers().max(16);
            let mut registers: Vec<Value> = closure_rc.environment.values().cloned().collect();
            registers.extend(args);
            registers.resize(total_regs, Value::Primitive(Primitive::Undefined));
            proc.frames.push(Frame {
                registers,
                ip: 0,
                function: Rc::new(RefCell::new(closure_rc.function.as_ref().clone())), //TODO: change
                return_value: None,
                caller_return_reg: Some(dst_reg),
                scope_environments: HashMap::new(),
            });
            ExecutionResult::Continue
        }

        other => panic!("attempted to call a non-function value: {:?}", other),
    }
}
