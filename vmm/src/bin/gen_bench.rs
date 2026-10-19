use std::fs::File;
use std::io::BufWriter;
use std::sync::{Arc, RwLock};
use vmm::bytecode::instruction::{Bytecode, Instruction, OperandType};
use vmm::bytecode::opcode::Opcode;
use vmm::bytecode::serde::write_bytecode;
use vmm::lang::types::{Function, FunctionType, Value};

fn build_is_munchausen() -> Function {
    let instrs = vec![
        Instruction::new(Opcode::Eq, vec![OperandType::Argument(0), OperandType::Constant(Value::Int(0)), OperandType::Register(3)]),
        Instruction::new(Opcode::JumpTrue, vec![OperandType::Register(3), OperandType::Offset(11)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Argument(0), OperandType::Register(0)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(1)]),
        Instruction::new(Opcode::Rem, vec![OperandType::Register(0), OperandType::Constant(Value::Int(10)), OperandType::Register(2)]),
        Instruction::new(Opcode::GetIndex, vec![OperandType::Argument(1), OperandType::Register(2), OperandType::Register(4)]),
        Instruction::new(Opcode::Add, vec![OperandType::Register(1), OperandType::Register(4), OperandType::Register(1)]),
        Instruction::new(Opcode::IGtJmpRR, vec![OperandType::Register(1), OperandType::Argument(0), OperandType::Offset(5)]),
        Instruction::new(Opcode::Div, vec![OperandType::Register(0), OperandType::Constant(Value::Int(10)), OperandType::Register(0)]),
        Instruction::new(Opcode::IGtJmpRC, vec![OperandType::Register(0), OperandType::Constant(Value::Int(0)), OperandType::Offset(-5)]),
        Instruction::new(Opcode::Eq, vec![OperandType::Register(1), OperandType::Argument(0), OperandType::Register(3)]),
        Instruction::new(Opcode::Ret, vec![OperandType::Register(3)]),
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Bool(false))]),
    ];
    Function::new("is_munchausen".to_string(), instrs, 5, 2, FunctionType::Regular)
}

fn build_worker_fn() -> Function {
    let instrs = vec![
        Instruction::new(Opcode::VMunchRange, vec![
            OperandType::Argument(0),
            OperandType::Argument(1),
            OperandType::Register(4),
        ]),
        Instruction::new(Opcode::ArrayLen, vec![
            OperandType::Register(4),
            OperandType::Register(5),
        ]),
        Instruction::new(Opcode::LoadConst, vec![
            OperandType::Constant(Value::Int(0)),
            OperandType::Register(6),
        ]),
        Instruction::new(Opcode::ILtRRR, vec![
            OperandType::Register(6),
            OperandType::Register(5),
            OperandType::Register(7),
        ]),
        Instruction::new(Opcode::IJumpFalseR, vec![
            OperandType::Register(7),
            OperandType::Offset(5),
        ]),
        Instruction::new(Opcode::IArrayGet, vec![
            OperandType::Register(4),
            OperandType::Register(6),
            OperandType::Register(8),
        ]),
        Instruction::new(Opcode::Send, vec![
            OperandType::Argument(3),
            OperandType::Register(8),
        ]),
        Instruction::new(Opcode::IAddRCR, vec![
            OperandType::Register(6),
            OperandType::Constant(Value::Int(1)),
            OperandType::Register(6),
        ]),
        Instruction::new(Opcode::Jump, vec![
            OperandType::Offset(-5),
        ]),
        Instruction::new(Opcode::Send, vec![
            OperandType::Argument(3),
            OperandType::Constant(Value::Int(-1)),
        ]),
        Instruction::new(Opcode::Ret, vec![
            OperandType::Constant(Value::Nil),
        ]),
    ];
    Function::new("worker_fn".to_string(), instrs, 10, 4, FunctionType::Regular)
}

fn build_main_int_array(n: i64, num_workers: usize) -> Function {
    let powers = vec![0i64, 1, 4, 27, 256, 3125, 46656, 823543, 16777216, 387420489];
    let mut instrs = vec![
        Instruction::new(Opcode::LoadConst, vec![
            OperandType::Constant(Value::IntArray(Arc::new(RwLock::new(powers)))),
            OperandType::Register(0),
        ]),
        Instruction::new(Opcode::Call, vec![OperandType::Function("time".to_string()), OperandType::Offset(0), OperandType::Register(2)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::SelfProc, OperandType::Register(3)]),
    ];

    let chunk = n / num_workers as i64;
    for i in 0..num_workers {
        let start = i as i64 * chunk;
        let end = if i == num_workers - 1 { n } else { (i as i64 + 1) * chunk };
        instrs.push(Instruction::new(
            Opcode::Spawn,
            vec![
                OperandType::Function("worker_fn".to_string()),
                OperandType::Offset(4),
                OperandType::Constant(Value::Int(start)),
                OperandType::Constant(Value::Int(end)),
                OperandType::Register(0),
                OperandType::Register(3),
                OperandType::Register(10 + i),
            ],
        ));
    }

    // Collector loop
    instrs.extend(vec![
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(5)]),
        Instruction::new(Opcode::Lt, vec![OperandType::Register(5), OperandType::Constant(Value::Int(num_workers as i64)), OperandType::Register(6)]),
        Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(6), OperandType::Offset(8)]),
        Instruction::new(Opcode::Receive, vec![OperandType::Register(7)]),
        Instruction::new(Opcode::Eq, vec![OperandType::Register(7), OperandType::Constant(Value::Int(-1)), OperandType::Register(8)]),
        Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(8), OperandType::Offset(3)]),
        Instruction::new(Opcode::Add, vec![OperandType::Register(5), OperandType::Constant(Value::Int(1)), OperandType::Register(5)]),
        Instruction::new(Opcode::Jump, vec![OperandType::Offset(2)]),
        Instruction::new(Opcode::Call, vec![OperandType::Function("println".to_string()), OperandType::Offset(1), OperandType::Register(7), OperandType::Register(9)]),
        Instruction::new(Opcode::Jump, vec![OperandType::Offset(-8)]),

        Instruction::new(Opcode::Call, vec![OperandType::Function("time".to_string()), OperandType::Offset(0), OperandType::Register(4)]),
        Instruction::new(Opcode::FSubRRR, vec![OperandType::Register(4), OperandType::Register(2), OperandType::Register(6)]),
        Instruction::new(Opcode::Call, vec![OperandType::Function("println".to_string()), OperandType::Offset(2), OperandType::Constant(Value::String("Time elapsed (s): ".to_string())), OperandType::Register(6), OperandType::Register(9)]),
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Nil)]),
    ]);

    Function::new("main".to_string(), instrs, 30, 0, FunctionType::Regular)
}

