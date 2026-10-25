use std::fs::File;
use std::io::BufReader;
use std::sync::{Arc, RwLock};
use vmm::bytecode::instruction::{Bytecode, Instruction, OperandType};
use vmm::bytecode::opcode::Opcode;
use vmm::bytecode::serde::{read_bytecode, write_bytecode};
use vmm::jit::translator::compile_function_with_ir;
use vmm::lang::types::{ArrayPattern, Function, FunctionType, ListSegment, Pattern, Value};
use vmm::vm::IonVM;

#[test]
fn test_enum_execution() {
    let file = File::open("/home/top-d/vm/test_enum.bin").expect("open test_enum.bin");
    let mut reader = BufReader::new(file);
    let bc = read_bytecode(&mut reader).expect("deserialize");
    let mut vm = IonVM::new(bc).expect("init vm");
    let res = vm.run();
    assert!(res.is_ok(), "VM execution failed: {:?}", res);
}

#[test]
fn test_float_execution() {
    let file = File::open("/home/top-d/vm/test_float.bin").expect("open test_float.bin");
    let mut reader = BufReader::new(file);
    let bc = read_bytecode(&mut reader).expect("deserialize");
    let mut vm = IonVM::new(bc).expect("init vm");
    let res = vm.run();
    assert!(res.is_ok(), "VM execution failed: {:?}", res);
}

#[test]
fn test_munchausen_fast_execution() {
    let file = File::open("/home/top-d/vm/test_munchausen_fast.bin").expect("open test_munchausen_fast.bin");
    let mut reader = BufReader::new(file);
    let bc = read_bytecode(&mut reader).expect("deserialize");
    let mut vm = IonVM::new(bc).expect("init vm");
    let res = vm.run();
    assert!(res.is_ok(), "VM execution failed: {:?}", res);
}

#[test]
fn test_serde_roundtrip() {
    let file = File::open("/home/top-d/vm/test_enum.bin").expect("open test_enum.bin");
    let mut reader = BufReader::new(file);
    let bc = read_bytecode(&mut reader).expect("deserialize");

    let mut buf = Vec::new();
    write_bytecode(&mut buf, &bc).expect("serialize");

    let mut reader2 = &buf[..];
    let bc2 = read_bytecode(&mut reader2).expect("deserialize serialized");

    assert_eq!(bc, bc2);
}

#[test]
fn test_load_time_object_init_is_registered_at_startup() {
    use vmm::lang::types::{Object, Protection};

    let mut base = Object::new(None);
    base.set_property("kind", Value::String("base".to_string()), Protection::Writeable);
    let base = Arc::new(RwLock::new(base));

    let mut point = Object::new(Some(Arc::clone(&base)));
    point.set_property("x", Value::Int(42), Protection::Writeable);
    point.set_property("0", Value::Int(7), Protection::Writeable);

    let bc = Bytecode {
        functions: vec![Function::new(
            "main".to_string(),
            vec![
                Instruction::new(
                    Opcode::LoadConst,
                    vec![OperandType::Property("Point".to_string()), OperandType::Register(0)],
                ),
                Instruction::new(
                    Opcode::GetProp,
                    vec![
                        OperandType::Register(0),
                        OperandType::Property("x".to_string()),
                        OperandType::Register(1),
                    ],
                ),
                Instruction::new(Opcode::Ret, vec![OperandType::Register(1)]),
            ],
            2,
            0,
            FunctionType::Regular,
        )],
        entry_point: "main".to_string(),
        native_declarations: vec![],
        object_inits: vec![
            ("Point".to_string(), Arc::new(RwLock::new(point))),
            ("Base".to_string(), Arc::clone(&base)),
        ],
    };

    let original_bc = bc.clone();
    let mut serialized = Vec::new();
    write_bytecode(&mut serialized, &bc).expect("serialize load-time objects");
    let bc = read_bytecode(&mut serialized.as_slice()).expect("deserialize load-time objects");
    assert_eq!(bc, original_bc);
    assert_eq!(bc.object_inits[0].0, "Base");
    let point = &bc.object_inits[1].1;
    let base = &bc.object_inits[0].1;
    assert!(Arc::ptr_eq(
        point.read().unwrap().prototype.as_ref().unwrap(),
        base,
    ));
    let mut vm = IonVM::new(bc).expect("init vm with load-time object prototypes");
    vm.run().expect("run VM");
    assert_eq!(vm.entrypoint_return(), Some(Value::Int(42)));
}

#[test]
fn test_array_and_simd_vector_execution() {
    use std::sync::{Arc, RwLock};
    use vmm::bytecode::instruction::{Bytecode, Instruction, OperandType};
    use vmm::bytecode::opcode::Opcode;
    use vmm::lang::types::{Function, FunctionType, Value};

    // Construct main function testing:
    // r0 = [1.0, 2.0, 3.0, 4.0]
    // r1 = [10.0, 20.0, 30.0, 40.0]
    // r2 = r0 + r1 (VAddF)
    // r3 = dot(r0, r1) (VDotF) -> 300.0
    // r4 = sum(r2) (VSumF) -> 110.0
    // r5 = [10, 20, 30] (IntArray)
    // r6 = sum(r5) (VSumI) -> 60
    let v_arr0 = Value::FloatArray(Arc::new(RwLock::new(vec![1.0, 2.0, 3.0, 4.0])));
    let v_arr1 = Value::FloatArray(Arc::new(RwLock::new(vec![10.0, 20.0, 30.0, 40.0])));
    let v_intarr = Value::IntArray(Arc::new(RwLock::new(vec![10, 20, 30])));

    let instructions = vec![
        Instruction {
            opcode: Opcode::LoadConst,
            operands: vec![OperandType::Constant(v_arr0), OperandType::Register(0)],
        },
        Instruction {
            opcode: Opcode::LoadConst,
            operands: vec![OperandType::Constant(v_arr1), OperandType::Register(1)],
        },
        Instruction {
            opcode: Opcode::VAddF,
            operands: vec![OperandType::Register(0), OperandType::Register(1), OperandType::Register(2)],
        },
        Instruction {
            opcode: Opcode::VDotF,
            operands: vec![OperandType::Register(0), OperandType::Register(1), OperandType::Register(3)],
        },
        Instruction {
            opcode: Opcode::VSumF,
            operands: vec![OperandType::Register(2), OperandType::Register(4)],
        },
        Instruction {
            opcode: Opcode::LoadConst,
            operands: vec![OperandType::Constant(v_intarr), OperandType::Register(5)],
        },
        Instruction {
            opcode: Opcode::VSumI,
            operands: vec![OperandType::Register(5), OperandType::Register(6)],
        },
        Instruction {
            opcode: Opcode::Ret,
            operands: vec![OperandType::Register(6)],
        },
    ];

    let main_func = Function::new("main".to_string(), instructions, 7, 0, FunctionType::Regular);
    let bc = Bytecode {
        functions: vec![main_func],
        entry_point: "main".to_string(),
        native_declarations: Vec::new(),
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bc).expect("init vm");
    let res = vm.run();
    assert!(res.is_ok(), "VM execution failed: {:?}", res);
}

#[test]
fn test_simd_string_execution() {
    use vmm::bytecode::instruction::{Bytecode, Instruction, OperandType};
    use vmm::bytecode::opcode::Opcode;
    use vmm::lang::types::{Function, FunctionType, Value};

    let instructions = vec![
        Instruction {
            opcode: Opcode::LoadConst,
            operands: vec![OperandType::Constant(Value::String("hello world from simd!".to_string())), OperandType::Register(0)],
        },
        Instruction {
            opcode: Opcode::SUpperRR,
            operands: vec![OperandType::Register(0), OperandType::Register(1)],
        },
        Instruction {
            opcode: Opcode::SLowerRR,
            operands: vec![OperandType::Register(1), OperandType::Register(2)],
        },
        Instruction {
            opcode: Opcode::LoadConst,
            operands: vec![OperandType::Constant(Value::String("o".to_string())), OperandType::Register(3)],
        },
        Instruction {
            opcode: Opcode::SCountByteRRR,
            operands: vec![OperandType::Register(0), OperandType::Register(3), OperandType::Register(4)],
        },
        Instruction {
            opcode: Opcode::LoadConst,
            operands: vec![OperandType::Constant(Value::String("world".to_string())), OperandType::Register(5)],
        },
        Instruction {
            opcode: Opcode::SFindRRR,
            operands: vec![OperandType::Register(0), OperandType::Register(5), OperandType::Register(6)],
        },
        Instruction {
            opcode: Opcode::SEqRRR,
            operands: vec![OperandType::Register(0), OperandType::Register(2), OperandType::Register(7)],
        },
        Instruction {
            opcode: Opcode::Ret,
            operands: vec![OperandType::Register(7)],
        },
    ];

    let main_func = Function::new("main".to_string(), instructions, 8, 0, FunctionType::Regular);
    let bc = Bytecode {
        functions: vec![main_func],
        entry_point: "main".to_string(),
        native_declarations: Vec::new(),
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bc).expect("init vm");
    let res = vm.run();
    assert!(res.is_ok(), "VM execution failed: {:?}", res);
}

