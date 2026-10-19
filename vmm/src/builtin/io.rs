use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::lang::types::Value;

pub fn println(args: &[Value]) -> io::Result<Value> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            write!(handle, " ")?;
        }
        match arg {
            Value::Int(v) => write!(handle, "{}", v)?,
            Value::Float(v) => write!(handle, "{}", v)?,
            Value::Bool(v) => write!(handle, "{}", if *v { "true" } else { "false" })?,
            Value::String(s) => write!(handle, "{}", s)?,
            Value::Nil => write!(handle, "nil")?,
            Value::Object(_) => write!(handle, "[Object]")?,
            Value::Function(f) => write!(handle, "[Function {}]", f.read().unwrap().name)?,
            Value::Array(a) => write!(handle, "[Array len={}]", a.read().unwrap().len())?,
            Value::IntArray(a) => write!(handle, "[IntArray len={}]", a.read().unwrap().len())?,
            Value::FloatArray(a) => write!(handle, "[FloatArray len={}]", a.read().unwrap().len())?,
        }
    }
    writeln!(handle)?;
    handle.flush()?;
    Ok(Value::Nil)
}

pub fn print(args: &[Value]) -> io::Result<Value> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            write!(handle, " ")?;
        }
        match arg {
            Value::Int(v) => write!(handle, "{}", v)?,
            Value::Float(v) => write!(handle, "{}", v)?,
            Value::Bool(v) => write!(handle, "{}", if *v { "true" } else { "false" })?,
            Value::String(s) => write!(handle, "{}", s)?,
            Value::Nil => write!(handle, "nil")?,
            Value::Object(_) => write!(handle, "[Object]")?,
            Value::Function(f) => write!(handle, "[Function {}]", f.read().unwrap().name)?,
            Value::Array(a) => write!(handle, "[Array len={}]", a.read().unwrap().len())?,
            Value::IntArray(a) => write!(handle, "[IntArray len={}]", a.read().unwrap().len())?,
            Value::FloatArray(a) => write!(handle, "[FloatArray len={}]", a.read().unwrap().len())?,
        }
    }
    handle.flush()?;
    Ok(Value::Nil)
}

pub fn time() -> Value {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs_f64();
    Value::Float(secs)
}
