"""
main.py
=======
Entry point for the compiler front-end.

Pipeline:
    source code
        │  layout.py
        ▼
    brace-delimited token string
        │  parser.py  (Lark LALR + LarkTransformer)
        ▼
    Module  (ast_nodes.py)
        │  name_resolution.py
        ▼
    ResolvedModule + DefMap
        │  typed_ast.py
        ▼
    TModule  ← hand off to your HIR / codegen

Usage:
    python main.py <file.ion>          # compile a file
    python main.py                     # run built-in demo
    python main.py --dump-layout <f>   # show layout pass output
    python main.py --dump-ast    <f>   # show resolved AST
    python main.py --dump-types  <f>   # show typed AST summary
"""

import argparse
import os
import pprint
import sys
import textwrap
from pathlib import Path
from typing import Optional

from codegen import Codegen
from layout import layout
from name_resolution import resolve
from typed_ast import (
    TAssignStmt,
    TConstStmt,
    TEnumDef,
    TExpr,
    TExprStmt,
    TFnDef,
    TImplDef,
    TModule,
    TProtocolDef,
    TReturnStmt,
    TStructDef,
    TyArray,
    TyBool,
    TyClosure,
    TyEnum,
    TyFloat,
    TyFn,
    TyGeneric,
    TyInt,
    TyObject,
    TyProcess,
    TyString,
    TyTuple,
    TyUnit,
    TyVar,
    typecheck,
)

# parser import is optional — requires lark
try:
    from parser import parse as lark_parse

    HAS_LARK = True
except ImportError:
    HAS_LARK = False


# ── ANSI colours (disabled on Windows / when not a tty) ─────────────────────

_USE_COLOR = sys.stdout.isatty() and sys.platform != "win32"


def _c(code: str, text: str) -> str:
    return f"\033[{code}m{text}\033[0m" if _USE_COLOR else text


def green(t):
    return _c("32", t)


def yellow(t):
    return _c("33", t)


def cyan(t):
    return _c("36", t)


def red(t):
    return _c("31", t)


def bold(t):
    return _c("1", t)


def dim(t):
    return _c("2", t)


# ── Pretty-print helpers ─────────────────────────────────────────────────────


def _type_str(ty) -> str:
    if isinstance(ty, TyInt):
        return cyan("Int")
    if isinstance(ty, TyFloat):
        return cyan("Float")
    if isinstance(ty, TyBool):
        return cyan("Bool")
    if isinstance(ty, TyString):
        return cyan("String")

    if isinstance(ty, TyUnit):
        return cyan("Unit")
    if isinstance(ty, TyProcess):
        return cyan("Process")
    if isinstance(ty, TyVar):
        return yellow(f"?t{ty.id}")
    if isinstance(ty, TyGeneric):
        return yellow(ty.name)
    if isinstance(ty, TyArray):
        return f"[{_type_str(ty.elem)}]"
    if isinstance(ty, TyTuple):
        inner = ", ".join(_type_str(e) for e in ty.elems)
        return f"({inner})"
    if isinstance(ty, TyObject):
        return cyan(f"struct {ty.name}")
    if isinstance(ty, TyEnum):
        return cyan(f"enum {ty.name}")
    if isinstance(ty, (TyFn, TyClosure)):
        params = ", ".join(_type_str(p) for p in ty.params)
        kind = "fn" if isinstance(ty, TyFn) else "closure"
        return f"{kind}({params}) -> {_type_str(ty.ret)}"
    return str(ty)


