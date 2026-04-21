"""
typed_ast.py
============
Pass 3 in the pipeline:  ResolvedModule  →  TModule

What this pass does
───────────────────
1. Defines the Type algebra that mirrors the VM's value types:

     TyInt      ↔  Value::int    (i64)
     TyFloat    ↔  Value::float  (f64)
     TyBool     ↔  Value::bool
     TyString   ↔  Value::string
     TyUnit     ↔  Value::nil
     TyObject   ↔  Value::object  (struct instance; fields via properties)
     TyEnum     ↔  Value::object  (tagged via prototype / type_tag property)
     TyFn       ↔  Value::function  (Regular / Native variant)
     TyClosure  ↔  Value::function  (Closure variant; carries env id)

     -- AST-only types (VM support pending) --
     TyTuple    —  fixed-length product  (a, b, c)
     TyArray    —  homogeneous list      [T]

     -- Semantic / compiler-internal types --
     TyProcess  —  process PID; lowers to Value::int at codegen
     TyVar      —  unification variable  (never reaches codegen)
     TyGeneric  —  instantiated generic slot

2. Constraint-based type inference (Hindley-Milner flavour):
     • infer_expr() / infer_stmt() return a (typed_node, Type) pair and
       accumulate equality constraints.
     • unify() solves the constraints, building a substitution map.
     • apply_subst() walks the entire TModule replacing TyVar with
       the solved concrete types.

3. Protocol conformance:
     • Each ImplDef registers a WitnessTable: (SelfType, ProtocolName) →
       { method_name: TFnDef }
     • check_constraint(ty, protocol) verifies a witness table exists.

Output
──────
    TModule — fully typed module ready for your lowering / codegen pass.
    Every expression node is wrapped in TExpr(inner, ty).
    Every function carries resolved param types + return type.
"""

from __future__ import annotations

import itertools
from dataclasses import dataclass, field
from enum import Enum, auto
from typing import Any, Dict, List, Optional, Union
from typing import Tuple as PyTuple

import ast_nodes as A
import ffi_bindings
from name_resolution import (
    BindPat,
    DefKind,
    DefMap,
    ResolvedAssignStmt,
    ResolvedConstStmt,
    ResolvedEnumDef,
    ResolvedFnDef,
    ResolvedFnSig,
    ResolvedImplDef,
    ResolvedLetStmt,
    ResolvedMatchArm,
    ResolvedModule,
    ResolvedParam,
    ResolvedProtocolDef,
    ResolvedRef,
    ResolvedStructDef,
    ResolvedTypeParam,
    ResolvedVariantDef,
    VariantPat,
)

# ═══════════════════════════════════════════════════════════════════════════════
# Type algebra
# ═══════════════════════════════════════════════════════════════════════════════

_tv_counter = itertools.count(1)


def fresh_tv() -> "TyVar":
    return TyVar(next(_tv_counter))


@dataclass(frozen=True)
class TyVar:
    """Unification variable — must not appear in final typed AST."""

    id: int

    def __repr__(self):
        return f"?t{self.id}"


@dataclass(frozen=True)
class TyInt:
    """Int"""

    def __repr__(self):
        return "Int"


@dataclass(frozen=True)
class TyFloat:
    """Float"""

    def __repr__(self):
        return "Float"


@dataclass(frozen=True)
class TyBool:
    def __repr__(self):
        return "Bool"


@dataclass(frozen=True)
class TyString:
    def __repr__(self):
        return "String"


@dataclass(frozen=True)
class TyUnit:
    """The unit value ()  — lowers to Value::nil in the VM."""

    def __repr__(self):
        return "Unit"


@dataclass(frozen=True)
class TyTuple:
    """Fixed-length product type (A, B, C)  — AST-only; no VM Value variant yet."""

    elems: PyTuple  # PyTuple[Type, ...]

    def __repr__(self):
        return f"({', '.join(map(repr, self.elems))})"


@dataclass(frozen=True)
class TyArray:
    """Homogeneous list [T]  — AST-only; no VM Value variant yet."""

    elem: Any  # Type

    def __repr__(self):
        return f"[{self.elem!r}]"


@dataclass(frozen=True)
class TyObject:
    """Struct instance — VM Value::object; fields stored as Object properties."""

    name: str
    fields: PyTuple  # PyTuple[(field_name, Type), ...]

    def __repr__(self):
        return f"struct {self.name}"


@dataclass(frozen=True)
class TyEnum:
    """Sum type — VM Value::object; variant tag stored as a property."""

    name: str
    variants: PyTuple  # PyTuple[(variant_name, PyTuple[Type,...]), ...]

    def __repr__(self):
        return f"enum {self.name}"


@dataclass(frozen=True)
class TyFn:
    """Named function — VM Value::Function."""

    params: PyTuple  # PyTuple[Type, ...]
    ret: Any  # Type
    variadic: bool = False

    def __repr__(self):
        v = "..." if self.variadic else ""
        return f"fn({', '.join(map(repr, self.params))}{v}) -> {self.ret!r}"


@dataclass(frozen=True)
class TyClosure:
    """Anonymous lambda — VM Value::function (Closure variant, carries closure env id)."""

    params: PyTuple
    ret: Any
    variadic: bool = False

    def __repr__(self):
        v = "..." if self.variadic else ""
        return f"closure({', '.join(map(repr, self.params))}{v}) -> {self.ret!r}"


@dataclass(frozen=True)
class TyProcess:
    """
    Process PID — the VM has no dedicated process value type;
    a PID is just a Value::int (the process's integer id).
    This type exists in the type checker for semantic clarity and is
    lowered to int at codegen time.
    """

    def __repr__(self):
        return "Process"


@dataclass(frozen=True)
class TyGeneric:
    """
    An instantiated generic type parameter.
    After monomorphisation / at the call site this is replaced
    by the actual concrete type.
    """

    name: str

    def __repr__(self):
        return self.name