#[test]
fn test_auto_vectorization_optimization_pass() {
    use std::sync::{Arc, RwLock};
    use vmm::bytecode::instruction::{Bytecode, Instruction, OperandType};
    use vmm::bytecode::opcode::Opcode;
    use vmm::lang::types::{Function, FunctionType, Value};
    use vmm::vm::optimizer::optimize_function;

    let v_arr0 = Value::FloatArray(Arc::new(RwLock::new(vec![1.0, 2.0])));
    let v_arr1 = Value::FloatArray(Arc::new(RwLock::new(vec![3.0, 4.0])));

    // Using generic Add and Mul instructions on FloatArray registers
    let mut main_func = Function::new(
        "main".to_string(),
        vec![
            Instruction {
                opcode: Opcode::LoadConst,
                operands: vec![OperandType::Constant(v_arr0), OperandType::Register(0)],
            },
            Instruction {
                opcode: Opcode::LoadConst,
                operands: vec![OperandType::Constant(v_arr1), OperandType::Register(1)],
            },
            Instruction {
                opcode: Opcode::Add, // generic Add!
                operands: vec![OperandType::Register(0), OperandType::Register(1), OperandType::Register(2)],
            },
            Instruction {
                opcode: Opcode::Mul, // generic Mul!
                operands: vec![OperandType::Register(0), OperandType::Register(1), OperandType::Register(3)],
            },
            Instruction {
                opcode: Opcode::Ret,
                operands: vec![OperandType::Register(2)],
            },
        ],
        4,
        0,
        FunctionType::Regular,
    );

    // Run the optimizer pass!
    optimize_function(&mut main_func);

    // Verify auto-vectorization!
    assert_eq!(main_func.instructions[2].opcode, Opcode::VAddF, "Add should be auto-vectorized to VAddF");
    assert_eq!(main_func.instructions[3].opcode, Opcode::VMulF, "Mul should be auto-vectorized to VMulF");

    let bc = Bytecode {
        functions: vec![main_func],
        entry_point: "main".to_string(),
        native_declarations: Vec::new(),
        object_inits: vec![],
    };
    let mut vm = IonVM::new(bc).expect("init vm");
    let res = vm.run();
    assert!(res.is_ok(), "Optimized VM execution succeeded");
}

#[test]
fn test_is_munchausen_single() {
    use std::sync::{Arc, RwLock};
    use vmm::bytecode::instruction::{Bytecode, Instruction, OperandType};
    use vmm::bytecode::opcode::Opcode;
    use vmm::lang::types::{Function, FunctionType, Value};

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
    let is_munch = Function::new("is_munchausen".to_string(), instrs, 5, 2, FunctionType::Regular);

    let powers = vec![0i64, 1, 4, 27, 256, 3125, 46656, 823543, 16777216, 387420489];
    let main_instrs = vec![
        Instruction::new(Opcode::LoadConst, vec![
            OperandType::Constant(Value::IntArray(Arc::new(RwLock::new(powers)))),
            OperandType::Register(0),
        ]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(3435)), OperandType::Register(1)]),
        Instruction::new(Opcode::Call, vec![
            OperandType::Function("is_munchausen".to_string()),
            OperandType::Offset(2),
            OperandType::Register(1),
            OperandType::Register(0),
            OperandType::Register(2),
        ]),
        Instruction::new(Opcode::Call, vec![
            OperandType::Function("println".to_string()),
            OperandType::Offset(1),
            OperandType::Register(2),
            OperandType::Register(3),
        ]),
        Instruction::new(Opcode::Ret, vec![OperandType::Register(2)]),
    ];
    let main_fn = Function::new("main".to_string(), main_instrs, 5, 0, FunctionType::Regular);

    let bc = Bytecode {
        functions: vec![is_munch, main_fn],
        entry_point: "main".to_string(),
        native_declarations: Vec::new(),
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bc).unwrap();
    let res = vm.run();
    assert!(res.is_ok());
}

#[test]
fn test_w0() {
    use std::sync::{Arc, RwLock};
    use vmm::bytecode::instruction::{Bytecode, Instruction, OperandType};
    use vmm::bytecode::opcode::Opcode;
    use vmm::lang::types::{Function, FunctionType, Value};

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
    let is_munch = Function::new("is_munchausen".to_string(), instrs, 5, 2, FunctionType::Regular);

    let worker_instrs = vec![
        Instruction::new(Opcode::LoadConst, vec![OperandType::Argument(0), OperandType::Register(1)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Argument(1), OperandType::Register(4)]),
        Instruction::new(Opcode::Lt, vec![OperandType::Register(1), OperandType::Register(4), OperandType::Register(5)]),
        Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(5), OperandType::Offset(6)]),
        Instruction::new(Opcode::Call, vec![OperandType::Function("is_munchausen".to_string()), OperandType::Offset(2), OperandType::Register(1), OperandType::Argument(2), OperandType::Register(6)]),
        Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(6), OperandType::Offset(2)]),
        Instruction::new(Opcode::Call, vec![OperandType::Function("println".to_string()), OperandType::Offset(1), OperandType::Register(1), OperandType::Register(7)]),
        Instruction::new(Opcode::Add, vec![OperandType::Register(1), OperandType::Constant(Value::Int(1)), OperandType::Register(1)]),
        Instruction::new(Opcode::Jump, vec![OperandType::Offset(-6)]),
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Nil)]),
    ];
    let worker_fn = Function::new("worker_fn".to_string(), worker_instrs, 8, 3, FunctionType::Regular);

    let powers = vec![0i64, 1, 4, 27, 256, 3125, 46656, 823543, 16777216, 387420489];
    let main_instrs = vec![
        Instruction::new(Opcode::LoadConst, vec![
            OperandType::Constant(Value::IntArray(Arc::new(RwLock::new(powers)))),
            OperandType::Register(0),
        ]),
        Instruction::new(Opcode::Call, vec![
            OperandType::Function("worker_fn".to_string()),
            OperandType::Offset(3),
            OperandType::Constant(Value::Int(0)),
            OperandType::Constant(Value::Int(10000)),
            OperandType::Register(0),
            OperandType::Register(1),
        ]),
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Nil)]),
    ];
    let main_fn = Function::new("main".to_string(), main_instrs, 5, 0, FunctionType::Regular);

    let bc = Bytecode {
        functions: vec![is_munch, worker_fn, main_fn],
        entry_point: "main".to_string(),
        native_declarations: Vec::new(),
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bc).unwrap();
    let res = vm.run();
    assert!(res.is_ok());
}

#[test]
fn test_loop_to_simd_vectorization() {
    use vmm::bytecode::instruction::{Bytecode, Instruction, OperandType};
    use vmm::bytecode::opcode::Opcode;
    use vmm::lang::types::{Function, FunctionType, Value};
    use vmm::vm::optimizer::optimize_function;
    use vmm::vm::IonVM;

    let loop_instrs = vec![
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(1)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(10)), OperandType::Register(2)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(0)]),
        // Loop head (pc = 3):
        Instruction::new(Opcode::Lt, vec![OperandType::Register(1), OperandType::Register(2), OperandType::Register(3)]),
        Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(3), OperandType::Offset(5)]),
        Instruction::new(Opcode::Mul, vec![OperandType::Register(1), OperandType::Register(1), OperandType::Register(4)]),
        Instruction::new(Opcode::Add, vec![OperandType::Register(0), OperandType::Register(4), OperandType::Register(0)]),
        Instruction::new(Opcode::Add, vec![OperandType::Register(1), OperandType::Constant(Value::Int(1)), OperandType::Register(1)]),
        Instruction::new(Opcode::Jump, vec![OperandType::Offset(-5)]),
        // Exit (pc = 9):
        Instruction::new(Opcode::Ret, vec![OperandType::Register(0)]),
    ];

    let mut func = Function::new("main".to_string(), loop_instrs, 5, 0, FunctionType::Regular);
    assert_eq!(func.instructions.len(), 10);

    optimize_function(&mut func);
    assert!(func.instructions.len() > 10, "Loop must be vectorized to SIMD width");

    let bc = Bytecode {
        functions: vec![func],
        entry_point: "main".to_string(),
        native_declarations: Vec::new(),
        object_inits: vec![],
    };
    let mut vm = IonVM::new(bc).unwrap();
    let res = vm.run();
    assert!(res.is_ok(), "Vectorized loop execution failed: {:?}", res);
}

#[test]
fn test_ffi_standard_library_execution() {
    use vmm::bytecode::instruction::{Bytecode, Instruction, OperandType};
    use vmm::bytecode::opcode::Opcode;
    use vmm::lang::types::{Function, FunctionType, Value};
    use vmm::vm::IonVM;

    let instructions = vec![
        // 1. Math: Sqrt(16.0) -> Reg(1)
        Instruction::new(
            Opcode::LoadConst,
            vec![OperandType::Constant(Value::Float(16.0)), OperandType::Register(0)],
        ),
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("Sqrt".to_string()),
                OperandType::Offset(1),
                OperandType::Register(0),
                OperandType::Register(1),
            ],
        ),
        // 2. Native SIMD string uppercase opcode: SUpperRR
        Instruction::new(
            Opcode::LoadConst,
            vec![OperandType::Constant(Value::String("hello ionvm".to_string())), OperandType::Register(2)],
        ),
        Instruction::new(
            Opcode::SUpperRR,
            vec![OperandType::Register(2), OperandType::Register(3)],
        ),
        // 3. Math FFI: Cos(0.0) -> Reg(4)
        Instruction::new(
            Opcode::LoadConst,
            vec![OperandType::Constant(Value::Float(0.0)), OperandType::Register(4)],
        ),
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("Cos".to_string()),
                OperandType::Offset(1),
                OperandType::Register(4),
                OperandType::Register(4),
            ],
        ),
        // 4. Hash: Crc32("123456789") -> Reg(6)
        Instruction::new(
            Opcode::LoadConst,
            vec![OperandType::Constant(Value::String("123456789".to_string())), OperandType::Register(5)],
        ),
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("Crc32".to_string()),
                OperandType::Offset(1),
                OperandType::Register(5),
                OperandType::Register(6),
            ],
        ),
        // 5. OS: OsName() -> Reg(7)
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("OsName".to_string()),
                OperandType::Offset(0),
                OperandType::Register(7),
            ],
        ),
        Instruction::new(Opcode::Ret, vec![OperandType::Register(7)]),
    ];

    let main_func = Function::new("main".to_string(), instructions, 8, 0, FunctionType::Regular);
    let bc = Bytecode {
        functions: vec![main_func],
        entry_point: "main".to_string(),
        native_declarations: Vec::new(),
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bc).unwrap();
    let res = vm.run();
    assert!(res.is_ok(), "FFI execution in IonVM failed: {:?}", res);
}

