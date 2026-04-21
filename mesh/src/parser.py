"""
parser.py
=========
Lark grammar + Transformer → AST nodes.

Pipeline:
    source  ──layout()──►  brace-delimited string
                                    │
                               Lark LALR
                                    │
                           LarkTransformer
                                    │
                                 Module
"""

from lark import Lark, Token, Transformer, Tree
from lark.visitors import v_args

from ast_nodes import (
    ArrayLit,
    AssignStmt,
    BinOp,
    BlockExpr,
    BoolLit,
    Call,
    # Stmts
    ConstStmt,
    EnumDef,
    EnumPat,
    ExprStmt,
    FieldAccess,
    FieldDef,
    FloatLit,
    # Decls
    FnDef,
    FnSig,
    FnType,
    IfExpr,
    ImplDef,
    IndexExpr,
    # Lits
    IntLit,
    LambdaExpr,
    LetStmt,
    LitPat,
    MatchArm,
    MatchExpr,
    # Root
    Module,
    # Types
    NamedType,
    NamePat,
    # Exprs
    NameRef,
    Param,
    ProtocolDef,
    ReceiveExpr,
    ReturnStmt,
    SelfType,
    SendExpr,
    SpawnExpr,
    StringLit,
    StructDef,
    TupleLit,
    TuplePat,
    TupleType,
    # Supporting
    TypeParam,
    UnaryOp,
    UnitLit,
    VariantDef,
    # Patterns
    WildcardPat,
)

# ── Grammar ──────────────────────────────────────────────────────────────────
#
# All blocks are { } — the layout pass already inserted them.
# ':' always appears before a '{' (block-intro colon emitted by layout pass)
# and also in type annotations (name : type).  Both forms are kept.