Type = Union[
    TyVar,
    TyInt,
    TyFloat,
    TyBool,
    TyString,
    TyUnit,
    TyTuple,
    TyArray,
    TyObject,
    TyEnum,
    TyFn,
    TyClosure,
    TyProcess,
    TyGeneric,
]

# Built-in type name → Type mapping (for resolving NamedType annotations)
_BUILTIN_TYPES: Dict[str, Type] = {
    "Int": TyInt(),
    "Float": TyFloat(),
    "Bool": TyBool(),
    "String": TyString(),
    "Unit": TyUnit(),  # lowers to nil
    "Process": TyProcess(),  # lowers to int (PID)
}

_BUILTIN_FNS: Dict[str, Type] = {
    "debug": TyFn((TyGeneric("T"),), TyUnit()),
    "print": TyFn((TyGeneric("T"),), TyUnit(), variadic=True),
    "println": TyFn((TyGeneric("T"),), TyUnit(), variadic=True),
}


def _build_ffi_function_types() -> Dict[str, Type]:
    """Build type signatures for FFI functions from bindings."""
    ffi_registry = ffi_bindings.get_global_ffi_bindings()
    ffi_types: Dict[str, Type] = {}
    
    for fn_name, fn_binding in ffi_registry.functions.items():
        # All FFI function parameters and returns are Float (numeric or generic)
        # String functions work on String type
        param_types = []
        
        if "String" in fn_name or "Str" in fn_name:
            # String functions
            if fn_name == "StrLength" or fn_name == "StrUpper" or fn_name == "StrLower" or fn_name == "StrTrim":
                param_types = (TyString(),)
            elif fn_name == "StrConcat":
                param_types = (TyString(), TyString())
            elif fn_name == "StrSplit":
                param_types = (TyString(), TyString())
        else:
            # Math functions - all Float
            param_types = tuple(TyFloat() for _ in range(fn_binding.arity))
        
        # Return type
        if "Str" in fn_name:
            if fn_binding.return_type == "Float":
                ret_type = TyFloat()
            elif fn_binding.return_type == "String":
                ret_type = TyString()
            elif fn_binding.return_type == "Array":
                ret_type = TyArray(TyString())
            elif fn_binding.return_type == "Unit":
                ret_type = TyUnit()
            else:
                ret_type = TyFloat()
        else:
            if fn_binding.return_type == "Array":
                ret_type = TyArray(TyFloat())
            elif fn_binding.return_type == "Unit":
                ret_type = TyUnit()
            else:
                ret_type = TyFloat()
        
        ffi_types[fn_name] = TyFn(param_types, ret_type)
    
    return ffi_types


# ═══════════════════════════════════════════════════════════════════════════════
# Typed AST nodes
# ═══════════════════════════════════════════════════════════════════════════════


@dataclass
class TExpr:
    """Every expression in the typed AST is wrapped with its resolved type."""

    inner: Any  # the original (resolved) expression node
    ty: Type


@dataclass
class TParam:
    name: str
    def_id: int
    ty: Type


@dataclass
class TFnDef:
    name: str
    def_id: int
    type_params: List[str]  # generic param names (for codegen note)
    params: List[TParam]
    return_ty: Type
    body: List[Any]  # List[typed stmt]
    # Derived info for the VM lowering pass
    arity: int = 0  # set post-construction
    is_method: bool = False

    def __post_init__(self):
        self.arity = len(self.params)


@dataclass
class TStructDef:
    name: str
    def_id: int
    type_params: List[str]
    fields: List[PyTuple]  # [(field_name, Type), ...]


@dataclass
class TEnumDef:
    name: str
    def_id: int
    type_params: List[str]
    variants: List[PyTuple]  # [(variant_name, [Type,...]), ...]


@dataclass
class TProtocolDef:
    name: str
    def_id: int
    type_params: List[str]
    sigs: List[TFnDef]  # may have dummy bodies for abstract sigs


@dataclass
class TWitnessTable:
    """
    Records that `self_ty` implements `protocol_name`.
    Maps each required method name to its concrete TFnDef.
    Produced for each ImplDef — the lowering pass uses this for dispatch.
    """

    self_ty: Type
    protocol_name: str
    methods: Dict[str, TFnDef]


@dataclass
class TImplDef:
    self_ty: Type
    protocol: Optional[str]  # None = inherent impl
    methods: List[TFnDef]
    witness: Optional[TWitnessTable]  # set when protocol is not None


@dataclass
class TConstStmt:
    name: str
    def_id: int
    ty: Type
    value: TExpr
    mutable: bool = False  # False=const, True=let


@dataclass
class TAssignStmt:
    target: str
    def_id: int
    value: TExpr


@dataclass
class TReturnStmt:
    value: Optional[TExpr]


@dataclass
class TExprStmt:
    expr: TExpr


@dataclass
class TModule:
    fns: List[TFnDef]
    structs: List[TStructDef]
    enums: List[TEnumDef]
    protocols: List[TProtocolDef]
    impls: List[TImplDef]
    witnesses: List[TWitnessTable]
    # Top-level non-decl statements (scripts / top-level let)
    stmts: List[Any]


# ═══════════════════════════════════════════════════════════════════════════════
# Unification
# ═══════════════════════════════════════════════════════════════════════════════


class UnificationError(Exception):
    pass


Subst = Dict[int, Type]  # TyVar.id → Type


def occurs(tv_id: int, ty: Type) -> bool:
    """Occurs check — prevents infinite types."""
    if isinstance(ty, TyVar):
        return ty.id == tv_id
    if isinstance(ty, TyTuple):
        return any(occurs(tv_id, e) for e in ty.elems)
    if isinstance(ty, TyArray):
        return occurs(tv_id, ty.elem)
    if isinstance(ty, (TyFn, TyClosure)):
        return any(occurs(tv_id, p) for p in ty.params) or occurs(tv_id, ty.ret)
    if isinstance(ty, TyObject):
        return any(occurs(tv_id, ft) for _, ft in ty.fields)
    if isinstance(ty, TyEnum):
        return any(any(occurs(tv_id, t) for t in payload) for _, payload in ty.variants)
    return False


