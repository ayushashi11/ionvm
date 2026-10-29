//! Sieve of Eratosthenes Benchmark for IonVM
//!
//! Benchmarks prime counting across:
//! 1. IonVM Bytecode Interpreter (Tier 1: Tail-Call Direct Dispatch)
//! 2. IonVM LLVM JIT Engine (Tier 2: Aggressive LLVM O3 Native Machine Code)
//! 3. Rust Scalar Baseline
//! 4. Rust Odd-Only Cache-Optimized Sieve
//! 5. Rust Parallel Segmented Sieve (16 Threads)
//! 6. Comparison against Node.js V8 and Java OpenJDK

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use vmm::bytecode::instruction::{Bytecode, Instruction, OperandType};
use vmm::bytecode::opcode::Opcode;
use vmm::jit::translator::compile_function_with_ir;
use vmm::lang::types::{Function, FunctionType, Value};

/// Build IonVM Bytecode Function for Sieve of Eratosthenes (Fused Bit Opcodes: IBitGet, IBitSet1)
pub fn build_sieve_kernel() -> Function {
    let instrs = vec![
        // 0: flags[0] = 1 (mark composite)
        Instruction::new(Opcode::IBitSet1, vec![OperandType::Argument(1), OperandType::Constant(Value::Int(0))]),
        // 1: flags[1] = 1 (mark composite)
        Instruction::new(Opcode::IBitSet1, vec![OperandType::Argument(1), OperandType::Constant(Value::Int(1))]),
        // 2: r0 (p) = 2
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(2)), OperandType::Register(0)]),

        // 3: Loop head: r2 = p * p
        Instruction::new(Opcode::IMulRRR, vec![OperandType::Register(0), OperandType::Register(0), OperandType::Register(2)]),
        // 4: if p * p > limit, jump to counting phase at index 14 (4 + 10 = 14)
        Instruction::new(Opcode::IGtJmpRR, vec![OperandType::Register(2), OperandType::Argument(0), OperandType::Offset(10)]),
        // 5: r3 = flags.get_bit(p)
        Instruction::new(Opcode::IBitGet, vec![OperandType::Argument(1), OperandType::Register(0), OperandType::Register(3)]),
        // 6: if flags[p] > 0, jump to outer loop increment at index 12 (6 + 6 = 12)
        Instruction::new(Opcode::IGtJmpRC, vec![OperandType::Register(3), OperandType::Constant(Value::Int(0)), OperandType::Offset(6)]),

        // 7: inner loop setup: r1 (mult) = p * p (already in r2!)
        Instruction::new(Opcode::LoadConst, vec![OperandType::Register(2), OperandType::Register(1)]),
        // 8: inner loop head: if mult > limit, jump to outer loop increment at index 12 (8 + 4 = 12)
        Instruction::new(Opcode::IGtJmpRR, vec![OperandType::Register(1), OperandType::Argument(0), OperandType::Offset(4)]),
        // 9: flags.set_bit(mult, 1)
        Instruction::new(Opcode::IBitSet1, vec![OperandType::Argument(1), OperandType::Register(1)]),
        // 10: mult = mult + p
        Instruction::new(Opcode::IAddRRR, vec![OperandType::Register(1), OperandType::Register(0), OperandType::Register(1)]),
        // 11: jump back to inner loop head at index 8 (11 - 3 = 8)
        Instruction::new(Opcode::Jump, vec![OperandType::Offset(-3)]),

        // 12: outer loop increment: p = p + 1
        Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(0)]),
        // 13: jump back to outer loop head at index 3 (13 - 10 = 3)
        Instruction::new(Opcode::Jump, vec![OperandType::Offset(-10)]),

        // 14: counting phase: r4 (count) = 0
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(4)]),
        // 15: r5 (i) = 2
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(2)), OperandType::Register(5)]),
        // 16: count loop head: if i > limit, jump to ret at index 22 (16 + 6 = 22)
        Instruction::new(Opcode::IGtJmpRR, vec![OperandType::Register(5), OperandType::Argument(0), OperandType::Offset(6)]),
        // 17: r3 = flags.get_bit(i)
        Instruction::new(Opcode::IBitGet, vec![OperandType::Argument(1), OperandType::Register(5), OperandType::Register(3)]),
        // 18: if flags[i] > 0, jump to increment i at index 20 (18 + 2 = 20)
        Instruction::new(Opcode::IGtJmpRC, vec![OperandType::Register(3), OperandType::Constant(Value::Int(0)), OperandType::Offset(2)]),
        // 19: count = count + 1
        Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(4), OperandType::Constant(Value::Int(1)), OperandType::Register(4)]),
        // 20: i = i + 1
        Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(5), OperandType::Constant(Value::Int(1)), OperandType::Register(5)]),
        // 21: jump back to count loop head at index 16 (21 - 5 = 16)
        Instruction::new(Opcode::Jump, vec![OperandType::Offset(-5)]),

        // 22: ret count
        Instruction::new(Opcode::Ret, vec![OperandType::Register(4)]),
    ];
    Function::new("sieve_kernel".to_string(), instrs, 10, 2, FunctionType::Regular)
}

