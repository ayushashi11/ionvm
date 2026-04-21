"""
name_resolution.py
==================
Pass 2 in the pipeline:  Module  →  ResolvedModule

What this pass does
───────────────────
1. Two-phase top-level scan:
     Phase 1 – register every top-level name (fn, struct, enum, protocol)
               so mutual recursion and forward references work.
     Phase 2 – walk all bodies, building local scopes and resolving every
               NameRef / NamePat to a DefId.

2. Mutability tracking:
     const  → DefKind.CONST    (immutable)
     let    → DefKind.LET      (mutable)
     AssignStmt to a CONST binding → ResolutionError

3. Pattern disambiguation:
     NamePat("Foo")  → VariantPat  (uppercase = enum variant constructor)
     NamePat("x")    → BindPat     (lowercase = new local binding)
     EnumPat is always a VariantPat.

4. Error collection (non-fatal – all errors gathered before raising):
     • Undefined name
     • Duplicate definition in same scope
     • Assignment to immutable binding
     • Unknown variant in pattern

Builtin types registered in the module scope
─────────────────────────────────────────────
    Int, Float, Bool, String   – primitive VM value types
    Unit                       – the unit value   (lowers to nil in VM)
    Process                    – process PID       (lowers to int in VM)
    Note: :Symbol syntax is reserved for future enum-variant shorthand.

Output
──────
    ResolvedModule  – mirrors Module but uses resolved node types
    DefMap          – dict[DefId, DefInfo]  (the symbol table)
    List[ResolutionError]
"""

from __future__ import annotations

import itertools
import os
from dataclasses import dataclass, field
from enum import Enum, auto
from typing import Any, Dict, List, Optional, Union

import ast_nodes as A
import ffi_bindings

# ── DefId ────────────────────────────────────────────────────────────────────

_id_counter = itertools.count(1)


def fresh_id() -> int:
    return next(_id_counter)


# ── DefKind ───────────────────────────────────────────────────────────────────


class DefKind(Enum):
    FN = auto()  # fn definition
    STRUCT = auto()  # struct type
    ENUM = auto()  # enum type
    VARIANT = auto()  # enum variant constructor
    PROTOCOL = auto()  # protocol definition
    CONST = auto()  # immutable local binding
    LET = auto()  # mutable local binding
    PARAM = auto()  # function parameter (treated as immutable)
    TYPE_PARAM = auto()  # generic type variable T
    FFI_FN = auto()  # FFI (native) function
    FFI_TYPE = auto()  # FFI (native) type


@dataclass
class DefInfo:
    id: int
    kind: DefKind
    name: str
    node: Any  # original AST node (FnDef, StructDef, etc.)
    # For VARIANT: which enum this belongs to
    enum_name: Optional[str] = None


# ── DefMap ────────────────────────────────────────────────────────────────────

DefMap = Dict[int, DefInfo]


# ── Scope ─────────────────────────────────────────────────────────────────────


@dataclass
class Scope:
    bindings: Dict[str, int] = field(default_factory=dict)  # name → DefId
    parent: Optional["Scope"] = None

    def define(self, name: str, def_id: int) -> None:
        self.bindings[name] = def_id

    def lookup(self, name: str) -> Optional[int]:
        if name in self.bindings:
            return self.bindings[name]
        if self.parent:
            return self.parent.lookup(name)
        return None

    def lookup_local(self, name: str) -> Optional[int]:
        """Only look in THIS scope, not parents."""
        return self.bindings.get(name)


# ── Errors ────────────────────────────────────────────────────────────────────


class ResolutionError(Exception):
    message: str

    def __str__(self) -> str:
        return f"ResolutionError: {self.message}"


# ── Resolved AST nodes ────────────────────────────────────────────────────────
# These replace the ambiguous nodes from ast_nodes.py.


@dataclass
class ResolvedRef:
    """A NameRef whose definition has been found."""

    name: str
    def_id: int


@dataclass
class BindPat:
    """A NamePat that introduces a new local binding."""

    name: str
    def_id: int


@dataclass
class VariantPat:
    """A NamePat / EnumPat referring to an enum variant constructor."""

    enum_name: str
    variant_name: str
    def_id: int
    payload: List[Any]  # resolved sub-patterns