GRAMMAR = r"""
    start: (top_stmt ";")*

    ?top_stmt: fn_def
             | struct_def
             | enum_def
             | protocol_def
             | impl_def
             | stmt

    // ── Declarations ───────────────────────────────────────────────────────

    fn_def: "fn" NAME type_params? "(" param_list? ")" ret_ann? ":" block

    struct_def:   "struct"   NAME type_params? ":" struct_body
    enum_def:     "enum"     NAME type_params? ":" enum_body
    protocol_def: "protocol" NAME type_params? ":" proto_body
    impl_def:     "impl" type_ann ("for" type_ann)? ":" impl_body

    block:        "{" (stmt ";")* "}"
    struct_body:  "{" (field_def ";")* "}"
    enum_body:    "{" (variant_def ";")* "}"
    proto_body:   "{" (proto_item ";")* "}"
    impl_body:    "{" (fn_def ";")* "}"


    field_def:   NAME type_ann
    variant_def: NAME ("(" type_list ")")?

    ?proto_item: fn_sig | fn_def
    fn_sig: "fn" NAME type_params? "(" param_list? ")" ret_ann?

    // ── Params & type-params ────────────────────────────────────────────────

    param_list: param ("," param)*
    ?param: self_param | typed_param
    self_param:  "self"
    typed_param: NAME type_ann

    type_params: "[" type_param_list "]"
    type_param_list: type_param ("," type_param)*
    type_param: NAME (":" type_constraint)?
    type_constraint: named_type ("+" named_type)*

    // ── Types ───────────────────────────────────────────────────────────────

    ?type_ann: fn_type_ann | tuple_type_ann | named_type
    fn_type_ann:    "fn" "(" type_list? ")" "->" type_ann
    tuple_type_ann: "(" type_list ")"
    named_type:     NAME ("[" type_list "]")?
    type_list:      type_ann ("," type_ann)*

    // ── Statements ──────────────────────────────────────────────────────────

    ?stmt: const_stmt
         | let_stmt
         | return_stmt
         | assign_stmt
         | call_block_stmt
         | expr_stmt

    const_stmt:    "const" NAME type_ann? "=" expr
    let_stmt:    "let" NAME type_ann? "=" expr
    return_stmt: "return" expr?
    assign_stmt: NAME "=" expr
    expr_stmt:   expr

    // call(args): block  or  obj.method(args): block
    call_block_stmt: postfix_expr ":" block


    // ── Expressions (precedence hierarchy) ─────────────────────────────────

    ?expr: or_expr

    ?or_expr:  and_expr | or_expr  "or"  and_expr  -> bin_or
    ?and_expr: not_expr | and_expr "and" not_expr  -> bin_and
    ?not_expr: "not" not_expr -> unary_not | cmp_expr

    ?cmp_expr: add_expr | cmp_expr CMP_OP add_expr -> bin_cmp

    ?add_expr: mul_expr
             | add_expr "+" mul_expr -> bin_add
             | add_expr "-" mul_expr -> bin_sub

    ?mul_expr: unary_expr
             | mul_expr "*" unary_expr -> bin_mul
             | mul_expr "/" unary_expr -> bin_div
             | mul_expr "%" unary_expr -> bin_mod

    ?unary_expr: "-" unary_expr -> unary_neg | postfix_expr

    ?postfix_expr: primary
                 | postfix_expr "." NAME          -> field_access
                 | postfix_expr "[" expr "]"       -> index_expr
                 | postfix_expr "(" arg_list? ")"  -> call_expr

    ?primary: "(" expr ")"
            | tuple_lit
            | list_expr
            | if_expr
            | match_expr
            | spawn_expr
            | lambda_expr
            | send_expr
            | receive_expr
            | block_expr
            | literal
            | ATOM "(" arg_list? ")"  -> atom_call
            | ATOM                    -> atom_ref
            | NAME -> name_ref

    // ── Expression forms ────────────────────────────────────────────────────

    block_expr: "{" (stmt ";")* "}"

    tuple_lit:  "(" expr "," arg_list ")"

    // list_expr: same as array but using arg_list (closure is just an expr now)
    list_expr:   "[" (_list_items)? "]"
    _list_items: expr ("," expr)* ","?

    if_expr: "if" expr ":" block else_branch?
    else_branch: "else" ":" block     -> else_block
               | "else" if_expr       -> else_if

    match_expr: "match" expr ":" "{" match_arm* "}"
    match_arm:  pattern "=>" expr ";"

    spawn_expr:   "spawn"   NAME "(" arg_list? ")"
    lambda_expr:  "fn"  type_params? "(" param_list? ")" ret_ann? ":" block
    send_expr:    "send"    "(" expr "," expr ")"
    receive_expr: "receive" "(" ")"

    ret_ann:  "->" type_ann
    arg_list: expr ("," expr)*

    // ── Patterns ────────────────────────────────────────────────────────────

    ?pattern: tuple_pat
            | enum_pat
            | name_pat      // _ → WildcardPat, :Foo → variant ref, else binding
            | lit_pat

    name_pat:  NAME | ATOM
    tuple_pat: "(" pattern ("," pattern)+ ")"
    enum_pat:  (ATOM | NAME) "(" pattern ("," pattern)* ")"
    lit_pat:   literal

    // ── Literals ────────────────────────────────────────────────────────────

    ?literal: INT    -> int_lit
            | FLOAT  -> float_lit
            | STRING -> string_lit
            | "true"  -> bool_true
            | "false" -> bool_false
            | "unit"  -> unit_lit


    // ── Terminals ───────────────────────────────────────────────────────────

    NAME:   /[a-zA-Z_][a-zA-Z0-9_]*/
    INT:    /[0-9]+/
    FLOAT:  /[0-9]+\.[0-9]+/
    STRING: /\"[^\"]*\"|\'[^\']*\'/
    ATOM:   /:[a-zA-Z_][a-zA-Z0-9_]*/
    CMP_OP: "==" | "!=" | "<=" | ">=" | "<" | ">"

    %ignore /\s+/
"""


# ── Transformer ──────────────────────────────────────────────────────────────