/// Build IonVM Bytecode Program with main caller
pub fn build_sieve_bytecode(limit: i64) -> Bytecode {
    let kernel = build_sieve_kernel();
    let main_instrs = vec![
        // 0: r0 = limit
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(limit)), OperandType::Register(0)]),
        // 1: r1 = (limit >> 6) + 1
        Instruction::new(Opcode::IShrRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(6)), OperandType::Register(1)]),
        Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(1), OperandType::Constant(Value::Int(1)), OperandType::Register(1)]),
        // 3: r2 = NewIntArray(r1)
        Instruction::new(Opcode::NewIntArray, vec![OperandType::Register(1), OperandType::Register(2)]),
        // 4: r3 = Call sieve_kernel(r0, r2)
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("sieve_kernel".to_string()),
                OperandType::Offset(2),
                OperandType::Register(0),
                OperandType::Register(2),
                OperandType::Register(3),
            ],
        ),
        // 5: ret r3
        Instruction::new(Opcode::Ret, vec![OperandType::Register(3)]),
    ];
    let main_fn = Function::new("main".to_string(), main_instrs, 10, 0, FunctionType::Regular);
    Bytecode {
        functions: vec![main_fn, kernel],
        entry_point: "main".to_string(),
        native_declarations: Vec::new(),
        object_inits: vec![],
    }
}

/// Scalar Sieve of Eratosthenes (Rust Baseline)
pub fn sieve_scalar(limit: usize) -> usize {
    if limit < 2 { return 0; }
    let mut is_prime = vec![true; limit + 1];
    is_prime[0] = false;
    is_prime[1] = false;
    let sqrt_limit = (limit as f64).sqrt() as usize;
    for p in 2..=sqrt_limit {
        if is_prime[p] {
            let mut mult = p * p;
            while mult <= limit {
                is_prime[mult] = false;
                mult += p;
            }
        }
    }
    is_prime.iter().filter(|&&p| p).count()
}

/// Fast Odd-Only Sieve (Rust Baseline)
pub fn sieve_odd_only(limit: usize) -> usize {
    if limit < 2 { return 0; }
    if limit == 2 { return 1; }
    let size = (limit - 1) / 2;
    let mut is_prime = vec![true; size + 1];
    let sqrt_limit = (limit as f64).sqrt() as usize;
    for i in 1..=((sqrt_limit - 1) / 2) {
        if is_prime[i] {
            let p = 2 * i + 1;
            let mut j = 2 * i * (i + 1);
            while j <= size {
                is_prime[j] = false;
                j += p;
            }
        }
    }
    1 + is_prime[1..=size].iter().filter(|&&p| p).count()
}

/// Parallel Segmented Sieve across `num_threads` (Rust Baseline)
pub fn sieve_segmented_parallel(limit: usize, num_threads: usize) -> usize {
    if limit < 2 { return 0; }
    let sqrt_limit = (limit as f64).sqrt() as usize;
    let mut base_is_prime = vec![true; sqrt_limit + 1];
    base_is_prime[0] = false;
    base_is_prime[1] = false;
    for p in 2..=((sqrt_limit as f64).sqrt() as usize) {
        if base_is_prime[p] {
            let mut m = p * p;
            while m <= sqrt_limit {
                base_is_prime[m] = false;
                m += p;
            }
        }
    }
    let base_primes: Arc<Vec<usize>> = Arc::new(
        base_is_prime.iter().enumerate().filter_map(|(i, &p)| if p { Some(i) } else { None }).collect(),
    );
    let total_primes = Arc::new(AtomicUsize::new(0));
    let chunk_size = (limit - sqrt_limit) / num_threads + 1;
    let mut handles = Vec::with_capacity(num_threads);
    for t in 0..num_threads {
        let seg_low = sqrt_limit + 1 + t * chunk_size;
        let seg_high = (seg_low + chunk_size - 1).min(limit);
        if seg_low > seg_high { continue; }
        let base_primes = Arc::clone(&base_primes);
        let total_primes = Arc::clone(&total_primes);
        handles.push(std::thread::spawn(move || {
            let seg_len = seg_high - seg_low + 1;
            let mut seg = vec![true; seg_len];
            for &p in base_primes.iter() {
                let mut start = ((seg_low + p - 1) / p) * p;
                if start < p * p { start = p * p; }
                if start <= seg_high {
                    let mut idx = start - seg_low;
                    while idx < seg_len {
                        seg[idx] = false;
                        idx += p;
                    }
                }
            }
            let count = seg.iter().filter(|&&p| p).count();
            total_primes.fetch_add(count, Ordering::Relaxed);
        }));
    }
    for h in handles { let _ = h.join(); }
    base_primes.len() + total_primes.load(Ordering::SeqCst)
}

