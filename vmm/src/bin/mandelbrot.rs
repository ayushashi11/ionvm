//! Interactive Mandelbrot Fractal Explorer powered by IonVM and Chafa FFI

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use vmm::lang::types::Value;

fn color_map(iter: u32, max_iters: u32) -> (u8, u8, u8) {
    if iter >= max_iters {
        return (0, 0, 0); // Inside the Mandelbrot set
    }

    let t = iter as f64 / max_iters as f64;
    // Ultra Fractal smooth aesthetic cycling palette
    let r = ((9.0 * (1.0 - t) * t * t * t * 255.0) as u32).min(255) as u8;
    let g = ((15.0 * (1.0 - t) * (1.0 - t) * t * t * 255.0) as u32).min(255) as u8;
    let b = ((8.5 * (1.0 - t) * (1.0 - t) * (1.0 - t) * t * 255.0) as u32).min(255) as u8;

    // Enhance contrast and brightness
    let r_boost = r.saturating_add((t * 50.0) as u8);
    let g_boost = g.saturating_add((t * 120.0) as u8);
    let b_boost = b.saturating_add((t * 220.0) as u8);

    (r_boost, g_boost, b_boost)
}

fn compute_mandelbrot(
    center_x: f64,
    center_y: f64,
    zoom: f64,
    max_iters: u32,
    width: usize,
    height: usize,
) -> Vec<Value> {
    let span_x = 3.5 / zoom;
    let span_y = 2.0 / zoom;
    let x_min = center_x - span_x * 0.5;
    let y_min = center_y - span_y * 0.5;

    let mut rgb_values = Vec::with_capacity(width * height * 3);

    for py in 0..height {
        let ci = y_min + (py as f64 / height as f64) * span_y;
        for px in 0..width {
            let cr = x_min + (px as f64 / width as f64) * span_x;

            let mut zr = 0.0f64;
            let mut zi = 0.0f64;
            let mut iter = 0u32;

            // Mandelbrot iteration loop: z = z^2 + c
            while zr * zr + zi * zi <= 4.0 && iter < max_iters {
                let next_zr = zr * zr - zi * zi + cr;
                let next_zi = 2.0 * zr * zi + ci;
                zr = next_zr;
                zi = next_zi;
                iter += 1;
            }

            let (r, g, b) = color_map(iter, max_iters);
            rgb_values.push(Value::Int(r as i64));
            rgb_values.push(Value::Int(g as i64));
            rgb_values.push(Value::Int(b as i64));
        }
    }

    rgb_values
}

fn main() {
    println!("============================================================");
    println!("     IonVM Interactive Mandelbrot Explorer with Chafa");
    println!("============================================================");

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc_handler(r);

    let mut center_x = -0.75f64;
    let mut center_y = 0.0f64;
    let mut zoom = 1.0f64;
    let mut max_iters = 80u32;

    // Default terminal rendering resolution
    let term_cols = 90;
    let term_rows = 35;
    let img_width = 180;
    let img_height = 140;

    let interactive = !std::env::args().any(|arg| arg == "--non-interactive");

    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        let t0 = Instant::now();
        let rgb_data = compute_mandelbrot(center_x, center_y, zoom, max_iters, img_width, img_height);
        let compute_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // Call Chafa FFI to render to terminal
        let args = vec![
            Value::Array(std::sync::Arc::new(std::sync::RwLock::new(rgb_data))),
            Value::Int(img_width as i64),
            Value::Int(img_height as i64),
            Value::Int(term_cols),
            Value::Int(term_rows),
        ];

        let t_render = Instant::now();
        let _ = vmm::builtin::call_native("ChafaPrint", &args);
        let render_ms = t_render.elapsed().as_secs_f64() * 1000.0;

        println!(
            "Center: ({:.6}, {:.6}) | Zoom: {:.2}x | Iters: {} | Compute: {:.2}ms | Render: {:.2}ms",
            center_x, center_y, zoom, max_iters, compute_ms, render_ms
        );

        if !interactive {
            break;
        }

        println!("Controls: [w/a/s/d: Pan | z/x or +/-: Zoom | i/k: Iters | r: Reset | q: Quit]");

        // Call ReadInput FFI
        let prompt_arg = vec![Value::String("mandelbrot> ".to_string())];
        let input_val = vmm::builtin::call_native("ReadInput", &prompt_arg)
            .unwrap_or(Value::String("q".to_string()));

        let cmd = match input_val {
            Value::String(s) => s.trim().to_lowercase(),
            _ => "q".to_string(),
        };

        if cmd.is_empty() {
            continue;
        }

        let pan_step = 0.25 / zoom;
        match cmd.as_str() {
            "w" => center_y -= pan_step,
            "s" => center_y += pan_step,
            "a" => center_x -= pan_step,
            "d" => center_x += pan_step,
            "z" | "+" | "=" => zoom *= 2.0,
            "x" | "-" => zoom = (zoom / 2.0).max(0.1),
            "i" => max_iters = (max_iters + 40).min(1000),
            "k" => max_iters = max_iters.saturating_sub(40).max(20),
            "r" => {
                center_x = -0.75;
                center_y = 0.0;
                zoom = 1.0;
                max_iters = 80;
            }
            "q" | "exit" => break,
            _ => {
                println!("Unknown command: '{}'", cmd);
            }
        }
    }

    println!("\nGoodbye!");
}

fn ctrlc_handler(running: Arc<AtomicBool>) {
    let _ = unsafe {
        libc::signal(
            libc::SIGINT,
            handle_sigint as *const () as libc::sighandler_t,
        )
    };
    GLOBAL_RUNNING.store(running.as_ref() as *const AtomicBool as usize, Ordering::SeqCst);
}

static GLOBAL_RUNNING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

extern "C" fn handle_sigint(_: libc::c_int) {
    let ptr = GLOBAL_RUNNING.load(Ordering::SeqCst) as *const AtomicBool;
    if !ptr.is_null() {
        unsafe { (*ptr).store(false, Ordering::SeqCst) };
    }
}