fn build_find_next_stub() -> Function {
    let instrs = vec![
        // 0: r0 = cur
        Instruction::new(Opcode::LoadConst, vec![OperandType::Argument(0), OperandType::Register(0)]),
        // 1: r5 = end
        Instruction::new(Opcode::LoadConst, vec![OperandType::Argument(1), OperandType::Register(5)]),
        // 2: outer loop head: if cur >= end goto ret_neg1 (at index 19)
        Instruction::new(Opcode::ILtRRR, vec![OperandType::Register(0), OperandType::Register(5), OperandType::Register(6)]),
        // 3: if not cur < end, jump to index 19 (3 + 16 = 19)
        Instruction::new(Opcode::IJumpFalseR, vec![OperandType::Register(6), OperandType::Offset(16)]),
        // 4: r1 = n = cur
        Instruction::new(Opcode::LoadConst, vec![OperandType::Register(0), OperandType::Register(1)]),
        // 5: r2 = sum = 0
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(2)]),
        // 6: inner loop head: r3 = d = n % 10
        Instruction::new(Opcode::IRemRCR, vec![OperandType::Register(1), OperandType::Constant(Value::Int(10)), OperandType::Register(3)]),
        // 7: r4 = powers[d]
        Instruction::new(Opcode::IArrayGet, vec![OperandType::Argument(2), OperandType::Register(3), OperandType::Register(4)]),
        // 8: sum = sum + powers[d]
        Instruction::new(Opcode::IAddRRR, vec![OperandType::Register(2), OperandType::Register(4), OperandType::Register(2)]),
        // 9: if sum > cur, break to next number at index 14 (9 + 5 = 14)
        Instruction::new(Opcode::IGtJmpRR, vec![OperandType::Register(2), OperandType::Register(0), OperandType::Offset(5)]),
        // 10: n = n / 10
        Instruction::new(Opcode::IDivRCR, vec![OperandType::Register(1), OperandType::Constant(Value::Int(10)), OperandType::Register(1)]),
        // 11: if n > 0, jump back to inner loop head at index 6 (11 - 5 = 6)
        Instruction::new(Opcode::IGtJmpRC, vec![OperandType::Register(1), OperandType::Constant(Value::Int(0)), OperandType::Offset(-5)]),
        // 12: if sum == cur, jump to index 16 (12 + 4 = 16)
        Instruction::new(Opcode::IEqJmpRR, vec![OperandType::Register(2), OperandType::Register(0), OperandType::Offset(4)]),
        // 13: jump to index 14 (13 + 1 = 14)
        Instruction::new(Opcode::Jump, vec![OperandType::Offset(1)]),
        // 14: cur += 1
        Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(0)]),
        // 15: jump to outer loop head at index 2 (15 - 13 = 2)
        Instruction::new(Opcode::Jump, vec![OperandType::Offset(-13)]),
        // 16: if cur > 0, jump to index 18 (16 + 2 = 18)
        Instruction::new(Opcode::IGtJmpRC, vec![OperandType::Register(0), OperandType::Constant(Value::Int(0)), OperandType::Offset(2)]),
        // 17: jump to next number at index 14 (17 - 3 = 14)
        Instruction::new(Opcode::Jump, vec![OperandType::Offset(-3)]),
        // 18: match found: return cur!
        Instruction::new(Opcode::Ret, vec![OperandType::Register(0)]),
        // 19: chunk finished, no match found: return -1!
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(-1))]),
    ];
    Function::new("find_next_stub".to_string(), instrs, 8, 3, FunctionType::Regular)
}