def _print_tmodule(tmod: TModule) -> None:
    section = lambda t: print(f"\n{bold(t)}")

    if tmod.structs:
        section("── Structs ──────────────────────────────────────")
        for s in tmod.structs:
            tp = f"[{', '.join(s.type_params)}]" if s.type_params else ""
            print(f"  {green('struct')} {bold(s.name)}{tp}")
            for fname, fty in s.fields:
                print(f"      {fname}: {_type_str(fty)}")

    if tmod.enums:
        section("── Enums ────────────────────────────────────────")
        for e in tmod.enums:
            tp = f"[{', '.join(e.type_params)}]" if e.type_params else ""
            print(f"  {green('enum')} {bold(e.name)}{tp}")
            for vname, vpayload in e.variants:
                if vpayload:
                    payload = ", ".join(_type_str(t) for t in vpayload)
                    print(f"      {vname}({payload})")
                else:
                    print(f"      {vname}")

    if tmod.protocols:
        section("── Protocols ────────────────────────────────────")
        for p in tmod.protocols:
            tp = f"[{', '.join(p.type_params)}]" if p.type_params else ""
            print(f"  {green('protocol')} {bold(p.name)}{tp}")
            for sig in p.sigs:
                params = ", ".join(
                    f"{pr.name}: {_type_str(pr.ty)}" for pr in sig.params
                )
                print(f"      fn {sig.name}({params}) -> {_type_str(sig.return_ty)}")

    if tmod.fns:
        section("── Functions ────────────────────────────────────")
        for fn in tmod.fns:
            tp = f"[{', '.join(fn.type_params)}]" if fn.type_params else ""
            params = ", ".join(f"{p.name}: {_type_str(p.ty)}" for p in fn.params)
            print(
                f"  {green('fn')} {bold(fn.name)}{tp}({params})"
                f"  ->  {_type_str(fn.return_ty)}"
            )

    if tmod.impls:
        section("── Impls ────────────────────────────────────────")
        for impl in tmod.impls:
            if impl.protocol:
                header = (
                    f"  {green('impl')} {bold(impl.protocol)}"
                    f" {green('for')} {_type_str(impl.self_ty)}"
                )
            else:
                header = f"  {green('impl')} {_type_str(impl.self_ty)}"
            print(header)
            for m in impl.methods:
                params = ", ".join(f"{p.name}: {_type_str(p.ty)}" for p in m.params)
                print(f"      fn {m.name}({params}) -> {_type_str(m.return_ty)}")

    if tmod.witnesses:
        section("── Witness tables ───────────────────────────────")
        for w in tmod.witnesses:
            print(f"  {_type_str(w.self_ty)}  implements  {bold(w.protocol_name)}")
            for mname in w.methods:
                print(f"      · {mname}")

    if tmod.stmts:
        section("── Top-level statements ─────────────────────────")
        for s in tmod.stmts:
            if isinstance(s, TConstStmt):
                mut = "let" if s.mutable else "const"
                print(f"  {green(mut)} {s.name}: {_type_str(s.ty)}")
            else:
                print(f"  {dim(repr(s))}")


# ── Compile function ─────────────────────────────────────────────────────────


def compile_source(src: str, filename: str = "<input>", ffi_bindings_path: Optional[str] = None) -> TModule | None:
    """
    Run the full front-end pipeline on `src`.
    Args:
        src: Source code to compile
        filename: Filename for error messages
        ffi_bindings_path: Optional path to FFI bindings JSON file
    Returns a TModule on success, None if there were errors.
    """
    # ── Stage 1: layout ──────────────────────────────────────────────────────
    laid_out = layout(src)

    # ── Stage 2: parse ───────────────────────────────────────────────────────
    if not HAS_LARK:
        print(red("✗ lark is not installed — cannot parse source files."))
        print(dim("  pip install lark"))
        return None

    try:
        module = lark_parse(laid_out)
    except Exception as exc:
        print(red(f"✗ Parse error in {filename}:"))
        print(f"  {exc}")
        return None

    # ── Stage 3: name resolution ─────────────────────────────────────────────
    resolved, res_errors = resolve(module, ffi_bindings_path)

    if res_errors:
        print(yellow(f"⚠  {len(res_errors)} name resolution error(s) in {filename}:"))
        for e in res_errors:
            print(f"  · {e}")

    # ── Stage 4: type checking ────────────────────────────────────────────────
    tmod, type_errors = typecheck(resolved)

    if type_errors:
        print(red(f"✗  {len(type_errors)} type error(s) in {filename}:"))
        for e in type_errors:
            print(f"  · {e}")
        return None

    if not res_errors:
        print(green(f"✓  {filename} compiled successfully"))

    # Attach the def_map for codegen
    tmod.def_map = resolved.def_map
    return tmod


def compile_to_file(src: str, filename: str, out_filename: Optional[str] = None, ffi_bindings_path: Optional[str] = None):
    tmod = compile_source(src, filename, ffi_bindings_path)
    if not tmod:
        return

    module_name = Path(filename).stem
    cg = Codegen(tmod, tmod.def_map, module_name)
    try:
        builder = cg.generate()
    except ValueError as exc:
        print(red(f"✗ Codegen error in {filename}:"))
        print(f"  · {exc}")
        return

    if builder:
        if not out_filename:
            out_filename = f"{module_name}.ionpack"

        with open(out_filename, "wb") as f:
            builder.build(f)
        print(green(f"✓  Generated {out_filename}"))


# ── Demo programs ────────────────────────────────────────────────────────────