#[test]
fn test_bitwise_and_shift_execution() {
    use std::sync::{Arc, RwLock};
    let out = Arc::new(RwLock::new(vec![0i64]));
    // Tests Shl, Shr, And, Or, Xor, Not on integers
    let instructions = vec![
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::IntArray(Arc::clone(&out))), OperandType::Register(15)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(1)), OperandType::Register(0)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(5)), OperandType::Register(1)]),
        // r2 = 1 << 5 = 32
        Instruction::new(Opcode::Shl, vec![OperandType::Register(0), OperandType::Register(1), OperandType::Register(2)]),

        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(64)), OperandType::Register(3)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(2)), OperandType::Register(4)]),
        // r5 = 64 >> 2 = 16
        Instruction::new(Opcode::Shr, vec![OperandType::Register(3), OperandType::Register(4), OperandType::Register(5)]),

        // r6 = 12 & 10 = 8
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(12)), OperandType::Register(6)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(10)), OperandType::Register(7)]),
        Instruction::new(Opcode::And, vec![OperandType::Register(6), OperandType::Register(7), OperandType::Register(8)]),

        // r9 = 12 | 3 = 15
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(3)), OperandType::Register(9)]),
        Instruction::new(Opcode::Or, vec![OperandType::Register(6), OperandType::Register(9), OperandType::Register(10)]),

        // r11 = 12 ^ 10 = 6
        Instruction::new(Opcode::Xor, vec![OperandType::Register(6), OperandType::Register(7), OperandType::Register(11)]),

        // r12 = !0 = -1
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(12)]),
        Instruction::new(Opcode::Not, vec![OperandType::Register(12), OperandType::Register(13)]),

        // Return r2 + r5 + r8 + r10 + r11 = 32 + 16 + 8 + 15 + 6 = 77
        Instruction::new(Opcode::Add, vec![OperandType::Register(2), OperandType::Register(5), OperandType::Register(14)]),
        Instruction::new(Opcode::Add, vec![OperandType::Register(14), OperandType::Register(8), OperandType::Register(14)]),
        Instruction::new(Opcode::Add, vec![OperandType::Register(14), OperandType::Register(10), OperandType::Register(14)]),
        Instruction::new(Opcode::Add, vec![OperandType::Register(14), OperandType::Register(11), OperandType::Register(14)]),
        // out[0] = r14
        Instruction::new(Opcode::IArraySet, vec![OperandType::Register(15), OperandType::Constant(Value::Int(0)), OperandType::Register(14)]),
        Instruction::new(Opcode::Ret, vec![OperandType::Register(14)]),
    ];

    let main_func = Function::new("main".to_string(), instructions, 16, 0, FunctionType::Regular);
    let bc = Bytecode {
        functions: vec![main_func],
        entry_point: "main".to_string(),
        native_declarations: Vec::new(),
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bc).unwrap();
    let res = vm.run();
    assert!(res.is_ok());
    assert_eq!(out.read().unwrap()[0], 77);
}

#[test]
fn test_bitwise_and_shift_jit() {
    use std::sync::{Arc, RwLock};
    let out = Arc::new(RwLock::new(vec![0i64]));
    // Tests that bitwise & shift functions compile in LLVM JIT and produce correct results
    let worker_instructions = vec![
        // arg0 << arg1
        Instruction::new(Opcode::IShlRRR, vec![OperandType::Argument(0), OperandType::Argument(1), OperandType::Register(0)]),
        // (arg0 << arg1) >> 2
        Instruction::new(Opcode::IShrRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(2)), OperandType::Register(1)]),
        // r1 & 0xFF
        Instruction::new(Opcode::IAndRCR, vec![OperandType::Register(1), OperandType::Constant(Value::Int(0xFF)), OperandType::Register(2)]),
        // r2 | 0x100
        Instruction::new(Opcode::IOrRCR, vec![OperandType::Register(2), OperandType::Constant(Value::Int(0x100)), OperandType::Register(3)]),
        // r3 ^ 0x101
        Instruction::new(Opcode::IXorRCR, vec![OperandType::Register(3), OperandType::Constant(Value::Int(0x101)), OperandType::Register(4)]),
        Instruction::new(Opcode::Ret, vec![OperandType::Register(4)]),
    ];

    let worker_func = Function::new("bit_worker".to_string(), worker_instructions, 6, 2, FunctionType::Regular);

    let main_instructions = vec![
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::IntArray(Arc::clone(&out))), OperandType::Register(3)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(7)), OperandType::Register(0)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(3)), OperandType::Register(1)]),
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("bit_worker".to_string()),
                OperandType::Offset(2),
                OperandType::Register(0),
                OperandType::Register(1),
                OperandType::Register(2),
            ],
        ),
        Instruction::new(Opcode::IArraySet, vec![OperandType::Register(3), OperandType::Constant(Value::Int(0)), OperandType::Register(2)]),
        Instruction::new(Opcode::Ret, vec![OperandType::Register(2)]),
    ];

    let main_func = Function::new("main".to_string(), main_instructions, 5, 0, FunctionType::Regular);

    let bc = Bytecode {
        functions: vec![worker_func, main_func],
        entry_point: "main".to_string(),
        native_declarations: Vec::new(),
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bc).unwrap();
    // Verify worker function was JIT compiled
    assert!(vm.scheduler.bytecode.functions[0].jit_compiled_fn.is_some(), "bit_worker must be JIT compiled!");

    let res = vm.run();
    assert!(res.is_ok());
    assert_eq!(out.read().unwrap()[0], 15);
}