fn build_worker_fn_with_stub() -> Function {
    let instrs = vec![
        // 0: r0 = cur = start
        Instruction::new(Opcode::LoadConst, vec![OperandType::Argument(0), OperandType::Register(0)]),
        // 1: r3 = end
        Instruction::new(Opcode::LoadConst, vec![OperandType::Argument(1), OperandType::Register(3)]),
        // 2: loop head: call find_next_stub(cur, end, powers) -> r1
        Instruction::new(Opcode::Call, vec![
            OperandType::Function("find_next_stub".to_string()),
            OperandType::Offset(3),
            OperandType::Register(0),
            OperandType::Register(3),
            OperandType::Argument(2),
            OperandType::Register(1),
        ]),
        // 3: if r1 < 0, jump to index 8 (3 + 5 = 8)
        Instruction::new(Opcode::ILtJmpRC, vec![OperandType::Register(1), OperandType::Constant(Value::Int(0)), OperandType::Offset(5)]),
        // 4: send found match to collector
        Instruction::new(Opcode::Send, vec![OperandType::Argument(3), OperandType::Register(1)]),
        // 5: cur = match + 1
        Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(1), OperandType::Constant(Value::Int(1)), OperandType::Register(0)]),
        // 6: check if cur < end (both r0 and r3 are registers)
        Instruction::new(Opcode::ILtRRR, vec![OperandType::Register(0), OperandType::Register(3), OperandType::Register(2)]),
        // 7: if cur < end, jump back to index 2 (7 - 5 = 2)
        Instruction::new(Opcode::IJumpTrueR, vec![OperandType::Register(2), OperandType::Offset(-5)]),
        // 8: send -1 to collector indicating worker is done
        Instruction::new(Opcode::Send, vec![OperandType::Argument(3), OperandType::Constant(Value::Int(-1))]),
        // 9: ret nil
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Nil)]),
    ];
    Function::new("worker_fn".to_string(), instrs, 8, 4, FunctionType::Regular)
}

fn main() {
    let is_munch = build_is_munchausen();
    let worker = build_worker_fn();
    let main_fn = build_main_int_array(10_000_000, 16);

    let bc = Bytecode {
        functions: vec![is_munch, worker, main_fn],
        entry_point: "main".to_string(),
    };

    let f = File::create("/tmp/bench_munchausen_10m.bin").expect("create file");
    let mut writer = BufWriter::new(f);
    write_bytecode(&mut writer, &bc).expect("write bytecode");
    println!("Successfully generated /tmp/bench_munchausen_10m.bin");

    // Decoupled architecture with pure mathematical stub JIT-compiled by LLVM
    let stub = build_find_next_stub();
    let worker_stub = build_worker_fn_with_stub();
    let main_stub = build_main_int_array(10_000_000, 16);

    let bc_stub = Bytecode {
        functions: vec![stub, worker_stub, main_stub],
        entry_point: "main".to_string(),
    };

    let f_stub = File::create("/tmp/bench_munchausen_10m_with_stub.bin").expect("create file");
    let mut writer_stub = BufWriter::new(f_stub);
    write_bytecode(&mut writer_stub, &bc_stub).expect("write bytecode");
    println!("Successfully generated /tmp/bench_munchausen_10m_with_stub.bin");

    // Sieve of Eratosthenes Bytecode Binary (10 Million numbers)
    let sieve_kernel = build_sieve_kernel();
    let sieve_main = build_sieve_main(10_000_000);
    let bc_sieve = Bytecode {
        functions: vec![sieve_kernel, sieve_main],
        entry_point: "main".to_string(),
    };
    let f_sieve = File::create("sieve_10m.bin").expect("create sieve binary");
    let mut writer_sieve = BufWriter::new(f_sieve);
    write_bytecode(&mut writer_sieve, &bc_sieve).expect("write sieve bytecode");
    // Sieve of Eratosthenes Bytecode Binary (50 Million numbers)
    let sieve_kernel = build_sieve_kernel();
    let sieve_main = build_sieve_main(50_000_000);
    let bc_sieve = Bytecode {
        functions: vec![sieve_kernel, sieve_main],
        entry_point: "main".to_string(),
    };
    let f_sieve = File::create("sieve_50m.bin").expect("create sieve binary");
    let mut writer_sieve = BufWriter::new(f_sieve);
    write_bytecode(&mut writer_sieve, &bc_sieve).expect("write sieve bytecode");
    // Sieve of Eratosthenes Bytecode Binary (100 Million numbers)
    let sieve_kernel = build_sieve_kernel();
    let sieve_main = build_sieve_main(100_000_000);
    let bc_sieve = Bytecode {
        functions: vec![sieve_kernel, sieve_main],
        entry_point: "main".to_string(),
    };
    let f_sieve = File::create("sieve_100m.bin").expect("create sieve binary");
    let mut writer_sieve = BufWriter::new(f_sieve);
    write_bytecode(&mut writer_sieve, &bc_sieve).expect("write sieve bytecode");
    // Mandelbrot Bytecode Binary (High-PPI Interactive Rendering)
    let mandel_width = 1200i64;
    let mandel_height = 600i64;
    let mandel_render = build_mandel_render();
    let mandel_main = build_mandel_main(mandel_width, mandel_height, 100, 40);
    let bc_mandel = Bytecode {
        functions: vec![mandel_render, mandel_main],
        entry_point: "main".to_string(),
    };
    let f_mandel = File::create("mandelbrot.bin").expect("create mandelbrot binary");
    let mut writer_mandel = BufWriter::new(f_mandel);
    write_bytecode(&mut writer_mandel, &bc_mandel).expect("write mandelbrot bytecode");
    println!("Successfully generated mandelbrot.bin (IonVM Bytecode Binary, {}x{} High-PPI Interactive)", mandel_width, mandel_height);
}

enum AsmItem {
    Label(String),
    Inst(Opcode, Vec<OperandType>),
    Branch(Opcode, Vec<OperandType>, String),
}

struct Assembler {
    items: Vec<AsmItem>,
}

impl Assembler {
    fn new() -> Self {
        Assembler { items: Vec::new() }
    }

    fn label(&mut self, name: &str) {
        self.items.push(AsmItem::Label(name.to_string()));
    }

    fn inst(&mut self, op: Opcode, operands: Vec<OperandType>) {
        self.items.push(AsmItem::Inst(op, operands));
    }