DEMO_PROGRAMS: dict[str, str] = {
    "basics": """\
struct Point:
    x Float
    y Float

fn add(x Int, y Int) -> Int:
    x + y

fn greet(name String) -> String:
    name

greet name
""",
    "enum_and_match": """\
enum Shape:
    Circle(Float)
    Rect(Float, Float)

fn area(s Shape) -> Float:
    match s:
        Circle(r) => r * r
        Rect(w, h) => w * h
""",
    "generics": """\
fn identity[T](x T) -> T:
    x

fn first[T](a T, b T) -> T:
    a
""",
    "protocol": """\
protocol Show:
    fn show(self) -> String

struct Cat:
    name String

impl Show for Cat:
    fn show(self) -> String:
        self.name
""",
    "processes": """\
fn worker(x Int) -> Int:
    x

fn orchestrator() -> Process:
    const pid = spawn worker(42)
    send(pid, 42)
    const reply = receive()
    pid
""",
    "closures_in_list": """\
fn make_handlers():
    const steps = [
        fn(x Int) -> Int:
            x + 1,
        fn(x Int) -> Int:
            x * 2,
    ]
    steps
""",
}


def run_demo() -> None:
    print(bold("\n╔══════════════════════════════════════════════╗"))
    print(bold("║   ion compiler — front-end demo              ║"))
    print(bold("╚══════════════════════════════════════════════╝"))

    if not HAS_LARK:
        print(
            yellow("\nℹ  lark not installed — running layout + name-resolution only.\n")
        )

    for name, src in DEMO_PROGRAMS.items():
        print(f"\n{'─' * 48}")
        print(bold(f" {name}"))
        print("─" * 48)
        print(dim("Source:"))
        for line in src.splitlines():
            print(dim(f"  {line}"))
        print()

        laid = layout(src)
        print(f"{dim('Layout:')} {laid[:120]}{'…' if len(laid) > 120 else ''}\n")

        if not HAS_LARK:
            continue

        tmod = compile_source(src, filename=name)
        if tmod:
            _print_tmodule(tmod)

    print(f"\n{'─' * 48}\n")


# ── CLI ──────────────────────────────────────────────────────────────────────


def main() -> None:
    ap = argparse.ArgumentParser(
        description="ion compiler front-end",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=textwrap.dedent("""\
            examples:
              python main.py                       run built-in demo
              python main.py program.ion           compile and show typed AST
              python main.py --dump-layout prog.ion
              python main.py --dump-types  prog.ion
        """),
    )
    ap.add_argument("file", nargs="?", help="source file to compile")
    ap.add_argument(
        "--dump-layout", metavar="FILE", help="print layout pass output and exit"
    )
    ap.add_argument(
        "--dump-types", metavar="FILE", help="print typed AST summary and exit"
    )
    ap.add_argument("--dump-ast", metavar="FILE", help="print resolved module and exit")
    ap.add_argument(
        "-c", "--compile", metavar="FILE", help="compile a file to .ionpack"
    )
    ap.add_argument(
        "-o", "--output", metavar="FILE", help="output filename for --compile"
    )
    ap.add_argument(
        "--ffi-bindings", metavar="FILE", help="path to FFI bindings JSON file"
    )
    args = ap.parse_args()
    # print(os.getpid())
    # input()
    # ── --dump-layout ────────────────────────────────────────────────────────
    if args.dump_layout:
        src = Path(args.dump_layout).read_text()
        print(layout(src))
        return

    # ── --dump-types ─────────────────────────────────────────────────────────
    if args.dump_types:
        src = Path(args.dump_types).read_text()
        tmod = compile_source(src, args.dump_types, args.ffi_bindings)
        if tmod:
            _print_tmodule(tmod)
        return

    # ── --dump-ast ───────────────────────────────────────────────────────────
    if args.dump_ast:
        if not HAS_LARK:
            print(red("lark required for --dump-ast"))
            return
        src = Path(args.dump_ast).read_text()
        laid = layout(src)
        module = lark_parse(laid)
        resolved, errors = resolve(module, args.ffi_bindings)
        for e in errors:
            print(yellow(f"⚠  {e}"))

        pprint.pprint(resolved)
        return

    # ── --compile ────────────────────────────────────────────────────────────
    if args.compile:
        src = Path(args.compile).read_text()
        compile_to_file(src, args.compile, args.output, args.ffi_bindings)
        return

    # ── compile a file ───────────────────────────────────────────────────────
    if args.file:
        src = Path(args.file).read_text()
        tmod = compile_source(src, args.file, args.ffi_bindings)
        if tmod:
            _print_tmodule(tmod)
        return

    # ── no args → demo ───────────────────────────────────────────────────────
    run_demo()


if __name__ == "__main__":
    main()
