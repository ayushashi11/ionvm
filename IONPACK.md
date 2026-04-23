# IonPack Specification (AI-Optimized)

> **LLM Usage:** This file defines the standard for module distribution in IonVM. Use this to guide package generation and module loading logic.

## 📦 Package Structure
IonPack is a standard ZIP archive with the following layout:
- `META-INF/MANIFEST.ion`: Package metadata (required).
- `classes/`: Compiled `.ionc` bytecode files.
- `lib/`: Platform-specific native libraries (FFI).
- `resources/`: Arbitrary data files accessible by the VM.

## 📄 Manifest Format (`MANIFEST.ion`)
Key-value text format (case-sensitive):
- `IonPack-Version`: `1.0`
- `Name`: String identifier.
- `Version`: SemVer string.
- `Main-Class`: Filename (without `.ionc`) containing the entry point.
- `Entry-Point`: Function name within the Main-Class to execute first.
- `Dependencies`: Comma-separated list of required IonPacks.

## 🚀 Execution Lifecycle
1. **Load Archive:** VM reads ZIP contents into memory.
2. **Parse Manifest:** Identify `Main-Class` and `Entry-Point`.
3. **Resolve Classes:** Load bytecode from `classes/` as needed.
4. **Link FFI:** Extract and load native libs from `lib/` into the FFI registry.
5. **Boot Process:** Create the initial process using the entry point function.

## 🛠️ Tooling
- **Builder:** `vmm/src/ionpack.rs` (`IonPackBuilder`).
- **Reader:** `vmm/src/ionpack.rs` (`IonPackReader`).
- **Python:** `python-ionvm/ionvm/ionpack.py`.