    fn branch(&mut self, op: Opcode, ops: Vec<OperandType>, target: &str) {
        self.items.push(AsmItem::Branch(op, ops, target.to_string()));
    }

    fn jump(&mut self, target: &str) {
        self.items.push(AsmItem::Branch(Opcode::Jump, vec![], target.to_string()));
    }

    fn assemble(self) -> Vec<Instruction> {
        let mut labels = std::collections::HashMap::new();
        let mut inst_count = 0;
        for item in &self.items {
            match item {
                AsmItem::Label(name) => {
                    labels.insert(name.clone(), inst_count);
                }
                AsmItem::Inst(..) | AsmItem::Branch(..) => {
                    inst_count += 1;
                }
            }
        }

        let mut res = Vec::with_capacity(inst_count);
        let mut cur_idx = 0;
        for item in self.items {
            match item {
                AsmItem::Label(_) => {}
                AsmItem::Inst(op, operands) => {
                    res.push(Instruction::new(op, operands));
                    cur_idx += 1;
                }
                AsmItem::Branch(op, mut ops, target) => {
                    let target_idx = *labels.get(&target).unwrap_or_else(|| panic!("Undefined label: {}", target));
                    let offset = target_idx as i64 - cur_idx as i64;
                    ops.push(OperandType::Offset(offset));
                    res.push(Instruction::new(op, ops));
                    cur_idx += 1;
                }
            }
        }
        res
    }
}

fn generate_palette(num_colors: usize) -> Vec<f64> {
    let mut palette = Vec::with_capacity(num_colors * 3);
    for i in 0..num_colors {
        let t = i as f64 / num_colors as f64;
        // Inigo Quilez vibrant multi-color cosine palette
        // Produces smooth continuous transitions through Red -> Orange -> Yellow -> Green -> Cyan -> Blue -> Magenta
        let r = (0.5 + 0.5 * (2.0 * std::f64::consts::PI * (1.0 * t + 0.00)).cos()) * 255.0;
        let g = (0.5 + 0.5 * (2.0 * std::f64::consts::PI * (1.0 * t + 0.33)).cos()) * 255.0;
        let b = (0.5 + 0.5 * (2.0 * std::f64::consts::PI * (1.0 * t + 0.67)).cos()) * 255.0;
        palette.push(r.clamp(0.0, 255.0));
        palette.push(g.clamp(0.0, 255.0));
        palette.push(b.clamp(0.0, 255.0));
    }
    palette
}

