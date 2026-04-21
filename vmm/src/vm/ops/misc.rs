use super::super::{ExecutionResult, IonVM};
use crate::instruction::Pattern;
use crate::value::process::{Frame, Process};
use crate::value::{Primitive, Value};
use std::cell::RefCell;
use std::rc::Rc;

pub(super) fn exec_load_const(vm: &IonVM, proc: &mut Process, reg: usize, val: Value) {
    let pid = proc.pid;
    let frame = proc.frames.last_mut().expect("active frame required");

    let resolved = match &val {
        Value::Primitive(Primitive::Atom(atom)) => {
            if atom == "__vm:this" {
                // Read bound_this from the current function
                frame
                    .function
                    .borrow()
                    .bound_this
                    .clone()
                    .unwrap_or(Value::Primitive(Primitive::Undefined))
            } else if atom == "self" || atom.starts_with("__vm:") {
                // Resolve other VM intrinsics
                let key = if atom == "self" { "__vm:self" } else { atom };
                vm.resolve_vm_atom(key, pid)
                    .unwrap_or(Value::Primitive(Primitive::Undefined))
            } else {
                val
            }
        }
        _ => val,
    };

    if vm.debug {
        eprintln!("[VM] LOAD_CONST r{} = {:?}", reg, resolved);
    }
    frame.registers[reg] = resolved;
}

pub(super) fn exec_move(frame: &mut Frame, dst: usize, src: usize) {
    frame.registers[dst] = frame.registers[src].clone();
}

pub(super) fn exec_array_init(frame: &mut Frame, dst: usize, srcs: Vec<usize>) {
    let mut vec = Vec::with_capacity(srcs.len());
    for src in srcs {
        vec.push(frame.registers[src].clone());
    }
    frame.registers[dst] = Value::Array(Rc::new(RefCell::new(vec)));
}

pub(super) fn exec_match(
    proc: &mut Process,
    src_reg: usize,
    patterns: Vec<(Pattern, isize)>,
) -> ExecutionResult {
    let value = proc.frames.last().expect("active frame required").registers[src_reg].clone();

    for (pattern, offset) in patterns {
        if matches_pattern(&value, &pattern) {
            let frame = proc.frames.last_mut().unwrap();
            frame.ip = (frame.ip as isize + offset - 1) as usize;
            return ExecutionResult::Continue;
        }
    }
    ExecutionResult::Continue
}

fn matches_pattern(value: &Value, pattern: &Pattern) -> bool {
    match (value, pattern) {
        (_, Pattern::Wildcard) => true,
        (Value::Primitive(vp), Pattern::Value(Value::Primitive(pp))) => vp == pp,
        (Value::Array(arr), Pattern::Array(pats)) => {
            let arr = arr.borrow();
            arr.len() == pats.len() && arr.iter().zip(pats).all(|(v, p)| matches_pattern(v, p))
        }
        (Value::Tuple(tup), Pattern::Tuple(pats)) => {
            tup.len() == pats.len() && tup.iter().zip(pats).all(|(v, p)| matches_pattern(v, p))
        }
        (Value::Array(arr), Pattern::Tuple(pats)) => {
            let arr = arr.borrow();
            arr.len() == pats.len() && arr.iter().zip(pats).all(|(v, p)| matches_pattern(v, p))
        }
        (Value::Object(obj), Pattern::Tuple(pats)) => {
            let obj = obj.borrow();
            // Check if object has properties "0", "1", ... matching the patterns
            pats.iter()
                .enumerate()
                .all(|(i, p)| match obj.get_property(&i.to_string()) {
                    Some(v) => matches_pattern(&v, p),
                    None => false,
                })
        }
        (Value::Object(obj), Pattern::TaggedEnum(tag, pat)) => {
            let obj = obj.borrow();
            // Match if __tag matches the expected tag (ignoring optional leading colon)
            let tag_match = match obj.get_property("__tag") {
                Some(Value::Primitive(Primitive::Atom(v))) => {
                    let v_trimmed = v.strip_prefix(':').unwrap_or(&v);
                    let tag_trimmed = tag.strip_prefix(':').unwrap_or(tag);
                    v_trimmed == tag_trimmed
                }
                Some(Value::Primitive(Primitive::String(v))) => {
                    let v_trimmed = v.strip_prefix(':').unwrap_or(&v);
                    let tag_trimmed = tag.strip_prefix(':').unwrap_or(tag);
                    v_trimmed == tag_trimmed
                }
                _ => false,
            };

            if !tag_match {
                return false;
            }

            // Match payload against __slots. Fallback to Unit if missing.
            let slots = obj
                .get_property("__slots")
                .unwrap_or(Value::Primitive(Primitive::Unit));
            matches_pattern(&slots, pat)
        }
        (v, Pattern::Value(pv)) => v == pv,
        _ => false,
    }
}
