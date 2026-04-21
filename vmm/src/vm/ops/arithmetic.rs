use crate::value::process::Frame;
use crate::value::{Primitive, Value};
use num_complex::Complex64;

// Applies a numeric operation to two values.
// Handles Number+Number, Complex+Complex, and mixed Number+Complex cases.
// Returns Undefined for unsupported type combinations.
fn apply_op(
    a: &Value,
    b: &Value,
    num_op: impl Fn(f64, f64) -> f64,
    cx_op: impl Fn(Complex64, Complex64) -> Complex64,
) -> Value {
    use Primitive::*;
    use Value::Primitive as P;
    match (a, b) {
        (P(Number(x)), P(Number(y))) => P(Number(num_op(*x, *y))),
        (P(Complex(cx)), P(Complex(cy))) => P(Complex(cx_op(*cx, *cy))),
        (P(Number(n)), P(Complex(c))) => P(Complex(cx_op(Complex64::new(*n, 0.0), *c))),
        (P(Complex(c)), P(Number(n))) => P(Complex(cx_op(*c, Complex64::new(*n, 0.0)))),
        _ => P(Undefined),
    }
}

pub(super) fn exec_add(frame: &mut Frame, dst: usize, a: usize, b: usize) {
    let result = match (&frame.registers[a], &frame.registers[b]) {
        (Value::Primitive(Primitive::String(sx)), Value::Primitive(Primitive::String(sy))) => {
            Value::Primitive(Primitive::String(sx.clone() + sy))
        }
        (av, bv) => apply_op(av, bv, |x, y| x + y, |cx, cy| cx + cy),
    };
    frame.registers[dst] = result;
}

pub(super) fn exec_sub(frame: &mut Frame, dst: usize, a: usize, b: usize) {
    frame.registers[dst] = apply_op(
        &frame.registers[a],
        &frame.registers[b],
        |x, y| x - y,
        |cx, cy| cx - cy,
    );
}

pub(super) fn exec_mul(frame: &mut Frame, dst: usize, a: usize, b: usize) {
    let result = match (&frame.registers[a], &frame.registers[b]) {
        (Value::Primitive(Primitive::String(sx)), Value::Primitive(Primitive::Number(n)))
        | (Value::Primitive(Primitive::Number(n)), Value::Primitive(Primitive::String(sx))) => {
            Value::Primitive(Primitive::String(sx.repeat(*n as usize)))
        }
        (av, bv) => apply_op(av, bv, |x, y| x * y, |cx, cy| cx * cy),
    };
    frame.registers[dst] = result;
}

pub(super) fn exec_div(frame: &mut Frame, dst: usize, a: usize, b: usize) {
    use Primitive::*;
    use Value::Primitive as P;
    let result = match (&frame.registers[a], &frame.registers[b]) {
        (P(Number(x)), P(Number(y))) => {
            if *y != 0.0 {
                P(Number(x / y))
            } else {
                P(Undefined)
            }
        }
        (P(Complex(cx)), P(Complex(cy))) => P(Complex(cx / cy)),
        (P(Number(x)), P(Complex(cy))) => P(Complex(Complex64::new(*x, 0.0) / cy)),
        (P(Complex(cx)), P(Number(y))) => {
            if *y != 0.0 {
                P(Complex(cx / y))
            } else {
                P(Undefined)
            }
        }
        _ => P(Undefined),
    };
    frame.registers[dst] = result;
}