#[test]
fn test_sieve_bitpacked_jit() {
    let worker_instructions = vec![
        // 0: r6 = 1
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(1)), OperandType::Register(6)]),
        // 1: r5 = 3
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(3)), OperandType::Register(5)]),
        // 2: flags[0] = 3 (composite 0 and 1)
        Instruction::new(Opcode::IArraySet, vec![OperandType::Argument(1), OperandType::Constant(Value::Int(0)), OperandType::Register(5)]),
        // 3: r0 (p) = 2
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(2)), OperandType::Register(0)]),

        // 4: Loop head: r2 = p * p
        Instruction::new(Opcode::IMulRRR, vec![OperandType::Register(0), OperandType::Register(0), OperandType::Register(2)]),
        // 5: if p * p > limit, jump to counting phase at index 24 (5 + 19 = 24)
        Instruction::new(Opcode::IGtJmpRR, vec![OperandType::Register(2), OperandType::Argument(0), OperandType::Offset(19)]),
        // 6: r3 = p >> 6
        Instruction::new(Opcode::IShrRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(6)), OperandType::Register(3)]),
        // 7: r4 = p & 63
        Instruction::new(Opcode::IAndRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(63)), OperandType::Register(4)]),
        // 8: r5 = flags[r3]
        Instruction::new(Opcode::IArrayGet, vec![OperandType::Argument(1), OperandType::Register(3), OperandType::Register(5)]),
        // 9: r5 = r5 >> r4
        Instruction::new(Opcode::IShrRRR, vec![OperandType::Register(5), OperandType::Register(4), OperandType::Register(5)]),
        // 10: r5 = r5 & 1
        Instruction::new(Opcode::IAndRCR, vec![OperandType::Register(5), OperandType::Constant(Value::Int(1)), OperandType::Register(5)]),
        // 11: if r5 > 0, jump to outer loop increment at index 22 (11 + 11 = 22)
        Instruction::new(Opcode::IGtJmpRC, vec![OperandType::Register(5), OperandType::Constant(Value::Int(0)), OperandType::Offset(11)]),

        // 12: inner loop setup: r1 (mult) = p * p (in r2)
        Instruction::new(Opcode::LoadConst, vec![OperandType::Register(2), OperandType::Register(1)]),
        // 13: inner loop head: if mult > limit, jump to outer loop increment at index 22 (13 + 9 = 22)
        Instruction::new(Opcode::IGtJmpRR, vec![OperandType::Register(1), OperandType::Argument(0), OperandType::Offset(9)]),
        // 14: r3 = mult >> 6
        Instruction::new(Opcode::IShrRCR, vec![OperandType::Register(1), OperandType::Constant(Value::Int(6)), OperandType::Register(3)]),
        // 15: r4 = mult & 63
        Instruction::new(Opcode::IAndRCR, vec![OperandType::Register(1), OperandType::Constant(Value::Int(63)), OperandType::Register(4)]),
        // 16: r5 = 1 << bit_pos (r6 << r4)
        Instruction::new(Opcode::IShlRRR, vec![OperandType::Register(6), OperandType::Register(4), OperandType::Register(5)]),
        // 17: r11 = flags[word_idx]
        Instruction::new(Opcode::IArrayGet, vec![OperandType::Argument(1), OperandType::Register(3), OperandType::Register(11)]),
        // 18: r11 = r11 | r5
        Instruction::new(Opcode::IOrRRR, vec![OperandType::Register(11), OperandType::Register(5), OperandType::Register(11)]),
        // 19: flags[word_idx] = r11
        Instruction::new(Opcode::IArraySet, vec![OperandType::Argument(1), OperandType::Register(3), OperandType::Register(11)]),
        // 20: mult = mult + p
        Instruction::new(Opcode::IAddRRR, vec![OperandType::Register(1), OperandType::Register(0), OperandType::Register(1)]),
        // 21: jump back to inner loop head at index 13 (21 - 8 = 13)
        Instruction::new(Opcode::Jump, vec![OperandType::Offset(-8)]),

        // 22: outer loop increment: p = p + 1
        Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(0)]),
        // 23: jump back to outer loop head at index 4 (23 - 19 = 4)
        Instruction::new(Opcode::Jump, vec![OperandType::Offset(-19)]),

        // 24: counting phase: r9 (count) = 0
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(9)]),
        // 25: r10 (i) = 2
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(2)), OperandType::Register(10)]),
        // 26: count loop head: if i > limit, jump to ret at index 36 (26 + 10 = 36)
        Instruction::new(Opcode::IGtJmpRR, vec![OperandType::Register(10), OperandType::Argument(0), OperandType::Offset(10)]),
        // 27: r3 = i >> 6
        Instruction::new(Opcode::IShrRCR, vec![OperandType::Register(10), OperandType::Constant(Value::Int(6)), OperandType::Register(3)]),
        // 28: r4 = i & 63
        Instruction::new(Opcode::IAndRCR, vec![OperandType::Register(10), OperandType::Constant(Value::Int(63)), OperandType::Register(4)]),
        // 29: r5 = flags[word_idx]
        Instruction::new(Opcode::IArrayGet, vec![OperandType::Argument(1), OperandType::Register(3), OperandType::Register(5)]),
        // 30: r5 = r5 >> r4
        Instruction::new(Opcode::IShrRRR, vec![OperandType::Register(5), OperandType::Register(4), OperandType::Register(5)]),
        // 31: r5 = r5 & 1
        Instruction::new(Opcode::IAndRCR, vec![OperandType::Register(5), OperandType::Constant(Value::Int(1)), OperandType::Register(5)]),
        // 32: if bit > 0, jump to count step at index 34 (32 + 2 = 34)
        Instruction::new(Opcode::IGtJmpRC, vec![OperandType::Register(5), OperandType::Constant(Value::Int(0)), OperandType::Offset(2)]),
        // 33: count = count + 1
        Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(9), OperandType::Constant(Value::Int(1)), OperandType::Register(9)]),
        // 34: i = i + 1
        Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(10), OperandType::Constant(Value::Int(1)), OperandType::Register(10)]),
        // 35: jump back to count loop head at index 26 (35 - 9 = 26)
        Instruction::new(Opcode::Jump, vec![OperandType::Offset(-9)]),

        // 36: ret count
        Instruction::new(Opcode::Ret, vec![OperandType::Register(9)]),
    ];

    let sieve_func = Function::new("sieve_kernel".to_string(), worker_instructions, 16, 2, FunctionType::Regular);

    let limit = 10_000i64; // primes <= 10,000 is 1229
    let num_words = (limit >> 6) + 1;
    let flags = Arc::new(RwLock::new(vec![0i64; num_words as usize]));

    let main_instructions = vec![
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(limit)), OperandType::Register(0)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::IntArray(Arc::clone(&flags))), OperandType::Register(1)]),
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("sieve_kernel".to_string()),
                OperandType::Offset(2),
                OperandType::Register(0),
                OperandType::Register(1),
                OperandType::Register(2),
            ],
        ),
        Instruction::new(Opcode::Ret, vec![OperandType::Register(2)]),
    ];

    let main_func = Function::new("main".to_string(), main_instructions, 5, 0, FunctionType::Regular);

    let bc = Bytecode {
        functions: vec![sieve_func, main_func],
        entry_point: "main".to_string(),
        native_declarations: Vec::new(),
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bc).unwrap();
    assert!(vm.scheduler.bytecode.functions[0].jit_compiled_fn.is_some(), "sieve_kernel must be JIT compiled!");

    let res = vm.run();
    assert!(res.is_ok());

    // Also test JIT stub directly with limit = 10_000_000
    let (jit_fn, _) = compile_function_with_ir(&vm.scheduler.bytecode.functions[0]).unwrap();
    let big_limit = 10_000_000i64;
    let big_words = (big_limit >> 6) + 1;
    let big_flags = Arc::new(RwLock::new(vec![0i64; big_words as usize]));
    let mut registers = vec![Value::Nil; 16];
    let args = vec![Value::Int(big_limit), Value::IntArray(Arc::clone(&big_flags))];
    let mut result = Value::Nil;
    let undefined_self: *mut Value = std::ptr::null_mut();

    unsafe {
        jit_fn(registers.as_mut_ptr(), undefined_self, args.as_ptr(), std::ptr::null_mut(), &mut result as *mut Value);
    }
    match result {
        Value::Int(count) => assert_eq!(count, 664579, "Primes up to 10M must be exactly 664,579!"),
        other => panic!("Expected Int result, got {:?}", other),
    }
}

#[test]
fn test_fused_bit_opcodes_jit() {
    let worker_instructions = vec![
        // IBitSet1 arr, 3
        Instruction::new(Opcode::IBitSet1, vec![OperandType::Argument(0), OperandType::Constant(Value::Int(3))]),
        // IBitSet1 arr, 67 (word 1, bit 3)
        Instruction::new(Opcode::IBitSet1, vec![OperandType::Argument(0), OperandType::Constant(Value::Int(67))]),
        // IBitGet arr, 3 -> r0 (expect 1)
        Instruction::new(Opcode::IBitGet, vec![OperandType::Argument(0), OperandType::Constant(Value::Int(3)), OperandType::Register(0)]),
        // IBitGet arr, 2 -> r1 (expect 0)
        Instruction::new(Opcode::IBitGet, vec![OperandType::Argument(0), OperandType::Constant(Value::Int(2)), OperandType::Register(1)]),
        // IBitGet arr, 67 -> r2 (expect 1)
        Instruction::new(Opcode::IBitGet, vec![OperandType::Argument(0), OperandType::Constant(Value::Int(67)), OperandType::Register(2)]),
        // IBitSet0 arr, 3
        Instruction::new(Opcode::IBitSet0, vec![OperandType::Argument(0), OperandType::Constant(Value::Int(3))]),
        // IBitGet arr, 3 -> r3 (expect 0)
        Instruction::new(Opcode::IBitGet, vec![OperandType::Argument(0), OperandType::Constant(Value::Int(3)), OperandType::Register(3)]),
        // IBitSet arr, 10, 1
        Instruction::new(Opcode::IBitSet, vec![OperandType::Argument(0), OperandType::Constant(Value::Int(10)), OperandType::Constant(Value::Int(1))]),
        // IBitGet arr, 10 -> r4 (expect 1)
        Instruction::new(Opcode::IBitGet, vec![OperandType::Argument(0), OperandType::Constant(Value::Int(10)), OperandType::Register(4)]),
        // IBitSet arr, 10, 0
        Instruction::new(Opcode::IBitSet, vec![OperandType::Argument(0), OperandType::Constant(Value::Int(10)), OperandType::Constant(Value::Int(0))]),
        // IBitGet arr, 10 -> r5 (expect 0)
        Instruction::new(Opcode::IBitGet, vec![OperandType::Argument(0), OperandType::Constant(Value::Int(10)), OperandType::Register(5)]),
        // Sum up: r0 + r1 + r2 + r3 + r4 + r5 = 1 + 0 + 1 + 0 + 1 + 0 = 3
        Instruction::new(Opcode::IAddRRR, vec![OperandType::Register(0), OperandType::Register(1), OperandType::Register(6)]),
        Instruction::new(Opcode::IAddRRR, vec![OperandType::Register(6), OperandType::Register(2), OperandType::Register(6)]),
        Instruction::new(Opcode::IAddRRR, vec![OperandType::Register(6), OperandType::Register(3), OperandType::Register(6)]),
        Instruction::new(Opcode::IAddRRR, vec![OperandType::Register(6), OperandType::Register(4), OperandType::Register(6)]),
        Instruction::new(Opcode::IAddRRR, vec![OperandType::Register(6), OperandType::Register(5), OperandType::Register(6)]),
        Instruction::new(Opcode::Ret, vec![OperandType::Register(6)]),
    ];

    let bit_fn = Function::new("bit_fn".to_string(), worker_instructions, 8, 1, FunctionType::Regular);

    let arr = Arc::new(RwLock::new(vec![0i64; 4]));
    let main_instructions = vec![
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::IntArray(Arc::clone(&arr))), OperandType::Register(0)]),
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("bit_fn".to_string()),
                OperandType::Offset(1),
                OperandType::Register(0),
                OperandType::Register(1),
            ],
        ),
        Instruction::new(Opcode::Ret, vec![OperandType::Register(1)]),
    ];

    let main_fn = Function::new("main".to_string(), main_instructions, 3, 0, FunctionType::Regular);

    let bc = Bytecode {
        functions: vec![bit_fn, main_fn],
        entry_point: "main".to_string(),
        native_declarations: Vec::new(),
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bc).unwrap();
    assert!(vm.scheduler.bytecode.functions[0].jit_compiled_fn.is_some(), "bit_fn must be JIT-compiled");

    let res = vm.run();
    assert!(res.is_ok());

    // Check array state:
    // bit 3 was set then cleared -> 0
    // bit 67 was set -> word 1 should have 1 << 3 = 8
    // bit 10 was set then cleared -> 0
    let borrowed = arr.read().unwrap();
    assert_eq!(borrowed[0], 0, "Word 0 should be 0");
    assert_eq!(borrowed[1], 8, "Word 1 should have bit 3 set (value 8)");
}

