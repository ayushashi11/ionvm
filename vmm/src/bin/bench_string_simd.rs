use std::time::Instant;
use vmm::bytecode::instruction::{Bytecode, Instruction, OperandType};
use vmm::bytecode::opcode::Opcode;
use vmm::lang::types::{Function, FunctionType, Value};
use vmm::vm::IonVM;
use vmm::vm::simd::{
    simd_str_count_byte, simd_str_eq, simd_str_find, simd_str_to_lower, simd_str_to_lower_inplace,
    simd_str_to_upper, simd_str_to_upper_inplace,
};

fn generate_sample_text(size_bytes: usize) -> String {
    let pattern = "The Quick Brown Fox Jumps Over The Lazy Dog! 0123456789. IonVM High Performance Actor Virtual Machine with SIMD.\n";
    let repeats = size_bytes / pattern.len() + 1;
    let mut s = pattern.repeat(repeats);
    s.truncate(size_bytes);
    s
}

fn main() {
    let text_size = 10 * 1024 * 1024; // 10 MB
    let s = generate_sample_text(text_size);
    let size_mb = text_size as f64 / (1024.0 * 1024.0);

    println!("============================================================");
    println!("     IonVM String SIMD (AVX2 256-bit) Benchmark");
    println!("     Buffer Size: {:.2} MB ({} bytes)", size_mb, text_size);
    println!("============================================================\n");

    // 1. To Uppercase
    println!("--- 1. To Uppercase (10 iterations) ---");
    let iters = 10;
    let t0 = Instant::now();
    let mut scalar_upper = String::new();
    for _ in 0..iters {
        scalar_upper = s.to_ascii_uppercase();
    }
    let scalar_upper_time = t0.elapsed().as_secs_f64() / iters as f64;
    let scalar_upper_gbps = (size_mb / 1024.0) / scalar_upper_time;

    let t0 = Instant::now();
    let mut simd_upper = String::new();
    for _ in 0..iters {
        simd_upper = simd_str_to_upper(&s);
    }
    let simd_upper_time = t0.elapsed().as_secs_f64() / iters as f64;
    let simd_upper_gbps = (size_mb / 1024.0) / simd_upper_time;

    assert_eq!(scalar_upper, simd_upper, "Upper output must match!");
    println!("  Scalar to_ascii_uppercase : {:>8.4} ms ({:>6.2} GB/s)", scalar_upper_time * 1000.0, scalar_upper_gbps);
    println!("  IonVM AVX2 simd_to_upper  : {:>8.4} ms ({:>6.2} GB/s)  [Speedup: {:.2}x]", simd_upper_time * 1000.0, simd_upper_gbps, scalar_upper_time / simd_upper_time);

    let mut in_place_buf = s.clone().into_bytes();
    let t0 = Instant::now();
    for _ in 0..iters {
        simd_str_to_upper_inplace(&mut in_place_buf);
    }
    let in_place_upper_time = t0.elapsed().as_secs_f64() / iters as f64;
    let in_place_upper_gbps = (size_mb / 1024.0) / in_place_upper_time;
    println!("  IonVM SIMD in-place (no-alloc): {:>8.4} ms ({:>6.2} GB/s)  [Speedup vs scalar: {:.2}x]", in_place_upper_time * 1000.0, in_place_upper_gbps, scalar_upper_time / in_place_upper_time);

    // 2. To Lowercase
    println!("\n--- 2. To Lowercase (10 iterations) ---");
    let t0 = Instant::now();
    let mut scalar_lower = String::new();
    for _ in 0..iters {
        scalar_lower = s.to_ascii_lowercase();
    }
    let scalar_lower_time = t0.elapsed().as_secs_f64() / iters as f64;
    let scalar_lower_gbps = (size_mb / 1024.0) / scalar_lower_time;

    let t0 = Instant::now();
    let mut simd_lower = String::new();
    for _ in 0..iters {
        simd_lower = simd_str_to_lower(&s);
    }
    let simd_lower_time = t0.elapsed().as_secs_f64() / iters as f64;
    let simd_lower_gbps = (size_mb / 1024.0) / simd_lower_time;

    assert_eq!(scalar_lower, simd_lower, "Lower output must match!");
    println!("  Scalar to_ascii_lowercase : {:>8.4} ms ({:>6.2} GB/s)", scalar_lower_time * 1000.0, scalar_lower_gbps);
    println!("  IonVM AVX2 simd_to_lower  : {:>8.4} ms ({:>6.2} GB/s)  [Speedup: {:.2}x]", simd_lower_time * 1000.0, simd_lower_gbps, scalar_lower_time / simd_lower_time);

    let mut in_place_buf = s.clone().into_bytes();
    let t0 = Instant::now();
    for _ in 0..iters {
        simd_str_to_lower_inplace(&mut in_place_buf);
    }
    let in_place_lower_time = t0.elapsed().as_secs_f64() / iters as f64;
    let in_place_lower_gbps = (size_mb / 1024.0) / in_place_lower_time;
    println!("  IonVM SIMD in-place (no-alloc): {:>8.4} ms ({:>6.2} GB/s)  [Speedup vs scalar: {:.2}x]", in_place_lower_time * 1000.0, in_place_lower_gbps, scalar_lower_time / in_place_lower_time);

    // 3. Byte Occurrence Counting
    println!("\n--- 3. Count Byte 'e' (50 iterations) ---");
    let count_iters = 50;
    let t0 = Instant::now();
    let mut scalar_cnt = 0;
    for _ in 0..count_iters {
        scalar_cnt = s.as_bytes().iter().filter(|&&b| b == b'e').count();
    }
    let scalar_cnt_time = t0.elapsed().as_secs_f64() / count_iters as f64;
    let scalar_cnt_gbps = (size_mb / 1024.0) / scalar_cnt_time;

    let t0 = Instant::now();
    let mut simd_cnt = 0;
    for _ in 0..count_iters {
        simd_cnt = simd_str_count_byte(&s, b'e');
    }
    let simd_cnt_time = t0.elapsed().as_secs_f64() / count_iters as f64;
    let simd_cnt_gbps = (size_mb / 1024.0) / simd_cnt_time;

    assert_eq!(scalar_cnt, simd_cnt, "Counts must match!");
    println!("  Scalar byte count (iter)  : {:>8.4} ms ({:>6.2} GB/s) (count = {})", scalar_cnt_time * 1000.0, scalar_cnt_gbps, scalar_cnt);
    println!("  IonVM AVX2 simd_count_byte: {:>8.4} ms ({:>6.2} GB/s) (count = {}) [Speedup: {:.2}x]", simd_cnt_time * 1000.0, simd_cnt_gbps, simd_cnt, scalar_cnt_time / simd_cnt_time);

    // 4. String Equality
    println!("\n--- 4. String Equality (Identical 10MB strings, 50 iterations) ---");
    let s2 = s.clone();
    let t0 = Instant::now();
    let mut scalar_eq = false;
    for _ in 0..count_iters {
        scalar_eq = s == s2;
    }
    let scalar_eq_time = t0.elapsed().as_secs_f64() / count_iters as f64;
    let scalar_eq_gbps = (size_mb / 1024.0) / scalar_eq_time;

    let t0 = Instant::now();
    let mut simd_eq_res = false;
    for _ in 0..count_iters {
        simd_eq_res = simd_str_eq(&s, &s2);
    }
    let simd_eq_time = t0.elapsed().as_secs_f64() / count_iters as f64;
    let simd_eq_gbps = (size_mb / 1024.0) / simd_eq_time;

    assert_eq!(scalar_eq, simd_eq_res);
    println!("  Scalar String eq (==)     : {:>8.4} ms ({:>6.2} GB/s)", scalar_eq_time * 1000.0, scalar_eq_gbps);
    println!("  IonVM AVX2 simd_str_eq    : {:>8.4} ms ({:>6.2} GB/s) [Speedup: {:.2}x]", simd_eq_time * 1000.0, simd_eq_gbps, scalar_eq_time / simd_eq_time);

    // 5. String Substring Search
    println!("\n--- 5. Substring Search (needle near end of 10MB buffer, 50 iterations) ---");
    let mut needle_target = s.clone();
    // Place needle at byte 9,500,000
    let target_offset = 9_500_000.min(text_size - 100);
    unsafe {
        needle_target.as_bytes_mut()[target_offset..target_offset + 14].copy_from_slice(b"UNIQUE_TARGET!");
    }
    let needle = "UNIQUE_TARGET!";

    let t0 = Instant::now();
    let mut scalar_pos = None;
    for _ in 0..count_iters {
        scalar_pos = needle_target.find(needle);
    }
    let scalar_find_time = t0.elapsed().as_secs_f64() / count_iters as f64;
    let scalar_find_gbps = ((target_offset as f64 / (1024.0 * 1024.0)) / 1024.0) / scalar_find_time;

    let t0 = Instant::now();
    let mut simd_pos = None;
    for _ in 0..count_iters {
        simd_pos = simd_str_find(&needle_target, needle);
    }
    let simd_find_time = t0.elapsed().as_secs_f64() / count_iters as f64;
    let simd_find_gbps = ((target_offset as f64 / (1024.0 * 1024.0)) / 1024.0) / simd_find_time;

    assert_eq!(scalar_pos, simd_pos);
    println!("  Scalar string find()      : {:>8.4} ms ({:>6.2} GB/s) (pos = {:?})", scalar_find_time * 1000.0, scalar_find_gbps, scalar_pos);
    println!("  IonVM AVX2 simd_str_find  : {:>8.4} ms ({:>6.2} GB/s) (pos = {:?}) [Speedup: {:.2}x]", simd_find_time * 1000.0, simd_find_gbps, simd_pos, scalar_find_time / simd_find_time);

    // 6. In-VM Bytecode Execution
    println!("\n--- 6. IonVM Bytecode Dispatch Pipeline ---");
    let instrs = vec![
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::String(s.clone())), OperandType::Register(0)]),
        Instruction::new(Opcode::SUpperRR, vec![OperandType::Register(0), OperandType::Register(1)]),
        Instruction::new(Opcode::SLowerRR, vec![OperandType::Register(1), OperandType::Register(2)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::String("e".to_string())), OperandType::Register(3)]),
        Instruction::new(Opcode::SCountByteRRR, vec![OperandType::Register(0), OperandType::Register(3), OperandType::Register(4)]),
        Instruction::new(Opcode::SEqRRR, vec![OperandType::Register(0), OperandType::Register(2), OperandType::Register(5)]),
        Instruction::new(Opcode::Ret, vec![OperandType::Register(4)]),
    ];
    let main_fn = Function::new("main".to_string(), instrs, 10, 0, FunctionType::Regular);
    let bc = Bytecode {
        functions: vec![main_fn],
        entry_point: "main".to_string(),
    };

    let t0 = Instant::now();
    let mut vm = IonVM::new(bc).expect("init vm");
    let res = vm.run();
    let vm_time = t0.elapsed().as_secs_f64();
    println!("  End-to-End IonVM Bytecode: {:>8.4} ms", vm_time * 1000.0);
    println!("  Ops Executed: LoadConst(10MB) -> SUpperRR -> SLowerRR -> SCountByteRRR -> SEqRRR");
    println!("  VM Return Result: {:?}", res);
    println!("\n============================================================");
}
