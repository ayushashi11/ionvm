use crate::lang::types::Value;

/// Format a `Value` using a standard format specifier.
///
/// Supported spec format:
/// `[[fill]align][sign][#][0][width][,|_][.precision][type]`
///
/// Types:
/// - `s`: string (default)
/// - `d`: decimal integer
/// - `b`: binary integer
/// - `o`: octal integer
/// - `x`: lowercase hexadecimal
/// - `X`: uppercase hexadecimal
/// - `f`: fixed-point float
/// - `e`: lowercase scientific notation
/// - `E`: uppercase scientific notation
/// - `c`: character (converts integer to unicode character)
/// - `%`: percentage (multiplies by 100 and appends '%')
///
/// Flags:
/// - `+`: force sign display (+ or -)
/// - `-`: negative sign only (default)
/// - ` `: space for positive numbers, - for negative
/// - `#`: alternate form (`0b`, `0o`, `0x`, `0X` prefixes)
/// - `0`: zero-padded (e.g. `05d`, `08`)
/// - `,`: comma thousands grouping
/// - `_`: underscore thousands grouping
/// - `<`: left align
/// - `>`: right align
/// - `^`: center align
pub fn format_value_with_spec(val: &Value, spec: &str) -> String {
    let spec = spec.trim();
    if spec.is_empty() {
        return val.to_str();
    }

    let mut chars = spec.chars().peekable();

    // 1. Check for fill & align: [[fill]align]
    let mut fill = ' ';
    let mut align = None;

    if let Some(&first) = chars.peek() {
        let mut chars_copy = chars.clone();
        chars_copy.next();
        if let Some(&second) = chars_copy.peek() {
            if second == '<' || second == '>' || second == '^' {
                fill = first;
                align = Some(second);
                chars.next();
                chars.next();
            }
        }
        if align.is_none() && (first == '<' || first == '>' || first == '^') {
            align = Some(first);
            chars.next();
        }
    }

    // 2. Sign: [sign]
    let mut sign_mode = None;
    if let Some(&c) = chars.peek() {
        if c == '+' || c == '-' || c == ' ' {
            sign_mode = Some(c);
            chars.next();
        }
    }

    // 3. Alternate form [#]
    let mut alt_form = false;
    if let Some(&'#') = chars.peek() {
        alt_form = true;
        chars.next();
    }

    // 4. Zero padding [0]
    let mut zero_pad = false;
    if let Some(&'0') = chars.peek() {
        zero_pad = true;
        chars.next();
    }

    // 5. Width [width]
    let mut width = 0;
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            width = width * 10 + (c as usize - '0' as usize);
            chars.next();
        } else {
            break;
        }
    }

    // 6. Grouping separator [,|_]
    let mut grouping = None;
    if let Some(&c) = chars.peek() {
        if c == ',' || c == '_' {
            grouping = Some(c);
            chars.next();
        }
    }

    // 7. Precision [.precision]
    let mut precision = None;
    if let Some(&'.') = chars.peek() {
        chars.next();
        let mut p = 0;
        let mut has_p = false;
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                has_p = true;
                p = p * 10 + (c as usize - '0' as usize);
                chars.next();
            } else {
                break;
            }
        }
        if has_p {
            precision = Some(p);
        } else {
            precision = Some(0);
        }
    }

    // 8. Type specifier [type]
    let type_char = chars.next();

    // Format the value based on type and value
    let mut formatted = match (val, type_char) {
        // Integer formats
        (Value::Int(i), Some('x')) => {
            let s = format!("{:x}", i);
            if alt_form { format!("0x{}", s) } else { s }
        }
        (Value::Int(i), Some('X')) => {
            let s = format!("{:X}", i);
            if alt_form { format!("0X{}", s) } else { s }
        }
        (Value::Int(i), Some('b')) => {
            let s = format!("{:b}", i);
            if alt_form { format!("0b{}", s) } else { s }
        }
        (Value::Int(i), Some('o')) => {
            let s = format!("{:o}", i);
            if alt_form { format!("0o{}", s) } else { s }
        }
        (Value::Int(i), Some('d') | None) => {
            if let Some(sep) = grouping {
                format_int_with_grouping(*i, sep)
            } else {
                format!("{}", i)
            }
        }
        (Value::Int(i), Some('f')) => {
            let f = *i as f64;
            let p = precision.unwrap_or(6);
            format!("{:.p$}", f)
        }
        (Value::Int(i), Some('e')) => {
            let f = *i as f64;
            let p = precision.unwrap_or(6);
            format!("{:.p$e}", f)
        }
        (Value::Int(i), Some('E')) => {
            let f = *i as f64;
            let p = precision.unwrap_or(6);
            format!("{:.p$E}", f)
        }
        (Value::Int(i), Some('c')) => {
            if let Ok(u) = u32::try_from(*i) {
                if let Some(ch) = char::from_u32(u) {
                    ch.to_string()
                } else {
                    format!("\\u{{{:x}}}", i)
                }
            } else {
                format!("{}", i)
            }
        }
        (Value::Int(i), Some('%')) => {
            let f = (*i as f64) * 100.0;
            let p = precision.unwrap_or(6);
            format!("{:.p$}%", f)
        }

        // Float formats
        (Value::Float(f), Some('f') | None) => {
            if let Some(p) = precision {
                format!("{:.p$}", f)
            } else if type_char == Some('f') {
                format!("{:.6}", f)
            } else {
                f.to_string()
            }
        }
        (Value::Float(f), Some('e')) => {
            let p = precision.unwrap_or(6);
            format!("{:.p$e}", f)
        }
        (Value::Float(f), Some('E')) => {
            let p = precision.unwrap_or(6);
            format!("{:.p$E}", f)
        }
        (Value::Float(f), Some('%')) => {
            let val = *f * 100.0;
            let p = precision.unwrap_or(6);
            format!("{:.p$}%", val)
        }
        (Value::Float(f), Some('d')) => {
            let i = *f as i64;
            if let Some(sep) = grouping {
                format_int_with_grouping(i, sep)
            } else {
                i.to_string()
            }
        }

        // String formats
        (Value::String(s), Some('c')) => {
            s.chars().next().map(|c| c.to_string()).unwrap_or_default()
        }
        (Value::String(s), _) => {
            if let Some(p) = precision {
                s.chars().take(p).collect()
            } else {
                s.clone()
            }
        }

        // Other values fallback to to_str()
        (v, _) => v.to_str(),
    };

    // Apply sign prefix if numeric (and not character format)
    if let Some(sign) = sign_mode {
        if !formatted.starts_with('-') && type_char != Some('c') {
            match val {
                Value::Int(_) | Value::Float(_) => {
                    if sign == '+' {
                        formatted = format!("+{}", formatted);
                    } else if sign == ' ' {
                        formatted = format!(" {}", formatted);
                    }
                }
                _ => {}
            }
        }
    }

    // Apply zero padding if requested and width > length
    if zero_pad && align.is_none() {
        let count = formatted.chars().count();
        if count < width {
            let padding = width - count;
            if formatted.starts_with('-') || formatted.starts_with('+') || formatted.starts_with(' ') {
                let sign = formatted.chars().next().unwrap();
                let rest: String = formatted.chars().skip(1).collect();
                formatted = format!("{}{}{}", sign, "0".repeat(padding), rest);
            } else if formatted.starts_with("0x") || formatted.starts_with("0X") || formatted.starts_with("0b") || formatted.starts_with("0o") {
                let prefix: String = formatted.chars().take(2).collect();
                let rest: String = formatted.chars().skip(2).collect();
                formatted = format!("{}{}{}", prefix, "0".repeat(padding), rest);
            } else {
                formatted = format!("{}{}", "0".repeat(padding), formatted);
            }
        }
    }

    // Apply alignment if width > length
    let final_align = align.unwrap_or('<');
    let count = formatted.chars().count();
    if width > count {
        let pad_len = width - count;
        match final_align {
            '<' => {
                for _ in 0..pad_len {
                    formatted.push(fill);
                }
            }
            '>' => {
                let mut padded = String::with_capacity(width);
                for _ in 0..pad_len {
                    padded.push(fill);
                }
                padded.push_str(&formatted);
                formatted = padded;
            }
            '^' => {
                let left = pad_len / 2;
                let right = pad_len - left;
                let mut padded = String::with_capacity(width);
                for _ in 0..left {
                    padded.push(fill);
                }
                padded.push_str(&formatted);
                for _ in 0..right {
                    padded.push(fill);
                }
                formatted = padded;
            }
            _ => {}
        }
    }

    formatted
}

