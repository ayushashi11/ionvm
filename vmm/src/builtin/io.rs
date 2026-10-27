
use std::collections::{BTreeMap, HashSet};
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::lang::types::Value;

pub fn println(args: &[Value]) -> io::Result<Value> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let mut active = HashSet::new();

    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            write!(handle, " ")?;
        }
        write_serialized_value(&mut handle, arg, 0, &mut active, false)?;
    }

    // Exactly one newline per println invocation.
    writeln!(handle)?;
    handle.flush()?;

    Ok(Value::Nil)
}

pub fn print(args: &[Value]) -> io::Result<Value> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let mut active = HashSet::new();

    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            write!(handle, " ")?;
        }
        write_serialized_value(&mut handle, arg, 0, &mut active, false)?;
    }

    handle.flush()?;
    Ok(Value::Nil)
}

pub fn time() -> Value {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    Value::Float(now.as_secs_f64())
}

type ContainerId = (u8, usize);

fn lock_error(kind: &'static str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Other,
        format!("{kind} lock poisoned"),
    )
}

fn write_indent<W: Write>(
    writer: &mut W,
    depth: usize,
) -> io::Result<()> {
    for _ in 0..depth {
        writer.write_all(b"  ")?;
    }
    Ok(())
}

/// Writes an escaped JSON string, including its surrounding quotes.
fn write_json_string<W: Write>(
    writer: &mut W,
    value: &str,
    debug: bool
) -> io::Result<()> {
    if !debug {
        // In non-debug mode, we can just write the string as-is, since it is
        // guaranteed to be a valid JSON string.
        return write!(writer, "{}", value);
    }

    writer.write_all(b"\"")?;

    for ch in value.chars() {
        match ch {
            '"' => writer.write_all(b"\\\"")?,
            '\\' => writer.write_all(b"\\\\")?,
            '\u{0008}' => writer.write_all(b"\\b")?,
            '\u{000C}' => writer.write_all(b"\\f")?,
            '\n' => writer.write_all(b"\\n")?,
            '\r' => writer.write_all(b"\\r")?,
            '\t' => writer.write_all(b"\\t")?,
            c if (c as u32) < 0x20 => {
                write!(writer, "\\u{:04x}", c as u32)?;
            }
            c => write!(writer, "{c}")?,
        }
    }

    writer.write_all(b"\"")?;
    Ok(())
}

fn write_float<W: Write>(
    writer: &mut W,
    value: f64,
) -> io::Result<()> {
    // NaN and Infinity are not valid JSON numbers.
    if value.is_finite() {
        write!(writer, "{value}")
    } else {
        writer.write_all(b"null")
    }
}

