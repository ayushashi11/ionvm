# IonVM Bytecode Specification

> **Specification Version:** 2.1  
> **Total Opcodes:** 110 (`0` through `109`)  
> **Dispatch Mechanism:** Direct Tail-Call Dispatch Table (`DISPATCH_TABLE: [HandlerFn; 110]`)

---

## 1. Virtual Machine Architecture

- **Execution Model:** Register-based virtual machine with per-frame registers (`r0..rN`) and arguments (`arg0..argM`).
- **Concurrency Model:** Multi-threaded work-stealing actor scheduler (`Scheduler` + worker pool) with thread-safe `Arc<RwLock<T>>` objects and non-blocking cross-process message mailboxes.
- **Dual-Tier Execution:**
  1. **Tier 1 (Bytecode Interpreter):** Direct tail-call dispatch loop (`DISPATCH_TABLE`) executing instruction handlers without call stack growth.
  2. **Tier 2 (LLVM JIT Compiler):** Functions exceeding hotness thresholds are JIT-compiled with LLVM (Inkwell) using `-O3`, loop-vectorization, and SLP vectorization.
- **Typed Primitive Arrays:** Direct unboxed contiguous vectors for `IntArray` (`Vec<i64>`) and `FloatArray` (`Vec<f64>`) alongside dynamic `Array` (`Vec<Value>`).

---

## 2. Operand Types

Every instruction consists of an `Opcode` and zero or more operands (`OperandType`):
- `Register(usize)`: Register index `r0..r63`.
- `Argument(usize)`: Argument index `arg0..arg15`.
- `Constant(Value)`: Literal constant value.
- `Offset(i64)`: Relative PC jump offset (signed).
- `Property(String)`: Named object property key.
- `Function(String)`: Function identifier.
- `Pattern(Pattern)`: Pattern matching specification.
- `PayloadKeyList(Vec<String>)`: Enum payload property bindings.
- `JumpTable(Vec<JumpCase>)`: Enum tag switch jump table (`[(tag, offset), ...]`).

---

## 3. Complete Instruction Set Architecture (ISA)

### 3.1 Core Scalar & Actor Operations (`0..32`)
| Opcode | Mnemonic | Format | Description |
|---|---|---|---|
| 0 | `load_const` | `LoadConst src, dst` | Copies constant, argument, or register into destination register. |
| 1 | `add` | `Add a, b, dst` | Generic addition / string concatenation. |
| 2 | `sub` | `Sub a, b, dst` | Generic subtraction. |
| 3 | `mul` | `Mul a, b, dst` | Generic multiplication. |
| 4 | `div` | `Div a, b, dst` | Generic division. |
| 5 | `rem` | `Rem a, b, dst` | Generic remainder / modulo. |
| 6 | `neg` | `Neg a, dst` | Generic numerical negation. |
| 7 | `jump` | `Jump off` | Unconditional relative jump (`pc += off`). |
| 8 | `jump_true` | `JumpTrue cond, off` | Jump if `cond` is truthy. |
| 9 | `jump_false` | `JumpFalse cond, off` | Jump if `cond` is falsy. |
| 10 | `call` | `Call fn, off, args..., dst` | Calls bytecode function or FFI native routine. |
| 11 | `ret` | `Ret val` | Returns value to caller frame. |
| 12 | `eq` | `Eq a, b, dst` | Equality check (`a == b`). |
| 13 | `lt` | `Lt a, b, dst` | Less-than check (`a < b`). |
| 14 | `gt` | `Gt a, b, dst` | Greater-than check (`a > b`). |
| 15 | `le` | `Le a, b, dst` | Less-than-or-equal check (`a <= b`). |
| 16 | `ge` | `Ge a, b, dst` | Greater-than-or-equal check (`a >= b`). |
| 17 | `neq` | `Neq a, b, dst` | Inequality check (`a != b`). |
| 18 | `match` | `Match val, [(pat, off)...]`| Pattern matching over value. |
| 19 | `and_` / `and` | `And a, b, dst` | Logical/bitwise AND. |
| 20 | `or_` / `or` | `Or a, b, dst` | Logical/bitwise OR. |
| 21 | `xor` | `Xor a, b, dst` | Logical/bitwise XOR. |
| 22 | `not` | `Not a, dst` | Logical/bitwise NOT. |
| 23 | `get_prop` | `GetProp obj, key, dst` | Reads property `key` from object (prototype-aware). |
| 24 | `set_prop` | `SetProp obj, key, val` | Sets property `key` on object. |
| 25 | `get_index` | `GetIndex obj, idx, dst` | Reads indexed property/element from object or array. |
| 26 | `new_object` | `NewObject dst` | Allocates a new empty `Object`. |
| 27 | `make_closure` | `MakeClosure fn, dst` | Instantiates closure binding current environment. |
| 28 | `send` | `Send pid, msg` | Asynchronously enqueues message into target process mailbox. |
| 29 | `receive` | `Receive dst` | Suspends execution until message arrives; pops into `dst`. |
| 30 | `spawn` | `Spawn fn, off, args..., dst`| Spawns concurrent actor process; returns PID. |
| 31 | `swap` | `Swap r1, r2` | Swaps contents of two registers. |
| 32 | `nop` | `Nop` | No-operation. |