fn build_mandel_render() -> Function {
    let mut asm = Assembler::new();

    // Argument 0: buffer (FloatArray)
    // Argument 1: palette (FloatArray)
    // Argument 2: width (Int)
    // Argument 3: height (Int)
    // Argument 4: max_iters (Int)
    // Argument 5: max_x (Int)
    // Argument 6: max_y (Int)
    // Argument 7: x_min (Float)
    // Argument 8: y_min (Float)
    // Argument 9: step_x (Float)
    // Argument 10: step_y (Float)

    // Load arguments into registers
    asm.inst(Opcode::LoadConst, vec![OperandType::Argument(2), OperandType::Register(21)]); // width
    asm.inst(Opcode::LoadConst, vec![OperandType::Argument(5), OperandType::Register(22)]); // max_x
    asm.inst(Opcode::LoadConst, vec![OperandType::Argument(6), OperandType::Register(23)]); // max_y
    asm.inst(Opcode::LoadConst, vec![OperandType::Argument(4), OperandType::Register(24)]); // max_iters
    asm.inst(Opcode::LoadConst, vec![OperandType::Argument(7), OperandType::Register(20)]); // x_min
    asm.inst(Opcode::LoadConst, vec![OperandType::Argument(8), OperandType::Register(2)]);  // y_min (ci initial)
    asm.inst(Opcode::LoadConst, vec![OperandType::Argument(9), OperandType::Register(18)]); // step_x
    asm.inst(Opcode::LoadConst, vec![OperandType::Argument(10), OperandType::Register(19)]); // step_y

    // Constants:
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(3)), OperandType::Register(25)]); // three = 3
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(4.0)), OperandType::Register(7)]); // four = 4.0
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(0.0)), OperandType::Register(30)]); // 0.0

    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(0)]); // py = 0

    asm.label("row_loop_head");
    asm.branch(Opcode::IGtJmpRR, vec![OperandType::Register(0), OperandType::Register(23)], "exit");

    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(1)]); // px = 0
    asm.inst(Opcode::LoadConst, vec![OperandType::Register(20), OperandType::Register(3)]); // cr = x_min

    asm.label("col_loop_head");
    asm.branch(Opcode::IGtJmpRR, vec![OperandType::Register(1), OperandType::Register(22)], "row_loop_next");

    // Point escape loop init:
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(0.0)), OperandType::Register(4)]); // zr = 0.0
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(0.0)), OperandType::Register(5)]); // zi = 0.0
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(6)]); // iter = 0

    asm.label("point_loop_head");
    // r8 = zr * zr
    asm.inst(Opcode::FMulRRR, vec![OperandType::Register(4), OperandType::Register(4), OperandType::Register(8)]);
    // r9 = zi * zi
    asm.inst(Opcode::FMulRRR, vec![OperandType::Register(5), OperandType::Register(5), OperandType::Register(9)]);
    // r10 = mag2 = zr2 + zi2
    asm.inst(Opcode::FAddRRR, vec![OperandType::Register(8), OperandType::Register(9), OperandType::Register(10)]);
    // if mag2 > four goto point_done
    asm.branch(Opcode::FGtJmpRR, vec![OperandType::Register(10), OperandType::Register(7)], "point_done");
    // if iter > max_iters goto point_done
    asm.branch(Opcode::IGtJmpRR, vec![OperandType::Register(6), OperandType::Register(24)], "point_done");

    // next_zr (r12) = zr2 - zi2 + cr
    asm.inst(Opcode::FSubRRR, vec![OperandType::Register(8), OperandType::Register(9), OperandType::Register(11)]);
    asm.inst(Opcode::FAddRRR, vec![OperandType::Register(11), OperandType::Register(3), OperandType::Register(12)]);

    // zi (r5) = 2.0 * zr * zi + ci
    asm.inst(Opcode::FMulRRR, vec![OperandType::Register(4), OperandType::Register(5), OperandType::Register(13)]);
    asm.inst(Opcode::FAddRRR, vec![OperandType::Register(13), OperandType::Register(13), OperandType::Register(13)]);
    asm.inst(Opcode::FAddRRR, vec![OperandType::Register(13), OperandType::Register(2), OperandType::Register(5)]);

    // zr (r4) = next_zr (r12)
    asm.inst(Opcode::LoadConst, vec![OperandType::Register(12), OperandType::Register(4)]);

    // iter += 1
    asm.inst(Opcode::IAddRCR, vec![OperandType::Register(6), OperandType::Constant(Value::Int(1)), OperandType::Register(6)]);

    asm.jump("point_loop_head");

    asm.label("point_done");
    // Pixel index: idx = (py * width + px) * 3
    asm.inst(Opcode::IMulRRR, vec![OperandType::Register(0), OperandType::Register(21), OperandType::Register(14)]);
    asm.inst(Opcode::IAddRRR, vec![OperandType::Register(14), OperandType::Register(1), OperandType::Register(14)]);
    asm.inst(Opcode::IMulRRR, vec![OperandType::Register(14), OperandType::Register(25), OperandType::Register(14)]);

    // If iter > max_iters, point did not escape -> draw black
    asm.branch(Opcode::IGtJmpRR, vec![OperandType::Register(6), OperandType::Register(24)], "draw_black");

    // Escaped point: look up color from palette (64 entries)
    // r15 = iter % 64
    asm.inst(Opcode::IRemRCR, vec![OperandType::Register(6), OperandType::Constant(Value::Int(64)), OperandType::Register(15)]);
    // r15 = r15 * 3
    asm.inst(Opcode::IMulRRR, vec![OperandType::Register(15), OperandType::Register(25), OperandType::Register(15)]);

    // R = palette[r15]
    asm.inst(Opcode::FArrayGet, vec![OperandType::Argument(1), OperandType::Register(15), OperandType::Register(16)]);
    asm.inst(Opcode::FArraySet, vec![OperandType::Argument(0), OperandType::Register(14), OperandType::Register(16)]);

    // G = palette[r15 + 1]
    asm.inst(Opcode::IAddRCR, vec![OperandType::Register(15), OperandType::Constant(Value::Int(1)), OperandType::Register(26)]);
    asm.inst(Opcode::FArrayGet, vec![OperandType::Argument(1), OperandType::Register(26), OperandType::Register(17)]);
    asm.inst(Opcode::IAddRCR, vec![OperandType::Register(14), OperandType::Constant(Value::Int(1)), OperandType::Register(28)]);
    asm.inst(Opcode::FArraySet, vec![OperandType::Argument(0), OperandType::Register(28), OperandType::Register(17)]);

    // B = palette[r15 + 2]
    asm.inst(Opcode::IAddRCR, vec![OperandType::Register(15), OperandType::Constant(Value::Int(2)), OperandType::Register(27)]);
    asm.inst(Opcode::FArrayGet, vec![OperandType::Argument(1), OperandType::Register(27), OperandType::Register(31)]);
    asm.inst(Opcode::IAddRCR, vec![OperandType::Register(14), OperandType::Constant(Value::Int(2)), OperandType::Register(29)]);
    asm.inst(Opcode::FArraySet, vec![OperandType::Argument(0), OperandType::Register(29), OperandType::Register(31)]);

    asm.jump("col_loop_next");

    asm.label("draw_black");
    asm.inst(Opcode::FArraySet, vec![OperandType::Argument(0), OperandType::Register(14), OperandType::Register(30)]);
    asm.inst(Opcode::IAddRCR, vec![OperandType::Register(14), OperandType::Constant(Value::Int(1)), OperandType::Register(28)]);
    asm.inst(Opcode::FArraySet, vec![OperandType::Argument(0), OperandType::Register(28), OperandType::Register(30)]);
    asm.inst(Opcode::IAddRCR, vec![OperandType::Register(14), OperandType::Constant(Value::Int(2)), OperandType::Register(29)]);
    asm.inst(Opcode::FArraySet, vec![OperandType::Argument(0), OperandType::Register(29), OperandType::Register(30)]);

    asm.label("col_loop_next");
    asm.inst(Opcode::IAddRCR, vec![OperandType::Register(1), OperandType::Constant(Value::Int(1)), OperandType::Register(1)]);
    asm.inst(Opcode::FAddRRR, vec![OperandType::Register(3), OperandType::Register(18), OperandType::Register(3)]);
    asm.jump("col_loop_head");

    asm.label("row_loop_next");
    asm.inst(Opcode::IAddRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(0)]);
    asm.inst(Opcode::FAddRRR, vec![OperandType::Register(2), OperandType::Register(19), OperandType::Register(2)]);
    asm.jump("row_loop_head");

    asm.label("exit");
    asm.inst(Opcode::Ret, vec![OperandType::Constant(Value::Nil)]);

    let instrs = asm.assemble();
    Function::new("mandel_render".to_string(), instrs, 36, 11, FunctionType::Regular)
}