def apply_subst_ty(subst: Subst, ty: Type) -> Type:
    """Walk a Type replacing all solved TyVars."""
    if isinstance(ty, TyVar):
        if ty.id in subst:
            return apply_subst_ty(subst, subst[ty.id])
        return ty
    if isinstance(ty, TyGeneric):
        if ty.name in subst:
            return apply_subst_ty(subst, subst[ty.name])
        return ty
    if isinstance(ty, TyTuple):
        return TyTuple(tuple(apply_subst_ty(subst, e) for e in ty.elems))
    if isinstance(ty, TyArray):
        return TyArray(apply_subst_ty(subst, ty.elem))
    if isinstance(ty, TyFn):
        return TyFn(
            tuple(apply_subst_ty(subst, p) for p in ty.params),
            apply_subst_ty(subst, ty.ret),
            variadic=ty.variadic,
        )
    if isinstance(ty, TyClosure):
        return TyClosure(
            tuple(apply_subst_ty(subst, p) for p in ty.params),
            apply_subst_ty(subst, ty.ret),
            variadic=ty.variadic,
        )
    if isinstance(ty, TyObject):
        return TyObject(
            ty.name, tuple((n, apply_subst_ty(subst, t)) for n, t in ty.fields)
        )
    if isinstance(ty, TyEnum):
        return TyEnum(
            ty.name,
            tuple(
                (n, tuple(apply_subst_ty(subst, t) for t in payload))
                for n, payload in ty.variants
            ),
        )
    return ty  # primitive / TyGeneric


def _subst_generics(ty: Type, mapping: Dict[str, Type]) -> Type:
    """
    Apply a name→Type mapping to every TyGeneric node in ty.
    Used for type-argument application, e.g. Result[Float] → substitute T→Float.
    """
    if isinstance(ty, TyGeneric):
        return mapping.get(ty.name, ty)
    if isinstance(ty, TyFn):
        return TyFn(
            tuple(_subst_generics(p, mapping) for p in ty.params),
            _subst_generics(ty.ret, mapping),
            variadic=ty.variadic,
        )
    if isinstance(ty, TyClosure):
        return TyClosure(
            tuple(_subst_generics(p, mapping) for p in ty.params),
            _subst_generics(ty.ret, mapping),
            variadic=ty.variadic,
        )
    if isinstance(ty, TyTuple):
        return TyTuple(tuple(_subst_generics(e, mapping) for e in ty.elems))
    if isinstance(ty, TyArray):
        return TyArray(_subst_generics(ty.elem, mapping))
    if isinstance(ty, TyObject):
        return TyObject(
            ty.name,
            tuple((n, _subst_generics(ft, mapping)) for n, ft in ty.fields),
        )
    if isinstance(ty, TyEnum):
        return TyEnum(
            ty.name,
            tuple(
                (n, tuple(_subst_generics(pt, mapping) for pt in pts))
                for n, pts in ty.variants
            ),
        )
    return ty


def unify(subst: Subst, a: Type, b: Type) -> None:
    """
    Unify types a and b, updating subst in-place.
    Raises UnificationError on mismatch.
    """
    a = apply_subst_ty(subst, a)
    b = apply_subst_ty(subst, b)

    if a == b:
        return

    if isinstance(a, TyVar):
        if occurs(a.id, b):
            raise UnificationError(f"Infinite type: {a} occurs in {b}")
        subst[a.id] = b
        return

    if isinstance(b, TyVar):
        unify(subst, b, a)
        return

    if isinstance(a, TyTuple) and isinstance(b, TyTuple):
        if len(a.elems) != len(b.elems):
            raise UnificationError(f"Tuple length mismatch: {a} vs {b}")
        for ea, eb in zip(a.elems, b.elems):
            unify(subst, ea, eb)
        return

    if isinstance(a, TyArray) and isinstance(b, TyArray):
        unify(subst, a.elem, b.elem)
        return

    if isinstance(a, TyObject) and isinstance(b, TyObject):
        if a.name != b.name or len(a.fields) != len(b.fields):
            raise UnificationError(f"Cannot unify {a!r} with {b!r}")
        for (an, at), (bn, bt) in zip(a.fields, b.fields):
            if an != bn:
                raise UnificationError(f"Struct field mismatch: {an!r} vs {bn!r}")
            unify(subst, at, bt)
        return

    if isinstance(a, TyEnum) and isinstance(b, TyEnum):
        if a.name != b.name or len(a.variants) != len(b.variants):
            raise UnificationError(f"Cannot unify {a!r} with {b!r}")
        for (an, ats), (bn, bts) in zip(a.variants, b.variants):
            if an != bn or len(ats) != len(bts):
                raise UnificationError(f"Variant mismatch: {an!r} vs {bn!r}")
            for at, bt in zip(ats, bts):
                unify(subst, at, bt)
        return

    # TyFn and TyClosure are both callable — unify them structurally
    if isinstance(a, (TyFn, TyClosure)) and isinstance(b, (TyFn, TyClosure)):
        # If either is variadic, we only check that we have at least as many
        # arguments as the non-variadic prefix.
        if a.variadic or b.variadic:
            min_params = min(len(a.params), len(b.params))
            for i in range(min_params):
                unify(subst, a.params[i], b.params[i])
            unify(subst, a.ret, b.ret)
            return

        if len(a.params) != len(b.params):
            raise UnificationError(
                f"Callable arity mismatch: {a} ({len(a.params)} params) "
                f"vs {b} ({len(b.params)} params)"
            )
        for pa, pb in zip(a.params, b.params):
            unify(subst, pa, pb)
        unify(subst, a.ret, b.ret)
        return

    raise UnificationError(f"Cannot unify {a!r} with {b!r}")


# ═══════════════════════════════════════════════════════════════════════════════
# Type environment
# ═══════════════════════════════════════════════════════════════════════════════


