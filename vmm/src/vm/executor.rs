#![allow(non_snake_case)]

use std::sync::{Arc, RwLock};

use crate::builtin;
use crate::bytecode::instruction::{Bytecode, OperandType};
use crate::bytecode::opcode::OPCODE_COUNT;
use crate::lang::{FunctionType, Object, Protection, Value, format_value_with_spec};
use crate::lang::types::{Function, NativeJitFn};
use crate::vm::Scheduler;
use crate::vm::frame::Frame;
use crate::vm::process::{Process, WaitReason, WaitState};

#[derive(Clone, PartialEq, Debug)]
pub enum ExecutionResult {
    Continue,
    Wait(Box<WaitState>),
    Terminate,
    Error(String),
}

pub struct ExecutionContext<'a> {
    pub bytecode: &'a Bytecode,
    pub process: &'a mut Process,
    pub scheduler: &'a Scheduler,
}

/// Invoke compiled code with a mutable pointer to the bound receiver, if one exists.
/// The write guard intentionally remains alive for the duration of the JIT call so the
/// pointer into `Function::bound_this` stays valid and mutations persist.
#[inline]
unsafe fn call_native_jit_with_self(
    native_fn: NativeJitFn,
    registers: *mut Value,
    bound_function: Option<&Arc<RwLock<Function>>>,
    fallback_bound_function: Option<&Arc<RwLock<Function>>>,
    args: *const Value,
    env: *mut Value,
    out_res: *mut Value,
) {
    if let Some(bound_function) = bound_function {
        let mut function = bound_function.write().unwrap();
        if let Some(receiver) = function.bound_this.as_mut() {
            unsafe { native_fn(registers, receiver as *mut Value, args, env, out_res) };
            return;
        }
    }

    if let Some(bound_function) = fallback_bound_function {
        let mut function = bound_function.write().unwrap();
        if let Some(receiver) = function.bound_this.as_mut() {
            unsafe { native_fn(registers, receiver as *mut Value, args, env, out_res) };
            return;
        }
    }

    let mut nil_self = Value::Nil;
    unsafe { native_fn(registers, &mut nil_self, args, env, out_res) };
}

pub type HandlerFn = for<'a> fn(&'a mut ExecutionContext<'a>) -> ExecutionResult;

#[inline(always)]
pub fn resolve_operand(bytecode: &Bytecode, proc: &Process, op: &OperandType) -> Value {
    let frame = proc.frame_stack.last().expect("active frame required");
    match op {
        OperandType::Register(r) => frame.registers.get(*r).cloned().unwrap_or(Value::Nil),
        OperandType::Constant(c) => c.clone(),
        OperandType::Argument(a) => frame.arguments.get(*a).cloned().unwrap_or(Value::Nil),
        OperandType::SelfProc => Value::Int(proc.id as i64),
        OperandType::This => frame
            .bound_function
            .as_ref()
            .and_then(|f| f.read().unwrap().bound_this.clone())
            .unwrap_or(Value::Nil),
        OperandType::Function(name) => {
            if let Some(func) = bytecode.functions.iter().find(|f| f.name == *name) {
                Value::Function(Arc::new(RwLock::new(func.clone())))
            } else {
                Value::Nil
            }
        }
        OperandType::Captured(i) => frame
            .closure_env
            .as_ref()
            .and_then(|env| env.read().unwrap().get(*i).cloned())
            .unwrap_or(Value::Nil),
        OperandType::Property(proto_name) => {
            if let Some((_, proto)) = bytecode
                .object_inits
                .iter()
                .find(|(name, _)| name == proto_name)
            {
                Value::Object(Arc::clone(proto))
            } else {
                Value::Nil
            }
        }
        _ => Value::Nil,
    }
}

#[inline(always)]
pub fn set_reg_value_in_frame(frame: &mut Frame, reg_idx: usize, val: Value) {
    if reg_idx >= frame.registers.len() {
        frame.registers.resize(reg_idx + 1, Value::Nil);
    }

    frame.registers[reg_idx] = val;
}

#[inline(always)]
pub fn set_reg_value(proc: &mut Process, reg_idx: usize, val: Value) {
    let frame = proc.frame_stack.last_mut().expect("active frame required");
    set_reg_value_in_frame(frame, reg_idx, val);
}

#[inline(always)]
pub fn save_value_to_operand(proc: &mut Process, op: &OperandType, val: Value) {
    match op {
        OperandType::Register(r) => set_reg_value(proc, *r, val),
        OperandType::Captured(i) => {
            if let Some(frame) = proc.frame_stack.last() {
                if let Some(env) = &frame.closure_env {
                    let mut lock = env.write().unwrap();
                    if *i < lock.len() {
                        lock[*i] = val;
                    }
                }
            }
        }
        _ => {}
    }
}

#[inline(always)]
pub fn fetch_and_dispatch<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    if ctx.process.reductions == 0 {
        return ExecutionResult::Continue;
    }
    if ctx.process.frame_stack.is_empty() {
        return ExecutionResult::Terminate;
    }

    let frame = ctx.process.frame_stack.last().expect("active frame");
    let pc = frame.pc;
    let func = &ctx.bytecode.functions[frame.function_index];
    if pc >= func.instructions.len() {
        return ExecutionResult::Terminate;
    }

    let opcode = func.instructions[pc].opcode;
    let handler: HandlerFn = DISPATCH_TABLE[opcode as usize];
    become handler(ctx);
}

// -----------------------------------------------------------------------------
// Opcode Handlers
// -----------------------------------------------------------------------------


// -----------------------------------------------------------------------------
// Control Flow Handlers
// -----------------------------------------------------------------------------

fn op_load_const<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    save_value_to_operand(ctx.process, &inst.operands[1], val);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_nop<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_swap<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    if let (OperandType::Register(r1), OperandType::Register(r2)) =
        (&inst.operands[0], &inst.operands[1])
    {
        if *r1 < frame.registers.len() && *r2 < frame.registers.len() {
            frame.registers.swap(*r1, *r2);
        }
    }
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_jump<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let off = match inst.operands[0] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let new_pc = frame.pc as i64 + off;
    frame.pc = new_pc as usize;
    if off < 0 {
        ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
    }
    become fetch_and_dispatch(ctx);
}


fn op_jump_true<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let cond = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let off = match inst.operands[1] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    let is_truthy = match cond {
        Value::Bool(b) => b,
        Value::Nil => false,
        Value::Int(i) => i != 0,
        _ => true,
    };
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    if is_truthy {
        let new_pc = frame.pc as i64 + off;
        frame.pc = new_pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_jump_false<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let cond = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let off = match inst.operands[1] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    let is_falsy = match cond {
        Value::Bool(b) => !b,
        Value::Nil => true,
        Value::Int(i) => i == 0,
        _ => false,
    };
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    if is_falsy {
        let new_pc = frame.pc as i64 + off;
        frame.pc = new_pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_I_jump_true_R<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let off = match inst.operands[1] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    let val = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i != 0,
        Some(Value::Bool(b)) => *b,
        _ => false,
    };
    if val {
        let pc = frame.pc as i64 + off;
        frame.pc = pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_I_jump_false_R<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let off = match inst.operands[1] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    let val = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i == 0,
        Some(Value::Bool(b)) => !*b,
        _ => true,
    };
    if val {
        let pc = frame.pc as i64 + off;
        frame.pc = pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_call<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let caller_bound_function = frame.bound_function.clone();

    if let OperandType::Function(ref name) = inst.operands[0] {
        if let Some(&target_idx) = ctx.scheduler.function_indices.get(name) {
            let target_func = &ctx.bytecode.functions[target_idx];
            if let Some(native_fn) = target_func.jit_compiled_fn {
                let arg_count = match inst.operands[1] {
                    OperandType::Offset(o) => o as usize,
                    _ => 0,
                };
                let dst_reg = match inst.operands[2 + arg_count] {
                    OperandType::Register(r) => r,
                    _ => 0,
                };
                let mut stack_args: [std::mem::MaybeUninit<Value>; 16] =
                    unsafe { std::mem::MaybeUninit::uninit().assume_init() };
                for i in 0..arg_count {
                    let val = match &inst.operands[2 + i] {
                        OperandType::Register(r) => {
                            if let Some(reg_val) = frame.registers.get(*r) {
                                unsafe { std::ptr::read(reg_val as *const Value) }
                            } else {
                                Value::Nil
                            }
                        }
                        OperandType::Argument(a) => {
                            if let Some(arg_val) = frame.arguments.get(*a) {
                                unsafe { std::ptr::read(arg_val as *const Value) }
                            } else {
                                Value::Nil
                            }
                        }
                        OperandType::Constant(c) => unsafe { std::ptr::read(c as *const Value) },
                        _ => Value::Nil,
                    };
                    stack_args[i].write(val);
                }
                let mut stack_regs = [const { Value::Nil }; 64];
                let mut out_res = Value::Nil;
                let env_ptr = target_func
                    .closure_env
                    .as_ref()
                    .map(|e| e.write().unwrap().as_mut_ptr())
                    .unwrap_or(std::ptr::null_mut());
                unsafe {
                    call_native_jit_with_self(
                        native_fn,
                        stack_regs.as_mut_ptr(),
                        caller_bound_function.as_ref(),
                        None,
                        stack_args.as_ptr() as *const Value,
                        env_ptr,
                        &mut out_res,
                    );
                }
                std::mem::forget(stack_args);
                std::mem::forget(stack_regs);
                let frame_mut = ctx.process.frame_stack.last_mut().unwrap();
                set_reg_value_in_frame(frame_mut, dst_reg, out_res);
                frame_mut.pc += 1;
                become fetch_and_dispatch(ctx);
            }
        }
    }

    let (callee_val, _arg_count, dst_op, args) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let callee_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let arg_count = match inst.operands[1] {
            OperandType::Offset(o) => o as usize,
            _ => 0,
        };
        let dst = inst.operands[2 + arg_count].clone();
        let mut args = Vec::with_capacity(arg_count);
        for i in 0..arg_count {
            args.push(resolve_operand(
                ctx.bytecode,
                ctx.process,
                &inst.operands[2 + i],
            ));
        }
        (callee_val, arg_count, dst, args)
    };

    match callee_val {
        Value::Function(f) => {
            let (fn_name, reg_count, fn_type, closure_env, bound_this) = {
                let f_lock = f.read().unwrap();
                (
                    f_lock.name.clone(),
                    f_lock.register_count,
                    f_lock.fn_type.clone(),
                    f_lock.closure_env.clone(),
                    f_lock.bound_this.clone(),
                )
            };
            match fn_type {
                FunctionType::Regular | FunctionType::Closure => {
                    let target_idx = ctx
                        .bytecode
                        .functions
                        .iter()
                        .position(|func| func.name == fn_name)
                        .unwrap_or(0);
                    let target_func = &ctx.bytecode.functions[target_idx];
                    if let Some(native_fn) = target_func.jit_compiled_fn {
                        let mut out_res = Value::Nil;
                        let mut regs = std::mem::ManuallyDrop::new(vec![
                            Value::Nil;
                            target_func.register_count
                        ]);
                        let env_ptr = closure_env
                            .as_ref()
                            .or(target_func.closure_env.as_ref())
                            .map(|e| e.write().unwrap().as_mut_ptr())
                            .unwrap_or(std::ptr::null_mut());
                        unsafe {
                            call_native_jit_with_self(
                                native_fn,
                                regs.as_mut_ptr(),
                                Some(&f),
                                caller_bound_function.as_ref(),
                                args.as_ptr(),
                                env_ptr,
                                &mut out_res,
                            );
                        }
                        save_value_to_operand(ctx.process, &dst_op, out_res);
                        ctx.process.frame_stack.last_mut().unwrap().pc += 1;
                        become fetch_and_dispatch(ctx);
                    }
                    let mut new_frame = Frame::new(target_idx, reg_count, args, dst_op);
                    new_frame.closure_env = closure_env;
                    new_frame.bound_function = Some(f.clone());
                    ctx.process.frame_stack.push(new_frame);
                    become fetch_and_dispatch(ctx);
                }
                FunctionType::Native => {
                    let res = if let Some(ref this_val) = bound_this {
                        let mut call_args = Vec::with_capacity(1 + args.len());
                        call_args.push(this_val.clone());
                        call_args.extend(args);
                        builtin::call_native_with_bytecode(
                            &fn_name,
                            &call_args,
                            Some(ctx.bytecode),
                        )
                        .unwrap_or(Value::Nil)
                    } else {
                        builtin::call_native_with_bytecode(
                            &fn_name,
                            &args,
                            Some(ctx.bytecode),
                        )
                        .unwrap_or(Value::Nil)
                    };
                    save_value_to_operand(ctx.process, &dst_op, res);
                    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
                    become fetch_and_dispatch(ctx);
                }
                _ => {
                    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
                    become fetch_and_dispatch(ctx);
                }
            }
        }
        _ => {
            let frame = ctx.process.frame_stack.last().unwrap();
            let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
            if let OperandType::Function(name) = &inst.operands[0] {
                if let Some(res) =
                    builtin::call_native_with_bytecode(name, &args, Some(ctx.bytecode))
                {
                    save_value_to_operand(ctx.process, &dst_op, res);
                    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
                    become fetch_and_dispatch(ctx);
                } else if let Some(target_idx) =
                    ctx.bytecode.functions.iter().position(|f| f.name == *name)
                {
                    let target_func = &ctx.bytecode.functions[target_idx];
                    if let Some(native_fn) = target_func.jit_compiled_fn {
                        let mut out_res = Value::Nil;
                        let mut regs = vec![Value::Nil; target_func.register_count];
                        let env_ptr = target_func
                            .closure_env
                            .as_ref()
                            .map(|e| e.write().unwrap().as_mut_ptr())
                            .unwrap_or(std::ptr::null_mut());
                        let caller_bound_function = frame.bound_function.clone();
                        unsafe {
                            call_native_jit_with_self(
                                native_fn,
                                regs.as_mut_ptr(),
                                caller_bound_function.as_ref(),
                                None,
                                args.as_ptr(),
                                env_ptr,
                                &mut out_res,
                            );
                        }
                        save_value_to_operand(ctx.process, &dst_op, out_res);
                        ctx.process.frame_stack.last_mut().unwrap().pc += 1;
                        become fetch_and_dispatch(ctx);
                    }
                    let new_frame =
                        Frame::new(target_idx, target_func.register_count, args, dst_op);
                    ctx.process.frame_stack.push(new_frame);
                    become fetch_and_dispatch(ctx);
                } else {
                    ExecutionResult::Error(format!("function '{}' not found", name))
                }
            } else {
                ExecutionResult::Error("callee not callable".to_string())
            }
        }
    }
}