### 3.2 Specialized Integer & Float Arithmetic (`33..48`)
| Opcode | Mnemonic | Format | Description |
|---|---|---|---|
| 33 | `I_add_RRR` | `IAddRRR r1, r2, dst` | Fast integer register addition (`i64 + i64`). |
| 34 | `I_sub_RRR` | `ISubRRR r1, r2, dst` | Fast integer register subtraction (`i64 - i64`). |
| 35 | `I_mul_RRR` | `IMulRRR r1, r2, dst` | Fast integer register multiplication (`i64 * i64`). |
| 36 | `I_div_RCR` | `IDivRCR r1, const, dst`| Fast integer division by constant (`i64 / c`). |
| 37 | `I_rem_RCR` | `IRemRCR r1, const, dst`| Fast integer modulo by constant (`i64 % c`). |
| 38 | `I_eq_RRR` | `IEqRRR r1, r2, dst` | Fast integer equality comparison. |
| 39 | `I_lt_RRR` | `ILtRRR r1, r2, dst` | Fast integer less-than comparison. |
| 40 | `I_gt_RRR` | `IGtRRR r1, r2, dst` | Fast integer greater-than comparison. |
| 41 | `I_gt_RCR` | `IGtRCR r1, const, dst` | Fast integer greater-than constant comparison. |
| 42 | `F_add_RRR` | `FAddRRR r1, r2, dst` | Fast float register addition (`f64 + f64`). |
| 43 | `F_sub_RRR` | `FSubRRR r1, r2, dst` | Fast float register subtraction (`f64 - f64`). |
| 44 | `F_mul_RRR` | `FMulRRR r1, r2, dst` | Fast float register multiplication (`f64 * f64`). |
| 45 | `F_div_RRR` | `FDivRRR r1, r2, dst` | Fast float register division (`f64 / f64`). |
| 46 | `I_jump_true_R` | `IJumpTrueR r, off` | Direct branch if integer register != 0. |
| 47 | `I_jump_false_R`| `IJumpFalseR r, off` | Direct branch if integer register == 0. |
| 48 | `A_get_index_AR`| `AGetIndexAR arg, r_idx, dst` | Direct argument array indexing without dynamic dispatch. |

### 3.3 Fused Compare & Branch (`49..56`)
Fused comparison and branch instructions eliminate intermediate boolean register writes and branch evaluation overhead:
| Opcode | Mnemonic | Format | Description |
|---|---|---|---|
| 49 | `I_gt_jmp_RR` | `IGtJmpRR r1, r2, off` | Jump if integer `r1 > r2`. |
| 50 | `I_gt_jmp_RC` | `IGtJmpRC r1, const, off` | Jump if integer `r1 > const`. |
| 51 | `I_lt_jmp_RR` | `ILtJmpRR r1, r2, off` | Jump if integer `r1 < r2`. |
| 52 | `I_lt_jmp_RC` | `ILtJmpRC r1, const, off` | Jump if integer `r1 < const`. |
| 53 | `I_eq_jmp_RR` | `IEqJmpRR r1, r2, off` | Jump if integer `r1 == r2`. |
| 54 | `I_eq_jmp_RC` | `IEqJmpRC r1, const, off` | Jump if integer `r1 == const`. |
| 55 | `F_gt_jmp_RR` | `FGtJmpRR r1, r2, off` | Jump if float `r1 > r2`. |
| 56 | `F_lt_jmp_RR` | `FLtJmpRR r1, r2, off` | Jump if float `r1 < r2`. |