@dataclass
class ResolvedMatchArm:
    pattern: Any  # resolved pattern
    body: Any  # resolved expr


@dataclass
class ResolvedParam:
    name: str
    def_id: int
    type_ann: Optional[Any]


@dataclass
class ResolvedTypeParam:
    name: str
    def_id: int
    constraints: List[Any]


@dataclass
class ResolvedFnDef:
    name: str
    def_id: int
    type_params: List[ResolvedTypeParam]
    params: List[ResolvedParam]
    return_type: Optional[Any]
    body: List[Any]


@dataclass
class ResolvedStructDef:
    name: str
    def_id: int
    type_params: List[ResolvedTypeParam]
    fields: List[A.FieldDef]


@dataclass
class ResolvedEnumDef:
    name: str
    def_id: int
    type_params: List[ResolvedTypeParam]
    variants: List[ResolvedVariantDef]


@dataclass
class ResolvedVariantDef:
    name: str
    def_id: int
    payload: List[Any]  # TypeAnn list


@dataclass
class ResolvedProtocolDef:
    name: str
    def_id: int
    type_params: List[ResolvedTypeParam]
    items: List[Any]  # ResolvedFnDef | ResolvedFnSig


@dataclass
class ResolvedFnSig:
    name: str
    def_id: int
    type_params: List[ResolvedTypeParam]
    params: List[ResolvedParam]
    return_type: Optional[Any]


@dataclass
class ResolvedImplDef:
    self_type: Any
    protocol: Optional[Any]
    methods: List[ResolvedFnDef]


@dataclass
class ResolvedConstStmt:
    name: str
    def_id: int
    type_ann: Optional[Any]
    value: Any


@dataclass
class ResolvedLetStmt:
    name: str
    def_id: int
    type_ann: Optional[Any]
    value: Any


@dataclass
class ResolvedAssignStmt:
    target: str
    def_id: int
    value: Any


@dataclass
class ResolvedModule:
    stmts: List[Any]
    def_map: DefMap


# ── Resolver ──────────────────────────────────────────────────────────────────