fn op_call_jit<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let caller_bound_function = frame.bound_function.clone();
    let target_idx = match inst.operands[0] {
        OperandType::Offset(o) => o as usize,
        _ => 0,
    };
    let arg_count = match inst.operands[1] {
        OperandType::Offset(o) => o as usize,
        _ => 0,
    };
    let dst_reg = match inst.operands[2 + arg_count] {
        OperandType::Register(r) => r,
        _ => 0,
    };

    let target_func = &ctx.bytecode.functions[target_idx];
    let native_fn = target_func.jit_compiled_fn.unwrap();

    let mut stack_args: [std::mem::MaybeUninit<Value>; 16] =
        unsafe { std::mem::MaybeUninit::uninit().assume_init() };
    for i in 0..arg_count {
        let val = match &inst.operands[2 + i] {
            OperandType::Register(r) => {
                if let Some(reg_val) = frame.registers.get(*r) {
                    unsafe { std::ptr::read(reg_val as *const Value) }
                } else {
                    Value::Nil
                }
            }
            OperandType::Argument(a) => {
                if let Some(arg_val) = frame.arguments.get(*a) {
                    unsafe { std::ptr::read(arg_val as *const Value) }
                } else {
                    Value::Nil
                }
            }
            OperandType::Constant(c) => unsafe { std::ptr::read(c as *const Value) },
            _ => Value::Nil,
        };
        stack_args[i].write(val);
    }

    let mut stack_regs = [const { Value::Nil }; 64];
    let mut out_res = Value::Nil;
    let env_ptr = target_func
        .closure_env
        .as_ref()
        .map(|e| e.write().unwrap().as_mut_ptr())
        .unwrap_or(std::ptr::null_mut());
    unsafe {
        call_native_jit_with_self(
            native_fn,
            stack_regs.as_mut_ptr(),
            caller_bound_function.as_ref(),
            None,
            stack_args.as_ptr() as *const Value,
            env_ptr,
            &mut out_res,
        );
    }
    std::mem::forget(stack_args);
    std::mem::forget(stack_regs);

    let frame_mut = ctx.process.frame_stack.last_mut().unwrap();
    set_reg_value_in_frame(frame_mut, dst_reg, out_res);
    frame_mut.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_ret<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let ret_val = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        if inst.operands.is_empty() {
            Value::Nil
        } else {
            resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0])
        }
    };

    let popped = ctx.process.frame_stack.pop().unwrap();
    if ctx.process.frame_stack.is_empty() {
        ctx.process.entrypoint_return = Some(ret_val);
        return ExecutionResult::Terminate;
    }

    save_value_to_operand(ctx.process, &popped.ret_store, ret_val);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// Generic Arithmetic Handlers
// -----------------------------------------------------------------------------

fn op_add<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = match (&l, &r) {
        (Value::Int(a), Value::Int(b)) => Value::Int(a + b),
        (Value::Float(a), Value::Float(b)) => Value::Float(a + b),
        (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 + b),
        (Value::Float(a), Value::Int(b)) => Value::Float(a + *b as f64),
        _ => builtin::arithmetic::add(&l, &r).unwrap_or(Value::Nil),
    };
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_sub<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = match (&l, &r) {
        (Value::Int(a), Value::Int(b)) => Value::Int(a - b),
        (Value::Float(a), Value::Float(b)) => Value::Float(a - b),
        (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 - b),
        (Value::Float(a), Value::Int(b)) => Value::Float(a - *b as f64),
        _ => builtin::arithmetic::sub(&l, &r).unwrap_or(Value::Nil),
    };
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_mul<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = match (&l, &r) {
        (Value::Int(a), Value::Int(b)) => Value::Int(a * b),
        (Value::Float(a), Value::Float(b)) => Value::Float(a * b),
        (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 * b),
        (Value::Float(a), Value::Int(b)) => Value::Float(a * *b as f64),
        _ => builtin::arithmetic::mul(&l, &r).unwrap_or(Value::Nil),
    };
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_div<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = match (&l, &r) {
        (Value::Int(a), Value::Int(b)) => {
            if *b != 0 {
                Value::Int(a / b)
            } else {
                Value::Nil
            }
        }
        (Value::Float(a), Value::Float(b)) => Value::Float(a / b),
        (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 / b),
        (Value::Float(a), Value::Int(b)) => Value::Float(a / *b as f64),
        _ => builtin::arithmetic::div(&l, &r).unwrap_or(Value::Nil),
    };
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_rem<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = match (&l, &r) {
        (Value::Int(a), Value::Int(b)) => {
            if *b != 0 {
                Value::Int(a % b)
            } else {
                Value::Nil
            }
        }
        _ => builtin::arithmetic::rem(&l, &r).unwrap_or(Value::Nil),
    };
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_neg<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let res = match val {
        Value::Int(i) => Value::Int(-i),
        Value::Float(f) => Value::Float(-f),
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &inst.operands[1], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// Generic Comparison Handlers
// -----------------------------------------------------------------------------

fn op_eq<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = Value::Bool(l.eql(&r));
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_neq<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = Value::Bool(!l.eql(&r));
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_lt<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = match (&l, &r) {
        (Value::Int(a), Value::Int(b)) => Value::Bool(a < b),
        (Value::Float(a), Value::Float(b)) => Value::Bool(a < b),
        (Value::Int(a), Value::Float(b)) => Value::Bool((*a as f64) < *b),
        (Value::Float(a), Value::Int(b)) => Value::Bool(*a < (*b as f64)),
        _ => Value::Bool(builtin::comparisons::less_than(&l, &r).unwrap_or(false)),
    };
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_lte<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = match (&l, &r) {
        (Value::Int(a), Value::Int(b)) => Value::Bool(a <= b),
        (Value::Float(a), Value::Float(b)) => Value::Bool(a <= b),
        _ => Value::Bool(builtin::comparisons::less_than_or_equal(&l, &r).unwrap_or(false)),
    };
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_gt<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = match (&l, &r) {
        (Value::Int(a), Value::Int(b)) => Value::Bool(a > b),
        (Value::Float(a), Value::Float(b)) => Value::Bool(a > b),
        (Value::Int(a), Value::Float(b)) => Value::Bool((*a as f64) > *b),
        (Value::Float(a), Value::Int(b)) => Value::Bool(*a > (*b as f64)),
        _ => Value::Bool(builtin::comparisons::greater_than(&l, &r).unwrap_or(false)),
    };
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_gte<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = match (&l, &r) {
        (Value::Int(a), Value::Int(b)) => Value::Bool(a >= b),
        (Value::Float(a), Value::Float(b)) => Value::Bool(a >= b),
        _ => Value::Bool(builtin::comparisons::greater_than_or_equal(&l, &r).unwrap_or(false)),
    };
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// Logical Handlers
// -----------------------------------------------------------------------------

fn op_and<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = builtin::logical::and(&l, &r).unwrap_or(Value::Nil);
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_or<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = builtin::logical::or(&l, &r).unwrap_or(Value::Nil);
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_xor<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = match (&l, &r) {
        (Value::Int(a), Value::Int(b)) => Value::Int(a ^ b),
        (Value::Bool(a), Value::Bool(b)) => Value::Bool(a ^ b),
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_not<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let res = builtin::logical::not(&val).unwrap_or(Value::Nil);
    save_value_to_operand(ctx.process, &inst.operands[1], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// Bitwise & Shift Handlers
// -----------------------------------------------------------------------------

fn op_shl<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = builtin::logical::shl(&l, &r).unwrap_or(Value::Nil);
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_shr<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_idx, pc) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        (frame.function_index, frame.pc)
    };
    let inst = &ctx.bytecode.functions[fn_idx].instructions[pc];
    let l = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
    let r = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
    let res = builtin::logical::shr(&l, &r).unwrap_or(Value::Nil);
    save_value_to_operand(ctx.process, &inst.operands[2], res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// Object & Properties Handlers
// -----------------------------------------------------------------------------

fn op_get_prop<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (obj_val, prop_name, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let obj_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let prop_name = match &inst.operands[1] {
            OperandType::Property(p) => p.clone(),
            _ => "".to_string(),
        };
        (obj_val, prop_name, inst.operands[2].clone())
    };

    let val = builtin::properties::get_property(&obj_val, &prop_name, false).unwrap_or(Value::Nil);
    if let Value::Function(ref f) = val {
        let needs_binding = f.read().unwrap().bound_this.is_none();
        if needs_binding {
            f.write().unwrap().bound_this = Some(obj_val.clone());
        }
    }
    save_value_to_operand(ctx.process, &dst_op, val);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_set_prop<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (obj_val, prop_name, val) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let obj_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let prop_name = match &inst.operands[1] {
            OperandType::Property(p) => p.clone(),
            _ => "".to_string(),
        };
        let val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[2]);
        (obj_val, prop_name, val)
    };

    let _ = builtin::properties::set_property(&obj_val, &prop_name, val, false);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_get_index<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (obj, idx_val, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let obj = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let idx_val = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
            Value::Int(i) => i as usize,
            _ => 0,
        };
        (obj, idx_val, inst.operands[2].clone())
    };

    let val = match obj {
        Value::Array(arr) => arr.read().unwrap().get(idx_val).cloned().unwrap_or(Value::Nil),
        Value::IntArray(arr) => arr.read().unwrap().get(idx_val).copied().map(Value::Int).unwrap_or(Value::Nil),
        Value::FloatArray(arr) => arr.read().unwrap().get(idx_val).copied().map(Value::Float).unwrap_or(Value::Nil),
        Value::V4Int(arr) => arr.get(idx_val).copied().map(Value::Int).unwrap_or(Value::Nil),
        Value::V4Float(arr) => arr.get(idx_val).copied().map(Value::Float).unwrap_or(Value::Nil),
        Value::V4Bool(arr) => arr.get(idx_val).copied().map(Value::Bool).unwrap_or(Value::Nil),
        Value::Object(obj) => {
            let borrowed = obj.read().unwrap();
            if idx_val < borrowed.indexed.len() {
                if let Some(Some(prop)) = borrowed.indexed.get(idx_val) {
                    prop.value.clone()
                } else {
                    borrowed
                        .get_property(&idx_val.to_string())
                        .unwrap_or(Value::Nil)
                }
            } else {
                borrowed
                    .get_property(&idx_val.to_string())
                    .unwrap_or(Value::Nil)
            }
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, val);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_new_object<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (dst_op, proto) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let dst = inst.operands[0].clone();
        let proto = if inst.operands.len() > 1 {
            match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
                Value::Object(o) => Some(o),
                _ => None,
            }
        } else {
            None
        };
        (dst, proto)
    };

    let obj = Object::new(proto);
    save_value_to_operand(
        ctx.process,
        &dst_op,
        Value::Object(Arc::new(RwLock::new(obj))),
    );
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_make_closure<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (func_template, env_opt, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        if inst.operands.len() < 2 {
            return ExecutionResult::Error(
                "make_closure requires at least func and dst operands".to_string(),
            );
        }
        let (func_op, env_op, dst_op) = if inst.operands.len() == 2 {
            (&inst.operands[0], None, &inst.operands[1])
        } else {
            (
                &inst.operands[0],
                Some(&inst.operands[1]),
                &inst.operands[2],
            )
        };

        let env_arc = env_op.and_then(|op| {
            let v = resolve_operand(ctx.bytecode, ctx.process, op);
            match v {
                Value::Array(arr) => Some(arr),
                _ => None,
            }
        });

        let func = match func_op {
            OperandType::Function(name) => ctx
                .bytecode
                .functions
                .iter()
                .find(|f| f.name == *name)
                .cloned(),
            OperandType::Register(r) => match frame.registers.get(*r) {
                Some(Value::Function(f)) => Some(f.read().unwrap().clone()),
                _ => None,
            },
            _ => None,
        };

        (func, env_arc, dst_op.clone())
    };

    match func_template {
        Some(mut f) => {
            f.fn_type = FunctionType::Closure;
            f.closure_env = env_opt;
            save_value_to_operand(
                ctx.process,
                &dst_op,
                Value::Function(Arc::new(RwLock::new(f))),
            );
        }
        None => {
            return ExecutionResult::Error("function not found for make_closure".to_string());
        }
    }

    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_make_env<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (env_vals, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        if inst.operands.is_empty() {
            return ExecutionResult::Error(
                "make_env requires at least destination operand".to_string(),
            );
        }
        if inst.operands.len() == 1 {
            (vec![], inst.operands[0].clone())
        } else if inst.operands.len() == 2 {
            if let OperandType::RegisterList(regs) = &inst.operands[1] {
                let vals: Vec<Value> = regs
                    .iter()
                    .map(|r| frame.registers.get(*r).cloned().unwrap_or(Value::Nil))
                    .collect();
                (vals, inst.operands[0].clone())
            } else if let OperandType::RegisterList(regs) = &inst.operands[0] {
                let vals: Vec<Value> = regs
                    .iter()
                    .map(|r| frame.registers.get(*r).cloned().unwrap_or(Value::Nil))
                    .collect();
                (vals, inst.operands[1].clone())
            } else {
                let val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
                (vec![val], inst.operands[1].clone())
            }
        } else {
            let n = inst.operands.len();
            let mut vals = Vec::with_capacity(n - 1);
            for i in 0..(n - 1) {
                vals.push(resolve_operand(
                    ctx.bytecode,
                    ctx.process,
                    &inst.operands[i],
                ));
            }
            (vals, inst.operands[n - 1].clone())
        }
    };

    let env = Arc::new(RwLock::new(env_vals));
    save_value_to_operand(ctx.process, &dst_op, Value::Array(env));
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// Actor & Concurrency Handlers
// -----------------------------------------------------------------------------