#[test]
fn test_sieve_fused_bit_opcodes_jit() {
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

    let sieve_fn = Function::new("sieve_fused".to_string(), instrs, 8, 2, FunctionType::Regular);

    // Direct test with 10_000_000
    let (jit_fn, _) = compile_function_with_ir(&sieve_fn).expect("compile sieve_fused");
    let limit = 10_000_000i64;
    let num_words = (limit >> 6) + 1;
    let flags = Arc::new(RwLock::new(vec![0i64; num_words as usize]));
    let mut registers = vec![Value::Nil; 8];
    let args = vec![Value::Int(limit), Value::IntArray(Arc::clone(&flags))];
    let mut result = Value::Nil;
    let undefined_self: *mut Value = std::ptr::null_mut();

    unsafe {
        jit_fn(registers.as_mut_ptr(), undefined_self, args.as_ptr(), std::ptr::null_mut(), &mut result as *mut Value);
    }

    match result {
        Value::Int(count) => assert_eq!(count, 664579, "Primes up to 10M must be 664,579!"),
        other => panic!("Expected Int result, got {:?}", other),
    }
}

#[test]
fn test_nvptx_target_support() {
    use inkwell::targets::{InitializationConfig, Target, TargetTriple};
    Target::initialize_nvptx(&InitializationConfig::default());
    let triple = TargetTriple::create("nvptx64-nvidia-cuda");
    let target = Target::from_triple(&triple);
    assert!(target.is_ok(), "NVPTX target should be supported");
    let target = target.unwrap();
    let machine = target.create_target_machine(
        &triple,
        "sm_89",
        "+ptx80",
        inkwell::OptimizationLevel::Aggressive,
        inkwell::targets::RelocMode::Default,
        inkwell::targets::CodeModel::Default,
    );
    assert!(machine.is_some(), "NVPTX TargetMachine for sm_89 should be created");
    let target_machine = machine.unwrap();

    let context = inkwell::context::Context::create();
    let module = context.create_module("test_cuda");
    let void_type = context.void_type();
    let fn_type = void_type.fn_type(&[], false);
    let function = module.add_function("kernel_noop", fn_type, None);
    function.set_call_conventions(71);
    let basic_block = context.append_basic_block(function, "entry");
    let builder = context.create_builder();
    builder.position_at_end(basic_block);
    builder.build_return(None).unwrap();

    let buffer = target_machine.write_to_memory_buffer(&module, inkwell::targets::FileType::Assembly).expect("PTX emission failed");
    let ptx_code = std::str::from_utf8(buffer.as_slice()).unwrap();
    assert!(ptx_code.contains(".entry kernel_noop"));
}

#[test]
fn test_cuda_kernel_execution() {
    use vmm::jit::cuda::*;
    use inkwell::context::Context;
    use inkwell::AddressSpace;
    use std::ffi::c_void;

    if !is_cuda_available() {
        eprintln!("CUDA not available on this machine, skipping");
        return;
    }

    let dev_info = get_cuda_device_info(0).expect("Get CUDA device info");
    println!("Testing CUDA on device: {} ({}), VRAM: {} MB", dev_info.name, dev_info.arch, dev_info.total_memory / (1024 * 1024));

    let context = Context::create();
    let module = context.create_module("gpu_test");
    let builder = context.create_builder();

    let i32_type = context.i32_type();
    let i64_type = context.i64_type();
    let void_type = context.void_type();
    let ptr_type = context.ptr_type(AddressSpace::from(1)); // global memory

    // NVVM Intrinsics
    let tid_fn = module.add_function("llvm.nvvm.read.ptx.sreg.tid.x", i32_type.fn_type(&[], false), None);
    let ctaid_fn = module.add_function("llvm.nvvm.read.ptx.sreg.ctaid.x", i32_type.fn_type(&[], false), None);
    let ntid_fn = module.add_function("llvm.nvvm.read.ptx.sreg.ntid.x", i32_type.fn_type(&[], false), None);

    // Kernel fn: void add_42_kernel(i64* arr, i64 n)
    let kernel_fn_type = void_type.fn_type(&[ptr_type.into(), i64_type.into()], false);
    let kernel_fn = module.add_function("add_42_kernel", kernel_fn_type, None);
    kernel_fn.set_call_conventions(71); // PTX_Kernel -> .entry

    let entry_bb = context.append_basic_block(kernel_fn, "entry");
    let body_bb = context.append_basic_block(kernel_fn, "body");
    let exit_bb = context.append_basic_block(kernel_fn, "exit");

    builder.position_at_end(entry_bb);
    let tid = match builder.build_call(tid_fn, &[], "tid").unwrap().try_as_basic_value() {
        inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
        _ => panic!(),
    };
    let ctaid = match builder.build_call(ctaid_fn, &[], "ctaid").unwrap().try_as_basic_value() {
        inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
        _ => panic!(),
    };
    let ntid = match builder.build_call(ntid_fn, &[], "ntid").unwrap().try_as_basic_value() {
        inkwell::values::ValueKind::Basic(b) => b.into_int_value(),
        _ => panic!(),
    };

    let block_offset = builder.build_int_mul(ctaid, ntid, "block_off").unwrap();
    let global_id_32 = builder.build_int_add(block_offset, tid, "gid32").unwrap();
    let global_id = builder.build_int_z_extend(global_id_32, i64_type, "gid").unwrap();

    let arr_param = kernel_fn.get_nth_param(0).unwrap().into_pointer_value();
    let n_param = kernel_fn.get_nth_param(1).unwrap().into_int_value();

    let cmp = builder.build_int_compare(inkwell::IntPredicate::ULT, global_id, n_param, "in_bounds").unwrap();
    builder.build_conditional_branch(cmp, body_bb, exit_bb).unwrap();

    // Body BB: arr[global_id] += 42
    builder.position_at_end(body_bb);
    let elem_ptr = unsafe { builder.build_gep(i64_type, arr_param, &[global_id], "elem_ptr").unwrap() };
    let cur_val = builder.build_load(i64_type, elem_ptr, "cur_val").unwrap().into_int_value();
    let new_val = builder.build_int_add(cur_val, i64_type.const_int(42, false), "new_val").unwrap();
    builder.build_store(elem_ptr, new_val).unwrap();
    builder.build_unconditional_branch(exit_bb).unwrap();

    // Exit BB: ret
    builder.position_at_end(exit_bb);
    builder.build_return(None).unwrap();

    // Emit PTX
    let ptx = emit_ptx_from_module(&module, &dev_info.arch).expect("Emit PTX failed");

    // Run on GPU!
    let cuda_ctx = CudaContext::new(0).expect("Create CUDA context");
    let cuda_mod = cuda_ctx.load_ptx(&ptx).expect("Load PTX module");
    let kernel = cuda_mod.get_function("add_42_kernel").expect("Get kernel function");

    let n_elems = 1024usize;
    let mut host_vec: Vec<i64> = (0..n_elems as i64).collect();
    let byte_size = n_elems * std::mem::size_of::<i64>();

    let d_arr = cuda_ctx.allocate_device_memory(byte_size).expect("Alloc GPU mem");
    cuda_ctx.copy_to_device(d_arr, unsafe { std::slice::from_raw_parts(host_vec.as_ptr() as *const u8, byte_size) }).expect("Copy HtoD");

    let mut n_param_val = n_elems as i64;
    let mut mut_d_arr = d_arr;
    let mut args: [*mut c_void; 2] = [
        &mut mut_d_arr as *mut _ as *mut c_void,
        &mut n_param_val as *mut _ as *mut c_void,
    ];

    unsafe {
        launch_kernel(kernel, (4, 1, 1), (256, 1, 1), 0, &mut args).expect("Launch kernel");
    }

    cuda_ctx.synchronize().expect("Sync");
    cuda_ctx.copy_to_host(unsafe { std::slice::from_raw_parts_mut(host_vec.as_mut_ptr() as *mut u8, byte_size) }, d_arr).expect("Copy DtoH");
    cuda_ctx.free_device_memory(d_arr);

    assert_eq!(host_vec[0], 42);
    assert_eq!(host_vec[100], 142);
    assert_eq!(host_vec[1023], 1065);
}