### 3.4 SIMD & String Operations (`57..62, 86..88`)
| Opcode | Mnemonic | Format | Description |
|---|---|---|---|
| 57 | `S_concat_RRR` | `SConcatRRR s1, s2, dst` | Concatenates two strings into `dst`. |
| 58 | `S_slice_RRR` | `SSliceRRR str, start, dst` | Substring slice from `start` to end. |
| 59 | `S_slice_RRRR` | `SSliceRRRR str, start, len, dst` | Substring slice of length `len`. |
| 60 | `S_eq_RRR` | `SEqRRR s1, s2, dst` | Fast AVX2/SSE string equality check. |
| 61 | `S_find_RRR` | `SFindRRR haystack, needle, dst` | Substring search; returns byte index or -1. |
| 62 | `S_to_int_RR` | `SToIntRR str, dst` | Fast ASCII string to integer conversion. |
| 86 | `S_upper_RR` | `SUpperRR str, dst` | SIMD ASCII uppercase transformation. |
| 87 | `S_lower_RR` | `SLowerRR str, dst` | SIMD ASCII lowercase transformation. |
| 88 | `S_count_byte_RRR` | `SCountByteRRR str, byte, dst` | Fast SIMD byte occurrence counting. |

### 3.5 Enum & Tagged Pattern Matching (`63..69`)
| Opcode | Mnemonic | Format | Description |
|---|---|---|---|
| 63 | `E_make_enum` | `EMakeEnum tag, payloads..., dst` | Creates an enum instance with integer tag. |
| 64 | `E_get_tag_RR` | `EGetTagRR enum, dst` | Extracts the integer tag from an enum. |
| 65 | `E_get_payload_RRC` | `EGetPayloadRRC enum, idx, dst` | Extracts payload by numeric index. |
| 66 | `E_get_payload_RR_multi` | `EGetPayloadRRMulti enum, keys, dst` | Unpacks multiple named enum properties. |
| 67 | `E_tag_jmp_RC` | `ETagJmpRC enum, tag, off` | Jump if enum tag matches `tag`. |
| 68 | `E_tag_switch` | `ETagSwitch enum, table` | Jump table dispatch on enum tag. |
| 69 | `A_match_fast` | `AMatchFast enum, table` | Optimized fast pattern matcher. |

### 3.6 Array Primitives (`70..76, 108..109`)
| Opcode | Mnemonic | Format | Description |
|---|---|---|---|
| 70 | `new_array` | `NewArray len, dst` | Allocates a dynamic `Value::Array`. |
| 71 | `new_int_array` | `NewIntArray len, dst` | Allocates unboxed `Value::IntArray` (`Vec<i64>`). |
| 72 | `new_float_array` | `NewFloatArray len, dst` | Allocates unboxed `Value::FloatArray` (`Vec<f64>`). |
| 73 | `array_len` | `ArrayLen arr, dst` | Retrieves array length. |
| 74 | `array_push` | `ArrayPush arr, val` | Appends element to array. |
| 75 | `array_get` | `ArrayGet arr, idx, dst` | Generic array access (polymorphic across array types). |
| 76 | `array_set` | `ArraySet arr, idx, val` | Writes element at index. |
| 108 | `I_array_get` | `IArrayGet int_arr, idx, dst` | **Dedicated IntArray Accessor:** Loads direct `i64` from `IntArray` without type checks or dynamic dispatch. In LLVM JIT, translates to raw pointer GEP + load. |
| 109 | `F_array_get` | `FArrayGet flt_arr, idx, dst` | **Dedicated FloatArray Accessor:** Loads direct `f64` from `FloatArray` without type checks or dynamic dispatch. In LLVM JIT, translates to raw pointer GEP + load. |

