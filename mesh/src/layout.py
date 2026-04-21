"""
layout.py
=========
Indented source  →  brace-delimited token string.

Rules (mirrors Rust lex_with_layout):
 • ':' emits ':' AND pushes _Pending(bd, colon_indent).
 • Line start: pending opens (emit '{') when
       next_leading > pending.colon_indent  AND  bracket_depth == pending.bd
 • ',' / ')' / ']': close open blocks whose bd >= current bracket_depth.
 • Outside brackets, no pending → normal indent/dedent.
"""

import re
from dataclasses import dataclass
from typing import List, Optional


# ── Lexer ────────────────────────────────────────────────────────────────────

@dataclass
class _Tok:
    type: str
    val:  str


_LEX = re.compile(
    r'(?P<ATOM>:[a-zA-Z_]\w*)'           # :symbol  — before COLON
    r'|(?P<STRING>"[^"]*"|\'[^\']*\')'
    r'|(?P<FLOAT>\d+\.\d+)'              # before INT
    r'|(?P<INT>\d+)'
    r'|(?P<NAME>[a-zA-Z_]\w*)'
    r'|(?P<ARROW>->)'                    # before OP
    r'|(?P<FAT_ARROW>=>)'                # before OP
    r'|(?P<COLON>:)'
    r'|(?P<COMMA>,)'
    r'|(?P<LPAR>\()'
    r'|(?P<RPAR>\))'
    r'|(?P<LSQB>\[)'
    r'|(?P<RSQB>\])'
    r'|(?P<DOT>\.)'
    r'|(?P<OP>[=!<>+\-*/%]+)'
    r'|(?P<WS>[ \t]+)'
    r'|(?P<COMMENT>//[^\n]*|#[^\n]*)'
)


def _lex_line(line: str) -> List[_Tok]:
    return [
        _Tok(m.lastgroup, m.group())
        for m in _LEX.finditer(line)
        if m.lastgroup not in ('WS', 'COMMENT')
    ]


# ── Layout state ─────────────────────────────────────────────────────────────

@dataclass
class _Pending:
    bd:           int  # bracket_depth when ':' was seen
    colon_indent: int  # leading whitespace of the line containing ':'


@dataclass
class _Open:
    indent: int  # leading whitespace when '{' was emitted
    bd:     int  # bracket_depth when '{' was emitted


def _dedent(
    leading:      int,
    indent_stack: List[int],
    open_blocks:  List[_Open],
    out:          List[str],
    pending:      List[_Pending] = None,
) -> None:
    top = indent_stack[-1]
    if leading > top:
        indent_stack.append(leading)
        open_blocks.append(_Open(leading, 0))
        out.append('{')
    else:
        closed = 0
        while indent_stack[-1] > leading:
            indent_stack.pop()
            if open_blocks:
                open_blocks.pop()
            if out and out[-1] not in ('{', ';'):
                out.append(';')
            out.append('}')
            closed += 1
        if closed > 0:
            out.append(';')
        # Flush stale pendings whose colon was inside a block we just closed.
        if pending is not None:
            i = 0
            while i < len(pending):
                if pending[i].colon_indent >= leading and leading <= indent_stack[-1]:
                    pending.pop(i)
                else:
                    i += 1


# ── Public API ───────────────────────────────────────────────────────────────