class Resolver:
    def __init__(self, ffi_bindings_path: Optional[str] = None):
        self.def_map: DefMap = {}
        self.errors: List[ResolutionError] = []
        self._module_scope: Scope = Scope()
        
        # Load FFI bindings if path provided, otherwise use default location
        if ffi_bindings_path is None:
            # Look for ffi_bindings.json in the same directory as this script
            script_dir = os.path.dirname(os.path.abspath(__file__))
            ffi_bindings_path = os.path.join(script_dir, "ffi_bindings.json")
        
        self.ffi_bindings = ffi_bindings.get_global_ffi_bindings()
        if not self.ffi_bindings.is_loaded():
            self.ffi_bindings.load_from_file(ffi_bindings_path)

    # ── helpers ───────────────────────────────────────────────────────────────

    def _register(
        self,
        scope: Scope,
        name: str,
        kind: DefKind,
        node: Any,
        enum_name: Optional[str] = None,
    ) -> int:
        did = fresh_id()
        info = DefInfo(did, kind, name, node, enum_name)
        self.def_map[did] = info
        if scope.lookup_local(name) is not None:
            self.errors.append(
                ResolutionError(f"'{name}' is already defined in this scope")
            )
        scope.define(name, did)
        return did

    def _error(self, msg: str) -> int:
        """Record an error and return a sentinel DefId (0)."""
        raise ResolutionError(msg)
        #self.errors.append(ResolutionError(msg))
        return 0

    # ── Phase 1: register top-level names ────────────────────────────────────

    def _collect_toplevel(self, stmts: List[Any]) -> None:
        for stmt in stmts:
            if isinstance(stmt, A.FnDef):
                self._register(self._module_scope, stmt.name, DefKind.FN, stmt)
            elif isinstance(stmt, A.StructDef):
                self._register(self._module_scope, stmt.name, DefKind.STRUCT, stmt)
            elif isinstance(stmt, A.EnumDef):
                did = self._register(self._module_scope, stmt.name, DefKind.ENUM, stmt)
                for v in stmt.variants:
                    self._register(
                        self._module_scope,
                        v.name,
                        DefKind.VARIANT,
                        v,
                        enum_name=stmt.name,
                    )
            elif isinstance(stmt, A.ProtocolDef):
                self._register(self._module_scope, stmt.name, DefKind.PROTOCOL, stmt)
            # ImplDef and plain stmts have no top-level name

    # ── Phase 2: resolve bodies ───────────────────────────────────────────────

    _BUILTIN_TYPE_NAMES = {
        "Int",
        "Float",
        "Bool",
        "String",
        "Unit",
        "Process",
    }

    _BUILTIN_FN_NAMES = {
        "debug",
        "print",
        "println",
    }

    def resolve_module(self, module: A.Module) -> ResolvedModule:
        # Register builtin types
        for name in self._BUILTIN_TYPE_NAMES:
            did = fresh_id()
            self.def_map[did] = DefInfo(did, DefKind.STRUCT, name, None)
            self._module_scope.define(name, did)
        
        # Register FFI functions from bindings
        for ffi_fn_name in self.ffi_bindings.functions.keys():
            did = fresh_id()
            self.def_map[did] = DefInfo(did, DefKind.FFI_FN, ffi_fn_name, None)
            self._module_scope.define(ffi_fn_name, did)
        
        # Register FFI object types from bindings
        for ffi_obj_name in self.ffi_bindings.objects.keys():
            did = fresh_id()
            self.def_map[did] = DefInfo(did, DefKind.FFI_TYPE, ffi_obj_name, None)
            self._module_scope.define(ffi_obj_name, did)
        
        # Register legacy builtin functions (for backward compatibility)
        for name in self._BUILTIN_FN_NAMES:
            if name not in self._module_scope.bindings:
                did = fresh_id()
                self.def_map[did] = DefInfo(did, DefKind.FN, name, None)
                self._module_scope.define(name, did)
        
        self._collect_toplevel(module.stmts)
        resolved = [self._resolve_stmt(s, self._module_scope) for s in module.stmts]
        return ResolvedModule(resolved, self.def_map)

    # ── Statements ────────────────────────────────────────────────────────────

    def _resolve_stmt(self, stmt: Any, scope: Scope) -> Any:
        if isinstance(stmt, A.FnDef):
            return self._resolve_fn_def(stmt, scope)
        if isinstance(stmt, A.StructDef):
            return self._resolve_struct_def(stmt, scope)
        if isinstance(stmt, A.EnumDef):
            return self._resolve_enum_def(stmt, scope)
        if isinstance(stmt, A.ProtocolDef):
            return self._resolve_protocol_def(stmt, scope)
        if isinstance(stmt, A.ImplDef):
            return self._resolve_impl_def(stmt, scope)
        if isinstance(stmt, A.ConstStmt):
            return self._resolve_const(stmt, scope)
        if isinstance(stmt, A.LetStmt):
            return self._resolve_let(stmt, scope)
        if isinstance(stmt, A.AssignStmt):
            return self._resolve_assign(stmt, scope)
        if isinstance(stmt, A.ReturnStmt):
            val = self._resolve_expr(stmt.value, scope) if stmt.value else None
            return A.ReturnStmt(val)
        if isinstance(stmt, A.ExprStmt):
            return A.ExprStmt(self._resolve_expr(stmt.expr, scope))
        return stmt  # passthrough for anything unrecognised

    def _resolve_fn_def(self, fn: A.FnDef, outer: Scope) -> ResolvedFnDef:
        # Look up (or register if local fn)
        did = outer.lookup(fn.name)
        if did is None:
            did = self._register(outer, fn.name, DefKind.FN, fn)

        fn_scope = Scope(parent=outer)

        # Type params
        rtp = self._resolve_type_params(fn.type_params, fn_scope)

        # Params
        rparams = []
        for p in fn.params:
            pd = self._register(fn_scope, p.name, DefKind.PARAM, p)
            rparams.append(
                ResolvedParam(
                    p.name,
                    pd,
                    self._resolve_type_ann(p.type_ann, fn_scope)
                    if p.type_ann
                    else None,
                )
            )

        ret = (
            self._resolve_type_ann(fn.return_type, fn_scope) if fn.return_type else None
        )
        body = [self._resolve_stmt(s, fn_scope) for s in fn.body]

        return ResolvedFnDef(fn.name, did, rtp, rparams, ret, body)

    def _resolve_struct_def(self, s: A.StructDef, outer: Scope) -> ResolvedStructDef:
        did = outer.lookup(s.name) or self._register(outer, s.name, DefKind.STRUCT, s)
        sc = Scope(parent=outer)
        rtp = self._resolve_type_params(s.type_params, sc)
        fields = [
            A.FieldDef(f.name, self._resolve_type_ann(f.type_ann, sc)) for f in s.fields
        ]
        return ResolvedStructDef(s.name, did, rtp, fields)

    def _resolve_enum_def(self, e: A.EnumDef, outer: Scope) -> ResolvedEnumDef:
        did = outer.lookup(e.name) or self._register(outer, e.name, DefKind.ENUM, e)
        sc = Scope(parent=outer)
        rtp = self._resolve_type_params(e.type_params, sc)
        variants = []
        for v in e.variants:
            vdid = outer.lookup(v.name) or self._register(
                outer, v.name, DefKind.VARIANT, v, enum_name=e.name
            )
            payload = [self._resolve_type_ann(t, sc) for t in v.payload]
            variants.append(ResolvedVariantDef(v.name, vdid, payload))
        return ResolvedEnumDef(e.name, did, rtp, variants)

    def _resolve_protocol_def(
        self, p: A.ProtocolDef, outer: Scope
    ) -> ResolvedProtocolDef:
        did = outer.lookup(p.name) or self._register(outer, p.name, DefKind.PROTOCOL, p)
        sc = Scope(parent=outer)
        rtp = self._resolve_type_params(p.type_params, sc)
        # self type available inside protocol
        self_did = self._register(sc, "Self", DefKind.TYPE_PARAM, None)
        items = []
        for item in p.items:
            if isinstance(item, A.FnDef):
                items.append(self._resolve_fn_def(item, sc))
            elif isinstance(item, A.FnSig):
                items.append(self._resolve_fn_sig(item, sc))
        return ResolvedProtocolDef(p.name, did, rtp, items)

    def _resolve_fn_sig(self, sig: A.FnSig, outer: Scope) -> ResolvedFnSig:
        did = self._register(outer, sig.name, DefKind.FN, sig)
        sc = Scope(parent=outer)
        rtp = self._resolve_type_params(sig.type_params, sc)
        rparams = []
        for p in sig.params:
            pd = self._register(sc, p.name, DefKind.PARAM, p)
            rparams.append(
                ResolvedParam(
                    p.name,
                    pd,
                    self._resolve_type_ann(p.type_ann, sc) if p.type_ann else None,
                )
            )
        ret = self._resolve_type_ann(sig.return_type, sc) if sig.return_type else None
        return ResolvedFnSig(sig.name, did, rtp, rparams, ret)

    def _resolve_impl_def(self, impl: A.ImplDef, outer: Scope) -> ResolvedImplDef:
        sc = Scope(parent=outer)
        stype = self._resolve_type_ann(impl.self_type, sc)
        proto = self._resolve_type_ann(impl.protocol, sc) if impl.protocol else None
        methods = [self._resolve_fn_def(m, sc) for m in impl.methods]
        return ResolvedImplDef(stype, proto, methods)

    def _resolve_const(self, stmt: A.ConstStmt, scope: Scope) -> ResolvedConstStmt:
        val = self._resolve_expr(stmt.value, scope)
        typ = self._resolve_type_ann(stmt.type_ann, scope) if stmt.type_ann else None
        did = self._register(scope, stmt.name, DefKind.CONST, stmt)
        return ResolvedConstStmt(stmt.name, did, typ, val)

    def _resolve_let(self, stmt: A.LetStmt, scope: Scope) -> ResolvedLetStmt:
        val = self._resolve_expr(stmt.value, scope)
        typ = self._resolve_type_ann(stmt.type_ann, scope) if stmt.type_ann else None
        did = self._register(scope, stmt.name, DefKind.LET, stmt)
        return ResolvedLetStmt(stmt.name, did, typ, val)

    def _resolve_assign(self, stmt: A.AssignStmt, scope: Scope) -> ResolvedAssignStmt:
        did = scope.lookup(stmt.target)
        if did is None:
            did = self._error(f"Undefined name '{stmt.target}'")
        else:
            info = self.def_map.get(did)
            if info and info.kind in (DefKind.CONST, DefKind.PARAM):
                self._error(f"Cannot assign to immutable binding '{stmt.target}'")
        val = self._resolve_expr(stmt.value, scope)
        return ResolvedAssignStmt(stmt.target, did or 0, val)

    # ── Type parameters ───────────────────────────────────────────────────────

    def _resolve_type_params(
        self, tps: List[A.TypeParam], scope: Scope
    ) -> List[ResolvedTypeParam]:
        out = []
        for tp in tps:
            did = self._register(scope, tp.name, DefKind.TYPE_PARAM, tp)
            constraints = [self._resolve_type_ann(c, scope) for c in tp.constraints]
            out.append(ResolvedTypeParam(tp.name, did, constraints))
        return out

    # ── Type annotations ─────────────────────────────────────────────────────

    def _resolve_type_ann(self, ann: Any, scope: Scope) -> Any:
        if ann is None:
            return None
        if isinstance(ann, A.NamedType):
            did = scope.lookup(ann.name)
            if did is None:
                self._error(f"Unknown type '{ann.name}'")
            args = [self._resolve_type_ann(a, scope) for a in ann.args]
            return A.NamedType(ann.name, args)
        if isinstance(ann, A.TupleType):
            return A.TupleType([self._resolve_type_ann(e, scope) for e in ann.elems])
        if isinstance(ann, A.FnType):
            return A.FnType(
                [self._resolve_type_ann(p, scope) for p in ann.params],
                self._resolve_type_ann(ann.ret, scope),
            )
        return ann  # SelfType or unknown passthrough

    # ── Expressions ──────────────────────────────────────────────────────────

    def _resolve_expr(self, expr: Any, scope: Scope) -> Any:
        if expr is None:
            return None

        # Literals pass through unchanged
        if isinstance(expr, (A.IntLit, A.FloatLit, A.StringLit, A.BoolLit, A.UnitLit)):
            return expr

        if isinstance(expr, A.NameRef):
            did = scope.lookup(expr.name)
            if did is None:
                self._error(f"Undefined name '{expr.name}'")
                return ResolvedRef(expr.name, 0)
            return ResolvedRef(expr.name, did)

        if isinstance(expr, A.BinOp):
            return A.BinOp(
                expr.op,
                self._resolve_expr(expr.left, scope),
                self._resolve_expr(expr.right, scope),
            )

        if isinstance(expr, A.UnaryOp):
            return A.UnaryOp(expr.op, self._resolve_expr(expr.operand, scope))

        if isinstance(expr, A.Call):
            return A.Call(
                self._resolve_expr(expr.callee, scope),
                [self._resolve_expr(a, scope) for a in expr.args],
            )

        if isinstance(expr, A.FieldAccess):
            return A.FieldAccess(self._resolve_expr(expr.obj, scope), expr.field)

        if isinstance(expr, A.IndexExpr):
            return A.IndexExpr(
                self._resolve_expr(expr.obj, scope),
                self._resolve_expr(expr.index, scope),
            )

        if isinstance(expr, A.TupleLit):
            return A.TupleLit([self._resolve_expr(e, scope) for e in expr.elems])

        if isinstance(expr, A.ArrayLit):
            return A.ArrayLit([self._resolve_expr(e, scope) for e in expr.elems])

        if isinstance(expr, A.IfExpr):
            return self._resolve_if(expr, scope)

        if isinstance(expr, A.MatchExpr):
            return self._resolve_match(expr, scope)

        if isinstance(expr, A.SpawnExpr):
            did = scope.lookup(expr.func)
            if did is None:
                self._error(f"Undefined function '{expr.func}'")
            return A.SpawnExpr(
                expr.func,
                [self._resolve_expr(a, scope) for a in expr.args],
                did,
            )

        if isinstance(expr, A.SendExpr):
            return A.SendExpr(
                self._resolve_expr(expr.pid, scope), self._resolve_expr(expr.msg, scope)
            )

        if isinstance(expr, A.ReceiveExpr):
            return expr

        if isinstance(expr, A.LambdaExpr):
            return self._resolve_lambda(expr, scope)

        return expr  # passthrough

    def _resolve_if(self, expr: A.IfExpr, scope: Scope) -> A.IfExpr:
        cond = self._resolve_expr(expr.cond, scope)
        then_scope = Scope(parent=scope)
        then_body = [self._resolve_stmt(s, then_scope) for s in expr.then_body]
        else_body = None
        if expr.else_body is not None:
            if isinstance(expr.else_body, A.IfExpr):
                else_body = self._resolve_if(expr.else_body, scope)
            else:
                else_scope = Scope(parent=scope)
                else_body = [
                    self._resolve_stmt(s, else_scope) for s in expr.else_body
                ]
        return A.IfExpr(cond, then_body, else_body)

    def _resolve_match(self, expr: A.MatchExpr, scope: Scope) -> A.MatchExpr:
        subject = self._resolve_expr(expr.subject, scope)
        arms = []
        for arm in expr.arms:
            arm_scope = Scope(parent=scope)
            pat = self._resolve_pattern(arm.pattern, arm_scope)
            body = self._resolve_expr(arm.body, arm_scope)
            arms.append(ResolvedMatchArm(pat, body))
        return A.MatchExpr(subject, arms)

    def _resolve_lambda(self, lam: A.LambdaExpr, outer: Scope) -> A.LambdaExpr:
        sc = Scope(parent=outer)
        rtp = self._resolve_type_params(lam.type_params, sc)
        rparams = []
        for p in lam.params:
            pd = self._register(sc, p.name, DefKind.PARAM, p)
            rparams.append(
                ResolvedParam(
                    p.name,
                    pd,
                    self._resolve_type_ann(p.type_ann, sc) if p.type_ann else None,
                )
            )
        ret = self._resolve_type_ann(lam.return_type, sc) if lam.return_type else None
        body = [self._resolve_stmt(s, sc) for s in lam.body]
        return A.LambdaExpr(rtp, rparams, ret, body)

    # ── Patterns ─────────────────────────────────────────────────────────────

    def _resolve_pattern(self, pat: Any, scope: Scope) -> Any:
        if isinstance(pat, A.WildcardPat):
            return pat

        if isinstance(pat, A.NamePat):
            name = pat.name
            # Uppercase first letter → variant reference
            if name[0].isupper():
                did = scope.lookup(name)
                if did is None:
                    self._error(f"Unknown variant '{name}' in pattern")
                    return pat
                info = self.def_map.get(did)
                enum_name = info.enum_name if info else "?"
                return VariantPat(enum_name, name, did, [])
            else:
                # Lowercase → new binding
                did = self._register(scope, name, DefKind.CONST, pat)
                return BindPat(name, did)

        if isinstance(pat, A.EnumPat):
            did = scope.lookup(pat.variant)
            if did is None:
                self._error(f"Unknown variant '{pat.variant}' in pattern")
                did = 0
            info = self.def_map.get(did)
            enum_name = info.enum_name if info else "?"
            payload = [self._resolve_pattern(p, scope) for p in pat.payload]
            return VariantPat(enum_name, pat.variant, did, payload)

        if isinstance(pat, A.TuplePat):
            return A.TuplePat([self._resolve_pattern(p, scope) for p in pat.elems])

        if isinstance(pat, A.LitPat):
            return pat

        return pat


# ── Public API ────────────────────────────────────────────────────────────────


def resolve(module: A.Module, ffi_bindings_path: Optional[str] = None) -> tuple[ResolvedModule, List[ResolutionError]]:
    """
    Resolve a parsed Module.
    Args:
        module: The parsed AST module
        ffi_bindings_path: Optional path to FFI bindings JSON file. If not provided,
                          looks for ffi_bindings.json in the same directory.
    Returns (ResolvedModule, errors).
    Raises ValueError if there were any errors (caller can suppress if desired).
    """
    r = Resolver(ffi_bindings_path)
    resolved = r.resolve_module(module)
    return resolved, r.errors