@v_args(inline=True)
class LarkTransformer(Transformer):
    # ── Root ──────────────────────────────────────────────────────────────

    def start(self, *stmts):
        return Module(list(stmts))

    # ── Literals ──────────────────────────────────────────────────────────

    def int_lit(self, tok):
        return IntLit(int(tok))

    def float_lit(self, tok):
        return FloatLit(float(tok))

    def string_lit(self, tok):
        return StringLit(str(tok)[1:-1])  # strip quotes

    def bool_true(self):
        return BoolLit(True)

    def bool_false(self):
        return BoolLit(False)

    def unit_lit(self):
        return UnitLit()

    # ── Types ──────────────────────────────────────────────────────────────

    def named_type(self, name, *args):
        type_list = args[0] if args else None
        return NamedType(str(name), list(type_list) if type_list else [])

    def tuple_type_ann(self, type_list):
        return TupleType(list(type_list))

    def fn_type_ann(self, *args):
        # args may be (type_list, ret) or (ret,) with empty param list
        if len(args) == 2:
            params, ret = args
            return FnType(list(params), ret)
        else:
            return FnType([], args[0])

    def type_list(self, *types):
        return list(types)

    def type_params(self, tpl):
        return list(tpl)

    def type_param_list(self, *tp):
        return list(tp)

    def type_param(self, name, *rest):
        constraint = rest[0] if rest else None
        return TypeParam(str(name), list(constraint) if constraint else [])

    def type_constraint(self, *named_types):
        return list(named_types)

    # ── Params ──────────────────────────────────────────────────────────────

    def self_param(self):
        return Param("self", None)

    def typed_param(self, name, typ):
        return Param(str(name), typ)

    def param_list(self, *params):
        return list(params)

    def ret_ann(self, typ):
        return typ

    # ── Supporting ──────────────────────────────────────────────────────────

    def field_def(self, name, typ):
        return FieldDef(str(name), typ)

    def variant_def(self, name, *rest):
        payload = list(rest[0]) if rest else []
        return VariantDef(str(name), payload)

    def fn_sig(self, name, *args):
        # type_params?, param_list?, ret_ann?
        type_params, params, return_type = [], [], None
        for a in args:
            if isinstance(a, list) and a and isinstance(a[0], TypeParam):
                type_params = a
            elif isinstance(a, list):
                params = a
            else:
                return_type = a
        return FnSig(str(name), type_params, params, return_type)

    # ── Declarations ────────────────────────────────────────────────────────

    def fn_def(self, name, *args):
        # "fn" NAME type_params? "(" param_list? ")" ret_ann? ":" block
        block = args[-1]
        rest = args[:-1]
        type_params, params, return_type = [], [], None
        for a in rest:
            if isinstance(a, list) and a and isinstance(a[0], TypeParam):
                type_params = a
            elif isinstance(a, list):
                params = a
            else:
                return_type = a
        return FnDef(str(name), type_params, params, return_type, block)

    def struct_def(self, name, *args):
        tps = (
            args[0]
            if args
            and isinstance(args[0], list)
            and args[0]
            and isinstance(args[0][0], TypeParam)
            else []
        )
        body = args[-1]
        return StructDef(str(name), tps, body)

    def enum_def(self, name, *args):
        tps = (
            args[0]
            if args
            and isinstance(args[0], list)
            and args[0]
            and isinstance(args[0][0], TypeParam)
            else []
        )
        body = args[-1]
        return EnumDef(str(name), tps, body)

    def protocol_def(self, name, *args):
        tps = (
            args[0]
            if args
            and isinstance(args[0], list)
            and args[0]
            and isinstance(args[0][0], TypeParam)
            else []
        )
        body = args[-1]
        return ProtocolDef(str(name), tps, body)

    def impl_def(self, *args):
        # "impl" type_ann ("for" type_ann)? ":" impl_body
        # args from grammar (after keyword stripping): type_ann [type_ann] impl_body
        methods = args[-1]
        types = args[:-1]
        if len(types) == 2:
            protocol, self_type = types[0], types[1]
        else:
            protocol, self_type = None, types[0]
        return ImplDef(self_type, protocol, methods)

    # Block rules just return their children as a list
    def block(self, *stmts):
        return list(stmts)

    def block_expr(self, *stmts):
        # A block as an expression - same structure as a block
        # Should appear in the AST as a Block node
        return BlockExpr(list(stmts)) if stmts else BlockExpr([])

    def struct_body(self, *fields):
        return list(fields)

    def enum_body(self, *variants):
        return list(variants)

    def proto_body(self, *items):
        return list(items)

    def impl_body(self, *fns):
        return list(fns)

    # ── Statements ───────────────────────────────────────────────────────────

    def const_stmt(self, name, *args):
        # name, (type_ann)?, expr
        value = args[-1]
        type_ann = args[0] if len(args) == 2 else None
        return ConstStmt(str(name), type_ann, value)

    def let_stmt(self, name, *args):
        value = args[-1]
        type_ann = args[0] if len(args) == 2 else None
        return LetStmt(str(name), type_ann, value)

    def return_stmt(self, *args):
        return ReturnStmt(args[0] if args else None)

    def assign_stmt(self, name, value):
        return AssignStmt(str(name), value)

    def call_block_stmt(self, callee, block):
        # call(args): block  →  append block as trailing lambda arg
        if isinstance(callee, Call):
            new_args = list(callee.args) + [LambdaExpr([], [], None, block)]
            return ExprStmt(Call(callee.callee, new_args))
        return ExprStmt(Call(callee, [LambdaExpr([], [], None, block)]))

    def expr_stmt(self, expr):
        return ExprStmt(expr)

    # ── Expressions ──────────────────────────────────────────────────────────

    def name_ref(self, tok):
        return NameRef(str(tok))

    def atom_call(self, atom_tok, *args):
        name = str(atom_tok)[1:]  # strip leading ':'
        arg_list = list(args[0]) if args else []
        return Call(NameRef(name), arg_list)

    def atom_ref(self, atom_tok):
        # :Err  ==  :Err()  — always a Call so codegen sees a uniform constructor
        return Call(NameRef(str(atom_tok)[1:]), [])

    def bin_or(self, l, r):
        return BinOp("or", l, r)

    def bin_and(self, l, r):
        return BinOp("and", l, r)

    def unary_not(self, operand):
        return UnaryOp("not", operand)

    def bin_cmp(self, l, op, r):
        return BinOp(str(op), l, r)

    def bin_add(self, l, r):
        return BinOp("+", l, r)

    def bin_sub(self, l, r):
        return BinOp("-", l, r)

    def bin_mul(self, l, r):
        return BinOp("*", l, r)

    def bin_div(self, l, r):
        return BinOp("/", l, r)

    def bin_mod(self, l, r):
        return BinOp("%", l, r)

    def unary_neg(self, operand):
        return UnaryOp("-", operand)

    def call_expr(self, callee, *args):
        arg_list = list(args[0]) if args else []
        return Call(callee, arg_list)

    def field_access(self, obj, name):
        return FieldAccess(obj, str(name))

    def index_expr(self, obj, idx):
        return FieldAccess(obj, str(idx.value)) #TODO: must be handled better, either make array indices values or kys

    def tuple_lit(self, first, rest):
        return TupleLit([first] + list(rest))

    def list_expr(self, *args):
        # args is the items that survived _list_items inlining (may be empty)
        return ArrayLit(list(args))

    def arg_list(self, *exprs):
        return list(exprs)

    def if_expr(self, cond, then_block, *rest):
        else_branch = rest[0] if rest else None
        return IfExpr(cond, then_block, else_branch)

    def else_block(self, block):
        return block  # List[Stmt]

    def else_if(self, if_expr):
        return if_expr  # IfExpr

    def match_expr(self, subject, *arms):
        return MatchExpr(subject, list(arms))

    def match_arm(self, pattern, body):
        return MatchArm(pattern, body)

    def spawn_expr(self, name, *args):
        return SpawnExpr(str(name), list(args[0]) if args else [])

    def lambda_expr(self, *args):
        # type_params?, param_list?, ret_ann?, block
        block = args[-1]
        rest = args[:-1]
        type_params, params, return_type = [], [], None
        for a in rest:
            if isinstance(a, list) and a and isinstance(a[0], TypeParam):
                type_params = a
            elif isinstance(a, list):
                params = a
            else:
                return_type = a
        return LambdaExpr(type_params, params, return_type, block)

    def send_expr(self, pid, msg):
        return SendExpr(pid, msg)

    def receive_expr(self):
        return ReceiveExpr()

    # ── Patterns ─────────────────────────────────────────────────────────────

    def name_pat(self, tok):
        name = str(tok)
        if name.startswith(":"):
            return NamePat(name[1:])  # strip ':', resolver treats upper as variant
        return WildcardPat() if name == "_" else NamePat(name)

    def tuple_pat(self, *pats):
        return TuplePat(list(pats))

    def enum_pat(self, name, *pats):
        n = str(name)
        if n.startswith(":"):
            n = n[1:]  # strip leading ':'
        return EnumPat(n, list(pats))

    def lit_pat(self, lit):
        return LitPat(lit)


# ── Public API ────────────────────────────────────────────────────────────────

_PARSER = None  # lazy singleton


def make_parser() -> Lark:
    return Lark(GRAMMAR, parser="lalr", transformer=LarkTransformer())


def get_parser() -> Lark:
    global _PARSER
    if _PARSER is None:
        _PARSER = make_parser()
    return _PARSER


def parse(src_post_layout: str) -> Module:
    """Parse a brace-delimited string (output of layout()) into a Module."""
    return get_parser().parse(src_post_layout)