@dataclass
class TypeEnv:
    """Maps DefId → Type. Supports nested scopes via parent chain."""

    bindings: Dict[int, Type] = field(default_factory=dict)
    parent: Optional["TypeEnv"] = None

    def bind(self, def_id: int, ty: Type) -> None:
        self.bindings[def_id] = ty

    def lookup(self, def_id: int) -> Optional[Type]:
        if def_id in self.bindings:
            return self.bindings[def_id]
        if self.parent:
            return self.parent.lookup(def_id)
        return None

    def child(self) -> "TypeEnv":
        return TypeEnv(parent=self)


# ═══════════════════════════════════════════════════════════════════════════════
# Type checker / inference engine
# ═══════════════════════════════════════════════════════════════════════════════


@dataclass
class TypeError_:
    message: str

    def __str__(self):
        return f"TypeError: {self.message}"


class TypeChecker:
    def __init__(self, def_map: DefMap):
        self.def_map = def_map
        self.subst: Subst = {}
        self.errors: List[TypeError_] = []
        self.witnesses: Dict[PyTuple, TWitnessTable] = {}  # (self_ty, proto) → witness
        # Global type env populated during first-pass of top-level decls
        self._global_env = TypeEnv()
        # Struct/enum type info: name → TyObject | TyEnum
        self._type_defs: Dict[str, Type] = {}
        # Generic type-parameter names in declaration order: type_name → [param_names]
        self._type_params: Dict[str, List[str]] = {}
        # Protocol sigs: proto_name → {method_name: TyFn}
        self._protocol_sigs: Dict[str, Dict[str, TyFn]] = {}

    # ── errors ────────────────────────────────────────────────────────────────

    def _err(self, msg: str) -> Type:
        self.errors.append(TypeError_(msg))
        return fresh_tv()

    def _require_numeric(self, ty: Type, ctx: str) -> None:
        """
        Verify that ty is (or could be) a numeric type: Int, Float, an
        unresolved TyVar, or a TyGeneric type-parameter.
        Issues a TypeError_ if it is already resolved to a non-numeric type.
        """
        resolved = apply_subst_ty(self.subst, ty)
        if not isinstance(resolved, (TyInt, TyFloat, TyVar, TyGeneric)):
            self._err(f"expected numeric type in {ctx}, got {resolved!r}")

    def _instantiate(self, ty: Type) -> Type:
        """
        Freshen a polymorphic type for one specific call site: every TyGeneric
        param is replaced with a brand-new TyVar so different call sites can
        each resolve the type arguments independently (HM let-polymorphism).
        """
        seen: Dict[str, TyVar] = {}

        def walk(t: Type) -> Type:
            if isinstance(t, TyGeneric):
                if t.name not in seen:
                    seen[t.name] = fresh_tv()
                return seen[t.name]
            if isinstance(t, TyFn):
                return TyFn(
                    tuple(walk(p) for p in t.params), walk(t.ret), variadic=t.variadic
                )
            if isinstance(t, TyClosure):
                return TyClosure(
                    tuple(walk(p) for p in t.params), walk(t.ret), variadic=t.variadic
                )
            if isinstance(t, TyTuple):
                return TyTuple(tuple(walk(e) for e in t.elems))
            if isinstance(t, TyArray):
                return TyArray(walk(t.elem))
            if isinstance(t, TyObject):
                return TyObject(t.name, tuple((n, walk(ft)) for n, ft in t.fields))
            if isinstance(t, TyEnum):
                return TyEnum(
                    t.name,
                    tuple((n, tuple(walk(pt) for pt in pts)) for n, pts in t.variants),
                )
            return t

        return walk(ty)

    def _unify(self, a: Type, b: Type, ctx: str = "") -> None:
        try:
            unify(self.subst, a, b)
        except UnificationError as e:
            self.errors.append(TypeError_(f"{e}{' in ' + ctx if ctx else ''}"))

    # ── annotation → Type ────────────────────────────────────────────────────

    def _ann_to_type(self, ann: Any, generic_map: Dict[str, Type]) -> Type:
        if ann is None:
            return fresh_tv()
        if isinstance(ann, A.NamedType):
            if ann.name in generic_map:
                return generic_map[ann.name]
            if ann.name in _BUILTIN_TYPES:
                return _BUILTIN_TYPES[ann.name]
            if ann.name in self._type_defs:
                base_ty = self._type_defs[ann.name]
                if ann.args:
                    # Apply type arguments: e.g. Result[Float] → substitute T→Float
                    param_names = self._type_params.get(ann.name, [])
                    arg_types = [self._ann_to_type(a, generic_map) for a in ann.args]
                    mapping = dict(zip(param_names, arg_types))
                    return _subst_generics(base_ty, mapping)
                return base_ty
            return self._err(f"Unknown type '{ann.name}'")
        if isinstance(ann, A.TupleType):
            return TyTuple(tuple(self._ann_to_type(e, generic_map) for e in ann.elems))
        if isinstance(ann, A.FnType):
            return TyFn(
                tuple(self._ann_to_type(p, generic_map) for p in ann.params),
                self._ann_to_type(ann.ret, generic_map),
            )
        if isinstance(ann, A.SelfType):
            return generic_map.get("Self", fresh_tv())
        return fresh_tv()

    def _make_generic_map(
        self, type_params: List[ResolvedTypeParam]
    ) -> Dict[str, Type]:
        """Create a fresh TyVar (or TyGeneric) for each type parameter."""
        return {tp.name: TyGeneric(tp.name) for tp in type_params}

    # ── first pass: register all top-level decl types ────────────────────────

    def _register_toplevel(self, stmts: List[Any]) -> None:
        # Register built-in functions
        ffi_fn_types = _build_ffi_function_types()
        for info in self.def_map.values():
            if info.name in _BUILTIN_FNS:
                self._global_env.bind(info.id, _BUILTIN_FNS[info.name])
            elif info.kind == DefKind.FFI_FN and info.name in ffi_fn_types:
                self._global_env.bind(info.id, ffi_fn_types[info.name])

        for stmt in stmts:
            if isinstance(stmt, ResolvedStructDef):
                gmap = self._make_generic_map(stmt.type_params)
                fields = tuple(
                    (f.name, self._ann_to_type(f.type_ann, gmap)) for f in stmt.fields
                )
                ty = TyObject(stmt.name, fields)
                self._type_defs[stmt.name] = ty
                self._type_params[stmt.name] = [tp.name for tp in stmt.type_params]
                self._global_env.bind(stmt.def_id, ty)

            elif isinstance(stmt, ResolvedEnumDef):
                gmap = self._make_generic_map(stmt.type_params)
                variants = tuple(
                    (v.name, tuple(self._ann_to_type(t, gmap) for t in v.payload))
                    for v in stmt.variants
                )
                ty = TyEnum(stmt.name, variants)
                self._type_defs[stmt.name] = ty
                self._type_params[stmt.name] = [tp.name for tp in stmt.type_params]
                self._global_env.bind(stmt.def_id, ty)
                # Register each variant constructor
                for v in stmt.variants:
                    payload_types = tuple(self._ann_to_type(t, gmap) for t in v.payload)
                    if payload_types:
                        vty = TyFn(payload_types, ty)
                    else:
                        vty = ty
                    self._global_env.bind(v.def_id, vty)

            elif isinstance(stmt, ResolvedProtocolDef):
                sigs = {}
                for item in stmt.items:
                    if isinstance(item, (ResolvedFnSig, ResolvedFnDef)):
                        gmap = self._make_generic_map(item.type_params)
                        ptypes = tuple(
                            self._ann_to_type(p.type_ann, gmap) for p in item.params
                        )
                        rtype = self._ann_to_type(item.return_type, gmap)
                        sigs[item.name] = TyFn(ptypes, rtype)
                self._protocol_sigs[stmt.name] = sigs

            elif isinstance(stmt, ResolvedFnDef):
                gmap = self._make_generic_map(stmt.type_params)
                ptypes = tuple(self._ann_to_type(p.type_ann, gmap) for p in stmt.params)
                rtype = self._ann_to_type(stmt.return_type, gmap)
                ty = TyFn(ptypes, rtype)
                self._global_env.bind(stmt.def_id, ty)

    # ── main entry ────────────────────────────────────────────────────────────

    def check_module(self, rmod: ResolvedModule) -> TModule:
        self._register_toplevel(rmod.stmts)

        t_fns: List[TFnDef] = []
        t_structs: List[TStructDef] = []
        t_enums: List[TEnumDef] = []
        t_protocols: List[TProtocolDef] = []
        t_impls: List[TImplDef] = []
        t_stmts: List[Any] = []

        for stmt in rmod.stmts:
            if isinstance(stmt, ResolvedFnDef):
                t_fns.append(self._check_fn(stmt, self._global_env))
            elif isinstance(stmt, ResolvedStructDef):
                t_structs.append(self._check_struct(stmt))
            elif isinstance(stmt, ResolvedEnumDef):
                t_enums.append(self._check_enum(stmt))
            elif isinstance(stmt, ResolvedProtocolDef):
                t_protocols.append(self._check_protocol(stmt))
            elif isinstance(stmt, ResolvedImplDef):
                t_impls.append(self._check_impl(stmt))
            else:
                result = self._check_stmt(stmt, self._global_env, {})
                if result:
                    t_stmts.append(result)

        # Apply substitution everywhere
        t_module = TModule(
            t_fns,
            t_structs,
            t_enums,
            t_protocols,
            t_impls,
            list(self.witnesses.values()),
            t_stmts,
        )
        _apply_subst_module(self.subst, t_module)
        return t_module

    # ── declarations ─────────────────────────────────────────────────────────

    def _check_fn(
        self, fn: ResolvedFnDef, env: TypeEnv, is_method: bool = False
    ) -> TFnDef:
        gmap = self._make_generic_map(fn.type_params)
        fn_env = env.child()

        t_params = []
        for p in fn.params:
            ty = self._ann_to_type(p.type_ann, gmap)
            fn_env.bind(p.def_id, ty)
            t_params.append(TParam(p.name, p.def_id, ty))

        ret_ty = self._ann_to_type(fn.return_type, gmap)
        body_stmts = []
        inferred_ret = fresh_tv()

        for s in fn.body:
            ts = self._check_stmt(s, fn_env, gmap)
            if ts:
                body_stmts.append(ts)
                if isinstance(ts, TReturnStmt) and ts.value:
                    self._unify(inferred_ret, ts.value.ty, f"return in '{fn.name}'")

        # If return type was annotated, unify with inferred; otherwise use inferred
        if fn.return_type:
            self._unify(ret_ty, inferred_ret, f"return type of '{fn.name}'")
        else:
            ret_ty = inferred_ret

        # If body is non-empty and last stmt is an ExprStmt, that's the implicit return
        if body_stmts and isinstance(body_stmts[-1], TExprStmt):
            self._unify(
                ret_ty, body_stmts[-1].expr.ty, f"implicit return in '{fn.name}'"
            )

        return TFnDef(
            fn.name,
            fn.def_id,
            [tp.name for tp in fn.type_params],
            t_params,
            ret_ty,
            body_stmts,
            is_method=is_method,
        )

    def _check_struct(self, s: ResolvedStructDef) -> TStructDef:
        gmap = self._make_generic_map(s.type_params)
        fields = [(f.name, self._ann_to_type(f.type_ann, gmap)) for f in s.fields]
        return TStructDef(s.name, s.def_id, [tp.name for tp in s.type_params], fields)

    def _check_enum(self, e: ResolvedEnumDef) -> TEnumDef:
        gmap = self._make_generic_map(e.type_params)
        variants = [
            (v.name, [self._ann_to_type(t, gmap) for t in v.payload])
            for v in e.variants
        ]
        return TEnumDef(e.name, e.def_id, [tp.name for tp in e.type_params], variants)

    def _check_protocol(self, p: ResolvedProtocolDef) -> TProtocolDef:
        sigs = []
        for item in p.items:
            if isinstance(item, (ResolvedFnDef, ResolvedFnSig)):
                gmap = self._make_generic_map(item.type_params)
                params = [
                    TParam(pr.name, pr.def_id, self._ann_to_type(pr.type_ann, gmap))
                    for pr in item.params
                ]
                ret = self._ann_to_type(
                    item.return_type if hasattr(item, "return_type") else None, gmap
                )
                body = []
                if isinstance(item, ResolvedFnDef):
                    env = self._global_env.child()
                    for pr in item.params:
                        ty = self._ann_to_type(pr.type_ann, gmap)
                        env.bind(pr.def_id, ty)
                    for s in item.body:
                        ts = self._check_stmt(s, env, gmap)
                        if ts:
                            body.append(ts)
                sigs.append(
                    TFnDef(
                        item.name,
                        item.def_id,
                        [tp.name for tp in item.type_params],
                        params,
                        ret,
                        body,
                    )
                )
        return TProtocolDef(p.name, p.def_id, [tp.name for tp in p.type_params], sigs)

    def _check_impl(self, impl: ResolvedImplDef) -> TImplDef:
        self_ty = self._ann_to_type(impl.self_type, {})
        proto_name = None
        if impl.protocol:
            if isinstance(impl.protocol, A.NamedType):
                proto_name = impl.protocol.name

        impl_env = self._global_env.child()
        impl_env.bind(0, self_ty)  # 'Self' sentinel

        methods = [self._check_fn(m, impl_env, is_method=True) for m in impl.methods]

        # Build witness table if this is a protocol impl
        witness = None
        if proto_name:
            method_map = {m.name: m for m in methods}
            # Check all required signatures are present
            required = self._protocol_sigs.get(proto_name, {})
            for req_name in required:
                if req_name not in method_map:
                    self._err(
                        f"impl of '{proto_name}' for '{self_ty}' "
                        f"is missing method '{req_name}'"
                    )
            witness = TWitnessTable(self_ty, proto_name, method_map)
            self.witnesses[(repr(self_ty), proto_name)] = witness

        return TImplDef(self_ty, proto_name, methods, witness)

    # ── statements ────────────────────────────────────────────────────────────

    def _check_stmt(self, stmt: Any, env: TypeEnv, gmap: Dict[str, Type]) -> Any:
        if isinstance(stmt, (ResolvedConstStmt, ResolvedLetStmt)):
            return self._check_const_let(stmt, env, gmap)

        if isinstance(stmt, ResolvedAssignStmt):
            vexpr = self._infer_expr(stmt.value, env, gmap)
            bound_ty = env.lookup(stmt.def_id)
            if bound_ty:
                self._unify(bound_ty, vexpr.ty, f"assignment to '{stmt.target}'")
            return TAssignStmt(stmt.target, stmt.def_id, vexpr)

        if isinstance(stmt, A.ReturnStmt):
            val = self._infer_expr(stmt.value, env, gmap) if stmt.value else None
            return TReturnStmt(val)

        if isinstance(stmt, A.ExprStmt):
            return TExprStmt(self._infer_expr(stmt.expr, env, gmap))

        if isinstance(stmt, ResolvedFnDef):
            tfn = self._check_fn(stmt, env)
            env.bind(stmt.def_id, TyFn(tuple(p.ty for p in tfn.params), tfn.return_ty))
            return tfn

        return None  # decls handled at module level

    def _check_const_let(
        self, stmt: Any, env: TypeEnv, gmap: Dict[str, Type]
    ) -> TConstStmt:
        texpr = self._infer_expr(stmt.value, env, gmap)
        ann_ty = self._ann_to_type(stmt.type_ann, gmap) if stmt.type_ann else None
        if ann_ty:
            self._unify(ann_ty, texpr.ty, f"binding '{stmt.name}'")
            final_ty = ann_ty
        else:
            final_ty = texpr.ty
        env.bind(stmt.def_id, final_ty)
        is_let = isinstance(stmt, ResolvedLetStmt)
        return TConstStmt(stmt.name, stmt.def_id, final_ty, texpr, mutable=is_let)

    # ── expressions ──────────────────────────────────────────────────────────

    def _infer_expr(self, expr: Any, env: TypeEnv, gmap: Dict[str, Type]) -> TExpr:
        ty, inner = self._infer(expr, env, gmap)
        return TExpr(inner, ty)

    def _infer(
        self, expr: Any, env: TypeEnv, gmap: Dict[str, Type]
    ) -> PyTuple[Type, Any]:
        """Returns (Type, resolved_inner_node)."""

        # ── Literals ─────────────────────────────────────────────────────────
        if isinstance(expr, A.IntLit):
            return TyInt(), expr
        if isinstance(expr, A.FloatLit):
            return TyFloat(), expr
        if isinstance(expr, A.StringLit):
            return TyString(), expr
        if isinstance(expr, A.BoolLit):
            return TyBool(), expr
        if isinstance(expr, A.UnitLit):
            return TyUnit(), expr

        # ── Name reference ───────────────────────────────────────────────────
        if isinstance(expr, ResolvedRef):
            ty = env.lookup(expr.def_id)
            if ty is None:
                ty = self._err(f"No type for '{expr.name}' (def_id={expr.def_id})")
            return ty, expr

        # ── Binary / Unary ───────────────────────────────────────────────────
        if isinstance(expr, A.BinOp):
            lt = self._infer_expr(expr.left, env, gmap)
            rt = self._infer_expr(expr.right, env, gmap)
            if expr.op in ("+", "-", "*", "/", "%"):
                # VM has separate int and float arithmetic; operands must share
                # the same type.  We unify them together so Int+Int→Int and
                # Float+Float→Float both work, but Int+Float is a type error.
                self._unify(lt.ty, rt.ty, f"'{expr.op}' operand mismatch")
                self._require_numeric(lt.ty, f"'{expr.op}'")
                return apply_subst_ty(self.subst, lt.ty), A.BinOp(expr.op, lt, rt)
            if expr.op in ("==", "!=", "<", ">", "<=", ">="):
                self._unify(lt.ty, rt.ty, f"comparison '{expr.op}'")
                return TyBool(), A.BinOp(expr.op, lt, rt)
            if expr.op in ("and", "or"):
                self._unify(lt.ty, TyBool(), f"left of '{expr.op}'")
                self._unify(rt.ty, TyBool(), f"right of '{expr.op}'")
                return TyBool(), A.BinOp(expr.op, lt, rt)
            return fresh_tv(), A.BinOp(expr.op, lt, rt)

        if isinstance(expr, A.UnaryOp):
            ot = self._infer_expr(expr.operand, env, gmap)
            if expr.op == "-":
                # Negation works on both Int and Float; preserve the operand type.
                self._require_numeric(ot.ty, "unary negation")
                return apply_subst_ty(self.subst, ot.ty), A.UnaryOp(expr.op, ot)
            if expr.op == "not":
                self._unify(ot.ty, TyBool(), "not")
                return TyBool(), A.UnaryOp(expr.op, ot)
            return fresh_tv(), A.UnaryOp(expr.op, ot)

        # ── Calls ─────────────────────────────────────────────────────────────
        if isinstance(expr, A.Call):
            callee_te = self._infer_expr(expr.callee, env, gmap)
            arg_tes = [self._infer_expr(a, env, gmap) for a in expr.args]
            ret_tv = fresh_tv()
            arg_types = tuple(a.ty for a in arg_tes)
            # Instantiate: give every TyGeneric param a fresh TyVar so this
            # call site can resolve type args independently of other call sites.
            callee_ty = self._instantiate(apply_subst_ty(self.subst, callee_te.ty))
            # Nullary variant: :Err / :Err() — callee is the enum type itself,
            # no args, not callable.  Just return the instantiated type directly.
            if not arg_types and not isinstance(callee_ty, (TyFn, TyClosure)):
                return callee_ty, A.Call(callee_te, arg_tes)
            # Accept both named functions and closures at call sites
            if isinstance(callee_ty, TyClosure):
                expected = TyClosure(arg_types, ret_tv)
            else:
                expected = TyFn(arg_types, ret_tv)
            self._unify(callee_ty, expected, "call")
            actual_ty = apply_subst_ty(self.subst, callee_ty)
            if isinstance(actual_ty, (TyFn, TyClosure)):
                return actual_ty.ret, A.Call(callee_te, arg_tes)
            return apply_subst_ty(self.subst, ret_tv), A.Call(callee_te, arg_tes)

        # ── Field access ──────────────────────────────────────────────────────
        if isinstance(expr, A.FieldAccess):
            obj_te = self._infer_expr(expr.obj, env, gmap)
            obj_ty = apply_subst_ty(self.subst, obj_te.ty)
            if isinstance(obj_ty, TyObject):
                for fname, fty in obj_ty.fields:
                    if fname == expr.field:
                        return fty, A.FieldAccess(obj_te, expr.field)
                return self._err(f"No field '{expr.field}' on {obj_ty}"), A.FieldAccess(
                    obj_te, expr.field
                )
            tv = fresh_tv()
            return tv, A.FieldAccess(obj_te, expr.field)

        # ── Index ─────────────────────────────────────────────────────────────
        if isinstance(expr, A.IndexExpr):
            obj_te = self._infer_expr(expr.obj, env, gmap)
            idx_te = self._infer_expr(expr.index, env, gmap)
            self._unify(idx_te.ty, TyInt(), "array index")
            elem_tv = fresh_tv()
            self._unify(obj_te.ty, TyArray(elem_tv), "index target")
            return apply_subst_ty(self.subst, elem_tv), A.IndexExpr(obj_te, idx_te)

        # ── Tuple / Array ─────────────────────────────────────────────────────
        if isinstance(expr, A.TupleLit):
            tes = [self._infer_expr(e, env, gmap) for e in expr.elems]
            return TyTuple(tuple(t.ty for t in tes)), A.TupleLit(tes)

        if isinstance(expr, A.ArrayLit):
            tes = [self._infer_expr(e, env, gmap) for e in expr.elems]
            elem_tv = fresh_tv()
            for te in tes:
                self._unify(elem_tv, te.ty, "array literal")
            return TyArray(apply_subst_ty(self.subst, elem_tv)), A.ArrayLit(tes)

        # ── If ────────────────────────────────────────────────────────────────
        if isinstance(expr, A.IfExpr):
            cond_te = self._infer_expr(expr.cond, env, gmap)
            self._unify(cond_te.ty, TyBool(), "if condition")
            then_env = env.child()
            then_body = [self._check_stmt(s, then_env, gmap) for s in expr.then_body]
            then_ty = self._block_type(then_body)
            result_tv = fresh_tv()
            self._unify(result_tv, then_ty, "if-then branch")

            else_resolved = None
            if expr.else_body is not None:
                if isinstance(expr.else_body, A.IfExpr):
                    else_te = self._infer_expr(expr.else_body, env, gmap)
                    self._unify(result_tv, else_te.ty, "else-if branch")
                    else_resolved = else_te
                else:
                    else_env = env.child()
                    else_body = [
                        self._check_stmt(s, else_env, gmap) for s in expr.else_body
                    ]
                    else_ty = self._block_type(else_body)
                    self._unify(result_tv, else_ty, "else branch")
                    else_resolved = else_body

            return apply_subst_ty(self.subst, result_tv), A.IfExpr(
                cond_te, then_body, else_resolved
            )

        # ── Match ─────────────────────────────────────────────────────────────
        if isinstance(expr, A.MatchExpr):
            subj_te = self._infer_expr(expr.subject, env, gmap)
            result_tv = fresh_tv()
            t_arms = []
            for arm in expr.arms:
                arm_env = env.child()
                if isinstance(arm, ResolvedMatchArm):
                    pat, body_expr = arm.pattern, arm.body
                else:
                    pat, body_expr = arm.pattern, arm.body
                self._bind_pattern(pat, subj_te.ty, arm_env)
                body_te = self._infer_expr(body_expr, arm_env, gmap)
                self._unify(result_tv, body_te.ty, "match arm")
                t_arms.append(ResolvedMatchArm(pat, body_te))
            return apply_subst_ty(self.subst, result_tv), A.MatchExpr(subj_te, t_arms)

        # ── Process primitives ────────────────────────────────────────────────
        # TyProcess is a semantic type in the checker; at codegen it lowers to
        # Value::int (the process's integer id).
        if isinstance(expr, A.SpawnExpr):
            arg_tes = [self._infer_expr(a, env, gmap) for a in expr.args]
            return TyProcess(), A.SpawnExpr(expr.func, arg_tes, expr.func_def_id)

        if isinstance(expr, A.SendExpr):
            pid_te = self._infer_expr(expr.pid, env, gmap)
            msg_te = self._infer_expr(expr.msg, env, gmap)
            self._unify(pid_te.ty, TyProcess(), "send target must be a Process PID")
            return TyUnit(), A.SendExpr(pid_te, msg_te)

        if isinstance(expr, A.ReceiveExpr):
            return fresh_tv(), expr  # type depends on what was sent

        # ── Lambda ────────────────────────────────────────────────────────────
        if isinstance(expr, A.LambdaExpr):
            lam_env = env.child()
            gmap2 = dict(gmap)
            for tp in expr.type_params:
                if isinstance(tp, ResolvedTypeParam):
                    gmap2[tp.name] = TyGeneric(tp.name)
            t_params = []
            for p in expr.params:
                if isinstance(p, ResolvedParam):
                    ty = self._ann_to_type(p.type_ann, gmap2)
                    lam_env.bind(p.def_id, ty)
                    t_params.append(TParam(p.name, p.def_id, ty))
            ret_tv = fresh_tv()
            body_stmts = [self._check_stmt(s, lam_env, gmap2) for s in expr.body]
            body_ty = self._block_type(body_stmts)
            self._unify(ret_tv, body_ty, "lambda return")
            lam_ty = TyClosure(
                tuple(p.ty for p in t_params), apply_subst_ty(self.subst, ret_tv)
            )
            return lam_ty, A.LambdaExpr(expr.type_params, t_params, None, body_stmts)

        # ── Block Expression ──────────────────────────────────────────────────
        if isinstance(expr, A.BlockExpr):
            block_env = env.child()
            body_stmts = [self._check_stmt(s, block_env, gmap) for s in expr.stmts]
            body_ty = self._block_type(body_stmts)
            return body_ty, A.BlockExpr(body_stmts)

        # Fallthrough
        tv = fresh_tv()
        return tv, expr

    def _block_type(self, stmts: List[Any]) -> Type:
        """The type of a block is the type of its last ExprStmt, else Unit."""
        if stmts:
            last = stmts[-1]
            if isinstance(last, TExprStmt):
                return last.expr.ty
            if isinstance(last, TReturnStmt) and last.value:
                return last.value.ty
        return TyUnit()

    def _bind_pattern(self, pat: Any, ty: Type, env: TypeEnv) -> None:
        """Introduce pattern bindings into env."""
        if isinstance(pat, BindPat):
            env.bind(pat.def_id, ty)
        elif isinstance(pat, VariantPat):
            enum_ty = apply_subst_ty(self.subst, ty)
            if isinstance(enum_ty, TyEnum):
                for vname, vpayload in enum_ty.variants:
                    if vname == pat.variant_name:
                        if len(vpayload) == 1 and len(pat.payload) == 1:
                            self._bind_pattern(pat.payload[0], vpayload[0], env)
                        elif len(vpayload) > 1 and len(pat.payload) == len(vpayload):
                            for sp, st in zip(pat.payload, vpayload):
                                self._bind_pattern(sp, st, env)
        elif isinstance(pat, A.TuplePat):
            tv_elems = [fresh_tv() for _ in pat.elems]
            self._unify(ty, TyTuple(tuple(tv_elems)), "tuple pattern")
            for sp, st in zip(pat.elems, tv_elems):
                self._bind_pattern(sp, apply_subst_ty(self.subst, st), env)