fn build_mandel_main(width: i64, height: i64, term_cols: i64, term_rows: i64) -> Function {
    let mut asm = Assembler::new();
    let total_pixels = (width * height + 10) * 3;
    let palette_data = generate_palette(64);
    let palette_val = Value::FloatArray(Arc::new(RwLock::new(palette_data)));

    // Load initial values
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(total_pixels)), OperandType::Register(0)]);
    asm.inst(Opcode::NewFloatArray, vec![OperandType::Register(0), OperandType::Register(0)]); // r0 = buffer
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(palette_val), OperandType::Register(1)]); // r1 = palette

    // r2 = center_x = -0.75
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(-0.75)), OperandType::Register(2)]);
    // r3 = center_y = 0.0
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(0.0)), OperandType::Register(3)]);
    // r4 = span_x = 2.5
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(2.5)), OperandType::Register(4)]);
    // r5 = span_y = 2.5
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(2.5)), OperandType::Register(5)]);

    // r10 = f_width
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(width as f64)), OperandType::Register(10)]);
    // r11 = f_height
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(height as f64)), OperandType::Register(11)]);
    // r12 = 0.5 (half)
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(0.5)), OperandType::Register(12)]);
    // r13 = zoom_in_factor = 0.6
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(0.6)), OperandType::Register(13)]);
    // r14 = zoom_out_factor = 1.6666667
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(1.6666667)), OperandType::Register(14)]);
    // r15 = move_factor = 0.2
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(0.2)), OperandType::Register(15)]);

    // Integer constants for render call:
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(width)), OperandType::Register(19)]);
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(height)), OperandType::Register(20)]);
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(100)), OperandType::Register(21)]); // max_iters
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(width - 1)), OperandType::Register(22)]);
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(height - 1)), OperandType::Register(23)]);

    asm.label("render_loop");

    // Clear / home cursor for clean in-place redraw
    asm.inst(Opcode::Call, vec![
        OperandType::Function("print".to_string()),
        OperandType::Offset(1),
        OperandType::Constant(Value::String("\x1b[H".to_string())),
        OperandType::Register(17),
    ]);

    // step_x = span_x / f_width
    asm.inst(Opcode::FDivRRR, vec![OperandType::Register(4), OperandType::Register(10), OperandType::Register(6)]);
    // step_y = span_y / f_height
    asm.inst(Opcode::FDivRRR, vec![OperandType::Register(5), OperandType::Register(11), OperandType::Register(7)]);

    // x_min = center_x - (span_x * 0.5)
    asm.inst(Opcode::FMulRRR, vec![OperandType::Register(4), OperandType::Register(12), OperandType::Register(17)]);
    asm.inst(Opcode::FSubRRR, vec![OperandType::Register(2), OperandType::Register(17), OperandType::Register(8)]);

    // y_min = center_y - (span_y * 0.5)
    asm.inst(Opcode::FMulRRR, vec![OperandType::Register(5), OperandType::Register(12), OperandType::Register(17)]);
    asm.inst(Opcode::FSubRRR, vec![OperandType::Register(3), OperandType::Register(17), OperandType::Register(9)]);

    // Call mandel_render(buffer, palette, width, height, max_iters, max_x, max_y, x_min, y_min, step_x, step_y)
    asm.inst(Opcode::Call, vec![
        OperandType::Function("mandel_render".to_string()),
        OperandType::Offset(11),
        OperandType::Register(0), // buffer
        OperandType::Register(1), // palette
        OperandType::Register(19), // width
        OperandType::Register(20), // height
        OperandType::Register(21), // max_iters
        OperandType::Register(22), // max_x
        OperandType::Register(23), // max_y
        OperandType::Register(8),  // x_min
        OperandType::Register(9),  // y_min
        OperandType::Register(6),  // step_x
        OperandType::Register(7),  // step_y
        OperandType::Register(17), // out
    ]);

    // Call ChafaPrint(buffer, width, height, term_cols, term_rows)
    asm.inst(Opcode::Call, vec![
        OperandType::Function("ChafaPrint".to_string()),
        OperandType::Offset(5),
        OperandType::Register(0),
        OperandType::Register(19),
        OperandType::Register(20),
        OperandType::Constant(Value::Int(term_cols)),
        OperandType::Constant(Value::Int(term_rows)),
        OperandType::Register(17),
    ]);

    // Prompt user and read single keypress
    asm.inst(Opcode::Call, vec![
        OperandType::Function("ReadKey".to_string()),
        OperandType::Offset(1),
        OperandType::Constant(Value::String("\x1b[1;36m[WASD/HJKL]\x1b[0m Move | \x1b[1;32m[+/-]\x1b[0m Zoom | \x1b[1;33m[R]\x1b[0m Reset | \x1b[1;31m[Q]\x1b[0m Quit > ".to_string())),
        OperandType::Register(16),
    ]);

    // If cmd == "q", exit
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("q".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "exit");

    // Zoom in: "+" or "=" or "i"
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("+".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_zoom_in");
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("=".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_zoom_in");
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("i".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_zoom_in");

    // Zoom out: "-" or "_" or "o"
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("-".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_zoom_out");
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("_".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_zoom_out");
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("o".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_zoom_out");

    // Move Up: "w" or "k"
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("w".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_move_up");
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("k".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_move_up");

    // Move Down: "s" or "j"
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("s".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_move_down");
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("j".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_move_down");

    // Move Left: "a" or "h"
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("a".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_move_left");
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("h".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_move_left");

    // Move Right: "d" or "l"
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("d".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_move_right");
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("l".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_move_right");

    // Reset: "r"
    asm.inst(Opcode::Eq, vec![OperandType::Register(16), OperandType::Constant(Value::String("r".to_string())), OperandType::Register(17)]);
    asm.branch(Opcode::JumpTrue, vec![OperandType::Register(17)], "do_reset");

    // Default: loop back
    asm.jump("render_loop");

    asm.label("do_zoom_in");
    asm.inst(Opcode::FMulRRR, vec![OperandType::Register(4), OperandType::Register(13), OperandType::Register(4)]);
    asm.inst(Opcode::FMulRRR, vec![OperandType::Register(5), OperandType::Register(13), OperandType::Register(5)]);
    asm.jump("render_loop");

    asm.label("do_zoom_out");
    asm.inst(Opcode::FMulRRR, vec![OperandType::Register(4), OperandType::Register(14), OperandType::Register(4)]);
    asm.inst(Opcode::FMulRRR, vec![OperandType::Register(5), OperandType::Register(14), OperandType::Register(5)]);
    asm.jump("render_loop");

    asm.label("do_move_up");
    asm.inst(Opcode::FMulRRR, vec![OperandType::Register(5), OperandType::Register(15), OperandType::Register(17)]);
    asm.inst(Opcode::FSubRRR, vec![OperandType::Register(3), OperandType::Register(17), OperandType::Register(3)]);
    asm.jump("render_loop");

    asm.label("do_move_down");
    asm.inst(Opcode::FMulRRR, vec![OperandType::Register(5), OperandType::Register(15), OperandType::Register(17)]);
    asm.inst(Opcode::FAddRRR, vec![OperandType::Register(3), OperandType::Register(17), OperandType::Register(3)]);
    asm.jump("render_loop");

    asm.label("do_move_left");
    asm.inst(Opcode::FMulRRR, vec![OperandType::Register(4), OperandType::Register(15), OperandType::Register(17)]);
    asm.inst(Opcode::FSubRRR, vec![OperandType::Register(2), OperandType::Register(17), OperandType::Register(2)]);
    asm.jump("render_loop");

    asm.label("do_move_right");
    asm.inst(Opcode::FMulRRR, vec![OperandType::Register(4), OperandType::Register(15), OperandType::Register(17)]);
    asm.inst(Opcode::FAddRRR, vec![OperandType::Register(2), OperandType::Register(17), OperandType::Register(2)]);
    asm.jump("render_loop");

    asm.label("do_reset");
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(-0.75)), OperandType::Register(2)]);
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(0.0)), OperandType::Register(3)]);
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(2.5)), OperandType::Register(4)]);
    asm.inst(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(2.5)), OperandType::Register(5)]);
    asm.jump("render_loop");

    asm.label("exit");
    asm.inst(Opcode::Call, vec![
        OperandType::Function("println".to_string()),
        OperandType::Offset(1),
        OperandType::Constant(Value::String("\nExiting IonVM Mandelbrot. Goodbye!".to_string())),
        OperandType::Register(17),
    ]);
    asm.inst(Opcode::Ret, vec![OperandType::Constant(Value::Nil)]);

    let instrs = asm.assemble();
    Function::new("main".to_string(), instrs, 30, 0, FunctionType::Regular)
}

fn build_sieve_kernel() -> Function {
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

fn build_sieve_main(limit: i64) -> Function {
    let instrs = vec![
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(limit)), OperandType::Register(0)]),
        Instruction::new(Opcode::IShrRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(6)), OperandType::Register(1)]),
        Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(1), OperandType::Constant(Value::Int(1)), OperandType::Register(1)]),
        Instruction::new(Opcode::NewIntArray, vec![OperandType::Register(1), OperandType::Register(2)]),
        Instruction::new(Opcode::Call, vec![OperandType::Function("time".to_string()), OperandType::Offset(0), OperandType::Register(3)]),
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("sieve_kernel".to_string()),
                OperandType::Offset(2),
                OperandType::Register(0),
                OperandType::Register(2),
                OperandType::Register(4),
            ],
        ),
        Instruction::new(Opcode::Call, vec![OperandType::Function("time".to_string()), OperandType::Offset(0), OperandType::Register(5)]),
        Instruction::new(Opcode::Sub, vec![OperandType::Register(5), OperandType::Register(3), OperandType::Register(6)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(1000.0)), OperandType::Register(7)]),
        Instruction::new(Opcode::Mul, vec![OperandType::Register(6), OperandType::Register(7), OperandType::Register(7)]),
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("println".to_string()),
                OperandType::Offset(2),
                OperandType::Constant(Value::String(format!("IonVM Bytecode Sieve ({}M): Primes = ", limit / 1_000_000))),
                OperandType::Register(4),
                OperandType::Register(8),
            ],
        ),
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("println".to_string()),
                OperandType::Offset(2),
                OperandType::Constant(Value::String("Time elapsed (ms): ".to_string())),
                OperandType::Register(7),
                OperandType::Register(8),
            ],
        ),
        Instruction::new(Opcode::Ret, vec![OperandType::Register(4)]),
    ];
    Function::new("main".to_string(), instrs, 12, 0, FunctionType::Regular)
}