#[test]
fn test_ffi_in_spawned_processes() {
    let worker_instrs = vec![
        // r0 = arg0 (parent pid)
        Instruction::new(Opcode::LoadConst, vec![OperandType::Argument(0), OperandType::Register(0)]),
        // r1 = arg1 (input: float 144.0)
        Instruction::new(Opcode::LoadConst, vec![OperandType::Argument(1), OperandType::Register(1)]),
        // r2 = Sqrt(r1) -> calls FFI function Sqrt from stdlib in spawned worker process
        Instruction::new(Opcode::Call, vec![OperandType::Function("Sqrt".to_string()), OperandType::Offset(1), OperandType::Register(1), OperandType::Register(2)]),
        // send(parent_pid: r0, msg: r2)
        Instruction::new(Opcode::Send, vec![OperandType::Register(0), OperandType::Register(2)]),
        // ret r2
        Instruction::new(Opcode::Ret, vec![OperandType::Register(2)]),
    ];
    let worker_fn = Function::new("worker_fn".to_string(), worker_instrs, 10, 2, FunctionType::Regular);

    let num_workers = 8;
    let mut main_instrs = vec![
        // r0 = SelfProc
        Instruction::new(Opcode::LoadConst, vec![OperandType::SelfProc, OperandType::Register(0)]),
        // r4 = sum (0.0)
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(0.0)), OperandType::Register(4)]),
    ];

    // Spawn num_workers across worker threads
    for i in 0..num_workers {
        main_instrs.push(Instruction::new(
            Opcode::Spawn,
            vec![
                OperandType::Function("worker_fn".to_string()),
                OperandType::Offset(2),
                OperandType::Register(0),
                OperandType::Constant(Value::Float(144.0)),
                OperandType::Register(10 + i),
            ],
        ));
    }

    // Collect results from all spawned processes
    for _ in 0..num_workers {
        main_instrs.push(Instruction::new(Opcode::Receive, vec![OperandType::Register(2)]));
        main_instrs.push(Instruction::new(
            Opcode::Add,
            vec![
                OperandType::Register(4),
                OperandType::Register(2),
                OperandType::Register(4),
            ],
        ));
    }

    // Check sum == 96.0 (8 * 12.0)
    main_instrs.push(Instruction::new(
        Opcode::Eq,
        vec![
            OperandType::Register(4),
            OperandType::Constant(Value::Float(96.0)),
            OperandType::Register(5),
        ],
    ));
    // If true, jump over division by zero to Ret
    main_instrs.push(Instruction::new(
        Opcode::JumpTrue,
        vec![OperandType::Register(5), OperandType::Offset(2)],
    ));
    // Fail: division by zero
    main_instrs.push(Instruction::new(
        Opcode::Div,
        vec![
            OperandType::Constant(Value::Int(1)),
            OperandType::Constant(Value::Int(0)),
            OperandType::Register(6),
        ],
    ));
    // Success: Ret Nil
    main_instrs.push(Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Nil)]));

    let main_fn = Function::new("main".to_string(), main_instrs, 30, 0, FunctionType::Regular);

    let bc = Bytecode {
        functions: vec![main_fn, worker_fn],
        entry_point: "main".to_string(),
        native_declarations: Vec::new(),
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bc).expect("init vm");
    let res = vm.run();
    assert!(res.is_ok(), "VM execution failed in spawned processes: {:?}", res);
}

#[test]
fn test_cpp_gtk_ffi_plugin_execution() {
    use vmm::bytecode::instruction::NativeDeclaration;

    let main_instrs = vec![
        // r0 = "Hello from IonVM C++ GTK3 Plugin!"
        Instruction::new(
            Opcode::LoadConst,
            vec![
                OperandType::Constant(Value::String("Hello from IonVM C++ GTK3 Plugin!".to_string())),
                OperandType::Register(0),
            ],
        ),
        // r1 = "IonVM GTK Dialog"
        Instruction::new(
            Opcode::LoadConst,
            vec![
                OperandType::Constant(Value::String("IonVM GTK Dialog".to_string())),
                OperandType::Register(1),
            ],
        ),
        // r2 = Call("gui.show_message", offset: 2, r0, r1) -> dst: r2
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("gui.show_message".to_string()),
                OperandType::Offset(2),
                OperandType::Register(0),
                OperandType::Register(1),
                OperandType::Register(2),
            ],
        ),
        Instruction::new(Opcode::Ret, vec![OperandType::Register(2)]),
    ];

    let main_fn = Function::new("main".to_string(), main_instrs, 10, 0, FunctionType::Regular);

    let bc = Bytecode {
        functions: vec![main_fn],
        entry_point: "main".to_string(),
        native_declarations: vec![NativeDeclaration {
            lib_name: "/big_projs/ionvm/libgui.so".to_string(),
            namespace: "gui".to_string(),
        }],
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bc).expect("init vm with C++ GTK plugin");
    let res = vm.run();
    assert!(res.is_ok(), "VM execution of C++ GTK3 plugin failed: {:?}", res);
}

#[test]
fn test_rust_dylib_plugin_execution() {
    use std::sync::{Arc, RwLock};
    use vmm::bytecode::instruction::NativeDeclaration;

    let out = Arc::new(RwLock::new(vec![0i64]));

    let main_instrs = vec![
        Instruction::new(
            Opcode::LoadConst,
            vec![
                OperandType::Constant(Value::IntArray(Arc::clone(&out))),
                OperandType::Register(2),
            ],
        ),
        Instruction::new(
            Opcode::LoadConst,
            vec![
                OperandType::Constant(Value::Int(9)),
                OperandType::Register(0),
            ],
        ),
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("rust_math.square".to_string()),
                OperandType::Offset(1),
                OperandType::Register(0),
                OperandType::Register(1),
            ],
        ),
        Instruction::new(
            Opcode::IArraySet,
            vec![
                OperandType::Register(2),
                OperandType::Constant(Value::Int(0)),
                OperandType::Register(1),
            ],
        ),
        Instruction::new(Opcode::Ret, vec![OperandType::Register(1)]),
    ];

    let main_fn = Function::new("main".to_string(), main_instrs, 10, 0, FunctionType::Regular);

    let bc = Bytecode {
        functions: vec![main_fn],
        entry_point: "main".to_string(),
        native_declarations: vec![NativeDeclaration {
            lib_name: if std::path::Path::new("/big_projs/ionvm/librust_plugin.so").exists() {
                "/big_projs/ionvm/librust_plugin.so".to_string()
            } else {
                "/tmp/librust_plugin.so".to_string()
            },
            namespace: "rust_math".to_string(),
        }],
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bc).expect("init vm with Rust dylib plugin");
    let res = vm.run();
    assert!(res.is_ok(), "VM execution of Rust dylib plugin failed: {:?}", res);
    assert_eq!(out.read().unwrap()[0], 81);
}

#[test]
fn test_c_fiber_yield_to_vm_execution() {
    use std::sync::{Arc, RwLock};
    use vmm::bytecode::instruction::NativeDeclaration;

    let out = Arc::new(RwLock::new(vec![0i64]));

    let main_instrs = vec![
        // r2 = out array
        Instruction::new(
            Opcode::LoadConst,
            vec![
                OperandType::Constant(Value::IntArray(Arc::clone(&out))),
                OperandType::Register(2),
            ],
        ),
        // r0 = 50
        Instruction::new(
            Opcode::LoadConst,
            vec![
                OperandType::Constant(Value::Int(50)),
                OperandType::Register(0),
            ],
        ),
        // r1 = Call("gui.yield_calc", arg: r0)
        // Inside C: pauses via ion_c_yield("sqrt", 100.0), VM executes sqrt(100.0) -> 10.0,
        // C resumes and calculates 50 + 10 = 60!
        Instruction::new(
            Opcode::Call,
            vec![
                OperandType::Function("gui.yield_calc".to_string()),
                OperandType::Offset(1),
                OperandType::Register(0),
                OperandType::Register(1),
            ],
        ),
        // out[0] = r1 (60)
        Instruction::new(
            Opcode::IArraySet,
            vec![
                OperandType::Register(2),
                OperandType::Constant(Value::Int(0)),
                OperandType::Register(1),
            ],
        ),
        Instruction::new(Opcode::Ret, vec![OperandType::Register(1)]),
    ];

    let main_fn = Function::new("main".to_string(), main_instrs, 10, 0, FunctionType::Regular);

    let bc = Bytecode {
        functions: vec![main_fn],
        entry_point: "main".to_string(),
        native_declarations: vec![NativeDeclaration {
            lib_name: "/big_projs/ionvm/libgui.so".to_string(),
            namespace: "gui".to_string(),
        }],
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bc).expect("init vm with C plugin for fiber yield test");
    let res = vm.run();
    assert!(res.is_ok(), "VM execution with C fiber yield failed: {:?}", res);
    assert_eq!(out.read().unwrap()[0], 60);
}