# ── Substitution application over TModule ────────────────────────────────────


def _apply_subst_module(subst: Subst, module: TModule) -> None:
    """Walk the entire TModule and apply the final substitution."""

    def walk_ty(ty):
        return apply_subst_ty(subst, ty)

    def walk_texpr(te):
        if not isinstance(te, TExpr):
            return te
        te.ty = walk_ty(te.ty)
        return te

    def walk_stmt(s):
        if isinstance(s, TConstStmt):
            s.ty = walk_ty(s.ty)
            s.value = walk_texpr(s.value)
        elif isinstance(s, TAssignStmt):
            s.value = walk_texpr(s.value)
        elif isinstance(s, TReturnStmt) and s.value:
            s.value = walk_texpr(s.value)
        elif isinstance(s, TExprStmt):
            s.expr = walk_texpr(s.expr)
        elif isinstance(s, TFnDef):
            walk_fn(s)
        return s

    def walk_fn(fn: TFnDef):
        for p in fn.params:
            p.ty = walk_ty(p.ty)
        fn.return_ty = walk_ty(fn.return_ty)
        for s in fn.body:
            walk_stmt(s)

    for fn in module.fns:
        walk_fn(fn)
    for impl in module.impls:
        for m in impl.methods:
            walk_fn(m)
        if impl.self_ty:
            impl.self_ty = walk_ty(impl.self_ty)
    for s in module.stmts:
        walk_stmt(s)


# ═══════════════════════════════════════════════════════════════════════════════
# Public API
# ═══════════════════════════════════════════════════════════════════════════════


def typecheck(rmod: ResolvedModule) -> PyTuple[TModule, List[TypeError_]]:
    """
    Run the type-checking pass over a ResolvedModule.
    Returns (TModule, errors).
    """
    checker = TypeChecker(rmod.def_map)
    tmod = checker.check_module(rmod)
    return tmod, checker.errors
