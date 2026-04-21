"""
codegen.py
==========
Pass 4 in the pipeline: TModule → IonVM Bytecode / IonPack

This pass lowers the typed AST into register-based bytecode for the IonVM.
It uses the ionvm Python library for instruction generation and packaging.
"""

import os
import sys
import uuid
from typing import Any, Dict, List, Optional

# Try to import ionvm
try:
    import ionvm
except ImportError:
    # If not installed, we can't do codegen
    ionvm = None

import ast_nodes as A
import ffi_bindings
from name_resolution import BindPat, DefKind, ResolvedRef, VariantPat
from typed_ast import (
    TAssignStmt,
    TConstStmt,
    TEnumDef,
    TExpr,
    TExprStmt,
    TFnDef,
    TImplDef,
    TModule,
    TParam,
    TReturnStmt,
    TStructDef,
)


class Codegen:
    """
    Code generator for lowering typed AST to IonVM bytecode.
    
    Closure/Environment Model:
    ========================
    - The closure environment is OWNED BY THE PROCESS through its frame stack
    - Each process has its own register state and local bindings
    - When a closure is created with make_closure, captured values are references
      to the registers in the defining scope (which exists in the process's frame)
    - Scope IDs (scope_id param in make_closure) allow sibling closures to share
      the same captured environment, representing lexically scoped bindings
    - Environment is immutable during closure lifetime (process owns it)
    """
    
    def __init__(
        self, tmod: TModule, def_map: Dict[int, Any], module_name: str = "main"
    ):
        self.tmod = tmod
        self.def_map = def_map
        self.module_name = module_name
        if ionvm:
            self.builder = ionvm.IonPackBuilder(module_name, "0.1.0")
        else:
            self.builder = None
        self.instructions: List[Any] = []
        self.def_to_reg: Dict[int, int] = {}  # Maps definition IDs to their registers
        self.next_reg = 0
        self.generated_functions: List[Any] = []

    def allocate_reg(self) -> int:
        r = self.next_reg
        self.next_reg += 1
        return r

    def generate(self) -> Optional[Any]:
        if not ionvm:
            print("Error: ionvm library not found. Cannot generate bytecode.")
            return None

        self.generated_functions = []

        # 1. Generate top-level functions
        for fn in self.tmod.fns:
            self.generated_functions.append(self.gen_function(fn))

        # 2. Generate impl methods
        for impl in self.tmod.impls:
            for method in impl.methods:
                self.generated_functions.append(self.gen_function(method))

        # 3. Generate top-level statements into a 'main' function if they exist
        if self.tmod.stmts:
            self.instructions = []
            self.def_to_reg = {}
            self.next_reg = 0

            # Special: top-level bindings
            for stmt in self.tmod.stmts:
                self.gen_stmt(stmt)

            # Ensure return
            if not self.instructions or self.instructions[-1].opcode != "return":
                r = self.allocate_reg()
                self.instructions.append(
                    ionvm.Instruction.load_const(r, ionvm.Value.unit())
                )
                self.instructions.append(ionvm.Instruction.return_reg(r))

            main_fn = ionvm.Function(
                name="main",
                arity=0,
                extra_regs=self.next_reg,
                instructions=self.instructions,
            )
            self.generated_functions.append(main_fn)
            self.builder.entry_point("main")

        # Put everything in a "Main" class for now
        self.builder.main_class("Main")
        self.builder.add_multi_function_class("Main", self.generated_functions)

        return self.builder

    def gen_function(self, fn: TFnDef) -> Any:
        self.instructions = []
        self.def_to_reg = {}
        self.next_reg = 0

        # Parameters start at reg 0
        for p in fn.params:
            self.def_to_reg[p.def_id] = self.allocate_reg()

        last_reg = None
        for stmt in fn.body:
            last_reg = self.gen_stmt(stmt)

        # Ensure return
        if not self.instructions or self.instructions[-1].opcode != "return":
            if last_reg is not None:
                self.instructions.append(ionvm.Instruction.return_reg(last_reg))
            else:
                r = self.allocate_reg()
                self.instructions.append(
                    ionvm.Instruction.load_const(r, ionvm.Value.unit())
                )
                self.instructions.append(ionvm.Instruction.return_reg(r))

        return ionvm.Function(
            name=fn.name,
            arity=fn.arity,
            extra_regs=max(0, self.next_reg - fn.arity),
            instructions=self.instructions,
        )

    def gen_lambda_function(self, lam: A.LambdaExpr) -> tuple[str, str, List[tuple[int, str]]]:
        """
        Lower a lambda to a synthetic function and return its generated name.
        Returns: (function_name, scope_id, captures)
        where captures is [(def_id, capture_name), ...] in first-use order.
        """
        captures = self.find_lambda_captures(lam)
        lambda_name = f"__lambda_{uuid.uuid4().hex}"
        scope_id = f"__scope_{uuid.uuid4().hex}"

        saved_instructions = self.instructions
        saved_def_to_reg = self.def_to_reg
        saved_next_reg = self.next_reg

        self.instructions = []
        self.def_to_reg = {}
        self.next_reg = 0

        # Captured values are loaded into leading registers at call-time.
        for cap_def_id, _cap_name in captures:
            self.def_to_reg[cap_def_id] = self.allocate_reg()

        for p in lam.params:
            self.def_to_reg[p.def_id] = self.allocate_reg()

        last_reg = None
        for stmt in lam.body:
            last_reg = self.gen_stmt(stmt)

        if not self.instructions or self.instructions[-1].opcode != "return":
            if last_reg is not None:
                self.instructions.append(ionvm.Instruction.return_reg(last_reg))
            else:
                r = self.allocate_reg()
                self.instructions.append(
                    ionvm.Instruction.load_const(r, ionvm.Value.unit())
                )
                self.instructions.append(ionvm.Instruction.return_reg(r))

        fn = ionvm.Function(
            name=lambda_name,
            arity=len(lam.params),
            extra_regs=max(0, self.next_reg - len(lam.params)),
            instructions=self.instructions,
        )
        self.generated_functions.append(fn)

        self.instructions = saved_instructions
        self.def_to_reg = saved_def_to_reg
        self.next_reg = saved_next_reg

        return lambda_name, scope_id, captures

    def _record_capture(self, def_id: int, name: str, captures: Dict[int, str]) -> None:
        if def_id == 0:
            return
        info = self.def_map.get(def_id)
        if not info:
            return
        if info.kind in (
            DefKind.FN,
            DefKind.STRUCT,
            DefKind.ENUM,
            DefKind.VARIANT,
            DefKind.PROTOCOL,
            DefKind.TYPE_PARAM,
        ):
            return
        captures[def_id] = name

    def _collect_pattern_defs(self, pat: Any, defs: set[int]) -> None:
        if isinstance(pat, BindPat):
            defs.add(pat.def_id)
            return
        if isinstance(pat, VariantPat):
            for p in pat.payload:
                self._collect_pattern_defs(p, defs)
            return
        if isinstance(pat, A.TuplePat):
            for p in pat.elems:
                self._collect_pattern_defs(p, defs)

    def _collect_expr_captures(
        self, expr: Any, local_defs: set[int], captures: Dict[int, str]
    ) -> None:
        inner = expr.inner if hasattr(expr, "inner") else expr

        if isinstance(inner, ResolvedRef):
            if inner.def_id not in local_defs:
                self._record_capture(inner.def_id, inner.name, captures)
            return

        if isinstance(
            inner, (A.IntLit, A.FloatLit, A.StringLit, A.BoolLit, A.UnitLit)
        ):
            return

        if isinstance(inner, A.BinOp):
            self._collect_expr_captures(inner.left, local_defs, captures)
            self._collect_expr_captures(inner.right, local_defs, captures)
            return

        if isinstance(inner, A.UnaryOp):
            self._collect_expr_captures(inner.operand, local_defs, captures)
            return

        if isinstance(inner, A.Call):
            self._collect_expr_captures(inner.callee, local_defs, captures)
            for arg in inner.args:
                self._collect_expr_captures(arg, local_defs, captures)
            return

        if isinstance(inner, A.FieldAccess):
            self._collect_expr_captures(inner.obj, local_defs, captures)
            return

        if isinstance(inner, A.IndexExpr):
            self._collect_expr_captures(inner.obj, local_defs, captures)
            self._collect_expr_captures(inner.index, local_defs, captures)
            return

        if isinstance(inner, (A.TupleLit, A.ArrayLit)):
            for elem in inner.elems:
                self._collect_expr_captures(elem, local_defs, captures)
            return

        if isinstance(inner, A.IfExpr):
            self._collect_expr_captures(inner.cond, local_defs, captures)

            then_defs = set(local_defs)
            for stmt in inner.then_body:
                self._collect_stmt_captures(stmt, then_defs, captures)

            if inner.else_body:
                if isinstance(inner.else_body, A.IfExpr):
                    self._collect_expr_captures(
                        inner.else_body, set(local_defs), captures
                    )
                else:
                    else_defs = set(local_defs)
                    for stmt in inner.else_body:
                        self._collect_stmt_captures(stmt, else_defs, captures)
            return

        if isinstance(inner, A.MatchExpr):
            self._collect_expr_captures(inner.subject, local_defs, captures)
            for arm in inner.arms:
                arm_defs = set(local_defs)
                self._collect_pattern_defs(arm.pattern, arm_defs)
                self._collect_expr_captures(arm.body, arm_defs, captures)
            return

        if isinstance(inner, A.SpawnExpr):
            if inner.func_def_id is not None and inner.func_def_id not in local_defs:
                self._record_capture(inner.func_def_id, inner.func, captures)
            for arg in inner.args:
                self._collect_expr_captures(arg, local_defs, captures)
            return

        if isinstance(inner, A.SendExpr):
            self._collect_expr_captures(inner.pid, local_defs, captures)
            self._collect_expr_captures(inner.msg, local_defs, captures)
            return

        if isinstance(inner, A.ReceiveExpr):
            return

        if isinstance(inner, A.LambdaExpr):
            # Nested lambda captures are checked when that lambda is lowered.
            return

    def _collect_stmt_captures(
        self, stmt: Any, local_defs: set[int], captures: Dict[int, str]
    ) -> None:
        if isinstance(stmt, TConstStmt):
            self._collect_expr_captures(stmt.value, local_defs, captures)
            local_defs.add(stmt.def_id)
            return

        if isinstance(stmt, TAssignStmt):
            self._collect_expr_captures(stmt.value, local_defs, captures)
            return

        if isinstance(stmt, TReturnStmt):
            if stmt.value:
                self._collect_expr_captures(stmt.value, local_defs, captures)
            return

        if isinstance(stmt, TExprStmt):
            self._collect_expr_captures(stmt.expr, local_defs, captures)
            return

        if isinstance(stmt, TFnDef):
            local_defs.add(stmt.def_id)

    def find_lambda_captures(self, lam: A.LambdaExpr) -> List[tuple[int, str]]:
        local_defs: set[int] = set()
        for p in lam.params:
            if hasattr(p, "def_id"):
                local_defs.add(p.def_id)

        captures: Dict[int, str] = {}
        for stmt in lam.body:
            self._collect_stmt_captures(stmt, local_defs, captures)

        return list(captures.items())

    def gen_stmt(self, stmt) -> Optional[int]:
        if isinstance(stmt, TConstStmt):
            reg = self.gen_expr(stmt.value)
            self.def_to_reg[stmt.def_id] = reg
            return reg
        elif isinstance(stmt, TAssignStmt):
            val_reg = self.gen_expr(stmt.value)
            target_reg = self.def_to_reg.get(stmt.def_id)
            if target_reg is not None:
                self.instructions.append(ionvm.Instruction.move(target_reg, val_reg))
            return target_reg
        elif isinstance(stmt, TReturnStmt):
            if stmt.value:
                reg = self.gen_expr(stmt.value)
                self.instructions.append(ionvm.Instruction.return_reg(reg))
                return reg
            else:
                r = self.allocate_reg()
                self.instructions.append(
                    ionvm.Instruction.load_const(r, ionvm.Value.unit())
                )
                self.instructions.append(ionvm.Instruction.return_reg(r))
                return r
        elif isinstance(stmt, TExprStmt):
            reg = self.gen_expr(stmt.expr)
            return reg
        return None

    def gen_expr(self, expr: TExpr) -> int:
        inner = expr.inner

        if hasattr(inner, "data"):  # Lark Tree
            # This shouldn't really happen if transformation was complete,
            # but let's handle it by assuming it might be a single-child expression.
            if len(inner.children) == 1:
                return self.gen_expr(TExpr(inner.children[0], expr.ty))
            else:
                return self.allocate_reg()

        if isinstance(inner, A.IntLit):
            r = self.allocate_reg()
            self.instructions.append(
                ionvm.Instruction.load_const(r, ionvm.Value.number(float(inner.value)))
            )
            return r
        elif isinstance(inner, A.FloatLit):
            r = self.allocate_reg()
            self.instructions.append(
                ionvm.Instruction.load_const(r, ionvm.Value.number(inner.value))
            )
            return r
        elif isinstance(inner, A.StringLit):
            r = self.allocate_reg()
            self.instructions.append(
                ionvm.Instruction.load_const(r, ionvm.Value.string(inner.value))
            )
            return r
        elif isinstance(inner, A.BoolLit):
            r = self.allocate_reg()
            self.instructions.append(
                ionvm.Instruction.load_const(r, ionvm.Value.boolean(inner.value))
            )
            return r
        elif isinstance(inner, A.UnitLit):
            r = self.allocate_reg()
            self.instructions.append(
                ionvm.Instruction.load_const(r, ionvm.Value.unit())
            )
            return r
        elif isinstance(inner, (A.TupleLit, A.ArrayLit)):
            arg_regs = [self.gen_expr(a) for a in inner.elems]
            dst = self.allocate_reg()
            self.instructions.append(ionvm.Instruction.array_init(dst, arg_regs))
            return dst

        elif isinstance(inner, ResolvedRef):
            reg = self.def_to_reg.get(inner.def_id)
            if reg is not None:
                return reg

            info = self.def_map.get(inner.def_id)
            if info and info.kind == DefKind.FFI_FN:
                # Reference to an FFI function
                r = self.allocate_reg()
                ffi_bindings_registry = ffi_bindings.get_global_ffi_bindings()
                ffi_fn = ffi_bindings_registry.get_function(inner.name)
                if ffi_fn:
                    ref = ffi_fn.full_name()
                else:
                    ref = f"__stdlib:{inner.name}"
                self.instructions.append(
                    ionvm.Instruction.load_const(r, ionvm.Value.atom(ref))
                )
                return r
            
            if info and info.kind == DefKind.FFI_TYPE:
                # Reference to an FFI object type (constructor or static factory)
                r = self.allocate_reg()
                ffi_bindings_registry = ffi_bindings.get_global_ffi_bindings()
                ffi_obj = ffi_bindings_registry.get_object(inner.name)
                if ffi_obj:
                    ref = ffi_obj.full_name()
                else:
                    ref = f"__type:{inner.name}"
                self.instructions.append(
                    ionvm.Instruction.load_const(r, ionvm.Value.atom(ref))
                )
                return r
            
            if info and info.kind == DefKind.FN:
                r = self.allocate_reg()
                if inner.name in ("debug", "print", "println"):
                    ref = f"__stdlib:{inner.name}"
                else:
                    ref = f"__function_ref:Main:{inner.name}"
                self.instructions.append(
                    ionvm.Instruction.load_const(r, ionvm.Value.atom(ref))
                )
                return r

            # Built-in or VM intrinsic
            r = self.allocate_reg()
            if inner.name in ("self", "pid", "processes"):
                self.instructions.append(
                    ionvm.Instruction.load_const(
                        r, ionvm.Value.atom(f"__vm:{inner.name}")
                    )
                )
            else:
                # Fallback to standard lib or something else
                self.instructions.append(
                    ionvm.Instruction.load_const(
                        r, ionvm.Value.atom(f"__stdlib:{inner.name}")
                    )
                )
            return r

        elif isinstance(inner, A.BinOp):
            l = self.gen_expr(inner.left)
            rv = self.gen_expr(inner.right)
            dst = self.allocate_reg()
            ops = {
                "+": ionvm.Instruction.add,
                "-": ionvm.Instruction.sub,
                "*": ionvm.Instruction.mul,
                "/": ionvm.Instruction.div,
                "==": ionvm.Instruction.equal,
                "!=": ionvm.Instruction.not_equal,
                "<": ionvm.Instruction.less_than,
                "<=": ionvm.Instruction.less_equal,
                ">": ionvm.Instruction.greater_than,
                ">=": ionvm.Instruction.greater_equal,
                "and": ionvm.Instruction.and_,
                "or": ionvm.Instruction.or_,
            }
            if inner.op in ops:
                self.instructions.append(ops[inner.op](dst, l, rv))
            return dst

        elif isinstance(inner, A.UnaryOp):
            operand = self.gen_expr(inner.operand)
            dst = self.allocate_reg()
            if inner.op == "-":
                zero = self.allocate_reg()
                self.instructions.append(
                    ionvm.Instruction.load_const(zero, ionvm.Value.number(0.0))
                )
                self.instructions.append(ionvm.Instruction.sub(dst, zero, operand))
            elif inner.op == "not":
                self.instructions.append(ionvm.Instruction.not_(dst, operand))
            return dst

        elif isinstance(inner, A.Call):
            if isinstance(inner.callee.inner, ResolvedRef):
                info = self.def_map.get(inner.callee.inner.def_id)
                if info and info.kind == DefKind.VARIANT:
                    dst = self.allocate_reg()
                    args = [self.gen_expr(a) for a in inner.args]

                    # Keep enum payload layout uniform: __slots is always a tuple-like array.
                    slots_reg = self.allocate_reg()
                    self.instructions.append(ionvm.Instruction.array_init(slots_reg, args))

                    props = [
                        ("__tag", ("val", ionvm.Value.atom(inner.callee.inner.name))),
                        ("__slots", ("reg", slots_reg)),
                    ]
                    self.instructions.append(ionvm.Instruction.object_init(dst, props))
                    return dst

            callee_reg = self.gen_expr(inner.callee)
            arg_regs = [self.gen_expr(a) for a in inner.args]
            dst = self.allocate_reg()
            self.instructions.append(ionvm.Instruction.call(dst, callee_reg, arg_regs))
            return dst

        elif isinstance(inner, A.FieldAccess):
            obj_reg = self.gen_expr(inner.obj)
            prop_reg = self.allocate_reg()
            self.instructions.append(
                ionvm.Instruction.load_const(prop_reg, ionvm.Value.atom(inner.field))
            )
            dst = self.allocate_reg()
            self.instructions.append(ionvm.Instruction.get_prop(dst, obj_reg, prop_reg))
            return dst

        elif isinstance(inner, A.IfExpr):
            dst = self.allocate_reg()
            cond_reg = self.gen_expr(inner.cond)

            # jump_if_false ELSE
            jump_false_idx = len(self.instructions)
            self.instructions.append(None)  # placeholder

            # THEN
            last = None
            for s in inner.then_body:
                last = self.gen_stmt(s)
            if (
                inner.then_body
                and isinstance(inner.then_body[-1], TExprStmt)
                and last is not None
            ):
                self.instructions.append(ionvm.Instruction.move(dst, last))

            # jump END
            jump_end_idx = len(self.instructions)
            self.instructions.append(None)  # placeholder

            # ELSE
            else_start = len(self.instructions)
            if inner.else_body:
                if isinstance(inner.else_body, A.IfExpr):
                    last = self.gen_expr(TExpr(inner.else_body, expr.ty))
                    self.instructions.append(ionvm.Instruction.move(dst, last))
                else:
                    last = None
                    for s in inner.else_body:
                        last = self.gen_stmt(s)
                    if (
                        inner.else_body
                        and isinstance(inner.else_body[-1], TExprStmt)
                        and last is not None
                    ):
                        self.instructions.append(ionvm.Instruction.move(dst, last))
            else:
                self.instructions.append(
                    ionvm.Instruction.load_const(dst, ionvm.Value.unit())
                )

            end_start = len(self.instructions)

            # Patch
            self.instructions[jump_false_idx] = ionvm.Instruction.jump_if_false(
                cond_reg, else_start - jump_false_idx
            )
            self.instructions[jump_end_idx] = ionvm.Instruction.jump(
                end_start - jump_end_idx
            )
            return dst

        elif isinstance(inner, A.MatchExpr):
            subj_reg = self.gen_expr(inner.subject)
            dst = self.allocate_reg()
            patterns, jumps = [], []
            match_idx = len(self.instructions)
            self.instructions.append(None)

            end_jump_idxs = []
            for arm in inner.arms:
                jumps.append(len(self.instructions) - match_idx)
                patterns.append(self.convert_pattern(arm.pattern))

                # Bind pattern variables
                self.gen_pattern_bindings(arm.pattern, subj_reg)

                res = self.gen_expr(arm.body)
                self.instructions.append(ionvm.Instruction.move(dst, res))
                end_jump_idxs.append(len(self.instructions))
                self.instructions.append(None)

            end_idx = len(self.instructions)
            self.instructions[match_idx] = ionvm.Instruction.match(
                subj_reg, patterns, jumps
            )
            for idx in end_jump_idxs:
                self.instructions[idx] = ionvm.Instruction.jump(end_idx - idx)
            return dst

        elif isinstance(inner, A.SpawnExpr):
            f_reg = None
            if inner.func_def_id is not None:
                f_reg = self.def_to_reg.get(inner.func_def_id)

            if f_reg is None:
                f_reg = self.allocate_reg()
                self.instructions.append(
                    ionvm.Instruction.load_const(
                        f_reg, ionvm.Value.atom(f"__function_ref:Main:{inner.func}")
                    )
                )
            args = [self.gen_expr(a) for a in inner.args]
            dst = self.allocate_reg()
            self.instructions.append(ionvm.Instruction.spawn(dst, f_reg, args))
            return dst

        elif isinstance(inner, A.SendExpr):
            p_reg = self.gen_expr(inner.pid)
            m_reg = self.gen_expr(inner.msg)
            self.instructions.append(ionvm.Instruction.send(p_reg, m_reg))
            dst = self.allocate_reg()
            self.instructions.append(
                ionvm.Instruction.load_const(dst, ionvm.Value.unit())
            )
            return dst

        elif isinstance(inner, A.ReceiveExpr):
            dst = self.allocate_reg()
            self.instructions.append(ionvm.Instruction.receive(dst))
            return dst

        elif isinstance(inner, A.BlockExpr):
            # Block expression: execute statements and return result of last statement
            last_reg = None
            for stmt in inner.stmts:
                last_reg = self.gen_stmt(stmt)
            
            # If last statement was an expression statement, it returns a register
            # Otherwise, we need to return unit
            if last_reg is None:
                dst = self.allocate_reg()
                return dst
            return last_reg

        elif isinstance(inner, A.LambdaExpr):
            fn_name, scope_id, captures = self.gen_lambda_function(inner)
            fn_reg = self.allocate_reg()
            self.instructions.append(
                ionvm.Instruction.load_const(
                    fn_reg, ionvm.Value.atom(f"__function_ref:Main:{fn_name}")
                )
            )
            if not captures:
                return fn_reg

            captured_regs = []
            for cap_def_id, cap_name in captures:
                cap_reg = self.def_to_reg.get(cap_def_id)
                if cap_reg is None:
                    raise ValueError(
                        f"Unable to resolve captured binding '{cap_name}' in lambda"
                    )
                captured_regs.append((cap_name, cap_reg))

            dst = self.allocate_reg()
            self.instructions.append(
                ionvm.Instruction.make_closure(dst, fn_reg, scope_id, captured_regs)
            )
            return dst

        return self.allocate_reg()

    def gen_pattern_bindings(self, pat, val_reg: int):
        if isinstance(pat, BindPat):
            reg = self.allocate_reg()
            self.def_to_reg[pat.def_id] = reg
            self.instructions.append(ionvm.Instruction.move(reg, val_reg))
        elif isinstance(pat, VariantPat):
            if not pat.payload:
                return

            # Extract __slots
            slots_reg = self.allocate_reg()
            prop_reg = self.allocate_reg()
            self.instructions.append(
                ionvm.Instruction.load_const(prop_reg, ionvm.Value.atom("__slots"))
            )
            self.instructions.append(
                ionvm.Instruction.get_prop(slots_reg, val_reg, prop_reg)
            )

            for i, sub_pat in enumerate(pat.payload):
                elem_reg = self.allocate_reg()
                idx_reg = self.allocate_reg()
                self.instructions.append(
                    ionvm.Instruction.load_const(idx_reg, ionvm.Value.atom(str(i)))
                )
                self.instructions.append(
                    ionvm.Instruction.get_prop(elem_reg, slots_reg, idx_reg)
                )
                self.gen_pattern_bindings(sub_pat, elem_reg)

    def convert_pattern(self, pat) -> Any:
        if isinstance(pat, A.WildcardPat):
            return ionvm.Pattern.wildcard()
        if isinstance(pat, BindPat):
            return ionvm.Pattern.wildcard()
        if isinstance(pat, VariantPat):
            inner = ionvm.Pattern.tuple([self.convert_pattern(p) for p in pat.payload])
            return ionvm.Pattern.tagged_enum(pat.variant_name, inner)

        if isinstance(pat, A.TuplePat):
            return ionvm.Pattern.tuple([self.convert_pattern(p) for p in pat.elems])
        if isinstance(pat, A.LitPat):
            return ionvm.Pattern.value(self.lit_to_value(pat.lit))
        return ionvm.Pattern.wildcard()

    def lit_to_value(self, lit) -> Any:
        if isinstance(lit, A.IntLit):
            return ionvm.Value.number(float(lit.value))
        if isinstance(lit, A.FloatLit):
            return ionvm.Value.number(lit.value)
        if isinstance(lit, A.StringLit):
            return ionvm.Value.string(lit.value)
        if isinstance(lit, A.BoolLit):
            return ionvm.Value.boolean(lit.value)
        if isinstance(lit, A.UnitLit):
            return ionvm.Value.unit()
        return ionvm.Value.unit()

    # ========== Environment/Closure Context Management ==========
    def get_capture_source(self, cap_def_id: int, cap_name: str) -> Optional[int]:
        """
        Resolve the register containing a captured binding.
        The closure environment is owned by the process (via its frame stack).
        Returns None if the binding cannot be resolved.
        """
        return self.def_to_reg.get(cap_def_id)

    def collect_transitive_captures(self, lam: A.LambdaExpr) -> Dict[int, str]:
        """
        Collect all captures needed by this lambda, including transitive captures
        from nested lambdas that this lambda references.
        This ensures proper environment chaining through closures.
        """
        direct_captures = self.find_lambda_captures(lam)
        transitive_captures: Dict[int, str] = {}
        
        for def_id, name in direct_captures:
            transitive_captures[def_id] = name
        
        # Check if any captured values are themselves closures with their own captures
        # This is handled implicitly because we call gen_lambda_function which will
        # recursively generate make_closure for nested lambdas.
        return transitive_captures

    def validate_capture_environment(self, captures: List[tuple[int, str]]) -> bool:
        """
        Validate that all captured variables can be resolved in the current scope.
        The process owns the environment via its register state.
        Returns True if all captures are valid, False otherwise.
        """
        for cap_def_id, cap_name in captures:
            if self.def_to_reg.get(cap_def_id) is None:
                # Check if it's a top-level definition
                info = self.def_map.get(cap_def_id)
                if not info:
                    return False
                # Top-level functions don't need to be in registers
                if info.kind != DefKind.FN:
                    return False
        return True