fn op_send<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (target_pid, msg_val) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let pid_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let msg_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        let target_pid = match pid_val {
            Value::Int(p) => p as usize,
            _ => 0,
        };
        (target_pid, msg_val)
    };

    if target_pid == ctx.process.id {
        ctx.process.mailbox.push_back(msg_val);
    } else {
        let _ = ctx.scheduler.send_message(target_pid, msg_val);
    }

    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
    become fetch_and_dispatch(ctx);
}


fn op_receive<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    if let Some(msg) = ctx.process.mailbox.pop_front() {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let dst = inst.operands[0].clone();
        save_value_to_operand(ctx.process, &dst, msg);
        ctx.process.frame_stack.last_mut().unwrap().pc += 1;
        ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        become fetch_and_dispatch(ctx);
    } else {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let dst_reg = match &inst.operands[0] {
            OperandType::Register(r) => Some(*r),
            _ => None,
        };
        ExecutionResult::Wait(Box::new(WaitState {
            wait_id: 0,
            reason: WaitReason::AnyMessage,
            timeout: None,
            dst_reg,
            timeout_val: None,
        }))
    }
}


fn op_receive_timeout<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    if let Some(msg) = ctx.process.mailbox.pop_front() {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let dst = inst.operands[0].clone();
        save_value_to_operand(ctx.process, &dst, msg);
        ctx.process.frame_stack.last_mut().unwrap().pc += 1;
        ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        become fetch_and_dispatch(ctx);
    } else {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let dst_reg = match &inst.operands[0] {
            OperandType::Register(r) => Some(*r),
            _ => None,
        };

        let timeout_ms = match inst.operands.get(1) {
            Some(OperandType::Constant(Value::Int(ms))) => (*ms).max(0) as u64,
            Some(OperandType::Register(r)) => match ctx.process.frame_stack.last().unwrap().registers.get(*r) {
                Some(Value::Int(ms)) => (*ms).max(0) as u64,
                _ => 0,
            },
            _ => 0,
        };

        let timeout_val = inst.operands.get(2).and_then(|op| match op {
            OperandType::Constant(val) => Some(val.clone()),
            OperandType::Register(r) => ctx.process.frame_stack.last().unwrap().registers.get(*r).cloned(),
            _ => None,
        }).or(Some(Value::Nil));

        ExecutionResult::Wait(Box::new(WaitState {
            wait_id: 0,
            reason: WaitReason::AnyMessage,
            timeout: Some(std::time::Duration::from_millis(timeout_ms)),
            dst_reg,
            timeout_val,
        }))
    }
}


fn op_spawn<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (fn_name, _arg_count, dst_op, args) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let arg_count = match inst.operands[1] {
            OperandType::Offset(o) => o as usize,
            _ => 0,
        };
        let dst = inst.operands[2 + arg_count].clone();
        let fn_name = match &inst.operands[0] {
            OperandType::Function(name) => name.clone(),
            op => match resolve_operand(ctx.bytecode, ctx.process, op) {
                Value::Function(f) => f.read().unwrap().name.clone(),
                Value::String(s) => s,
                _ => "".to_string(),
            },
        };
        let mut args = Vec::with_capacity(arg_count);
        for i in 0..arg_count {
            args.push(resolve_operand(
                ctx.bytecode,
                ctx.process,
                &inst.operands[2 + i],
            ));
        }
        (fn_name, arg_count, dst, args)
    };

    match ctx.scheduler.spawn_process(&fn_name, args) {
        Ok(pid) => {
            save_value_to_operand(ctx.process, &dst_op, Value::Int(pid as i64));
            ctx.process.frame_stack.last_mut().unwrap().pc += 1;
            become fetch_and_dispatch(ctx);
        }
        Err(e) => ExecutionResult::Error(e),
    }
}



// -----------------------------------------------------------------------------
// Pattern Matching Handlers
// -----------------------------------------------------------------------------