### 3.7 Vector SIMD Operations (`77..85, 92..107`)
Hardware-accelerated vector instructions operating over 256-bit and 512-bit vector lanes:
| Opcode | Mnemonic | Format | Description |
|---|---|---|---|
| 77 | `V_add_F` | `VAddF v1, v2, dst` | SIMD float vector addition (`v1 + v2`). |
| 78 | `V_sub_F` | `VSubF v1, v2, dst` | SIMD float vector subtraction (`v1 - v2`). |
| 79 | `V_mul_F` | `VMulF v1, v2, dst` | SIMD float vector multiplication (`v1 * v2`). |
| 80 | `V_dot_F` | `VDotF v1, v2, dst` | SIMD float vector dot product (`sum(v1[i] * v2[i])`). |
| 81 | `V_add_I` | `VAddI v1, v2, dst` | SIMD integer vector addition. |
| 82 | `V_sub_I` | `VSubI v1, v2, dst` | SIMD integer vector subtraction. |
| 83 | `V_mul_I` | `VMulI v1, v2, dst` | SIMD integer vector multiplication. |
| 84 | `VSumI` | `VSumI vec, dst` | SIMD horizontal sum of integer vector elements. |
| 85 | `VSumF` | `VSumF vec, dst` | SIMD horizontal sum of float vector elements. |
| 92 | `V_iota_I` | `VIotaI start, count, dst` | Generates contiguous integer range vector `[start..start+count]`. |
| 93 | `V_splat_I` | `VSplatI val, count, dst` | Broadcasts integer scalar across vector lanes. |
| 94 | `V_splat_F` | `VSplatF val, count, dst` | Broadcasts float scalar across vector lanes. |
| 95 | `V_add_IC` | `VAddIC vec, const, dst` | Adds integer scalar constant to all vector elements. |
| 96 | `V_sub_IC` | `VSubIC vec, const, dst` | Subtracts integer scalar constant from all elements. |
| 97 | `V_mul_IC` | `VMulIC vec, const, dst` | Multiplies all elements by integer scalar constant. |
| 98 | `V_div_IC` | `VDivIC vec, const, dst` | Divides all elements by integer scalar constant. |
| 99 | `V_rem_IC` | `VRemIC vec, const, dst` | Modulo of all elements by integer scalar constant. |
| 100 | `V_div_I` | `VDivI v1, v2, dst` | Vector element-wise integer division. |
| 101 | `V_rem_I` | `VRemI v1, v2, dst` | Vector element-wise integer remainder. |
| 102 | `V_div_F` | `VDivF v1, v2, dst` | Vector element-wise float division. |
| 103 | `V_gather_I` | `VGatherI arr, indices, dst` | Vector gather indexing: `dst[i] = arr[indices[i]]`. |
| 104 | `V_cmp_eq_I` | `VCmpEqI v1, v2, dst` | Vector element-wise equality comparison. |
| 105 | `V_cmp_gt_I` | `VCmpGtI v1, v2, dst` | Vector element-wise greater-than comparison. |
| 106 | `V_cmp_lt_I` | `VCmpLtI v1, v2, dst` | Vector element-wise less-than comparison. |
| 107 | `V_filter_I` | `VFilterI vec, mask, dst` | Filters vector elements matching boolean mask. |

### 3.8 JIT & Execution Special (`89..91`)
| Opcode | Mnemonic | Format | Description |
|---|---|---|---|
| 89 | `I_add_RCR` | `IAddRCR r, const, dst` | Add constant to integer register (`r + c`). |
| 90 | `call_jit` | `CallJit fn_idx, args...` | Transfers execution directly to LLVM JIT native function pointer. |
| 91 | `V_munch_range` | `VMunchRange start, end, dst` | AVX-512 accelerated vectorized range computation. |

---

## 4. Binary Codec (`vmm/src/bytecode/serde.rs`)

### 4.1 Header & Function Table
The bytecode file format is self-describing and supports forward/backward compatibility:
```
[u64: Function Count]
  For each function:
    [String: Function Name]
    [u64: Instruction Count]
      For each instruction:
        [String: Opcode Mnemonic]
        [u64: Operand Count]
        [For each operand: Serialized OperandType]
    [u64: Register Count]
    [u64: Argument Count]
    [String: Function Type ("Regular" | "Native" | "Ffi")]
    [String: Data Tag ("Regular")]
    [u64: Reference Count]
[String: Entry Point Name]
```

### 4.2 Operand Serialization Formats
- `String`: `[u64: length in bytes] [bytes...]`
- `Int`: `[i64 little-endian]`
- `Float`: `[f64 little-endian]`
- `Bool`: `[u8: 0 or 1]`
- `Register`: `[u8 tag: "register"] [u64: reg index]`
- `Argument`: `[u8 tag: "argument"] [u64: arg index]`
- `Offset`: `[u8 tag: "offset"] [i64: jump offset]`
- `JumpTable`: `[u8 tag: "jump_table"] [u64: case count] [(i64 tag, i64 offset)...]`