#[test]
fn test_ffi_return_callback_and_wait_and_check_execution() {
    use vm_ffi::{FfiContext, FfiFunction, FfiResult, FfiReturn, FfiValue, RawFfiFunction};
    use vmm::builtin::ffi::{call_ffi, with_ffi_registry};

    // 1. Function returning FfiReturn::Callback
    struct TestCallbackFn;
    impl RawFfiFunction for TestCallbackFn {
        fn call(&self, _ctx: &mut dyn FfiContext, _args: Vec<FfiValue>) -> FfiResult {
            Ok(FfiReturn::callback("sqrt", vec![FfiValue::Float(144.0)]))
        }
        fn name(&self) -> &str { "test_callback_fn" }
        fn arity(&self) -> usize { 0 }
    }
    impl FfiFunction for TestCallbackFn {}

    // 2. Poll target and function returning FfiReturn::WaitAndCheck
    static POLL_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    struct TestPollTarget;
    impl RawFfiFunction for TestPollTarget {
        fn call(&self, _ctx: &mut dyn FfiContext, _args: Vec<FfiValue>) -> FfiResult {
            let count = POLL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if count >= 3 {
                Ok(FfiValue::Integer(42).into())
            } else {
                Ok(FfiValue::Boolean(false).into())
            }
        }
        fn name(&self) -> &str { "test_poll_target" }
        fn arity(&self) -> usize { 0 }
    }
    impl FfiFunction for TestPollTarget {}

    struct TestWaitCheckFn;
    impl RawFfiFunction for TestWaitCheckFn {
        fn call(&self, _ctx: &mut dyn FfiContext, _args: Vec<FfiValue>) -> FfiResult {
            Ok(FfiReturn::wait_and_check("test_poll_target", vec![], Some(1000)))
        }
        fn name(&self) -> &str { "test_wait_check_fn" }
        fn arity(&self) -> usize { 0 }
    }
    impl FfiFunction for TestWaitCheckFn {}

    with_ffi_registry(|reg| {
        reg.register(TestCallbackFn);
        reg.register(TestPollTarget);
        reg.register(TestWaitCheckFn);
    });

    // Test Callback execution
    let cb_res = call_ffi("test_callback_fn", &[]).expect("callback result");
    assert_eq!(cb_res, Value::Float(12.0));

    // Test WaitAndCheck execution
    let wait_res = call_ffi("test_wait_check_fn", &[]).expect("wait and check result");
    assert_eq!(wait_res, Value::Int(42));
    assert!(POLL_COUNTER.load(std::sync::atomic::Ordering::SeqCst) >= 4);
}

#[test]
fn test_format_value_and_build_string_execution() {
    use std::sync::{Arc, RwLock};
    use vmm::bytecode::instruction::{Bytecode, Instruction, OperandType};
    use vmm::bytecode::opcode::Opcode;
    use vmm::lang::types::{Function, FunctionType, Value};
    use vmm::vm::IonVM;

    let out_arr = Arc::new(RwLock::new(vec![Value::Nil, Value::Nil]));

    // Bytecode function equivalent to f-string:
    // f"User {name} (#0x{id:x}) scored {score:.2f}%!"
    //
    // r0: name ("Alice")
    // r1: id (255)
    // r2: score (0.9525)
    // r3: formatted name ("Alice")
    // r4: formatted id ("ff")
    // r5: formatted score ("95.25%")
    // r6: final built string ("User Alice (#0xff) scored 95.25%!")
    // r7: RegisterList built string ("Aliceff95.25%")
    // r8: out_arr
    let instructions = vec![
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Array(Arc::clone(&out_arr))), OperandType::Register(8)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::String("Alice".to_string())), OperandType::Register(0)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(255)), OperandType::Register(1)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Float(0.9525)), OperandType::Register(2)]),

        // Format name with default format
        Instruction::new(Opcode::FormatValue, vec![OperandType::Register(0), OperandType::Register(3)]),

        // Format id as lowercase hex ("ff")
        Instruction::new(Opcode::FormatValue, vec![
            OperandType::Register(1),
            OperandType::Constant(Value::String("x".to_string())),
            OperandType::Register(4),
        ]),

        // Format score as percentage with 2 decimal places ("95.25%")
        Instruction::new(Opcode::FormatValue, vec![
            OperandType::Register(2),
            OperandType::Constant(Value::String(".2%".to_string())),
            OperandType::Register(5),
        ]),

        // Build string using multiple operands
        Instruction::new(Opcode::BuildString, vec![
            OperandType::Constant(Value::String("User ".to_string())),
            OperandType::Register(3),
            OperandType::Constant(Value::String(" (#0x".to_string())),
            OperandType::Register(4),
            OperandType::Constant(Value::String(") scored ".to_string())),
            OperandType::Register(5),
            OperandType::Constant(Value::String("!".to_string())),
            OperandType::Register(6),
        ]),

        // Also test RegisterList build_string
        Instruction::new(Opcode::BuildString, vec![
            OperandType::RegisterList(vec![3, 4, 5]),
            OperandType::Register(7),
        ]),

        // Store into out_arr
        Instruction::new(Opcode::ArraySet, vec![
            OperandType::Register(8),
            OperandType::Constant(Value::Int(0)),
            OperandType::Register(6),
        ]),
        Instruction::new(Opcode::ArraySet, vec![
            OperandType::Register(8),
            OperandType::Constant(Value::Int(1)),
            OperandType::Register(7),
        ]),

        Instruction::new(Opcode::Ret, vec![OperandType::Register(6)]),
    ];

    let func = Function::new("main".to_string(), instructions, 10, 0, FunctionType::Regular);
    let bytecode = Bytecode {
        functions: vec![func],
        entry_point: "main".to_string(),
        native_declarations: vec![],
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bytecode).unwrap();
    let res = vm.run();
    assert!(res.is_ok(), "VM run failed: {:?}", res);

    let results = out_arr.read().unwrap();
    assert_eq!(
        results[0],
        Value::String("User Alice (#0xff) scored 95.25%!".to_string())
    );
    assert_eq!(
        results[1],
        Value::String("Aliceff95.25%".to_string())
    );
}

#[test]
fn test_format_value_char_specifier() {
    let out_arr = Arc::new(RwLock::new(vec![Value::Nil; 2]));
    let instructions = vec![
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Array(Arc::clone(&out_arr))), OperandType::Register(0)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(65)), OperandType::Register(1)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::String("c".to_string())), OperandType::Register(2)]),
        Instruction::format_value(OperandType::Register(1), OperandType::Register(2), OperandType::Register(3)),
        Instruction::new(Opcode::ArraySet, vec![OperandType::Register(0), OperandType::Constant(Value::Int(0)), OperandType::Register(3)]),

        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(97)), OperandType::Register(4)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::String(">4c".to_string())), OperandType::Register(5)]),
        Instruction::format_value(OperandType::Register(4), OperandType::Register(5), OperandType::Register(6)),
        Instruction::new(Opcode::ArraySet, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(6)]),
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Nil)]),
    ];

    let func = Function::new("main".to_string(), instructions, 10, 0, FunctionType::Regular);
    let bytecode = Bytecode {
        functions: vec![func],
        entry_point: "main".to_string(),
        native_declarations: vec![],
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bytecode).unwrap();
    let res = vm.run();
    assert!(res.is_ok(), "VM run failed: {:?}", res);

    let results = out_arr.read().unwrap();
    assert_eq!(results[0], Value::String("A".to_string()));
    assert_eq!(results[1], Value::String("   a".to_string()));
}

#[test]
fn test_match_pattern_array_decomposition_execution() {
    // Array: [1, 2, 3, 4, 5, 6, 7]
    // Pattern: [..a | b | ..c] -> a in r2, b in r3, c in r4
    let list_data = Value::IntArray(Arc::new(RwLock::new(vec![1, 2, 3, 4, 5, 6, 7])));
    let out_arr = Arc::new(RwLock::new(vec![Value::Nil; 3]));

    let pat = Pattern::Array(ArrayPattern::head_mid_tail(
        ListSegment::BindList(2),
        ListSegment::Element(Pattern::Bind(3)),
        ListSegment::BindList(4),
    ));

    let instructions = vec![
        // 0: Load output array into r0
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Array(Arc::clone(&out_arr))), OperandType::Register(0)]),
        // 1: Load list into r1
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(list_data), OperandType::Register(1)]),
        // 2: Match r1 against pattern; if matches, jump +2 (to inst 4); if no match, fall through to inst 3
        Instruction::new(Opcode::Match, vec![OperandType::Register(1), OperandType::Pattern(pat), OperandType::Offset(2)]),
        // 3: Failure fallthrough: store -1 into out[0] and ret
        Instruction::new(Opcode::ArraySet, vec![OperandType::Register(0), OperandType::Constant(Value::Int(0)), OperandType::Constant(Value::Int(-1))]),
        // 4: Success: out[0] = b (r3)
        Instruction::new(Opcode::ArraySet, vec![OperandType::Register(0), OperandType::Constant(Value::Int(0)), OperandType::Register(3)]),
        // 5: out[1] = a (r2)
        Instruction::new(Opcode::ArraySet, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(2)]),
        // 6: out[2] = c (r4)
        Instruction::new(Opcode::ArraySet, vec![OperandType::Register(0), OperandType::Constant(Value::Int(2)), OperandType::Register(4)]),
        // 7: Ret
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Nil)]),
    ];

    let func = Function::new("main".to_string(), instructions, 10, 0, FunctionType::Regular);
    let bytecode = Bytecode {
        functions: vec![func],
        entry_point: "main".to_string(),
        native_declarations: vec![],
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bytecode).unwrap();
    let res = vm.run();
    assert!(res.is_ok(), "VM run failed: {:?}", res);

    let results = out_arr.read().unwrap();
    assert_eq!(results[0], Value::Int(4));
    if let Value::IntArray(a) = &results[1] {
        assert_eq!(*a.read().unwrap(), vec![1, 2, 3]);
    } else {
        panic!("Expected IntArray for a, got {:?}", results[1]);
    }
    if let Value::IntArray(c) = &results[2] {
        assert_eq!(*c.read().unwrap(), vec![5, 6, 7]);
    } else {
        panic!("Expected IntArray for c, got {:?}", results[2]);
    }
}

