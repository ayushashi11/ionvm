# Bytecode Binary Format

This document describes the binary format for the VM's bytecode, including the layout, instruction encoding, and examples.

---

## File Structure

All multi-byte values are little-endian.

```
+-------------------+
| Magic (u32)       | 0x564D4259 ("VMBY")
+-------------------+
| Version (u16)     |
+-------------------+
| Function Count (u16) |
+-------------------+
| Function Table    | (see below)
+-------------------+
```

### Function Table

For each function:

```
+-------------------+
| Name Length (u8)  |
+-------------------+
| Name (bytes)      |
+-------------------+
| Arity (u8)        |
+-------------------+
| Register Count (u16) |
+-------------------+
| Bytecode Length (u32) |
+-------------------+
| Bytecode (bytes)  |
+-------------------+
```

---

## Instruction Encoding

Each instruction is encoded as:

```
+-------------------+
| Opcode (u8)       |
+-------------------+
| Operands (varies) |
+-------------------+
```

### Opcodes

| Opcode | Mnemonic      | Operands                        | Description                      |
|--------|---------------|----------------------------------|----------------------------------|
| 0x01   | LOAD_CONST    | reg (u8), type (u8), value      | reg = value                      |
| 0x02   | MOVE          | dst (u8), src (u8)              | dst = src                        |
| 0x03   | ADD           | dst (u8), a (u8), b (u8)        | dst = a + b                      |
| 0x04   | SUB           | dst (u8), a (u8), b (u8)        | dst = a - b                      |
| 0x05   | MUL           | dst (u8), a (u8), b (u8)        | dst = a * b                      |
| 0x06   | DIV           | dst (u8), a (u8), b (u8)        | dst = a / b                      |
| 0x10   | GET_PROP      | dst (u8), obj (u8), key (u8)    | dst = obj[key]                   |
| 0x11   | SET_PROP      | obj (u8), key (u8), val (u8)    | obj[key] = val                   |
| 0x20   | CALL          | dst (u8), func (u8), argc (u8), args... | dst = func(args...)      |
| 0x21   | RETURN        | reg (u8)                        | return reg                       |
| 0x30   | JUMP          | offset (i16)                    | ip += offset                     |
| 0x31   | JUMP_IF_TRUE  | cond (u8), offset (i16)         | if cond: ip += offset            |
| 0x32   | JUMP_IF_FALSE | cond (u8), offset (i16)         | if !cond: ip += offset           |
| 0x40   | SPAWN         | dst (u8), func (u8), argc (u8), args... | dst = spawn(func, args...)|
| 0x41   | SEND          | proc (u8), msg (u8)             | send msg to proc                 |
| 0x42   | RECEIVE       | dst (u8)                        | dst = receive()                  |
| 0x43   | LINK          | proc (u8)                       | link to proc                     |
| 0x50   | MATCH         | src (u8), pattern_idx (u16)     | match src with pattern           |
| 0xFF   | NOP           |                                  | no operation                     |

### Value Types for LOAD_CONST

| Type | Meaning   | Encoding           |
|------|-----------|--------------------|
| 0x01 | Number    | f64 (8 bytes)      |
| 0x02 | Boolean   | u8 (0=false,1=true)|
| 0x03 | Atom      | u8 len, bytes      |
| 0x04 | Unit      | (none)             |
| 0x05 | Undefined | (none)             |

---

## Example: Add Two Numbers and Return

### Pseudocode

```
r0 = 2.0
r1 = 3.0
r2 = r0 + r1
return r2
```

### Binary Encoding

```
01 00 01 00 00 00 00 00 00 00 40  (LOAD_CONST r0, Number, 2.0)
01 01 01 00 00 00 00 00 00 08 40  (LOAD_CONST r1, Number, 3.0)
03 02 00 01                       (ADD r2, r0, r1)
21 02                             (RETURN r2)
```

#### Explanation

- `01` = LOAD_CONST, `00` = r0, `01` = Number, `00 00 00 00 00 00 00 40` = 2.0 (f64)
- `01` = LOAD_CONST, `01` = r1, `01` = Number, `00 00 00 00 00 00 08 40` = 3.0 (f64)
- `03` = ADD, `02` = r2, `00` = r0, `01` = r1
- `21` = RETURN, `02` = r2

---

## Example: Function Call

### Pseudocode

```
r0 = 10
r1 = 20
r2 = call add_func(r0, r1)
return r2
```

### Binary Encoding

```
01 00 01 00 00 00 00 00 24 40     (LOAD_CONST r0, Number, 10.0)
01 01 01 00 00 00 00 00 34 40     (LOAD_CONST r1, Number, 20.0)
20 02 03 02 00 01                 (CALL r2, func=3, argc=2, args=[r0, r1])
21 02                             (RETURN r2)
```

---

## Example: Property Access

### Pseudocode

```
r0 = obj
r1 = "foo"
r2 = obj["foo"]
```

### Binary Encoding

```
10 02 00 01                       (GET_PROP r2, r0, r1)
```

---

## Notes

- All register indices are u8.
- All function indices are u8.
- All offsets are signed 16-bit integers (relative jumps).
- Atoms are length-prefixed UTF-8 strings.
- Patterns for MATCH are stored in a separate pattern table (not shown here).

---

## Extending the Format

- Add new opcodes as needed.
- Extend value types for tuples, arrays, objects, tagged enums, etc.
- Add metadata sections for debugging, source maps, etc.

---