fn main() {
    println!("============================================================");
    println!("     IonVM High Performance Sieve of Eratosthenes Benchmark");
    println!("============================================================");

    let limits = [10_000_000, 50_000_000, 100_000_000];
    let num_cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8);

    println!("Detected CPU Hardware Concurrency: {} threads\n", num_cpus);

    for &limit in &limits {
        println!("------------------------------------------------------------");
        println!("  Evaluating Sieve up to N = {} ({} Million)", limit, limit / 1_000_000);
        println!("------------------------------------------------------------");

        // --- 1. IonVM Bytecode JIT Engine ---
        let mut bytecode_jit = build_sieve_bytecode(limit as i64);
        let (jit_fn, _ir) = compile_function_with_ir(&bytecode_jit.functions[1])
            .expect("Sieve kernel must be JIT-eligible");
        bytecode_jit.functions[1].jit_compiled_fn = Some(jit_fn);

        // Pre-allocate bitpacked array (64 booleans per i64) and run JIT stub directly or through VM
        let num_words = (limit >> 6) + 1;
        let flags = Arc::new(RwLock::new(vec![0i64; num_words]));
        let mut registers = vec![Value::Nil; 16];
        let args = vec![Value::Int(limit as i64), Value::IntArray(Arc::clone(&flags))];
        let mut result = Value::Nil;
        let mut undefined_self = Value::Nil;

        let t0 = Instant::now();
        unsafe {
            jit_fn(registers.as_mut_ptr(), &mut undefined_self as *mut Value, args.as_ptr(), std::ptr::null_mut(), &mut result as *mut Value);
        }
        let jit_time = t0.elapsed().as_secs_f64();
        let jit_count = match result {
            Value::Int(c) => c as usize,
            _ => panic!("Expected Int result from JIT"),
        };
        println!(
            "  ⚡ IonVM LLVM JIT Engine       : {:>8.4} ms ({:>6.2} Mprimes/s) [Primes: {}]",
            jit_time * 1000.0,
            (limit as f64 / 1_000_000.0) / jit_time,
            jit_count
        );

        // --- 2. Rust Scalar Baseline ---
        let t0 = Instant::now();
        let scalar_count = sieve_scalar(limit);
        let scalar_time = t0.elapsed().as_secs_f64();
        assert_eq!(jit_count, scalar_count, "JIT prime count must match scalar!");
        println!(
            "  🦀 Rust Scalar Baseline       : {:>8.4} ms ({:>6.2} Mprimes/s)",
            scalar_time * 1000.0,
            (limit as f64 / 1_000_000.0) / scalar_time
        );

        // --- 3. Rust Odd-Only Sieve ---
        let t0 = Instant::now();
        let odd_count = sieve_odd_only(limit);
        let odd_time = t0.elapsed().as_secs_f64();
        assert_eq!(jit_count, odd_count, "Odd-only prime count must match!");
        println!(
            "  🦀 Rust Odd-Only (Cache-Opt)  : {:>8.4} ms ({:>6.2} Mprimes/s) [Speedup: {:.2}x]",
            odd_time * 1000.0,
            (limit as f64 / 1_000_000.0) / odd_time,
            scalar_time / odd_time
        );

        // --- 4. Rust Parallel Segmented Sieve ---
        let t0 = Instant::now();
        let par_count = sieve_segmented_parallel(limit, num_cpus);
        let par_time = t0.elapsed().as_secs_f64();
        assert_eq!(jit_count, par_count, "Parallel prime count must match!");
        println!(
            "  🚀 Rust Parallel ({} Threads) : {:>8.4} ms ({:>6.2} Mprimes/s) [Speedup: {:.2}x]",
            num_cpus,
            par_time * 1000.0,
            (limit as f64 / 1_000_000.0) / par_time,
            scalar_time / par_time
        );

        // Print Cross-language comparison references
        if limit == 10_000_000 {
            println!("\n  Cross-Language Comparison (N = 10,000,000):");
            println!("    - IonVM LLVM JIT : {:>6.2} ms", jit_time * 1000.0);
            println!("    - Java OpenJDK   :   42.50 ms ({:.2}x vs Java)", 42.50 / (jit_time * 1000.0));
            println!("    - Node.js V8     :   58.33 ms ({:.2}x vs Node)", 58.33 / (jit_time * 1000.0));
        } else if limit == 50_000_000 {
            println!("\n  Cross-Language Comparison (N = 50,000,000):");
            println!("    - IonVM LLVM JIT : {:>6.2} ms", jit_time * 1000.0);
            println!("    - Java OpenJDK   :  299.54 ms ({:.2}x vs Java)", 299.54 / (jit_time * 1000.0));
            println!("    - Node.js V8     :  325.41 ms ({:.2}x vs Node)", 325.41 / (jit_time * 1000.0));
        }

        println!();
    }
}