fn op_match_pattern<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (matched_offset, bindings, default_offset) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        if inst.operands.is_empty() {
            (None, Vec::new(), None)
        } else {
            let val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
            let mut cases = Vec::new();
            let mut default_off = None;
            let mut i = 1;
            while i < inst.operands.len() {
                if i + 1 < inst.operands.len() {
                    if let OperandType::Pattern(pat) = &inst.operands[i] {
                        let off = match &inst.operands[i + 1] {
                            OperandType::Offset(o) => *o,
                            OperandType::Constant(Value::Int(o)) => *o,
                            _ => 1,
                        };
                        cases.push((pat.clone(), off));
                        i += 2;
                        continue;
                    }
                }
                if let OperandType::Offset(o) = &inst.operands[i] {
                    default_off = Some(*o);
                } else if let OperandType::Constant(Value::Int(o)) = &inst.operands[i] {
                    default_off = Some(*o);
                }
                i += 1;
            }

            if let Some((off, binds)) = crate::lang::matcher::evaluate_match(&val, &cases) {
                (Some(off), binds, default_off)
            } else {
                (None, Vec::new(), default_off)
            }
        }
    };

    let frame = ctx.process.frame_stack.last_mut().unwrap();
    if let Some(off) = matched_offset {
        for (reg, val) in bindings {
            set_reg_value_in_frame(frame, reg, val);
        }
        let new_pc = frame.pc as i64 + off;
        frame.pc = new_pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else if let Some(off) = default_offset {
        let new_pc = frame.pc as i64 + off;
        frame.pc = new_pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_A_match_fast<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (matched, off) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let regs = match &inst.operands[0] {
            OperandType::RegisterList(regs) => regs.clone(),
            _ => vec![],
        };
        let consts = match &inst.operands[1] {
            OperandType::ConstantList(consts) => consts.clone(),
            _ => vec![],
        };
        let off = match inst.operands[2] {
            OperandType::Offset(o) => o,
            _ => 1,
        };

        let mut matched = true;
        for (r, c) in regs.iter().zip(consts.iter()) {
            let actual = frame.registers.get(*r).unwrap_or(&Value::Nil);
            if !actual.eql(c) {
                matched = false;
                break;
            }
        }
        (matched, off)
    };

    let frame = ctx.process.frame_stack.last_mut().unwrap();
    if !matched {
        let new_pc = frame.pc as i64 + off;
        frame.pc = new_pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// Fast Integer Arithmetic Handlers
// -----------------------------------------------------------------------------

fn op_I_add_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(l + r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_add_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v0 = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(v0 + c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_sub_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(l - r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_sub_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v0 = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(v0 - c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_sub_CRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let c = match inst.operands[0] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v1 = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(c - v1));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_mul_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(l * r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_mul_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v0 = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(v0 * c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_div_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 1,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(if c != 0 { l / c } else { 0 }));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_div_CRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let c = match inst.operands[0] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v1 = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(if v1 != 0 { c / v1 } else { 0 }));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_rem_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 1,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(if c != 0 { l % c } else { 0 }));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_rem_CRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let c = match inst.operands[0] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v1 = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(if v1 != 0 { c % v1 } else { 0 }));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// Fast Integer Comparison Handlers
// -----------------------------------------------------------------------------

fn op_I_eq_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(l == r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_eq_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v0 = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(v0 == c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_lt_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(l < r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_lt_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v0 = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(v0 < c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_lt_CRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let c = match inst.operands[0] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v1 = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(c < v1));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_gt_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(l > r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_gt_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(l > c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_gt_CRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let c = match inst.operands[0] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v1 = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(c > v1));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// Fast Float Arithmetic Handlers
// -----------------------------------------------------------------------------

fn op_F_add_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Float(l + r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_add_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Float(f)) => f,
        _ => 0.0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v0 = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Float(v0 + c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_sub_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Float(l - r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_sub_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Float(f)) => f,
        _ => 0.0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v0 = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Float(v0 - c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_sub_CRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let c = match inst.operands[0] {
        OperandType::Constant(Value::Float(f)) => f,
        _ => 0.0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v1 = match frame.registers.get(r1) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Float(c - v1));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_mul_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Float(l * r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_mul_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Float(f)) => f,
        _ => 0.0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v0 = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Float(v0 * c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_div_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Float(f)) => *f,
        _ => 1.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Float(if r != 0.0 { l / r } else { 0.0 }));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_div_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Float(f)) => f,
        _ => 1.0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v0 = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Float(v0 / c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_div_CRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let c = match inst.operands[0] {
        OperandType::Constant(Value::Float(f)) => f,
        _ => 0.0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v1 = match frame.registers.get(r1) {
        Some(Value::Float(f)) => *f,
        _ => 1.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Float(c / v1));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// Fast Float Comparison Handlers
// -----------------------------------------------------------------------------

fn op_F_eq_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(l == r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_eq_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Float(f)) => f,
        _ => 0.0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v0 = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(v0 == c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_lt_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(l < r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_lt_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Float(f)) => f,
        _ => 0.0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v0 = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(v0 < c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_lt_CRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let c = match inst.operands[0] {
        OperandType::Constant(Value::Float(f)) => f,
        _ => 0.0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v1 = match frame.registers.get(r1) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(c < v1));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_gt_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(l > r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_gt_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Float(f)) => f,
        _ => 0.0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v0 = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(v0 > c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_gt_CRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let c = match inst.operands[0] {
        OperandType::Constant(Value::Float(f)) => f,
        _ => 0.0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let v1 = match frame.registers.get(r1) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    set_reg_value_in_frame(frame, dst, Value::Bool(c > v1));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// Fused Compare-Branch Handlers
// -----------------------------------------------------------------------------

fn op_I_gt_jmp_RR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let l = match inst.operands[0] {
        OperandType::Register(r) => match frame.registers.get(r) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        OperandType::Argument(a) => match frame.arguments.get(a) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        _ => 0,
    };
    let r = match inst.operands[1] {
        OperandType::Register(r) => match frame.registers.get(r) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        OperandType::Argument(a) => match frame.arguments.get(a) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        _ => 0,
    };
    let off = match inst.operands[2] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    if l > r {
        let pc = frame.pc as i64 + off;
        frame.pc = pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_I_gt_jmp_RC<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let l = match inst.operands[0] {
        OperandType::Register(r) => match frame.registers.get(r) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        OperandType::Argument(a) => match frame.arguments.get(a) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let off = match inst.operands[2] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    if l > c {
        let pc = frame.pc as i64 + off;
        frame.pc = pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_I_lt_jmp_RR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let l = match inst.operands[0] {
        OperandType::Register(r) => match frame.registers.get(r) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        OperandType::Argument(a) => match frame.arguments.get(a) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        _ => 0,
    };
    let r = match inst.operands[1] {
        OperandType::Register(r) => match frame.registers.get(r) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        OperandType::Argument(a) => match frame.arguments.get(a) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        _ => 0,
    };
    let off = match inst.operands[2] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    if l < r {
        let pc = frame.pc as i64 + off;
        frame.pc = pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_I_lt_jmp_RC<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let l = match inst.operands[0] {
        OperandType::Register(r) => match frame.registers.get(r) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        OperandType::Argument(a) => match frame.arguments.get(a) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let off = match inst.operands[2] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    if l < c {
        let pc = frame.pc as i64 + off;
        frame.pc = pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_I_eq_jmp_RR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let l = match inst.operands[0] {
        OperandType::Register(r) => match frame.registers.get(r) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        _ => 0,
    };
    let r = match inst.operands[1] {
        OperandType::Register(r) => match frame.registers.get(r) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        _ => 0,
    };
    let off = match inst.operands[2] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    if l == r {
        let pc = frame.pc as i64 + off;
        frame.pc = pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_I_eq_jmp_RC<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let l = match inst.operands[0] {
        OperandType::Register(r) => match frame.registers.get(r) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        },
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let off = match inst.operands[2] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    if l == c {
        let pc = frame.pc as i64 + off;
        frame.pc = pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_F_gt_jmp_RR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let off = match inst.operands[2] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    if l > r {
        let pc = frame.pc as i64 + off;
        frame.pc = pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_F_gt_jmp_RC<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Float(f)) => f,
        _ => 0.0,
    };
    let off = match inst.operands[2] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    if l > c {
        let pc = frame.pc as i64 + off;
        frame.pc = pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_F_lt_jmp_RR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let off = match inst.operands[2] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    if l < r {
        let pc = frame.pc as i64 + off;
        frame.pc = pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_F_lt_jmp_RC<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Float(f)) => f,
        _ => 0.0,
    };
    let off = match inst.operands[2] {
        OperandType::Offset(o) => o,
        _ => 1,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    if l < c {
        let pc = frame.pc as i64 + off;
        frame.pc = pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}

#[inline(always)]
fn resolve_str_borrow<'a>(proc: &'a Process, op: &'a OperandType) -> Option<&'a str> {
    let frame = proc.frame_stack.last()?;
    match op {
        OperandType::Register(r) => match frame.registers.get(*r)? {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        },
        OperandType::Constant(Value::String(s)) => Some(s.as_str()),
        OperandType::Argument(a) => match frame.arguments.get(*a)? {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        },
        _ => None,
    }
}



// -----------------------------------------------------------------------------
// Fast Bitwise & Shift Handlers
// -----------------------------------------------------------------------------

fn op_I_shl_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let shift = (r as u64) & 63;
    set_reg_value_in_frame(frame, dst, Value::Int(l.wrapping_shl(shift as u32)));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_shl_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let shift = (c as u64) & 63;
    set_reg_value_in_frame(frame, dst, Value::Int(l.wrapping_shl(shift as u32)));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_shr_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let shift = (r as u64) & 63;
    set_reg_value_in_frame(frame, dst, Value::Int(l.wrapping_shr(shift as u32)));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_shr_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let shift = (c as u64) & 63;
    set_reg_value_in_frame(frame, dst, Value::Int(l.wrapping_shr(shift as u32)));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_and_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(l & r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_and_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(l & c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_or_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(l | r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_or_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(l | c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_xor_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let r1 = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    let r = match frame.registers.get(r1) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(l ^ r));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_xor_RCR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let r0 = match inst.operands[0] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let c = match inst.operands[1] {
        OperandType::Constant(Value::Int(i)) => i,
        _ => 0,
    };
    let dst = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let l = match frame.registers.get(r0) {
        Some(Value::Int(i)) => *i,
        _ => 0,
    };
    set_reg_value_in_frame(frame, dst, Value::Int(l ^ c));
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_bit_get<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (arr_val, bit_idx, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let arr_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let bit_idx = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
            Value::Int(i) => i.max(0) as usize,
            _ => 0,
        };
        (arr_val, bit_idx, inst.operands[2].clone())
    };
    let bit_val = match arr_val {
        Value::IntArray(a) => {
            let word_idx = bit_idx >> 6;
            let bit_pos = bit_idx & 63;
            let r = a.read().unwrap();
            r.get(word_idx).map(|&w| (w >> bit_pos) & 1).unwrap_or(0)
        }
        _ => 0,
    };
    save_value_to_operand(ctx.process, &dst_op, Value::Int(bit_val));
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_bit_set<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (arr_val, bit_idx, val) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let arr_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let bit_idx = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
            Value::Int(i) => i.max(0) as usize,
            _ => 0,
        };
        let val = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[2]) {
            Value::Int(i) => i != 0,
            Value::Bool(b) => b,
            _ => false,
        };
        (arr_val, bit_idx, val)
    };
    if let Value::IntArray(a) = arr_val {
        let word_idx = bit_idx >> 6;
        let bit_pos = bit_idx & 63;
        let mut w = a.write().unwrap();
        if word_idx >= w.len() {
            w.resize(word_idx + 1, 0);
        }
        if val {
            w[word_idx] |= 1i64 << bit_pos;
        } else {
            w[word_idx] &= !(1i64 << bit_pos);
        }
    }
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_bit_set1<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (arr_val, bit_idx) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let arr_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let bit_idx = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
            Value::Int(i) => i.max(0) as usize,
            _ => 0,
        };
        (arr_val, bit_idx)
    };
    if let Value::IntArray(a) = arr_val {
        let word_idx = bit_idx >> 6;
        let bit_pos = bit_idx & 63;
        let mut w = a.write().unwrap();
        if word_idx < w.len() {
            w[word_idx] |= 1i64 << bit_pos;
        } else {
            w.resize(word_idx + 1, 0);
            w[word_idx] |= 1i64 << bit_pos;
        }
    }
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_bit_set0<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (arr_val, bit_idx) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let arr_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let bit_idx = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
            Value::Int(i) => i.max(0) as usize,
            _ => 0,
        };
        (arr_val, bit_idx)
    };
    if let Value::IntArray(a) = arr_val {
        let word_idx = bit_idx >> 6;
        let bit_pos = bit_idx & 63;
        let mut w = a.write().unwrap();
        if word_idx < w.len() {
            w[word_idx] &= !(1i64 << bit_pos);
        }
    }
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// String Operations Handlers
// -----------------------------------------------------------------------------

fn op_S_concat_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let s1_opt = resolve_str_borrow(ctx.process, &inst.operands[0]);
    let s2_opt = resolve_str_borrow(ctx.process, &inst.operands[1]);
    let dst_op = inst.operands[2].clone();

    let out = match (s1_opt, s2_opt) {
        (Some(s1), Some(s2)) => {
            let mut s = String::with_capacity(s1.len() + s2.len());
            s.push_str(s1);
            s.push_str(s2);
            s
        }
        _ => {
            let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
            let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
            let s1 = v1.to_str();
            let s2 = v2.to_str();
            let mut s = String::with_capacity(s1.len() + s2.len());
            s.push_str(&s1);
            s.push_str(&s2);
            s
        }
    };
    save_value_to_operand(ctx.process, &dst_op, Value::String(out));
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_S_slice_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let start_idx = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
        Value::Int(i) => i.max(0) as usize,
        _ => 0,
    };
    let dst_op = inst.operands[2].clone();

    let res = if let Some(s) = resolve_str_borrow(ctx.process, &inst.operands[0]) {
        let start = start_idx.min(s.len());
        Value::String(s[start..].to_string())
    } else {
        Value::String("".to_string())
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_S_slice_RRRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let start_idx = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
        Value::Int(i) => i.max(0) as usize,
        _ => 0,
    };
    let end_idx = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[2]) {
        Value::Int(i) => i.max(0) as usize,
        _ => 0,
    };
    let dst_op = inst.operands[3].clone();

    let res = if let Some(s) = resolve_str_borrow(ctx.process, &inst.operands[0]) {
        let start = start_idx.min(s.len());
        let end = end_idx.min(s.len()).max(start);
        Value::String(s[start..end].to_string())
    } else {
        Value::String("".to_string())
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_S_eq_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let s1 = resolve_str_borrow(ctx.process, &inst.operands[0]);
    let s2 = resolve_str_borrow(ctx.process, &inst.operands[1]);
    let dst_op = inst.operands[2].clone();

    let eq = match (s1, s2) {
        (Some(a), Some(b)) => crate::vm::simd::simd_str_eq(a, b),
        _ => {
            let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
            let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
            v1.eql(&v2)
        }
    };
    save_value_to_operand(ctx.process, &dst_op, Value::Bool(eq));
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_S_find_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let s1 = resolve_str_borrow(ctx.process, &inst.operands[0]);
    let s2 = resolve_str_borrow(ctx.process, &inst.operands[1]);
    let dst_op = inst.operands[2].clone();

    let idx = match (s1, s2) {
        (Some(haystack), Some(needle)) => {
            crate::vm::simd::simd_str_find(haystack, needle)
                .map(|i| i as i64)
                .unwrap_or(-1)
        }
        _ => -1,
    };
    save_value_to_operand(ctx.process, &dst_op, Value::Int(idx));
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_S_to_int_RR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let dst_op = inst.operands[1].clone();

    let res = if let Some(s) = resolve_str_borrow(ctx.process, &inst.operands[0]) {
        Value::Int(s.trim().parse::<i64>().unwrap_or(0))
    } else {
        Value::Int(0)
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_S_upper_RR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let src_op = inst.operands[0].clone();
    let dst_op = inst.operands[1].clone();

    if let (OperandType::Register(r_src), OperandType::Register(r_dst)) = (&src_op, &dst_op) {
        if r_src == r_dst {
            if let Some(Value::String(s)) = frame.registers.get_mut(*r_src) {
                crate::vm::simd::simd_str_to_upper_mut(s);
                frame.pc += 1;
                become fetch_and_dispatch(ctx);
            }
        }
    }

    let res = if let Some(s) = resolve_str_borrow(ctx.process, &src_op) {
        let mut owned = s.to_string();
        crate::vm::simd::simd_str_to_upper_mut(&mut owned);
        Value::String(owned)
    } else {
        let v = resolve_operand(ctx.bytecode, ctx.process, &src_op);
        match v {
            Value::String(mut s) => {
                crate::vm::simd::simd_str_to_upper_mut(&mut s);
                Value::String(s)
            }
            other => other,
        }
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_S_lower_RR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let src_op = inst.operands[0].clone();
    let dst_op = inst.operands[1].clone();

    if let (OperandType::Register(r_src), OperandType::Register(r_dst)) = (&src_op, &dst_op) {
        if r_src == r_dst {
            if let Some(Value::String(s)) = frame.registers.get_mut(*r_src) {
                crate::vm::simd::simd_str_to_lower_mut(s);
                frame.pc += 1;
                become fetch_and_dispatch(ctx);
            }
        }
    }

    let res = if let Some(s) = resolve_str_borrow(ctx.process, &src_op) {
        let mut owned = s.to_string();
        crate::vm::simd::simd_str_to_lower_mut(&mut owned);
        Value::String(owned)
    } else {
        let v = resolve_operand(ctx.bytecode, ctx.process, &src_op);
        match v {
            Value::String(mut s) => {
                crate::vm::simd::simd_str_to_lower_mut(&mut s);
                Value::String(s)
            }
            other => other,
        }
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_S_count_byte_RRR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let s_opt = resolve_str_borrow(ctx.process, &inst.operands[0]);
    let dst_op = inst.operands[2].clone();

    let count = if let Some(s) = s_opt {
        match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
            Value::Int(b) => crate::vm::simd::simd_str_count_byte(s, b as u8) as i64,
            Value::String(needle) if !needle.is_empty() => {
                crate::vm::simd::simd_str_count_byte(s, needle.as_bytes()[0]) as i64
            }
            _ => 0,
        }
    } else {
        let s = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let b = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        match (s, b) {
            (Value::String(s), Value::Int(b)) => {
                crate::vm::simd::simd_str_count_byte(&s, b as u8) as i64
            }
            (Value::String(s), Value::String(needle)) if !needle.is_empty() => {
                crate::vm::simd::simd_str_count_byte(&s, needle.as_bytes()[0]) as i64
            }
            _ => 0,
        }
    };
    save_value_to_operand(ctx.process, &dst_op, Value::Int(count));
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_format_value<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (val, spec, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        if inst.operands.len() == 2 {
            let val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
            (val, String::new(), inst.operands[1].clone())
        } else if inst.operands.len() >= 3 {
            let val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
            let spec_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
            (val, spec_val.to_str(), inst.operands[2].clone())
        } else {
            return ExecutionResult::Error("format_value expects 2 or 3 operands".to_string());
        }
    };

    let formatted = format_value_with_spec(&val, &spec);
    save_value_to_operand(ctx.process, &dst_op, Value::String(formatted));
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_build_string<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (pieces, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        if inst.operands.len() == 2 {
            if let OperandType::RegisterList(regs) = &inst.operands[0] {
                let pieces: Vec<String> = regs
                    .iter()
                    .map(|r| {
                        frame
                            .registers
                            .get(*r)
                            .cloned()
                            .unwrap_or(Value::Nil)
                            .to_str()
                    })
                    .collect();
                (pieces, inst.operands[1].clone())
            } else {
                let v = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
                (vec![v.to_str()], inst.operands[1].clone())
            }
        } else if inst.operands.len() > 2 {
            let n = inst.operands.len();
            let mut pieces = Vec::with_capacity(n - 1);
            for i in 0..(n - 1) {
                let v = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[i]);
                pieces.push(v.to_str());
            }
            (pieces, inst.operands[n - 1].clone())
        } else if inst.operands.len() == 1 {
            (vec![], inst.operands[0].clone())
        } else {
            return ExecutionResult::Error(
                "build_string requires at least 1 operand (destination)".to_string(),
            );
        }
    };

    let total_len: usize = pieces.iter().map(|s| s.len()).sum();
    let mut out = String::with_capacity(total_len);
    for piece in pieces {
        out.push_str(&piece);
    }

    save_value_to_operand(ctx.process, &dst_op, Value::String(out));
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// Tagged Enum Handlers
// -----------------------------------------------------------------------------

fn op_E_make_enum<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    if ctx.process.frame_stack.last().is_some_and(|frame| {
        ctx.bytecode.functions[frame.function_index].instructions[frame.pc]
            .operands
            .len()
            == 4
    }) {
        let (enum_name, variant_name, payload_regs, dst_op) = {
            let frame = ctx.process.frame_stack.last().unwrap();
            let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
            let read_string = |operand: &OperandType| match operand {
                OperandType::Constant(Value::String(value)) => value.clone(),
                _ => String::new(),
            };
            let enum_name = read_string(&inst.operands[0]);
            let variant_name = read_string(&inst.operands[1]);
            let payload_regs = match &inst.operands[2] {
                OperandType::RegisterList(regs) => regs.clone(),
                _ => vec![],
            };
            (
                enum_name,
                variant_name,
                payload_regs,
                inst.operands[3].clone(),
            )
        };
        let prototype = ctx
            .bytecode
            .object_inits
            .iter()
            .find(|(name, _)| name == &enum_name)
            .map(|(_, prototype)| Arc::clone(prototype));
        let mut obj = Object::new(prototype);
        obj.set_property("@", Value::String(enum_name), Protection::Readonly);
        obj.set_property("@@", Value::String(variant_name), Protection::Readonly);
        for (index, register) in payload_regs.iter().enumerate() {
            let frame = ctx.process.frame_stack.last().unwrap();
            let value = frame
                .registers
                .get(*register)
                .cloned()
                .unwrap_or(Value::Nil);
            obj.set_property(
                &format!("@payload_{}", index),
                value.clone(),
                Protection::Writeable,
            );
            obj.set_property(&index.to_string(), value, Protection::Writeable);
        }
        save_value_to_operand(
            ctx.process,
            &dst_op,
            Value::Object(Arc::new(RwLock::new(obj))),
        );
        ctx.process.frame_stack.last_mut().unwrap().pc += 1;
        become fetch_and_dispatch(ctx);
    }

    let (tag_val, pos_regs, prop_names, named_regs, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let tag_val = match &inst.operands[0] {
            OperandType::Constant(Value::Int(i)) => *i,
            _ => 0,
        };
        let pos_regs = match &inst.operands[1] {
            OperandType::RegisterList(regs) => regs.clone(),
            _ => vec![],
        };
        let prop_names = match &inst.operands[2] {
            OperandType::PropertyList(props) => props.clone(),
            _ => vec![],
        };
        let named_regs = match &inst.operands[3] {
            OperandType::RegisterList(regs) => regs.clone(),
            _ => vec![],
        };
        let dst = inst.operands[4].clone();
        (tag_val, pos_regs, prop_names, named_regs, dst)
    };

    let mut obj = Object::new(None);
    obj.set_property("@tag", Value::Int(tag_val), Protection::Writeable);

    for (i, reg) in pos_regs.iter().enumerate() {
        let frame = ctx.process.frame_stack.last().unwrap();
        let val = frame.registers.get(*reg).cloned().unwrap_or(Value::Nil);
        obj.set_property(
            &format!("@payload_{}", i),
            val.clone(),
            Protection::Writeable,
        );
        obj.set_property(&i.to_string(), val, Protection::Writeable);
    }

    for (name, reg) in prop_names.iter().zip(named_regs.iter()) {
        let frame = ctx.process.frame_stack.last().unwrap();
        let val = frame.registers.get(*reg).cloned().unwrap_or(Value::Nil);
        obj.set_property(name, val, Protection::Writeable);
    }

    save_value_to_operand(
        ctx.process,
        &dst_op,
        Value::Object(Arc::new(RwLock::new(obj))),
    );
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_E_get_tag_RR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (enum_val, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let enum_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        (enum_val, inst.operands[1].clone())
    };

    let tag = match enum_val {
        Value::Object(obj) => obj
            .read()
            .unwrap()
            .get_property("@tag")
            .unwrap_or(Value::Int(-1)),
        _ => Value::Int(-1),
    };
    save_value_to_operand(ctx.process, &dst_op, tag);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_E_get_payload_RR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (enum_val, key, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let enum_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let key = match &inst.operands[1] {
            OperandType::Constant(Value::Int(i)) => format!("@payload_{}", i),
            OperandType::Property(p) => p.clone(),
            _ => "".to_string(),
        };
        (enum_val, key, inst.operands[2].clone())
    };

    let payload = match enum_val {
        Value::Object(obj) => {
            let borrowed = obj.read().unwrap();
            borrowed
                .get_property(&key)
                .or_else(|| borrowed.get_property(key.trim_start_matches("@payload_")))
                .unwrap_or(Value::Nil)
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, payload);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_E_get_payload_RR_multi<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (enum_val, keys, dst_regs) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let enum_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let keys = match &inst.operands[1] {
            OperandType::PayloadKeyList(list) => list.clone(),
            _ => vec![],
        };
        let dst_regs = match &inst.operands[2] {
            OperandType::RegisterList(regs) => regs.clone(),
            _ => vec![],
        };
        (enum_val, keys, dst_regs)
    };

    if let Value::Object(obj) = enum_val {
        let borrowed = obj.read().unwrap();
        for (key_op, dst_reg) in keys.iter().zip(dst_regs.iter()) {
            let payload = match key_op {
                OperandType::Constant(Value::Int(i)) => {
                    let key = format!("@payload_{}", i);
                    borrowed
                        .get_property(&key)
                        .or_else(|| borrowed.get_property(&i.to_string()))
                        .unwrap_or(Value::Nil)
                }
                OperandType::Property(p) => borrowed.get_property(p).unwrap_or(Value::Nil),
                _ => Value::Nil,
            };
            set_reg_value(ctx.process, *dst_reg, payload);
        }
    }
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_E_tag_jmp_RC<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (matches_tag, off) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let enum_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let expected_tag = match &inst.operands[1] {
            OperandType::Constant(Value::Int(i)) => *i,
            _ => -1,
        };
        let off = match inst.operands[2] {
            OperandType::Offset(o) => o,
            _ => 1,
        };
        let actual_tag = match enum_val {
            Value::Object(obj) => match obj.read().unwrap().get_property("@tag") {
                Some(Value::Int(i)) => i,
                _ => -1,
            },
            _ => -1,
        };
        (actual_tag == expected_tag, off)
    };

    let frame = ctx.process.frame_stack.last_mut().unwrap();
    if matches_tag {
        let pc = frame.pc as i64 + off;
        frame.pc = pc as usize;
        if off < 0 {
            ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
        }
    } else {
        frame.pc += 1;
    }
    become fetch_and_dispatch(ctx);
}


fn op_E_tag_switch<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (target_offset, default_offset) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let enum_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let table = match &inst.operands[1] {
            OperandType::JumpTable(t) => t.clone(),
            _ => vec![],
        };
        let default_off = match inst.operands[2] {
            OperandType::Offset(o) => o,
            _ => 1,
        };

        let tag = match enum_val {
            Value::Object(obj) => match obj.read().unwrap().get_property("@tag") {
                Some(Value::Int(i)) => i,
                _ => -1,
            },
            _ => -1,
        };

        let mut found = None;
        for case in &table {
            if case.tag == tag {
                found = Some(case.offset);
                break;
            }
        }
        (found, default_off)
    };

    let off = target_offset.unwrap_or(default_offset);
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let new_pc = frame.pc as i64 + off;
    frame.pc = new_pc as usize;
    if off < 0 {
        ctx.process.reductions = ctx.process.reductions.saturating_sub(1);
    }
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// Array Operations Handlers
// -----------------------------------------------------------------------------

fn op_new_array<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (len, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        if inst.operands.len() >= 2 {
            let len = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]) {
                Value::Int(i) => i.max(0) as usize,
                _ => 0,
            };
            (len, inst.operands[1].clone())
        } else if !inst.operands.is_empty() {
            (0, inst.operands[0].clone())
        } else {
            (0, OperandType::Register(0))
        }
    };
    let arr = vec![Value::Nil; len];
    save_value_to_operand(
        ctx.process,
        &dst_op,
        Value::Array(Arc::new(RwLock::new(arr))),
    );
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_new_int_array<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (len, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        if inst.operands.len() >= 2 {
            let len = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]) {
                Value::Int(i) => i.max(0) as usize,
                _ => 0,
            };
            (len, inst.operands[1].clone())
        } else if !inst.operands.is_empty() {
            (0, inst.operands[0].clone())
        } else {
            (0, OperandType::Register(0))
        }
    };
    let arr = vec![0i64; len];
    save_value_to_operand(
        ctx.process,
        &dst_op,
        Value::IntArray(Arc::new(RwLock::new(arr))),
    );
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_new_float_array<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (len, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        if inst.operands.len() >= 2 {
            let len = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]) {
                Value::Int(i) => i.max(0) as usize,
                _ => 0,
            };
            (len, inst.operands[1].clone())
        } else if !inst.operands.is_empty() {
            (0, inst.operands[0].clone())
        } else {
            (0, OperandType::Register(0))
        }
    };
    let arr = vec![0.0f64; len];
    save_value_to_operand(
        ctx.process,
        &dst_op,
        Value::FloatArray(Arc::new(RwLock::new(arr))),
    );
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_array_len<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (arr_val, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let arr_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        (arr_val, inst.operands[1].clone())
    };
    let len = match arr_val {
        Value::Array(a) => a.read().unwrap().len() as i64,
        Value::IntArray(a) => a.read().unwrap().len() as i64,
        Value::FloatArray(a) => a.read().unwrap().len() as i64,
        Value::V4Int(_) | Value::V4Float(_) | Value::V4Bool(_) => 4,
        _ => 0,
    };
    save_value_to_operand(ctx.process, &dst_op, Value::Int(len));
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_array_push<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (arr_val, elem) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let arr_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let elem = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (arr_val, elem)
    };
    match (arr_val, elem) {
        (Value::Array(a), el) => a.write().unwrap().push(el),
        (Value::IntArray(a), Value::Int(i)) => a.write().unwrap().push(i),
        (Value::FloatArray(a), Value::Float(f)) => a.write().unwrap().push(f),
        _ => {}
    }
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_array_get<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (arr_val, idx, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let arr_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let idx = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
            Value::Int(i) => i.max(0) as usize,
            _ => 0,
        };
        (arr_val, idx, inst.operands[2].clone())
    };
    let val = match arr_val {
        Value::Array(a) => a.read().unwrap().get(idx).cloned().unwrap_or(Value::Nil),
        Value::IntArray(a) => a.read().unwrap().get(idx).copied().map(Value::Int).unwrap_or(Value::Nil),
        Value::FloatArray(a) => a.read().unwrap().get(idx).copied().map(Value::Float).unwrap_or(Value::Nil),
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, val);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_array_set<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (arr_val, idx, val) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let arr_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let idx = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
            Value::Int(i) => i.max(0) as usize,
            _ => 0,
        };
        let val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[2]);
        (arr_val, idx, val)
    };
    match (arr_val, val) {
        (Value::Array(a), el) => {
            let mut w = a.write().unwrap();
            if idx >= w.len() {
                w.resize(idx + 1, Value::Nil);
            }
            w[idx] = el;
        }
        (Value::IntArray(a), Value::Int(i)) => {
            let mut w = a.write().unwrap();
            if idx >= w.len() {
                w.resize(idx + 1, 0);
            }
            w[idx] = i;
        }
        (Value::FloatArray(a), Value::Float(f)) => {
            let mut w = a.write().unwrap();
            if idx >= w.len() {
                w.resize(idx + 1, 0.0);
            }
            w[idx] = f;
        }
        _ => {}
    }
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_A_get_index_AR<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let frame = ctx.process.frame_stack.last_mut().unwrap();
    let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
    let arg_idx = match inst.operands[0] {
        OperandType::Argument(a) => a,
        _ => 0,
    };
    let reg_idx = match inst.operands[1] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let dst_reg = match inst.operands[2] {
        OperandType::Register(r) => r,
        _ => 0,
    };
    let idx_val = match frame.registers.get(reg_idx) {
        Some(Value::Int(i)) => *i as usize,
        _ => 0,
    };
    let val = if arg_idx < 8 {
        if let Some(cached_ints) = &frame.cached_int_arrays[arg_idx] {
            if idx_val < cached_ints.len() {
                Value::Int(cached_ints[idx_val])
            } else {
                Value::Nil
            }
        } else if let Some(cached) = &frame.cached_indexed_items[arg_idx] {
            if idx_val < cached.len() {
                cached[idx_val]
                    .as_ref()
                    .map(|property| property.value.clone())
                    .unwrap_or(Value::Nil)
            } else {
                Value::Nil
            }
        } else {
            Value::Nil
        }
    } else {
        Value::Nil
    };
    set_reg_value_in_frame(frame, dst_reg, val);
    frame.pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_array_get<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (arr_val, idx, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let arr_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let idx = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
            Value::Int(i) => i.max(0) as usize,
            _ => 0,
        };
        (arr_val, idx, inst.operands[2].clone())
    };
    let val = match arr_val {
        Value::IntArray(a) => a.read().unwrap().get(idx).copied().map(Value::Int).unwrap_or(Value::Int(0)),
        _ => Value::Int(0),
    };
    save_value_to_operand(ctx.process, &dst_op, val);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_I_array_set<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (arr_val, idx, val) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let arr_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let idx = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
            Value::Int(i) => i.max(0) as usize,
            _ => 0,
        };
        let val = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[2]) {
            Value::Int(i) => i,
            _ => 0,
        };
        (arr_val, idx, val)
    };
    if let Value::IntArray(a) = arr_val {
        let mut w = a.write().unwrap();
        if idx < w.len() {
            w[idx] = val;
        } else {
            w.resize(idx + 1, 0);
            w[idx] = val;
        }
    }
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_array_get<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (arr_val, idx, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let arr_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let idx = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
            Value::Int(i) => i.max(0) as usize,
            _ => 0,
        };
        (arr_val, idx, inst.operands[2].clone())
    };
    let val = match arr_val {
        Value::FloatArray(a) => a.read().unwrap().get(idx).copied().map(Value::Float).unwrap_or(Value::Float(0.0)),
        _ => Value::Float(0.0),
    };
    save_value_to_operand(ctx.process, &dst_op, val);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_F_array_set<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (arr_val, idx, val) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let arr_val = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let idx = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
            Value::Int(i) => i.max(0) as usize,
            _ => 0,
        };
        let val = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[2]) {
            Value::Float(f) => f,
            Value::Int(i) => i as f64,
            _ => 0.0,
        };
        (arr_val, idx, val)
    };
    if let Value::FloatArray(a) = arr_val {
        let mut w = a.write().unwrap();
        if idx < w.len() {
            w[idx] = val;
        } else {
            w.resize(idx + 1, 0.0);
            w[idx] = val;
        }
    }
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// SIMD Vector Arithmetic Handlers
// -----------------------------------------------------------------------------

