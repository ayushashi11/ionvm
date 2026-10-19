//! Terminal graphics rendering using generic image display protocols (Kitty, Sixel, iTerm2, Truecolor) and interactive input

use image::{DynamicImage, RgbImage};
use std::io::{self, Write};
use std::process::Command;

use crate::{FfiError, FfiFunction, FfiRegistry, FfiResult, FfiValue};

fn extract_raw_bytes(arg: &FfiValue, expected_len: usize) -> Result<Vec<u8>, FfiError> {
    match arg {
        FfiValue::String(s) => Ok(s.as_bytes().to_vec()),
        FfiValue::Array(arr) => {
            let mut bytes = Vec::with_capacity(arr.len().max(expected_len));
            for val in arr {
                match val {
                    FfiValue::Integer(i) => bytes.push((*i).clamp(0, 255) as u8),
                    FfiValue::Float(f) => bytes.push((*f).clamp(0.0, 255.0) as u8),
                    FfiValue::Number(n) => bytes.push((*n).clamp(0.0, 255.0) as u8),
                    _ => bytes.push(0),
                }
            }
            Ok(bytes)
        }
        _ => Err(FfiError::ArgumentType {
            expected: "Array of Numbers or String".to_string(),
            got: arg.type_name().to_string(),
        }),
    }
}

pub struct ChafaRender;

impl FfiFunction for ChafaRender {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() < 5 {
            return Err(FfiError::ArgumentCount {
                expected: 5,
                got: args.len(),
            });
        }

        // args: [rgb_data, width, height, term_cols, term_rows]
        let width = match args[1].as_number() {
            Some(n) => n.max(1.0) as usize,
            None => {
                return Err(FfiError::ArgumentType {
                    expected: "Number".to_string(),
                    got: args[1].type_name().to_string(),
                })
            }
        };
        let height = match args[2].as_number() {
            Some(n) => n.max(1.0) as usize,
            None => {
                return Err(FfiError::ArgumentType {
                    expected: "Number".to_string(),
                    got: args[2].type_name().to_string(),
                })
            }
        };
        let term_cols = match args[3].as_number() {
            Some(n) if n > 0.0 => n as usize,
            _ => 80,
        };
        let term_rows = match args[4].as_number() {
            Some(n) if n > 0.0 => n as usize,
            _ => 30,
        };

        let raw_bytes = extract_raw_bytes(&args[0], width * height * 3)?;

        // If system chafa is available, pipe PNG or raw data with --probe=off for truecolor output
        if let Ok(mut child) = Command::new("chafa")
            .args(["--probe=off", "-c", "full", &format!("--size={}x{}", term_cols, term_rows), "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                // Encode to PNG buffer in-memory
                if let Some(rgb_img) = RgbImage::from_raw(width as u32, height as u32, raw_bytes.clone()) {
                    let mut png_buf = io::Cursor::new(Vec::new());
                    if rgb_img.write_to(&mut png_buf, image::ImageFormat::Png).is_ok() {
                        let _ = stdin.write_all(&png_buf.into_inner());
                    }
                }
            }
            if let Ok(output) = child.wait_with_output() {
                if !output.stdout.is_empty() {
                    return Ok(FfiValue::String(String::from_utf8_lossy(&output.stdout).to_string()));
                }
            }
        }

        // Truecolor half-block ANSI fallback
        let mut ansi = String::new();
        let scale_x = width as f64 / term_cols as f64;
        let scale_y = height as f64 / (term_rows * 2) as f64;
        for row in 0..term_rows {
            let y_top = ((row * 2) as f64 * scale_y) as usize;
            let y_bot = (((row * 2 + 1) as f64 * scale_y) as usize).min(height - 1);
            for col in 0..term_cols {
                let x = ((col as f64 * scale_x) as usize).min(width - 1);
                let top_idx = (y_top * width + x) * 3;
                let bot_idx = (y_bot * width + x) * 3;
                let (r1, g1, b1) = if top_idx + 2 < raw_bytes.len() {
                    (raw_bytes[top_idx], raw_bytes[top_idx + 1], raw_bytes[top_idx + 2])
                } else {
                    (0, 0, 0)
                };
                let (r2, g2, b2) = if bot_idx + 2 < raw_bytes.len() {
                    (raw_bytes[bot_idx], raw_bytes[bot_idx + 1], raw_bytes[bot_idx + 2])
                } else {
                    (0, 0, 0)
                };
                ansi.push_str(&format!("\x1b[38;2;{};{};{};48;2;{};{};{}m▀", r1, g1, b1, r2, g2, b2));
            }
            ansi.push_str("\x1b[0m\n");
        }

        Ok(FfiValue::String(ansi))
    }

    fn name(&self) -> &str {
        "ChafaRender"
    }

    fn arity(&self) -> usize {
        5
    }

    fn return_type(&self) -> crate::FfiType {
        crate::FfiType::Void
    }

    fn description(&self) -> Option<&str> {
        Some("Render RGB pixel buffer into terminal ANSI graphics (supports system chafa and truecolor)")
    }
}

pub struct ChafaPrint;

impl FfiFunction for ChafaPrint {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() < 5 {
            return Err(FfiError::ArgumentCount {
                expected: 5,
                got: args.len(),
            });
        }