def layout(src: str) -> str:
    """
    Convert indented source to a flat brace-delimited token string.
    Feed the result directly to the Lark parser.
    """
    out:           List[str]      = []
    indent_stack:  List[int]      = [0]
    bracket_depth: int            = 0
    pending:       List[_Pending] = []
    open_blocks:   List[_Open]    = []

    for raw_line in src.expandtabs(4).splitlines():
        stripped = raw_line.lstrip()
        if not stripped or stripped.startswith('#') or stripped.startswith('//'):
            continue

        leading = len(raw_line) - len(stripped)

        # ── open pending blocks / dedent ──────────────────────────────────
        if pending:
            opened = False
            i = 0
            while i < len(pending):
                p = pending[i]
                if leading > p.colon_indent and bracket_depth == p.bd:
                    indent_stack.append(leading)
                    open_blocks.append(_Open(leading, p.bd))
                    out.append('{')
                    pending.pop(i)
                    opened = True
                else:
                    i += 1
            if not opened and bracket_depth == 0:
                _dedent(leading, indent_stack, open_blocks, out, pending)
        else:
            if bracket_depth == 0:
                _dedent(leading, indent_stack, open_blocks, out, pending)

        # ── tokenise ─────────────────────────────────────────────────────
        pending_before = len(pending)
        line_toks = _lex_line(stripped)
        for tok_idx, tok in enumerate(line_toks):
            t = tok.type

            if t in ('LPAR', 'LSQB'):
                bracket_depth += 1
                out.append(tok.val)

            elif t in ('RPAR', 'RSQB'):
                while open_blocks and open_blocks[-1].bd >= bracket_depth:
                    if len(indent_stack) > 1:
                        indent_stack.pop()
                    open_blocks.pop()
                    if out and out[-1] not in ('{', ';'):
                        out.append(';')
                    out.append('}')
                    out.append(';')
                bracket_depth = max(0, bracket_depth - 1)
                out.append(tok.val)

            elif t == 'COMMA':
                while open_blocks and open_blocks[-1].bd >= bracket_depth:
                    if len(indent_stack) > 1:
                        indent_stack.pop()
                    open_blocks.pop()
                    if out and out[-1] not in ('{', ';'):
                        out.append(';')
                    out.append('}')
                out.append(',')

            elif t == 'COLON' or t == 'FAT_ARROW':
                remaining = line_toks[tok_idx + 1:]
                if remaining:
                    # Same-line body: emit ": { body ; }" or "=> { body ; }" inline.
                    # Stop body at any COMMA/RPAR/RSQB at the current bracket depth
                    # (those belong to the outer context, not the body).
                    OUTER_STOPS = {'RPAR', 'RSQB', 'COMMA'}
                    body_toks   = []
                    suffix_toks = []
                    inner_bd    = bracket_depth  # depth at the ':' or '=>' token
                    scan_bd     = bracket_depth
                    for rt in remaining:
                        if rt.type in ('LPAR', 'LSQB'):
                            scan_bd += 1
                            body_toks.append(rt)
                        elif rt.type in ('RPAR', 'RSQB'):
                            scan_bd -= 1
                            if scan_bd < inner_bd:
                                # This closer belongs to the outer context
                                suffix_toks.append(rt)
                            else:
                                body_toks.append(rt)
                        elif rt.type == 'COMMA' and scan_bd == inner_bd:
                            # Comma at the outer level — ends the body
                            suffix_toks.append(rt)
                        elif suffix_toks:
                            # Already past the body
                            suffix_toks.append(rt)
                        else:
                            body_toks.append(rt)
                    out.append(tok.val)
                    out.append('{')
                    for rt in body_toks:
                        out.append(rt.val)
                    if body_toks and out[-1] not in ('{', ';'):
                        out.append(';')
                    out.append('}')
                    for rt in suffix_toks:
                        if rt.type in ('LPAR', 'LSQB'):
                            bracket_depth += 1
                            out.append(rt.val)
                        elif rt.type in ('RPAR', 'RSQB'):
                            # close any open_blocks inside this bracket
                            while open_blocks and open_blocks[-1].bd >= bracket_depth:
                                if len(indent_stack) > 1:
                                    indent_stack.pop()
                                open_blocks.pop()
                                if out and out[-1] not in ('{', ';'):
                                    out.append(';')
                                out.append('}')
                                out.append(';')
                            bracket_depth = max(0, bracket_depth - 1)
                            out.append(rt.val)
                        elif rt.type == 'COMMA':
                            while open_blocks and open_blocks[-1].bd >= bracket_depth:
                                if len(indent_stack) > 1:
                                    indent_stack.pop()
                                open_blocks.pop()
                                if out and out[-1] not in ('{', ';'):
                                    out.append(';')
                                out.append('}')
                                out.append(';')
                            out.append(',')
                        else:
                            out.append(rt.val)
                    break  # done with this line's tokens
                else:
                    pending.append(_Pending(bracket_depth, leading))
                    out.append(tok.val)

            else:
                out.append(tok.val)



        # emit ';' after statements that don't open a block.
        # 'statement level' means bracket_depth == the bd of the innermost open block,
        # or bracket_depth == 0 when outside all blocks.
        innermost_bd = open_blocks[-1].bd if open_blocks else 0
        if bracket_depth == innermost_bd and len(pending) == pending_before:
            out.append(';')

    # ── close remaining blocks ────────────────────────────────────────────
    while len(indent_stack) > 1:
        indent_stack.pop()
        if open_blocks:
            open_blocks.pop()
        if out and out[-1] not in ('{', ';'):
            out.append(';')
        out.append('}')
        out.append(';')

    return ' '.join(out)