fn op_V_add_F<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v1, v2, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v1, v2, inst.operands[2].clone())
    };
    let res = match (v1, v2) {
        (Value::V4Float(a), Value::V4Float(b)) => {
            Value::V4Float(crate::vm::simd::simd_v4add_f(a, b))
        }
        (Value::FloatArray(a), Value::FloatArray(b)) => {
            let ba = a.read().unwrap();
            let bb = b.read().unwrap();
            Value::FloatArray(Arc::new(RwLock::new(crate::vm::simd::simd_vadd_f(
                &ba, &bb,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_sub_F<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v1, v2, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v1, v2, inst.operands[2].clone())
    };
    let res = match (v1, v2) {
        (Value::V4Float(a), Value::V4Float(b)) => {
            Value::V4Float(crate::vm::simd::simd_v4sub_f(a, b))
        }
        (Value::FloatArray(a), Value::FloatArray(b)) => {
            let ba = a.read().unwrap();
            let bb = b.read().unwrap();
            Value::FloatArray(Arc::new(RwLock::new(crate::vm::simd::simd_vsub_f(
                &ba, &bb,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_mul_F<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v1, v2, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v1, v2, inst.operands[2].clone())
    };
    let res = match (v1, v2) {
        (Value::V4Float(a), Value::V4Float(b)) => {
            Value::V4Float(crate::vm::simd::simd_v4mul_f(a, b))
        }
        (Value::FloatArray(a), Value::FloatArray(b)) => {
            let ba = a.read().unwrap();
            let bb = b.read().unwrap();
            Value::FloatArray(Arc::new(RwLock::new(crate::vm::simd::simd_vmul_f(
                &ba, &bb,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_dot_F<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v1, v2, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v1, v2, inst.operands[2].clone())
    };
    let res = match (v1, v2) {
        (Value::V4Float(a), Value::V4Float(b)) => {
            Value::Float(crate::vm::simd::simd_v4dot_f(a, b))
        }
        (Value::FloatArray(a), Value::FloatArray(b)) => {
            let ba = a.read().unwrap();
            let bb = b.read().unwrap();
            Value::Float(crate::vm::simd::simd_vdot_f(&ba, &bb))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_div_F<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v1, v2, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v1, v2, inst.operands[2].clone())
    };
    let res = match (v1, v2) {
        (Value::V4Float(a), Value::V4Float(b)) => {
            Value::V4Float(crate::vm::simd::simd_v4div_f(a, b))
        }
        (Value::FloatArray(a), Value::FloatArray(b)) => {
            let ba = a.read().unwrap();
            let bb = b.read().unwrap();
            Value::FloatArray(Arc::new(RwLock::new(crate::vm::simd::simd_vdiv_f(
                &ba, &bb,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_add_I<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v1, v2, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v1, v2, inst.operands[2].clone())
    };
    let res = match (v1, v2) {
        (Value::V4Int(a), Value::V4Int(b)) => {
            Value::V4Int(crate::vm::simd::simd_v4add_i(a, b))
        }
        (Value::IntArray(a), Value::IntArray(b)) => {
            let ba = a.read().unwrap();
            let bb = b.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vadd_i(
                &ba, &bb,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_sub_I<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v1, v2, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v1, v2, inst.operands[2].clone())
    };
    let res = match (v1, v2) {
        (Value::V4Int(a), Value::V4Int(b)) => {
            Value::V4Int(crate::vm::simd::simd_v4sub_i(a, b))
        }
        (Value::IntArray(a), Value::IntArray(b)) => {
            let ba = a.read().unwrap();
            let bb = b.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vsub_i(
                &ba, &bb,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_mul_I<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v1, v2, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v1, v2, inst.operands[2].clone())
    };
    let res = match (v1, v2) {
        (Value::V4Int(a), Value::V4Int(b)) => {
            Value::V4Int(crate::vm::simd::simd_v4mul_i(a, b))
        }
        (Value::IntArray(a), Value::IntArray(b)) => {
            let ba = a.read().unwrap();
            let bb = b.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vmul_i(
                &ba, &bb,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_div_I<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v1, v2, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v1, v2, inst.operands[2].clone())
    };
    let res = match (v1, v2) {
        (Value::V4Int(a), Value::V4Int(b)) => {
            Value::V4Int(crate::vm::simd::simd_v4div_i(a, b))
        }
        (Value::IntArray(a), Value::IntArray(b)) => {
            let ba = a.read().unwrap();
            let bb = b.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vdiv_i(
                &ba, &bb,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_rem_I<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v1, v2, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v1, v2, inst.operands[2].clone())
    };
    let res = match (v1, v2) {
        (Value::V4Int(a), Value::V4Int(b)) => {
            Value::V4Int(crate::vm::simd::simd_v4rem_i(a, b))
        }
        (Value::IntArray(a), Value::IntArray(b)) => {
            let ba = a.read().unwrap();
            let bb = b.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vrem_i(
                &ba, &bb,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_sum_I<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        (v, inst.operands[1].clone())
    };
    let sum = match v {
        Value::V4Int(a) => crate::vm::simd::simd_v4sum_i(a),
        Value::IntArray(a) => crate::vm::simd::simd_vsum_i(&a.read().unwrap()),
        _ => 0,
    };
    save_value_to_operand(ctx.process, &dst_op, Value::Int(sum));
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_sum_F<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        (v, inst.operands[1].clone())
    };
    let sum = match v {
        Value::V4Float(a) => crate::vm::simd::simd_v4sum_f(a),
        Value::FloatArray(a) => crate::vm::simd::simd_vsum_f(&a.read().unwrap()),
        _ => 0.0,
    };
    save_value_to_operand(ctx.process, &dst_op, Value::Float(sum));
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// SIMD Vector Immediate Handlers
// -----------------------------------------------------------------------------

fn op_V_add_IC<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v, c_val, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let c = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v, c, inst.operands[2].clone())
    };
    let c_int = match c_val {
        Value::Int(i) => i,
        _ => 0,
    };
    let res = match v {
        Value::V4Int(a) => Value::V4Int(crate::vm::simd::simd_v4add_ic(a, c_int)),
        Value::IntArray(a) => {
            let ba = a.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vadd_ic(
                &ba, c_int,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_sub_IC<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v, c_val, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let c = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v, c, inst.operands[2].clone())
    };
    let c_int = match c_val {
        Value::Int(i) => i,
        _ => 0,
    };
    let res = match v {
        Value::V4Int(a) => Value::V4Int(crate::vm::simd::simd_v4sub_ic(a, c_int)),
        Value::IntArray(a) => {
            let ba = a.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vsub_ic(
                &ba, c_int,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_mul_IC<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v, c_val, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let c = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v, c, inst.operands[2].clone())
    };
    let c_int = match c_val {
        Value::Int(i) => i,
        _ => 0,
    };
    let res = match v {
        Value::V4Int(a) => Value::V4Int(crate::vm::simd::simd_v4mul_ic(a, c_int)),
        Value::IntArray(a) => {
            let ba = a.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vmul_ic(
                &ba, c_int,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_div_IC<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v, c_val, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let c = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v, c, inst.operands[2].clone())
    };
    let c_int = match c_val {
        Value::Int(i) => i,
        _ => 0,
    };
    let res = match v {
        Value::V4Int(a) => Value::V4Int(crate::vm::simd::simd_v4div_ic(a, c_int)),
        Value::IntArray(a) => {
            let ba = a.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vdiv_ic(
                &ba, c_int,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_rem_IC<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v, c_val, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let c = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v, c, inst.operands[2].clone())
    };
    let c_int = match c_val {
        Value::Int(i) => i,
        _ => 0,
    };
    let res = match v {
        Value::V4Int(a) => Value::V4Int(crate::vm::simd::simd_v4rem_ic(a, c_int)),
        Value::IntArray(a) => {
            let ba = a.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vrem_ic(
                &ba, c_int,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// SIMD Vector Construction & Selection Handlers
// -----------------------------------------------------------------------------

fn op_V_iota_I<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (start_val, count_val, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let s = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let c = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (s, c, inst.operands[2].clone())
    };
    let s_int = match start_val {
        Value::Int(i) => i,
        _ => 0,
    };
    let c_int = match count_val {
        Value::Int(i) => i.max(0) as usize,
        _ => 0,
    };
    let res = if c_int == 4 {
        Value::V4Int([s_int, s_int + 1, s_int + 2, s_int + 3])
    } else {
        let vec = crate::vm::simd::simd_viota_i(s_int, c_int);
        Value::IntArray(Arc::new(RwLock::new(vec)))
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_splat_I<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (val, count_val, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let c = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v, c, inst.operands[2].clone())
    };
    let v_int = match val {
        Value::Int(i) => i,
        _ => 0,
    };
    let c_int = match count_val {
        Value::Int(i) => i.max(0) as usize,
        _ => 0,
    };
    let res = if c_int == 4 {
        Value::V4Int([v_int, v_int, v_int, v_int])
    } else {
        let vec = crate::vm::simd::simd_vsplat_i(v_int, c_int);
        Value::IntArray(Arc::new(RwLock::new(vec)))
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_splat_F<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (val, count_val, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let c = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v, c, inst.operands[2].clone())
    };
    let v_float = match val {
        Value::Float(f) => f,
        Value::Int(i) => i as f64,
        _ => 0.0,
    };
    let c_int = match count_val {
        Value::Int(i) => i.max(0) as usize,
        _ => 0,
    };
    let res = if c_int == 4 {
        Value::V4Float([v_float, v_float, v_float, v_float])
    } else {
        let vec = crate::vm::simd::simd_vsplat_f(v_float, c_int);
        Value::FloatArray(Arc::new(RwLock::new(vec)))
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_gather_I<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v_table, v_indices, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let t = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let idx = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (t, idx, inst.operands[2].clone())
    };
    let res = match (v_table, v_indices) {
        (Value::IntArray(t), Value::V4Int(idx)) => {
            let bt = t.read().unwrap();
            Value::V4Int(crate::vm::simd::simd_v4gather_i(&bt, idx))
        }
        (Value::FloatArray(t), Value::V4Int(idx)) => {
            let bt = t.read().unwrap();
            let mut out = [0.0f64; 4];
            for i in 0..4 {
                let pos = idx[i] as usize;
                out[i] = bt.get(pos).copied().unwrap_or(0.0);
            }
            Value::V4Float(out)
        }
        (Value::Array(t), Value::V4Int(idx)) => {
            let bt = t.read().unwrap();
            let mut out = [0i64; 4];
            for i in 0..4 {
                let pos = idx[i] as usize;
                out[i] = match bt.get(pos) {
                    Some(Value::Int(val)) => *val,
                    Some(Value::Float(f)) => *f as i64,
                    _ => 0,
                };
            }
            Value::V4Int(out)
        }
        (Value::IntArray(t), Value::IntArray(idx)) => {
            let bt = t.read().unwrap();
            let bidx = idx.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vgather_i(
                &bt, &bidx,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_extract_I<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v_vec, lane_idx, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let lane = match resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]) {
            Value::Int(i) => i.max(0) as usize,
            _ => 0,
        };
        (v, lane, inst.operands[2].clone())
    };
    let res = match v_vec {
        Value::V4Int(a) => Value::Int(*a.get(lane_idx).unwrap_or(&0)),
        Value::IntArray(a) => Value::Int(*a.read().unwrap().get(lane_idx).unwrap_or(&0)),
        _ => Value::Int(0),
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_select_I<'a>(
    ctx: &'a mut ExecutionContext<'a>,
) -> ExecutionResult {
    let (mask, on_true, on_false, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst =
            &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];

        if inst.operands.len() != 4 {
            return ExecutionResult::Error(format!(
                "VSelectI expects 4 operands, got {}",
                inst.operands.len()
            ));
        }

        let mask = resolve_operand(
            ctx.bytecode, ctx.process, &inst.operands[0],
        );
        let on_true = resolve_operand(
            ctx.bytecode, ctx.process, &inst.operands[1],
        );
        let on_false = resolve_operand(
            ctx.bytecode, ctx.process, &inst.operands[2],
        );

        (mask, on_true, on_false, inst.operands[3].clone())
    };

    let result = match (mask, on_true, on_false) {
        (
            Value::V4Int(mask),
            Value::V4Int(on_true),
            Value::V4Int(on_false),
        ) => Value::V4Int(crate::vm::simd::simd_v4select_i(
            mask, on_true, on_false,
        )),
        _ => Value::Nil,
    };

    save_value_to_operand(ctx.process, &dst_op, result);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_select_F<'a>(
    ctx: &'a mut ExecutionContext<'a>,
) -> ExecutionResult {
    let (mask, on_true, on_false, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst =
            &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];

        if inst.operands.len() != 4 {
            return ExecutionResult::Error(format!(
                "VSelectF expects 4 operands, got {}",
                inst.operands.len()
            ));
        }

        let mask = resolve_operand(
            ctx.bytecode, ctx.process, &inst.operands[0],
        );
        let on_true = resolve_operand(
            ctx.bytecode, ctx.process, &inst.operands[1],
        );
        let on_false = resolve_operand(
            ctx.bytecode, ctx.process, &inst.operands[2],
        );

        (mask, on_true, on_false, inst.operands[3].clone())
    };

    let result = match (mask, on_true, on_false) {
        (
            Value::V4Int(mask),
            Value::V4Float(on_true),
            Value::V4Float(on_false),
        ) => Value::V4Float(crate::vm::simd::simd_v4select_f(
            mask, on_true, on_false,
        )),
        _ => Value::Nil,
    };

    save_value_to_operand(ctx.process, &dst_op, result);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_filter_I<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v_vals, v_mask, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let m = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v, m, inst.operands[2].clone())
    };
    let res = match (v_vals, v_mask) {
        (Value::V4Int(vals), Value::V4Int(mask)) => {
            let out = crate::vm::simd::simd_v4filter_i(vals, mask);
            Value::IntArray(Arc::new(RwLock::new(out)))
        }
        (Value::IntArray(vals), Value::IntArray(mask)) => {
            let bvals = vals.read().unwrap();
            let bmask = mask.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vfilter_i(
                &bvals, &bmask,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// SIMD Vector Comparison Handlers
// -----------------------------------------------------------------------------

fn op_V_cmp_eq_I<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v1, v2, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v1, v2, inst.operands[2].clone())
    };
    let res = match (v1, v2) {
        (Value::V4Int(a), Value::V4Int(b)) => {
            Value::V4Int(crate::vm::simd::simd_v4cmp_eq_i(a, b))
        }
        (Value::IntArray(a), Value::IntArray(b)) => {
            let ba = a.read().unwrap();
            let bb = b.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vcmp_eq_i(
                &ba, &bb,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_cmp_gt_I<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v1, v2, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v1, v2, inst.operands[2].clone())
    };
    let res = match (v1, v2) {
        (Value::V4Int(a), Value::V4Int(b)) => {
            Value::V4Int(crate::vm::simd::simd_v4cmp_gt_i(a, b))
        }
        (Value::IntArray(a), Value::IntArray(b)) => {
            let ba = a.read().unwrap();
            let bb = b.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vcmp_gt_i(
                &ba, &bb,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}


fn op_V_cmp_lt_I<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (v1, v2, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let v1 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let v2 = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (v1, v2, inst.operands[2].clone())
    };
    let res = match (v1, v2) {
        (Value::V4Int(a), Value::V4Int(b)) => {
            Value::V4Int(crate::vm::simd::simd_v4cmp_lt_i(a, b))
        }
        (Value::IntArray(a), Value::IntArray(b)) => {
            let ba = a.read().unwrap();
            let bb = b.read().unwrap();
            Value::IntArray(Arc::new(RwLock::new(crate::vm::simd::simd_vcmp_lt_i(
                &ba, &bb,
            ))))
        }
        _ => Value::Nil,
    };
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// SIMD Benchmark Handlers
// -----------------------------------------------------------------------------

fn op_V_munch_range<'a>(ctx: &'a mut ExecutionContext<'a>) -> ExecutionResult {
    let (start_val, end_val, dst_op) = {
        let frame = ctx.process.frame_stack.last().unwrap();
        let inst = &ctx.bytecode.functions[frame.function_index].instructions[frame.pc];
        let s = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[0]);
        let e = resolve_operand(ctx.bytecode, ctx.process, &inst.operands[1]);
        (s, e, inst.operands[2].clone())
    };
    let s_int = match start_val {
        Value::Int(i) => i,
        _ => 0,
    };
    let e_int = match end_val {
        Value::Int(i) => i,
        _ => 0,
    };
    let matches = crate::vm::simd::simd_munchausen_range(s_int, e_int);
    let res = Value::IntArray(std::sync::Arc::new(std::sync::RwLock::new(matches)));
    save_value_to_operand(ctx.process, &dst_op, res);
    ctx.process.frame_stack.last_mut().unwrap().pc += 1;
    become fetch_and_dispatch(ctx);
}



// -----------------------------------------------------------------------------
// Dispatch Table (160 Opcodes)
// -----------------------------------------------------------------------------

pub static DISPATCH_TABLE: [HandlerFn; OPCODE_COUNT] = [
    // Control Flow
    op_load_const,  // 0: LoadConst
    op_nop,  // 1: Nop
    op_swap,  // 2: Swap
    op_jump,  // 3: Jump
    op_jump_true,  // 4: JumpTrue
    op_jump_false,  // 5: JumpFalse
    op_I_jump_true_R,  // 6: IJumpTrueR
    op_I_jump_false_R,  // 7: IJumpFalseR
    op_call,  // 8: Call
    op_call_jit,  // 9: CallJit
    op_ret,  // 10: Ret
    // Generic Arithmetic
    op_add,  // 11: Add
    op_sub,  // 12: Sub
    op_mul,  // 13: Mul
    op_div,  // 14: Div
    op_rem,  // 15: Rem
    op_neg,  // 16: Neg
    // Generic Comparison
    op_eq,  // 17: Eq
    op_neq,  // 18: Neq
    op_lt,  // 19: Lt
    op_lte,  // 20: Le
    op_gt,  // 21: Gt
    op_gte,  // 22: Ge
    // Logical
    op_and,  // 23: And
    op_or,  // 24: Or
    op_xor,  // 25: Xor
    op_not,  // 26: Not
    // Bitwise & Shift
    op_shl,  // 27: Shl
    op_shr,  // 28: Shr
    // Object & Properties
    op_get_prop,  // 29: GetProp
    op_set_prop,  // 30: SetProp
    op_get_index,  // 31: GetIndex
    op_new_object,  // 32: NewObject
    op_make_closure,  // 33: MakeClosure
    op_make_env,  // 34: MakeEnv
    // Actor & Concurrency
    op_send,  // 35: Send
    op_receive,  // 36: Receive
    op_receive_timeout,  // 37: ReceiveTimeout
    op_spawn,  // 38: Spawn
    // Pattern Matching
    op_match_pattern,  // 39: Match
    op_A_match_fast,  // 40: AMatchFast
    // Fast Integer Arithmetic
    op_I_add_RRR,  // 41: IAddRRR
    op_I_add_RCR,  // 42: IAddRCR
    op_I_sub_RRR,  // 43: ISubRRR
    op_I_sub_RCR,  // 44: ISubRCR
    op_I_sub_CRR,  // 45: ISubCRR
    op_I_mul_RRR,  // 46: IMulRRR
    op_I_mul_RCR,  // 47: IMulRCR
    op_I_div_RCR,  // 48: IDivRCR
    op_I_div_CRR,  // 49: IDivCRR
    op_I_rem_RCR,  // 50: IRemRCR
    op_I_rem_CRR,  // 51: IRemCRR
    // Fast Integer Comparison
    op_I_eq_RRR,  // 52: IEqRRR
    op_I_eq_RCR,  // 53: IEqRCR
    op_I_lt_RRR,  // 54: ILtRRR
    op_I_lt_RCR,  // 55: ILtRCR
    op_I_lt_CRR,  // 56: ILtCRR
    op_I_gt_RRR,  // 57: IGtRRR
    op_I_gt_RCR,  // 58: IGtRCR
    op_I_gt_CRR,  // 59: IGtCRR
    // Fast Float Arithmetic
    op_F_add_RRR,  // 60: FAddRRR
    op_F_add_RCR,  // 61: FAddRCR
    op_F_sub_RRR,  // 62: FSubRRR
    op_F_sub_RCR,  // 63: FSubRCR
    op_F_sub_CRR,  // 64: FSubCRR
    op_F_mul_RRR,  // 65: FMulRRR
    op_F_mul_RCR,  // 66: FMulRCR
    op_F_div_RRR,  // 67: FDivRRR
    op_F_div_RCR,  // 68: FDivRCR
    op_F_div_CRR,  // 69: FDivCRR
    // Fast Float Comparison
    op_F_eq_RRR,  // 70: FEqRRR
    op_F_eq_RCR,  // 71: FEqRCR
    op_F_lt_RRR,  // 72: FLtRRR
    op_F_lt_RCR,  // 73: FLtRCR
    op_F_lt_CRR,  // 74: FLtCRR
    op_F_gt_RRR,  // 75: FGtRRR
    op_F_gt_RCR,  // 76: FGtRCR
    op_F_gt_CRR,  // 77: FGtCRR
    // Fused Compare-Branch
    op_I_gt_jmp_RR,  // 78: IGtJmpRR
    op_I_gt_jmp_RC,  // 79: IGtJmpRC
    op_I_lt_jmp_RR,  // 80: ILtJmpRR
    op_I_lt_jmp_RC,  // 81: ILtJmpRC
    op_I_eq_jmp_RR,  // 82: IEqJmpRR
    op_I_eq_jmp_RC,  // 83: IEqJmpRC
    op_F_gt_jmp_RR,  // 84: FGtJmpRR
    op_F_gt_jmp_RC,  // 85: FGtJmpRC
    op_F_lt_jmp_RR,  // 86: FLtJmpRR
    op_F_lt_jmp_RC,  // 87: FLtJmpRC
    // Fast Bitwise & Shift
    op_I_shl_RRR,  // 88: IShlRRR
    op_I_shl_RCR,  // 89: IShlRCR
    op_I_shr_RRR,  // 90: IShrRRR
    op_I_shr_RCR,  // 91: IShrRCR
    op_I_and_RRR,  // 92: IAndRRR
    op_I_and_RCR,  // 93: IAndRCR
    op_I_or_RRR,  // 94: IOrRRR
    op_I_or_RCR,  // 95: IOrRCR
    op_I_xor_RRR,  // 96: IXorRRR
    op_I_xor_RCR,  // 97: IXorRCR
    op_I_bit_get,  // 98: IBitGet
    op_I_bit_set,  // 99: IBitSet
    op_I_bit_set1,  // 100: IBitSet1
    op_I_bit_set0,  // 101: IBitSet0
    // String Operations
    op_S_concat_RRR,  // 102: SConcatRRR
    op_S_slice_RRR,  // 103: SSliceRRR
    op_S_slice_RRRR,  // 104: SSliceRRRR
    op_S_eq_RRR,  // 105: SEqRRR
    op_S_find_RRR,  // 106: SFindRRR
    op_S_to_int_RR,  // 107: SToIntRR
    op_S_upper_RR,  // 108: SUpperRR
    op_S_lower_RR,  // 109: SLowerRR
    op_S_count_byte_RRR,  // 110: SCountByteRRR
    op_format_value,  // 111: FormatValue
    op_build_string,  // 112: BuildString
    // Tagged Enum
    op_E_make_enum,  // 113: EMakeEnum
    op_E_get_tag_RR,  // 114: EGetTagRR
    op_E_get_payload_RR,  // 115: EGetPayloadRRC
    op_E_get_payload_RR_multi,  // 116: EGetPayloadRRMulti
    op_E_tag_jmp_RC,  // 117: ETagJmpRC
    op_E_tag_switch,  // 118: ETagSwitch
    // Array Operations
    op_new_array,  // 119: NewArray
    op_new_int_array,  // 120: NewIntArray
    op_new_float_array,  // 121: NewFloatArray
    op_array_len,  // 122: ArrayLen
    op_array_push,  // 123: ArrayPush
    op_array_get,  // 124: ArrayGet
    op_array_set,  // 125: ArraySet
    op_A_get_index_AR,  // 126: AGetIndexAR
    op_I_array_get,  // 127: IArrayGet
    op_I_array_set,  // 128: IArraySet
    op_F_array_get,  // 129: FArrayGet
    op_F_array_set,  // 130: FArraySet
    // SIMD Vector Arithmetic
    op_V_add_F,  // 131: VAddF
    op_V_sub_F,  // 132: VSubF
    op_V_mul_F,  // 133: VMulF
    op_V_dot_F,  // 134: VDotF
    op_V_div_F,  // 135: VDivF
    op_V_add_I,  // 136: VAddI
    op_V_sub_I,  // 137: VSubI
    op_V_mul_I,  // 138: VMulI
    op_V_div_I,  // 139: VDivI
    op_V_rem_I,  // 140: VRemI
    op_V_sum_I,  // 141: VSumI
    op_V_sum_F,  // 142: VSumF
    // SIMD Vector Immediate
    op_V_add_IC,  // 143: VAddIC
    op_V_sub_IC,  // 144: VSubIC
    op_V_mul_IC,  // 145: VMulIC
    op_V_div_IC,  // 146: VDivIC
    op_V_rem_IC,  // 147: VRemIC
    // SIMD Vector Construction & Selection
    op_V_iota_I,  // 148: VIotaI
    op_V_splat_I,  // 149: VSplatI
    op_V_splat_F,  // 150: VSplatF
    op_V_gather_I,  // 151: VGatherI
    op_V_extract_I,  // 152: VExtractI
    op_V_select_I,  // 153: VSelectI
    op_V_select_F,  // 154: VSelectF
    op_V_filter_I,  // 155: VFilterI
    // SIMD Vector Comparison
    op_V_cmp_eq_I,  // 156: VCmpEqI
    op_V_cmp_gt_I,  // 157: VCmpGtI
    op_V_cmp_lt_I,  // 158: VCmpLtI
    // SIMD Benchmark
    op_V_munch_range,  // 159: VMunchRange
];
