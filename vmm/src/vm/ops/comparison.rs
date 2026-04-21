use crate::value::process::Frame;
use crate::value::{Primitive, Value};
use std::cmp::Ordering;

fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    use Primitive::*;
    use Value::Primitive as P;
    match (a, b) {
        (P(Number(x)), P(Number(y))) => x.partial_cmp(y),
        (P(String(x)), P(String(y))) => Some(x.cmp(y)),
        (P(Atom(x)), P(Atom(y))) => Some(x.cmp(y)),
        _ => None,
    }
}

fn equal_values(a: &Value, b: &Value) -> bool {
    use Primitive::*;
    use Value::Primitive as P;
    match (a, b) {
        (P(Number(x)), P(Number(y))) => x == y,
        (P(Boolean(x)), P(Boolean(y))) => x == y,
        (P(String(x)), P(String(y))) | (P(Atom(x)), P(String(y))) | (P(String(x)), P(Atom(y))) => {
            x == y
        }
        (P(Atom(x)), P(Atom(y))) => x == y,
        (P(Unit), P(Unit)) | (P(Undefined), P(Undefined)) => true,
        _ => false,
    }
}

// Exported so that control flow ops can reuse truthiness logic
pub(crate) fn truthy(v: &Value) -> bool {
    match v {
        Value::Primitive(Primitive::Boolean(b)) => *b,
        Value::Primitive(Primitive::Number(n)) => *n != 0.0,
        Value::Primitive(Primitive::String(s)) => !s.is_empty(),
        Value::Primitive(Primitive::Atom(a)) => !a.is_empty(),
        Value::Primitive(Primitive::Unit) | Value::Primitive(Primitive::Undefined) => false,
        _ => true,
    }
}

pub(super) fn exec_equal(frame: &mut Frame, dst: usize, a: usize, b: usize) {
    let r = equal_values(&frame.registers[a], &frame.registers[b]);
    frame.registers[dst] = Value::Primitive(Primitive::Boolean(r));
}

pub(super) fn exec_not_equal(frame: &mut Frame, dst: usize, a: usize, b: usize) {
    let r = !equal_values(&frame.registers[a], &frame.registers[b]);
    frame.registers[dst] = Value::Primitive(Primitive::Boolean(r));
}

pub(super) fn exec_less_than(frame: &mut Frame, dst: usize, a: usize, b: usize) {
    let r =
        compare(&frame.registers[a], &frame.registers[b]).map_or(false, |o| o == Ordering::Less);
    frame.registers[dst] = Value::Primitive(Primitive::Boolean(r));
}

pub(super) fn exec_less_equal(frame: &mut Frame, dst: usize, a: usize, b: usize) {
    let r =
        compare(&frame.registers[a], &frame.registers[b]).map_or(false, |o| o != Ordering::Greater);
    frame.registers[dst] = Value::Primitive(Primitive::Boolean(r));
}

pub(super) fn exec_greater_than(frame: &mut Frame, dst: usize, a: usize, b: usize) {
    let r =
        compare(&frame.registers[a], &frame.registers[b]).map_or(false, |o| o == Ordering::Greater);
    frame.registers[dst] = Value::Primitive(Primitive::Boolean(r));
}

pub(super) fn exec_greater_equal(frame: &mut Frame, dst: usize, a: usize, b: usize) {
    let r =
        compare(&frame.registers[a], &frame.registers[b]).map_or(false, |o| o != Ordering::Less);
    frame.registers[dst] = Value::Primitive(Primitive::Boolean(r));
}

pub(super) fn exec_and(frame: &mut Frame, dst: usize, a: usize, b: usize) {
    let r = truthy(&frame.registers[a]) && truthy(&frame.registers[b]);
    frame.registers[dst] = Value::Primitive(Primitive::Boolean(r));
}

pub(super) fn exec_or(frame: &mut Frame, dst: usize, a: usize, b: usize) {
    let r = truthy(&frame.registers[a]) || truthy(&frame.registers[b]);
    frame.registers[dst] = Value::Primitive(Primitive::Boolean(r));
}

pub(super) fn exec_not(frame: &mut Frame, dst: usize, src: usize) {
    let r = !truthy(&frame.registers[src]);
    frame.registers[dst] = Value::Primitive(Primitive::Boolean(r));
}