fn format_int_with_grouping(mut n: i64, sep: char) -> String {
    let negative = n < 0;
    if negative {
        n = -n;
    }
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            result.push(sep);
        }
        result.push(c);
    }
    if negative {
        format!("-{}", result)
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_value_default() {
        assert_eq!(format_value_with_spec(&Value::Int(42), ""), "42");
        assert_eq!(format_value_with_spec(&Value::Float(3.14), ""), "3.14");
        assert_eq!(format_value_with_spec(&Value::Bool(true), ""), "true");
        assert_eq!(format_value_with_spec(&Value::String("hello".into()), ""), "hello");
    }

    #[test]
    fn test_format_value_float_precision() {
        assert_eq!(format_value_with_spec(&Value::Float(3.14159), ".2f"), "3.14");
        assert_eq!(format_value_with_spec(&Value::Float(3.14159), ".0f"), "3");
        assert_eq!(format_value_with_spec(&Value::Int(42), ".2f"), "42.00");
    }

    #[test]
    fn test_format_value_int_bases() {
        assert_eq!(format_value_with_spec(&Value::Int(255), "x"), "ff");
        assert_eq!(format_value_with_spec(&Value::Int(255), "X"), "FF");
        assert_eq!(format_value_with_spec(&Value::Int(255), "#x"), "0xff");
        assert_eq!(format_value_with_spec(&Value::Int(5), "b"), "101");
        assert_eq!(format_value_with_spec(&Value::Int(5), "#b"), "0b101");
        assert_eq!(format_value_with_spec(&Value::Int(8), "o"), "10");
        assert_eq!(format_value_with_spec(&Value::Int(8), "#o"), "0o10");
    }

    #[test]
    fn test_format_value_padding_and_alignment() {
        assert_eq!(format_value_with_spec(&Value::Int(42), "05d"), "00042");
        assert_eq!(format_value_with_spec(&Value::Int(-42), "05d"), "-0042");
        assert_eq!(format_value_with_spec(&Value::String("hi".into()), ">5"), "   hi");
        assert_eq!(format_value_with_spec(&Value::String("hi".into()), "<5"), "hi   ");
        assert_eq!(format_value_with_spec(&Value::String("hi".into()), "^5"), " hi  ");
    }

    #[test]
    fn test_format_value_grouping_and_percent() {
        assert_eq!(format_value_with_spec(&Value::Int(1000000), ","), "1,000,000");
        assert_eq!(format_value_with_spec(&Value::Int(1000000), "_"), "1_000_000");
        assert_eq!(format_value_with_spec(&Value::Float(0.756), ".1%"), "75.6%");
    }

    #[test]
    fn test_format_value_char() {
        assert_eq!(format_value_with_spec(&Value::Int(65), "c"), "A");
        assert_eq!(format_value_with_spec(&Value::Int(97), "c"), "a");
        assert_eq!(format_value_with_spec(&Value::Int(0x1F600), "c"), "😀");
        assert_eq!(format_value_with_spec(&Value::String("xyz".into()), "c"), "x");
        assert_eq!(format_value_with_spec(&Value::Int(65), ">3c"), "  A");
    }
}