fn write_serialized_value<W: Write>(
    writer: &mut W,
    val: &Value,
    depth: usize,
    active: &mut HashSet<ContainerId>,
    debug: bool,
) -> io::Result<()> {
    match val {
        Value::Int(v) => write!(writer, "{v}"),

        Value::Float(v) => write_float(writer, *v),

        Value::Bool(v) => {
            writer.write_all(if *v { b"true" } else { b"false" })
        }

        Value::String(s) => write_json_string(writer, s, debug),

        Value::Nil => writer.write_all(b"null"),

        Value::Function(f) => {
            let name = f
                .read()
                .map_err(|_| lock_error("function"))?
                .name
                .clone();

            // Diagnostic representation, not a restorable function.
            write_json_string(writer, &format!("[Function {name}]"), false)
        }

        Value::Object(obj) => {
            // Include the container kind to distinguish object/array IDs.
            let id = (0, Arc::as_ptr(obj) as usize);

            if !active.insert(id) {
                return write_json_string(writer, "[Circular]", true);
            }

            let result = (|| {
                // Snapshot the object, releasing its lock before recursing.
                let entries: BTreeMap<String, Value> = {
                    let object = obj
                        .read()
                        .map_err(|_| lock_error("object"))?;

                    let mut entries = BTreeMap::new();

                    // Preserve indexed slots as numeric string keys.
                    // Missing indexed slots become null.
                    for (index, property) in object.indexed.iter().enumerate() {
                        let value = property
                            .as_ref()
                            .map(|p| p.value.clone())
                            .unwrap_or(Value::Nil);

                        entries.insert(index.to_string(), value);
                    }

                    // Named properties take precedence over an indexed
                    // property with the same string key.
                    for (key, property) in &object.properties {
                        entries.insert(key.clone(), property.value.clone());
                    }

                    entries
                };

                write_object_entries(writer, &entries, depth, active)
            })();

            active.remove(&id);
            result
        }

        Value::Array(array) => {
            let id = (1, Arc::as_ptr(array) as usize);

            if !active.insert(id) {
                return write_json_string(writer, "[Circular]", true);
            }

            let result = (|| {
                // Clone the elements so no RwLock guard is held while
                // recursively serializing nested values.
                let items = array
                    .read()
                    .map_err(|_| lock_error("array"))?
                    .to_vec();

                if items.is_empty() {
                    return writer.write_all(b"[]");
                }

                writer.write_all(b"[\n")?;

                for (i, item) in items.iter().enumerate() {
                    write_indent(writer, depth + 1)?;
                    write_serialized_value(
                        writer,
                        item,
                        depth + 1,
                        active,
                        true
                    )?;

                    if i + 1 < items.len() {
                        writer.write_all(b",")?;
                    }

                    writer.write_all(b"\n")?;
                }

                write_indent(writer, depth)?;
                writer.write_all(b"]")
            })();

            active.remove(&id);
            result
        }

        Value::IntArray(array) => {
            let items = array
                .read()
                .map_err(|_| lock_error("integer array"))?
                .to_vec();

            writer.write_all(b"[")?;

            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    writer.write_all(b", ")?;
                }
                write!(writer, "{item}")?;
            }

            writer.write_all(b"]")
        }

        Value::FloatArray(array) => {
            let items = array
                .read()
                .map_err(|_| lock_error("float array"))?
                .to_vec();

            writer.write_all(b"[")?;

            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    writer.write_all(b", ")?;
                }
                write_float(writer, *item)?;
            }

            writer.write_all(b"]")
        }

        Value::V4Int(array) => {
            writer.write_all(b"[")?;
            for (i, item) in array.iter().enumerate() {
                if i > 0 {
                    writer.write_all(b", ")?;
                }
                write!(writer, "{item}")?;
            }
            writer.write_all(b"]")
        }

        Value::V4Float(array) => {
            writer.write_all(b"[")?;
            for (i, item) in array.iter().enumerate() {
                if i > 0 {
                    writer.write_all(b", ")?;
                }
                write_float(writer, *item)?;
            }
            writer.write_all(b"]")
        }

        Value::V4Bool(array) => {
            writer.write_all(b"[")?;
            for (i, item) in array.iter().enumerate() {
                if i > 0 {
                    writer.write_all(b", ")?;
                }
                writer.write_all(if *item { b"true" } else { b"false" })?;
            }
            writer.write_all(b"]")
        }
    }
}

fn write_object_entries<W: Write>(
    writer: &mut W,
    entries: &BTreeMap<String, Value>,
    depth: usize,
    active: &mut HashSet<ContainerId>,
) -> io::Result<()> {
    if entries.is_empty() {
        return writer.write_all(b"{}");
    }

    writer.write_all(b"{\n")?;

    for (i, (key, value)) in entries.iter().enumerate() {
        write_indent(writer, depth + 1)?;
        write_json_string(writer, key, true)?;
        writer.write_all(b": ")?;

        write_serialized_value(
            writer,
            value,
            depth + 1,
            active,
            true
        )?;

        if i + 1 < entries.len() {
            writer.write_all(b",")?;
        }

        writer.write_all(b"\n")?;
    }

    write_indent(writer, depth)?;
    writer.write_all(b"}")
}
