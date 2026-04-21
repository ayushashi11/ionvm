"""
ast_nodes.py
============
AST node dataclasses produced by the parser transformer.
All fields use Python typing; Optional means the node may be absent.

Node families
─────────────
  Lit         — compile-time literal values
  TypeAnn     — type annotation expressions
  Pattern     — match patterns
  Expr        — value expressions
  Stmt        — statements (inside fn bodies / top-level)
  Decl        — top-level declarations
  Module      — root node
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import List, Optional, Union

# ── Literals ─────────────────────────────────────────────────────────────────


@dataclass
class IntLit:
    value: int


@dataclass
class FloatLit:
    value: float


@dataclass
class StringLit:
    value: str


@dataclass
class BoolLit:
    value: bool


@dataclass
class UnitLit:
    """The unit value ()  — maps to VM Value::nil."""


Lit = Union[IntLit, FloatLit, StringLit, BoolLit, UnitLit]


# ── Type annotations ──────────────────────────────────────────────────────────


@dataclass
class NamedType:
    """Foo  or  Foo[T, U]"""

    name: str
    args: List[TypeAnn] = field(default_factory=list)


@dataclass
class TupleType:
    """(A, B, C)"""

    elems: List[TypeAnn]


@dataclass
class FnType:
    """fn(A, B) -> C"""

    params: List[TypeAnn]
    ret: TypeAnn


@dataclass
class SelfType:
    """self in protocol definitions."""


TypeAnn = Union[NamedType, TupleType, FnType, SelfType]


# ── Patterns ─────────────────────────────────────────────────────────────────


@dataclass
class WildcardPat:
    """_"""


@dataclass
class NamePat:
    """x  — binding (lowercase) or nullary variant (uppercase, resolved later)."""

    name: str


@dataclass
class TuplePat:
    """(a, b, c)"""

    elems: List[Pattern]


@dataclass
class EnumPat:
    """Some(x)  or  Pair(a, b)"""

    variant: str
    payload: List[Pattern]


@dataclass
class LitPat:
    lit: Lit


Pattern = Union[WildcardPat, NamePat, TuplePat, EnumPat, LitPat]


# ── Expressions ──────────────────────────────────────────────────────────────


@dataclass
class NameRef:
    name: str


@dataclass
class BinOp:
    op: str  # "+", "-", "*", "/", "%", "==", "!=", "<", ">", "<=", ">=", "and", "or"
    left: Expr
    right: Expr


@dataclass
class UnaryOp:
    op: str  # "-", "not"
    operand: Expr


@dataclass
class Call:
    callee: Expr
    args: List[Expr]


@dataclass
class FieldAccess:
    obj: Expr
    field: str


@dataclass
class IndexExpr:
    obj: Expr
    index: Expr


@dataclass
class TupleLit:
    elems: List[Expr]


@dataclass
class ArrayLit:
    elems: List[Expr]


@dataclass
class IfExpr:
    cond: Expr
    then_body: List[Stmt]
    else_body: Optional[Union[List[Stmt], IfExpr]]  # None | block stmts | elif chain


@dataclass
class MatchArm:
    pattern: Pattern
    body: Expr


@dataclass
class MatchExpr:
    subject: Expr
    arms: List[MatchArm]


@dataclass
class SpawnExpr:
    """spawn process_fn(args)  — returns a PID (Process value)."""

    func: str
    args: List[Expr]
    func_def_id: Optional[int] = None


@dataclass
class SendExpr:
    """send(pid, msg)  — sends a message to a process mailbox."""

    pid: Expr
    msg: Expr


@dataclass
class ReceiveExpr:
    """receive()  — blocks until a message arrives in the mailbox."""


@dataclass
class LambdaExpr:
    type_params: List[TypeParam]
    params: List[Param]
    return_type: Optional[TypeAnn]
    body: List[Stmt]


@dataclass
class BlockExpr:
    """A block expression { stmt; ... result_expr } used in match arms."""
    stmts: List[Stmt]


Expr = Union[
    Lit,
    NameRef,
    BinOp,
    UnaryOp,
    Call,
    FieldAccess,
    IndexExpr,
    TupleLit,
    ArrayLit,
    IfExpr,
    MatchExpr,
    SpawnExpr,
    SendExpr,
    ReceiveExpr,
    LambdaExpr,
    BlockExpr,
]


# ── Supporting structures ────────────────────────────────────────────────────


@dataclass
class TypeParam:
    """T  or  T: Protocol  or  T: P1 + P2."""

    name: str
    constraints: List[NamedType] = field(default_factory=list)


@dataclass
class Param:
    """A function parameter.  name='self' has no type_ann."""

    name: str
    type_ann: Optional[TypeAnn]


@dataclass
class FieldDef:
    name: str
    type_ann: TypeAnn


@dataclass
class VariantDef:
    """Circle  or  Rect(Float, Float)."""

    name: str
    payload: List[TypeAnn] = field(default_factory=list)


@dataclass
class FnSig:
    """A protocol method signature (no body)."""

    name: str
    type_params: List[TypeParam]
    params: List[Param]
    return_type: Optional[TypeAnn]


# ── Statements ────────────────────────────────────────────────────────────────


@dataclass
class ConstStmt:
    """Immutable binding:  const x: T = expr"""

    name: str
    type_ann: Optional[TypeAnn]
    value: Expr


@dataclass
class LetStmt:
    """Mutable binding:  let x: T = expr"""

    name: str
    type_ann: Optional[TypeAnn]
    value: Expr


@dataclass
class AssignStmt:
    """Reassignment of a let:  x = expr"""

    target: str
    value: Expr


@dataclass
class ReturnStmt:
    value: Optional[Expr]


@dataclass
class ExprStmt:
    expr: Expr


Stmt = Union[
    ConstStmt,
    LetStmt,
    AssignStmt,
    ReturnStmt,
    ExprStmt,
    "FnDef",
    "StructDef",
    "EnumDef",
    "ProtocolDef",
    "ImplDef",
]


# ── Declarations ─────────────────────────────────────────────────────────────


@dataclass
class FnDef:
    name: str
    type_params: List[TypeParam]
    params: List[Param]
    return_type: Optional[TypeAnn]
    body: List[Stmt]


@dataclass
class StructDef:
    name: str
    type_params: List[TypeParam]
    fields: List[FieldDef]


@dataclass
class EnumDef:
    name: str
    type_params: List[TypeParam]
    variants: List[VariantDef]


@dataclass
class ProtocolDef:
    name: str
    type_params: List[TypeParam]
    items: List[Union[FnDef, FnSig]]


@dataclass
class ImplDef:
    """impl SelfType: ...  or  impl Protocol for SelfType: ..."""

    self_type: TypeAnn
    protocol: Optional[TypeAnn]  # None = inherent impl
    methods: List[FnDef]


# ── Root ─────────────────────────────────────────────────────────────────────


@dataclass
class Module:
    stmts: List[Stmt]