#[test]
fn test_match_pattern_range_and_literal_execution() {
    // Array: [5, 42, 7]
    // Pattern: [..10, b, 7] -> b in r2
    let list_data = Value::IntArray(Arc::new(RwLock::new(vec![5, 42, 7])));
    let out_arr = Arc::new(RwLock::new(vec![Value::Nil; 1]));

    let pat = Pattern::Array(ArrayPattern::new(vec![
        ListSegment::Element(Pattern::IntRange(None, Some(10), false)),
        ListSegment::Element(Pattern::Bind(2)),
        ListSegment::Element(Pattern::Literal(Value::Int(7))),
    ]));

    let instructions = vec![
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Array(Arc::clone(&out_arr))), OperandType::Register(0)]),
        Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(list_data), OperandType::Register(1)]),
        Instruction::new(Opcode::Match, vec![OperandType::Register(1), OperandType::Pattern(pat), OperandType::Offset(2)]),
        // Fallthrough on fail
        Instruction::new(Opcode::ArraySet, vec![OperandType::Register(0), OperandType::Constant(Value::Int(0)), OperandType::Constant(Value::Int(-999))]),
        // Jump target on success
        Instruction::new(Opcode::ArraySet, vec![OperandType::Register(0), OperandType::Constant(Value::Int(0)), OperandType::Register(2)]),
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Nil)]),
    ];

    let func = Function::new("main".to_string(), instructions, 10, 0, FunctionType::Regular);
    let bytecode = Bytecode {
        functions: vec![func],
        entry_point: "main".to_string(),
        native_declarations: vec![],
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bytecode).unwrap();
    let res = vm.run();
    assert!(res.is_ok(), "VM run failed: {:?}", res);

    let results = out_arr.read().unwrap();
    assert_eq!(results[0], Value::Int(42));
}

#[test]
fn test_pattern_serde_roundtrip() {
    let pat = Pattern::Array(ArrayPattern::head_mid_tail(
        ListSegment::BindList(2),
        ListSegment::Element(Pattern::IntRange(Some(1), Some(100), true)),
        ListSegment::BindList(4),
    ));

    let inst = Instruction::new(Opcode::Match, vec![
        OperandType::Register(0),
        OperandType::Pattern(pat.clone()),
        OperandType::Offset(5),
    ]);

    let func = Function::new("main".to_string(), vec![inst, Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Nil)])], 5, 0, FunctionType::Regular);
    let bytecode = Bytecode {
        functions: vec![func],
        entry_point: "main".to_string(),
        native_declarations: vec![],
        object_inits: vec![],
    };

    let mut buf = Vec::new();
    write_bytecode(&mut buf, &bytecode).expect("write bytecode");

    let mut reader = std::io::Cursor::new(buf);
    let roundtripped = read_bytecode(&mut reader).expect("read bytecode");

    assert_eq!(bytecode.functions.len(), roundtripped.functions.len());
    assert_eq!(bytecode.functions[0].instructions[0], roundtripped.functions[0].instructions[0]);
}

#[test]
fn test_llvm_jit_match_switch() {
    let match_inst = Instruction::new(Opcode::Match, vec![
        OperandType::Argument(0),
        OperandType::Pattern(Pattern::Literal(Value::Int(1))),
        OperandType::Offset(2),
        OperandType::Pattern(Pattern::Literal(Value::Int(2))),
        OperandType::Offset(3),
        OperandType::Pattern(Pattern::Literal(Value::Int(3))),
        OperandType::Offset(4),
    ]);

    let instructions = vec![
        // 0: match
        match_inst,
        // 1: default ret -1
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(-1))]),
        // 2: ret 100 (0 + 2 = 2)
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(100))]),
        // 3: ret 200 (0 + 3 = 3)
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(200))]),
        // 4: ret 300 (0 + 4 = 4)
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(300))]),
    ];

    let func = Function::new("match_switch".to_string(), instructions, 5, 1, FunctionType::Regular);
    let (jit_fn, ir) = compile_function_with_ir(&func).expect("JIT compile match switch");

    // LLVM compiles and optimizes the match arms
    assert!(!ir.is_empty());

    let mut registers = vec![Value::Nil; 5];
    let mut result = Value::Nil;
    let undefined_self: *mut Value = std::ptr::null_mut();

    // Test case 2 -> 200
    let args = vec![Value::Int(2)];
    unsafe {
        jit_fn(registers.as_mut_ptr(), undefined_self, args.as_ptr(), std::ptr::null_mut(), &mut result as *mut Value);
    }
    assert_eq!(result, Value::Int(200));

    // Test case 1 -> 100
    let args = vec![Value::Int(1)];
    unsafe {
        jit_fn(registers.as_mut_ptr(), undefined_self, args.as_ptr(), std::ptr::null_mut(), &mut result as *mut Value);
    }
    assert_eq!(result, Value::Int(100));

    // Test case 3 -> 300
    let args = vec![Value::Int(3)];
    unsafe {
        jit_fn(registers.as_mut_ptr(), undefined_self, args.as_ptr(), std::ptr::null_mut(), &mut result as *mut Value);
    }
    assert_eq!(result, Value::Int(300));

    // Test default -> -1
    let args = vec![Value::Int(99)];
    unsafe {
        jit_fn(registers.as_mut_ptr(), undefined_self, args.as_ptr(), std::ptr::null_mut(), &mut result as *mut Value);
    }
    assert_eq!(result, Value::Int(-1));
}

#[test]
fn test_llvm_jit_match_ranges() {
    let match_inst = Instruction::new(Opcode::Match, vec![
        OperandType::Argument(0),
        OperandType::Pattern(Pattern::IntRange(Some(5), Some(10), false)),
        OperandType::Offset(2),
        OperandType::Pattern(Pattern::IntRange(Some(10), Some(20), true)),
        OperandType::Offset(3),
    ]);

    let instructions = vec![
        // 0: match
        match_inst,
        // 1: default ret 3
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(3))]),
        // 2: ret 1 (0 + 2 = 2)
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(1))]),
        // 3: ret 2 (0 + 3 = 3)
        Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(2))]),
    ];

    let func = Function::new("match_ranges".to_string(), instructions, 5, 1, FunctionType::Regular);
    let (jit_fn, _ir) = compile_function_with_ir(&func).expect("JIT compile match ranges");

    let mut registers = vec![Value::Nil; 5];
    let mut result = Value::Nil;
    let undefined_self: *mut Value = std::ptr::null_mut();

    // 7 is in 5..10 -> ret 1
    let args = vec![Value::Int(7)];
    unsafe {
        jit_fn(registers.as_mut_ptr(), undefined_self, args.as_ptr(), std::ptr::null_mut(), &mut result as *mut Value);
    }
    assert_eq!(result, Value::Int(1));

    // 20 is in 10..=20 -> ret 2
    let args = vec![Value::Int(20)];
    unsafe {
        jit_fn(registers.as_mut_ptr(), undefined_self, args.as_ptr(), std::ptr::null_mut(), &mut result as *mut Value);
    }
    assert_eq!(result, Value::Int(2));

    // 10 is in 10..=20 (since 5..10 is exclusive) -> ret 2
    let args = vec![Value::Int(10)];
    unsafe {
        jit_fn(registers.as_mut_ptr(), undefined_self, args.as_ptr(), std::ptr::null_mut(), &mut result as *mut Value);
    }
    assert_eq!(result, Value::Int(2));

    // 100 is in default -> ret 3
    let args = vec![Value::Int(100)];
    unsafe {
        jit_fn(registers.as_mut_ptr(), undefined_self, args.as_ptr(), std::ptr::null_mut(), &mut result as *mut Value);
    }
    assert_eq!(result, Value::Int(3));
}

#[test]
fn test_closure_make_env_and_captured_execution() {
    let closure_fn = Function::new(
        "adder_closure".to_string(),
        vec![
            // r0 = arg0 + cap(0)
            Instruction::new(Opcode::Add, vec![
                OperandType::Argument(0),
                OperandType::Captured(0),
                OperandType::Register(0),
            ]),
            Instruction::new(Opcode::Ret, vec![OperandType::Register(0)]),
        ],
        2,
        1,
        FunctionType::Regular,
    );

    let main_fn = Function::new(
        "main".to_string(),
        vec![
            // r0 = 40 (value to capture)
            Instruction::new(Opcode::LoadConst, vec![
                OperandType::Constant(Value::Int(40)),
                OperandType::Register(0),
            ]),
            // r1 = make_env([r0])
            Instruction::new(Opcode::MakeEnv, vec![
                OperandType::Register(1),
                OperandType::RegisterList(vec![0]),
            ]),
            // r2 = make_closure("adder_closure", env=r1)
            Instruction::new(Opcode::MakeClosure, vec![
                OperandType::Function("adder_closure".to_string()),
                OperandType::Register(1),
                OperandType::Register(2),
            ]),
            // r4 = 2 (argument to pass)
            Instruction::new(Opcode::LoadConst, vec![
                OperandType::Constant(Value::Int(2)),
                OperandType::Register(4),
            ]),
            // r3 = call r2(r4)
            Instruction::new(Opcode::Call, vec![
                OperandType::Register(2),
                OperandType::Offset(1),
                OperandType::Register(4),
                OperandType::Register(3),
            ]),
            Instruction::new(Opcode::Ret, vec![OperandType::Register(3)]),
        ],
        5,
        0,
        FunctionType::Regular,
    );

    let bc = Bytecode {
        functions: vec![main_fn, closure_fn],
        entry_point: "main".to_string(),
        native_declarations: vec![],
        object_inits: vec![],
    };

    let mut vm = IonVM::new(bc).expect("init vm");
    let res = vm.run();
    assert!(res.is_ok(), "VM execution failed: {:?}", res);
    assert_eq!(vm.entrypoint_return(), Some(Value::Int(42)));
}