#[test]
fn test_single_munch() {
    let stub = build_find_next_stub();
    let worker_stub = build_worker_fn_with_stub();
    let main_stub = build_main_int_array(5000, 2);

    let bc_stub = Bytecode {
        functions: vec![stub, worker_stub, main_stub],
        entry_point: "main".to_string(),
    };

    let mut vm = vmm::vm::IonVM::new(bc_stub).unwrap();
    let res = vm.run();
    assert!(res.is_ok());
}

#[test]
fn test_mandel_render() {
    let width = 120i64;
    let height = 60i64;
    let palette_data = generate_palette(64);
    let palette = Arc::new(RwLock::new(palette_data));
    let mandel_render = build_mandel_render();
    let total_pixels = (width * height + 10) * 3;
    let buffer = Arc::new(RwLock::new(vec![0.0f64; total_pixels as usize]));
    let main_instrs = vec![
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::FloatArray(buffer.clone())), OperandType::Register(0)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::FloatArray(palette.clone())), OperandType::Register(1)]),
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("mandel_render".to_string()),
                OperandType::Offset(11),
                OperandType::Register(0), // buffer
                OperandType::Register(1), // palette
                OperandType::Constant(Value::Int(width)),
                OperandType::Constant(Value::Int(height)),
                OperandType::Constant(Value::Int(80)),
                OperandType::Constant(Value::Int(width - 1)),
                OperandType::Constant(Value::Int(height - 1)),
                OperandType::Constant(Value::Float(-2.0)), // x_min
                OperandType::Constant(Value::Float(-1.25)), // y_min
                OperandType::Constant(Value::Float(2.5 / width as f64)), // step_x
                OperandType::Constant(Value::Float(2.5 / height as f64)), // step_y
                OperandType::Register(2),
            ],
        ),
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Nil)]),
    ];
    let main_fn = Function::new("main".to_string(), main_instrs, 10, 0, FunctionType::Regular);

    let bc = Bytecode {
        functions: vec![mandel_render, main_fn],
        entry_point: "main".to_string(),
    };

    let mut vm = vmm::vm::IonVM::new(bc).unwrap();
    let res = vm.run();
    assert!(res.is_ok(), "VM run failed: {:?}", res);

    let borrowed = buffer.read().unwrap();
    let zero_count = borrowed.iter().step_by(3).filter(|&&v| v == 0.0).count();
    let nonzero_count = borrowed.iter().step_by(3).filter(|&&v| v > 0.0).count();
    println!("zero_count: {}, nonzero_count: {}", zero_count, nonzero_count);
    assert!(nonzero_count > 0, "Pixels must contain colors");
    assert!(zero_count > 0, "Must have black points inside the set");

    // Color verification: check for presence of multiple distinct hues (red, green, blue)
    let mut has_red = false;
    let mut has_green = false;
    let mut has_blue = false;
    for chunk in borrowed.chunks(3) {
        if chunk.len() == 3 {
            if chunk[0] > 180.0 && chunk[1] < 120.0 && chunk[2] < 120.0 { has_red = true; }
            if chunk[1] > 180.0 && chunk[0] < 120.0 && chunk[2] < 120.0 { has_green = true; }
            if chunk[2] > 180.0 && chunk[0] < 120.0 && chunk[1] < 120.0 { has_blue = true; }
        }
    }
    println!("Multi-color palette check: red={}, green={}, blue={}", has_red, has_green, has_blue);
    assert!(has_red && has_green && has_blue, "Palette must generate multiple distinct colors!");
}