        let width = match args[1].as_number() {
            Some(n) => n.max(1.0) as usize,
            None => return Err(FfiError::ArgumentType { expected: "Number".into(), got: args[1].type_name().into() }),
        };
        let height = match args[2].as_number() {
            Some(n) => n.max(1.0) as usize,
            None => return Err(FfiError::ArgumentType { expected: "Number".into(), got: args[2].type_name().into() }),
        };
        let term_cols = match args[3].as_number() {
            Some(n) if n > 0.0 => Some(n as u32),
            _ => None,
        };
        let term_rows = match args[4].as_number() {
            Some(n) if n > 0.0 => Some(n as u32),
            _ => None,
        };

        let raw_bytes = extract_raw_bytes(&args[0], width * height * 3)?;

        // Attempt 1: Print directly using viuer (supports Kitty graphics protocol, Sixel, iTerm2, half-blocks)
        if let Some(rgb_img) = RgbImage::from_raw(width as u32, height as u32, raw_bytes.clone()) {
            let dyn_img = DynamicImage::ImageRgb8(rgb_img);
            let conf = viuer::Config {
                width: term_cols,
                height: term_rows,
                truecolor: true,
                use_kitty: true,
                use_iterm: true,
                absolute_offset: false,
                restore_cursor: false,
                ..Default::default()
            };
            if viuer::print(&dyn_img, &conf).is_ok() {
                let _ = io::stdout().flush();
                return Ok(FfiValue::Unit);
            }
        }

        // Attempt 2: Fall back to ChafaRender ANSI
        let render_fn = ChafaRender;
        let res = render_fn.call(_ctx, args)?;
        if let FfiValue::String(ref s) = res {
            print!("{}", s);
            let _ = io::stdout().flush();
        }
        Ok(FfiValue::Unit)
    }

    fn name(&self) -> &str {
        "ChafaPrint"
    }

    fn arity(&self) -> usize {
        5
    }

    fn description(&self) -> Option<&str> {
        Some("Render and print RGB pixel buffer into terminal image using Kitty/Sixel/iTerm2/Chafa")
    }
}

pub struct DisplayImage;

impl FfiFunction for DisplayImage {
    fn call(&self, ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        let print_fn = ChafaPrint;
        print_fn.call(ctx, args)
    }

    fn name(&self) -> &str {
        "DisplayImage"
    }

    fn arity(&self) -> usize {
        5
    }

    fn description(&self) -> Option<&str> {
        Some("Display image on terminal via Kitty/iTerm2/Sixel/Truecolor protocol")
    }
}

pub struct ReadInput;

impl FfiFunction for ReadInput {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if !args.is_empty() {
            if let FfiValue::String(ref prompt) = args[0] {
                print!("{}", prompt);
                let _ = io::stdout().flush();
            }
        }
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => Ok(FfiValue::String(input.trim().to_lowercase())),
            Err(e) => Err(FfiError::RuntimeError(format!("stdin error: {}", e))),
        }
    }

    fn name(&self) -> &str {
        "ReadInput"
    }

    fn arity(&self) -> usize {
        0
    }

    fn is_variadic(&self) -> bool {
        true
    }

    fn return_type(&self) -> crate::FfiType {
        crate::FfiType::String
    }

    fn description(&self) -> Option<&str> {
        Some("Read a line of text from standard input with optional prompt")
    }
}

pub struct ReadKey;

impl FfiFunction for ReadKey {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if !args.is_empty() {
            if let FfiValue::String(ref prompt) = args[0] {
                print!("{}", prompt);
                let _ = io::stdout().flush();
            }
        }
        let term = console::Term::stdout();
        if term.is_term() {
            match term.read_char() {
                Ok(c) => Ok(FfiValue::String(c.to_lowercase().to_string())),
                Err(_) => {
                    let mut input = String::new();
                    let _ = io::stdin().read_line(&mut input);
                    Ok(FfiValue::String(input.trim().to_lowercase()))
                }
            }
        } else {
            let mut input = String::new();
            let _ = io::stdin().read_line(&mut input);
            Ok(FfiValue::String(input.trim().to_lowercase()))
        }
    }

    fn name(&self) -> &str {
        "ReadKey"
    }

    fn arity(&self) -> usize {
        0
    }

    fn is_variadic(&self) -> bool {
        true
    }

    fn return_type(&self) -> crate::FfiType {
        crate::FfiType::String
    }

    fn description(&self) -> Option<&str> {
        Some("Read a single keypress without waiting for Enter")
    }
}

pub fn register_chafa_functions(registry: &mut FfiRegistry) {
    registry.register(ChafaRender);
    registry.register(ChafaPrint);
    registry.register(DisplayImage);
    registry.register(ReadInput);
    registry.register(ReadKey);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockContext;
    impl crate::FfiContext for MockContext {
        fn get_this(&self) -> Option<FfiValue> {
            None
        }
    }

    #[test]
    fn test_chafa_render() {
        let mut registry = FfiRegistry::new();
        register_chafa_functions(&mut registry);
        let mut ctx = MockContext;

        let width = 4;
        let height = 4;
        let mut rgb = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgb.push(FfiValue::Number((x * 60) as f64)); // R
                rgb.push(FfiValue::Number((y * 60) as f64)); // G
                rgb.push(FfiValue::Number(200.0)); // B
            }
        }

        let res = registry
            .call(
                &mut ctx,
                "ChafaRender",
                vec![
                    FfiValue::Array(rgb),
                    FfiValue::Number(width as f64),
                    FfiValue::Number(height as f64),
                    FfiValue::Number(20.0),
                    FfiValue::Number(10.0),
                ],
            )
            .unwrap();

        match res {
            FfiValue::String(ansi) => {
                assert!(!ansi.is_empty(), "Chafa output must not be empty");
            }
            _ => panic!("Expected string ANSI output"),
        }
    }
}