#[test]
fn test_mandel_high_res() {
    let width = 1200i64;
    let height = 600i64;
    let palette_data = generate_palette(64);
    let palette = Arc::new(RwLock::new(palette_data));
    let mandel_render = build_mandel_render();
    let total_pixels = (width * height + 10) * 3;
    let buffer = Arc::new(RwLock::new(vec![0.0f64; total_pixels as usize]));
    let main_instrs = vec![
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::FloatArray(buffer.clone())), OperandType::Register(0)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::FloatArray(palette.clone())), OperandType::Register(1)]),
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("mandel_render".to_string()),
                OperandType::Offset(11),
                OperandType::Register(0), // buffer
                OperandType::Register(1), // palette
                OperandType::Constant(Value::Int(width)),
                OperandType::Constant(Value::Int(height)),
                OperandType::Constant(Value::Int(80)),
                OperandType::Constant(Value::Int(width - 1)),
                OperandType::Constant(Value::Int(height - 1)),
                OperandType::Constant(Value::Float(-2.0)), // x_min
                OperandType::Constant(Value::Float(-1.25)), // y_min
                OperandType::Constant(Value::Float(2.5 / width as f64)), // step_x
                OperandType::Constant(Value::Float(2.5 / height as f64)), // step_y
                OperandType::Register(2),
            ],
        ),
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Nil)]),
    ];
    let main_fn = Function::new("main".to_string(), main_instrs, 10, 0, FunctionType::Regular);

    let bc = Bytecode {
        functions: vec![mandel_render, main_fn],
        entry_point: "main".to_string(),
    };

    let t0 = std::time::Instant::now();
    let mut vm = vmm::vm::IonVM::new(bc).unwrap();
    let res = vm.run();
    let elapsed = t0.elapsed();
    assert!(res.is_ok(), "VM run failed: {:?}", res);

    let borrowed = buffer.read().unwrap();
    let zero_count = borrowed.iter().step_by(3).filter(|&&v| v == 0.0).count();
    let nonzero_count = borrowed.iter().step_by(3).filter(|&&v| v > 0.0).count();
    println!("High-Res Colorful (1200x600 = 720k pixels) rendered in {:.2} ms | zero_count: {}, nonzero_count: {}",
        elapsed.as_secs_f64() * 1000.0, zero_count, nonzero_count);
    assert!(nonzero_count > 0);
    assert!(zero_count > 0);
}

