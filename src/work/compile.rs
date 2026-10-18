//! Compiler frontend model: a Kotlin-like toy language goes through
//! lexer -> recursive-descent parser -> constant folding -> type check -> bytecode.
//! Branchy, cache-sensitive, allocation-heavy code — the same shape as kotlinc/swiftc/dart frontends.

use crate::util::{fnv1a, mix, par_jobs, FastMap, Rng};
use std::fmt::Write;

const FILES: usize = 48;
const FUNCS_PER_FILE: usize = 260;
const ST_JOBS: usize = 240;
const MT_JOBS: usize = 2400;

const WORDS: &[&str] = &[
    "user", "item", "count", "index", "value", "total", "offset", "size", "state", "result", "buffer",
    "node", "view", "model", "layout", "config", "width", "height", "color", "page", "cache", "token",
    "session", "profile", "feed", "image", "scroll", "anchor",
];

pub fn setup_st() -> Box<dyn FnMut(usize) -> u64> {
    let files = gen_files();
    Box::new(move |_| (0..ST_JOBS).fold(0u64, |acc, i| acc.wrapping_add(compile_job(&files, i))))
}

pub fn setup_mt() -> Box<dyn FnMut(usize) -> u64> {
    let files = gen_files();
    Box::new(move |threads| par_jobs(threads, MT_JOBS, |i| compile_job(&files, i)))
}

fn gen_files() -> Vec<String> {
    (0..FILES).map(|i| gen_source(1000 + i as u64, FUNCS_PER_FILE)).collect()
}

fn compile_job(files: &[String], i: usize) -> u64 {
    mix(i as u64 % FILES as u64, compile(&files[i % FILES]))
}

// ---------------------------------------------------------------- source generator

struct Gen {
    r: Rng,
    out: String,
    vars: Vec<(String, bool)>,
    funcs: Vec<(String, usize)>,
    uid: u32,
}

impl Gen {
    fn ident(&mut self) -> String {
        self.uid += 1;
        format!("{}{}", WORDS[self.r.below(WORDS.len())], self.uid)
    }

    fn indent(&mut self, depth: usize) {
        for _ in 0..depth {
            self.out.push_str("    ");
        }
    }

    fn expr(&mut self, depth: u32) {
        if depth >= 4 || self.r.chance(30) {
            let k = self.r.below(100);
            if k < 35 || self.vars.is_empty() {
                let v = self.r.below(1000);
                let _ = write!(self.out, "{}", v);
            } else if k < 85 || self.funcs.is_empty() {
                let i = self.r.below(self.vars.len());
                self.out.push_str(&self.vars[i].0);
            } else {
                let (name, arity) = self.funcs[self.r.below(self.funcs.len())].clone();
                self.out.push_str(&name);
                self.out.push('(');
                for a in 0..arity {
                    if a > 0 {
                        self.out.push_str(", ");
                    }
                    self.expr(depth + 2);
                }
                self.out.push(')');
            }
        } else if self.r.chance(8) {
            self.out.push_str("-(");
            self.expr(depth + 1);
            self.out.push(')');
        } else {
            const OPS: &[&str] = &[" + ", " - ", " * ", " / ", " % ", " + ", " * "];
            let paren = self.r.chance(25);
            if paren {
                self.out.push('(');
            }
            self.expr(depth + 1);
            self.out.push_str(OPS[self.r.below(OPS.len())]);
            self.expr(depth + 1);
            if paren {
                self.out.push(')');
            }
        }
    }

    fn cond(&mut self) {
        const CMP: &[&str] = &[" < ", " > ", " <= ", " >= ", " == ", " != "];
        self.expr(2);
        self.out.push_str(CMP[self.r.below(CMP.len())]);
        self.expr(2);
        if self.r.chance(30) {
            self.out.push_str(if self.r.chance(50) { " && " } else { " || " });
            self.expr(3);
            self.out.push_str(CMP[self.r.below(CMP.len())]);
            self.expr(3);
        }
    }

    fn block(&mut self, depth: usize) {
        let mark = self.vars.len();
        for _ in 0..self.r.range(2, 6) {
            self.stmt(depth);
        }
        self.vars.truncate(mark);
    }

    fn decl(&mut self, depth: usize) {
        let mutable = self.r.chance(50);
        let name = self.ident();
        self.indent(depth);
        let _ = write!(self.out, "{} {}: Int = ", if mutable { "var" } else { "val" }, name);
        self.expr(0);
        self.out.push_str(";\n");
        self.vars.push((name, mutable));
    }

    fn stmt(&mut self, depth: usize) {
        let k = self.r.below(100);
        if k < 30 {
            self.decl(depth);
        } else if k < 45 {
            let start = if self.vars.is_empty() { 0 } else { self.r.below(self.vars.len()) };
            match (start..self.vars.len()).find(|&i| self.vars[i].1) {
                Some(i) => {
                    let name = self.vars[i].0.clone();
                    self.indent(depth);
                    let _ = write!(self.out, "{} = ", name);
                    self.expr(0);
                    self.out.push_str(";\n");
                }
                None => self.decl(depth),
            }
        } else if k < 60 && depth < 4 {
            self.indent(depth);
            self.out.push_str("if (");
            self.cond();
            self.out.push_str(") {\n");
            self.block(depth + 1);
            self.indent(depth);
            if self.r.chance(40) {
                self.out.push_str("} else {\n");
                self.block(depth + 1);
                self.indent(depth);
            }
            self.out.push_str("}\n");
        } else if k < 68 && depth < 4 {
            self.indent(depth);
            self.out.push_str("while (");
            self.cond();
            self.out.push_str(") {\n");
            self.block(depth + 1);
            self.indent(depth);
            self.out.push_str("}\n");
        } else if k < 78 {
            self.indent(depth);
            let _ = write!(self.out, "println(\"{} {} = \" + (", WORDS[self.r.below(WORDS.len())], self.uid);
            self.expr(1);
            self.out.push_str("));\n");
        } else if k < 86 {
            self.indent(depth);
            self.out.push_str("//");
            for _ in 0..self.r.range(3, 10) {
                self.out.push(' ');
                self.out.push_str(WORDS[self.r.below(WORDS.len())]);
            }
            self.out.push('\n');
        } else {
            self.decl(depth);
        }
    }
}

pub fn gen_source(seed: u64, n_funcs: usize) -> String {
    let mut g = Gen { r: Rng::new(seed), out: String::new(), vars: Vec::new(), funcs: Vec::new(), uid: 0 };
    for fi in 0..n_funcs {
        if g.r.chance(20) {
            g.out.push_str("/**\n * ");
            for _ in 0..g.r.range(5, 20) {
                g.out.push_str(WORDS[g.r.below(WORDS.len())]);
                g.out.push(' ');
            }
            g.out.push_str("\n */\n");
        }
        let name = format!("{}{}{}", WORDS[g.r.below(WORDS.len())], "Fn", fi);
        let arity = g.r.range(0, 5);
        g.vars.clear();
        let _ = write!(g.out, "fun {}(", name);
        for a in 0..arity {
            let p = g.ident();
            if a > 0 {
                g.out.push_str(", ");
            }
            let _ = write!(g.out, "{}: Int", p);
            g.vars.push((p, false));
        }
        g.out.push_str("): Int {\n");
        for _ in 0..g.r.range(4, 12) {
            g.stmt(1);
        }
        g.out.push_str("    return ");
        g.expr(0);
        g.out.push_str(";\n}\n\n");
        g.funcs.push((name, arity));
    }
    g.out
}

// ---------------------------------------------------------------- lexer

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum K {
    Ident, Int, Str, Fun, Val, Var, If, Else, While, Return,
    LParen, RParen, LBrace, RBrace, Colon, Semi, Comma, Assign,
    Plus, Minus, Star, Slash, Percent, Lt, Gt, Le, Ge, EqEq, Ne, Not, AndAnd, OrOr, Eof,
}

#[derive(Clone, Copy)]
struct Tok {
    k: K,
    start: u32,
    end: u32,
}

fn lex(src: &[u8], toks: &mut Vec<Tok>) -> u32 {
    let n = src.len();
    let mut i = 0;
    let mut errs = 0;
    let push = |toks: &mut Vec<Tok>, k, s: usize, e: usize| toks.push(Tok { k, start: s as u32, end: e as u32 });
    while i < n {
        let c = src[i];
        let next = if i + 1 < n { src[i + 1] } else { 0 };
        match c {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'/' if next == b'/' => {
                while i < n && src[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if next == b'*' => {
                i += 2;
                while i + 1 < n && !(src[i] == b'*' && src[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(n);
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let s = i;
                while i < n && (src[i].is_ascii_alphanumeric() || src[i] == b'_') {
                    i += 1;
                }
                let k = match &src[s..i] {
                    b"fun" => K::Fun,
                    b"val" => K::Val,
                    b"var" => K::Var,
                    b"if" => K::If,
                    b"else" => K::Else,
                    b"while" => K::While,
                    b"return" => K::Return,
                    _ => K::Ident,
                };
                push(toks, k, s, i);
            }
            b'0'..=b'9' => {
                let s = i;
                while i < n && src[i].is_ascii_digit() {
                    i += 1;
                }
                push(toks, K::Int, s, i);
            }
            b'"' => {
                let s = i;
                i += 1;
                while i < n && src[i] != b'"' {
                    if src[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i = (i + 1).min(n);
                push(toks, K::Str, s, i);
            }
            _ => {
                let (k, len) = match (c, next) {
                    (b'=', b'=') => (K::EqEq, 2),
                    (b'!', b'=') => (K::Ne, 2),
                    (b'<', b'=') => (K::Le, 2),
                    (b'>', b'=') => (K::Ge, 2),
                    (b'&', b'&') => (K::AndAnd, 2),
                    (b'|', b'|') => (K::OrOr, 2),
                    (b'(', _) => (K::LParen, 1),
                    (b')', _) => (K::RParen, 1),
                    (b'{', _) => (K::LBrace, 1),
                    (b'}', _) => (K::RBrace, 1),
                    (b':', _) => (K::Colon, 1),
                    (b';', _) => (K::Semi, 1),
                    (b',', _) => (K::Comma, 1),
                    (b'=', _) => (K::Assign, 1),
                    (b'+', _) => (K::Plus, 1),
                    (b'-', _) => (K::Minus, 1),
                    (b'*', _) => (K::Star, 1),
                    (b'/', _) => (K::Slash, 1),
                    (b'%', _) => (K::Percent, 1),
                    (b'<', _) => (K::Lt, 1),
                    (b'>', _) => (K::Gt, 1),
                    (b'!', _) => (K::Not, 1),
                    _ => {
                        errs += 1;
                        i += 1;
                        continue;
                    }
                };
                push(toks, k, i, i + len);
                i += len;
            }
        }
    }
    push(toks, K::Eof, n, n);
    errs
}

// ---------------------------------------------------------------- parser

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op { Add, Sub, Mul, Div, Rem, Lt, Gt, Le, Ge, Eq, Ne, And, Or }

enum Expr {
    Int(i64),
    Str(u32),
    Var(u32),
    Call(u32, u32, u32),
    Bin(Op, u32, u32),
    Neg(u32),
    Not(u32),
}

enum Stmt {
    Let { name: u32, mutable: bool, init: u32 },
    Assign { name: u32, val: u32 },
    If { cond: u32, then_: (u32, u32), els: (u32, u32) },
    While { cond: u32, body: (u32, u32) },
    Ret(u32),
    Expr(u32),
}

struct Func {
    name: u32,
    params: (u32, u32),
    body: (u32, u32),
}

struct Parser<'a> {
    toks: &'a [Tok],
    src: &'a [u8],
    pos: usize,
    exprs: Vec<Expr>,
    args: Vec<u32>,
    stmts: Vec<Stmt>,
    lists: Vec<u32>,
    params: Vec<u32>,
    funcs: Vec<Func>,
    errs: u32,
}

fn prec(k: K) -> Option<(u8, Op)> {
    Some(match k {
        K::OrOr => (1, Op::Or),
        K::AndAnd => (2, Op::And),
        K::EqEq => (3, Op::Eq),
        K::Ne => (3, Op::Ne),
        K::Lt => (4, Op::Lt),
        K::Gt => (4, Op::Gt),
        K::Le => (4, Op::Le),
        K::Ge => (4, Op::Ge),
        K::Plus => (5, Op::Add),
        K::Minus => (5, Op::Sub),
        K::Star => (6, Op::Mul),
        K::Slash => (6, Op::Div),
        K::Percent => (6, Op::Rem),
        _ => return None,
    })
}

impl<'a> Parser<'a> {
    fn peek(&self) -> K {
        self.toks[self.pos].k
    }

    fn bump(&mut self) -> u32 {
        let p = self.pos;
        if self.toks[p].k != K::Eof {
            self.pos += 1;
        }
        p as u32
    }

    fn expect(&mut self, k: K) -> u32 {
        if self.peek() == k {
            self.bump()
        } else {
            self.errs += 1;
            self.pos as u32
        }
    }

    fn push_expr(&mut self, e: Expr) -> u32 {
        self.exprs.push(e);
        (self.exprs.len() - 1) as u32
    }

    fn file(&mut self) {
        while self.peek() != K::Eof {
            if self.peek() == K::Fun {
                self.func();
            } else {
                self.errs += 1;
                self.bump();
            }
        }
    }

    fn func(&mut self) {
        self.bump();
        let name = self.expect(K::Ident);
        self.expect(K::LParen);
        let mut ps = Vec::new();
        while self.peek() == K::Ident {
            ps.push(self.bump());
            self.expect(K::Colon);
            self.expect(K::Ident);
            if self.peek() == K::Comma {
                self.bump();
            } else {
                break;
            }
        }
        self.expect(K::RParen);
        self.expect(K::Colon);
        self.expect(K::Ident);
        let start = self.params.len() as u32;
        self.params.extend(ps);
        let params = (start, self.params.len() as u32);
        let body = self.block();
        self.funcs.push(Func { name, params, body });
    }

    fn block(&mut self) -> (u32, u32) {
        self.expect(K::LBrace);
        let mut list = Vec::new();
        while self.peek() != K::RBrace && self.peek() != K::Eof {
            list.push(self.stmt());
        }
        self.expect(K::RBrace);
        let start = self.lists.len() as u32;
        self.lists.extend(list);
        (start, self.lists.len() as u32)
    }

    fn stmt(&mut self) -> u32 {
        let s = match self.peek() {
            K::Val | K::Var => {
                let mutable = self.peek() == K::Var;
                self.bump();
                let name = self.expect(K::Ident);
                if self.peek() == K::Colon {
                    self.bump();
                    self.expect(K::Ident);
                }
                self.expect(K::Assign);
                let init = self.expr();
                self.expect(K::Semi);
                Stmt::Let { name, mutable, init }
            }
            K::If => {
                self.bump();
                self.expect(K::LParen);
                let cond = self.expr();
                self.expect(K::RParen);
                let then_ = self.block();
                let els = if self.peek() == K::Else {
                    self.bump();
                    self.block()
                } else {
                    (0, 0)
                };
                Stmt::If { cond, then_, els }
            }
            K::While => {
                self.bump();
                self.expect(K::LParen);
                let cond = self.expr();
                self.expect(K::RParen);
                Stmt::While { cond, body: self.block() }
            }
            K::Return => {
                self.bump();
                let e = self.expr();
                self.expect(K::Semi);
                Stmt::Ret(e)
            }
            K::Ident if self.toks[self.pos + 1].k == K::Assign => {
                let name = self.bump();
                self.bump();
                let val = self.expr();
                self.expect(K::Semi);
                Stmt::Assign { name, val }
            }
            _ => {
                let e = self.expr();
                self.expect(K::Semi);
                Stmt::Expr(e)
            }
        };
        self.stmts.push(s);
        (self.stmts.len() - 1) as u32
    }

    fn expr(&mut self) -> u32 {
        self.binary(1)
    }

    fn binary(&mut self, min: u8) -> u32 {
        let mut l = self.unary();
        while let Some((p, op)) = prec(self.peek()) {
            if p < min {
                break;
            }
            self.bump();
            let r = self.binary(p + 1);
            l = self.push_expr(Expr::Bin(op, l, r));
        }
        l
    }

    fn unary(&mut self) -> u32 {
        match self.peek() {
            K::Minus => {
                self.bump();
                let e = self.unary();
                self.push_expr(Expr::Neg(e))
            }
            K::Not => {
                self.bump();
                let e = self.unary();
                self.push_expr(Expr::Not(e))
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> u32 {
        match self.peek() {
            K::Int => {
                let t = self.toks[self.bump() as usize];
                let v = self.src[t.start as usize..t.end as usize]
                    .iter()
                    .fold(0i64, |v, d| v.wrapping_mul(10).wrapping_add((d - b'0') as i64));
                self.push_expr(Expr::Int(v))
            }
            K::Str => {
                let t = self.bump();
                self.push_expr(Expr::Str(t))
            }
            K::Ident if self.toks[self.pos + 1].k == K::LParen => {
                let name = self.bump();
                self.bump();
                let mut args = Vec::new();
                while self.peek() != K::RParen && self.peek() != K::Eof {
                    args.push(self.expr());
                    if self.peek() == K::Comma {
                        self.bump();
                    } else {
                        break;
                    }
                }
                self.expect(K::RParen);
                let start = self.args.len() as u32;
                self.args.extend(args);
                let argc = self.args.len() as u32 - start;
                self.push_expr(Expr::Call(name, start, argc))
            }
            K::Ident => {
                let t = self.bump();
                self.push_expr(Expr::Var(t))
            }
            K::LParen => {
                self.bump();
                let e = self.expr();
                self.expect(K::RParen);
                e
            }
            _ => {
                self.errs += 1;
                self.bump();
                self.push_expr(Expr::Int(0))
            }
        }
    }
}

// ---------------------------------------------------------------- sema + codegen

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ty { Int, Str, Bool, Err }

mod opc {
    pub const PUSH: u8 = 1;
    pub const PUSHS: u8 = 2;
    pub const LOAD: u8 = 3;
    pub const STORE: u8 = 4;
    pub const CALL: u8 = 5;
    pub const PRINT: u8 = 6;
    pub const CONCAT: u8 = 7;
    pub const NEG: u8 = 8;
    pub const NOT: u8 = 9;
    pub const JZ: u8 = 10;
    pub const JMP: u8 = 11;
    pub const RET: u8 = 12;
    pub const POP: u8 = 13;
    pub const BIN: u8 = 32;
}

struct Cg<'a> {
    p: &'a Parser<'a>,
    consts: Vec<Option<i64>>,
    funcs: FastMap<&'a [u8], (u32, u32)>,
    syms: FastMap<&'a [u8], Vec<(u32, Ty, bool)>>,
    log: Vec<&'a [u8]>,
    strs: FastMap<&'a [u8], u32>,
    code: Vec<u8>,
    nslots: u32,
    errs: u32,
}

fn varint(code: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        code.push((v as u8) | 0x80);
        v >>= 7;
    }
    code.push(v as u8);
}

impl<'a> Cg<'a> {
    fn text(&self, tok: u32) -> &'a [u8] {
        let t = self.p.toks[tok as usize];
        &self.p.src[t.start as usize..t.end as usize]
    }

    fn fold(&mut self) {
        let p = self.p;
        self.consts = Vec::with_capacity(p.exprs.len());
        for e in &p.exprs {
            let c = |i: &u32| self.consts[*i as usize];
            let v = match e {
                Expr::Int(v) => Some(*v),
                Expr::Neg(a) => c(a).map(|v| v.wrapping_neg()),
                Expr::Bin(op, a, b) => match (c(a), c(b)) {
                    (Some(x), Some(y)) => match op {
                        Op::Add => Some(x.wrapping_add(y)),
                        Op::Sub => Some(x.wrapping_sub(y)),
                        Op::Mul => Some(x.wrapping_mul(y)),
                        Op::Div if y != 0 => Some(x.wrapping_div(y)),
                        Op::Rem if y != 0 => Some(x.wrapping_rem(y)),
                        _ => None,
                    },
                    _ => None,
                },
                _ => None,
            };
            self.consts.push(v);
        }
    }

    fn lookup(&self, name: &[u8]) -> Option<(u32, Ty, bool)> {
        self.syms.get(name).and_then(|v| v.last().copied())
    }

    fn declare(&mut self, name: &'a [u8], ty: Ty, mutable: bool) -> u32 {
        let slot = self.nslots;
        self.nslots += 1;
        self.syms.entry(name).or_default().push((slot, ty, mutable));
        self.log.push(name);
        slot
    }

    fn expr(&mut self, e: u32) -> Ty {
        if let Some(v) = self.consts[e as usize] {
            self.code.push(opc::PUSH);
            varint(&mut self.code, ((v << 1) ^ (v >> 63)) as u64);
            return Ty::Int;
        }
        match self.p.exprs[e as usize] {
            Expr::Int(_) => unreachable!(),
            Expr::Str(t) => {
                let s = self.text(t);
                let n = self.strs.len() as u32;
                let idx = *self.strs.entry(s).or_insert(n);
                self.code.push(opc::PUSHS);
                varint(&mut self.code, idx as u64);
                Ty::Str
            }
            Expr::Var(t) => match self.lookup(self.text(t)) {
                Some((slot, ty, _)) => {
                    self.code.push(opc::LOAD);
                    varint(&mut self.code, slot as u64);
                    ty
                }
                None => {
                    self.errs += 1;
                    Ty::Err
                }
            },
            Expr::Call(t, start, argc) => {
                let name = self.text(t);
                let mut tys = [Ty::Int; 8];
                for i in 0..argc {
                    let ty = self.expr(self.p.args[(start + i) as usize]);
                    tys[(i as usize).min(7)] = ty;
                }
                if name == b"println" {
                    if argc != 1 {
                        self.errs += 1;
                    }
                    self.code.push(opc::PRINT);
                    return Ty::Int;
                }
                match self.funcs.get(name).copied() {
                    Some((idx, arity)) => {
                        if arity != argc || tys[..argc.min(8) as usize].iter().any(|&t| t != Ty::Int) {
                            self.errs += 1;
                        }
                        self.code.push(opc::CALL);
                        varint(&mut self.code, idx as u64);
                        varint(&mut self.code, argc as u64);
                    }
                    None => self.errs += 1,
                }
                Ty::Int
            }
            Expr::Bin(op, a, b) => {
                let ta = self.expr(a);
                let tb = self.expr(b);
                let ty = match op {
                    Op::Add if ta == Ty::Str || tb == Ty::Str => {
                        self.code.push(opc::CONCAT);
                        return Ty::Str;
                    }
                    Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Rem => {
                        if ta != Ty::Int || tb != Ty::Int {
                            self.errs += 1;
                        }
                        Ty::Int
                    }
                    Op::Lt | Op::Gt | Op::Le | Op::Ge => {
                        if ta != Ty::Int || tb != Ty::Int {
                            self.errs += 1;
                        }
                        Ty::Bool
                    }
                    Op::Eq | Op::Ne => {
                        if ta != tb {
                            self.errs += 1;
                        }
                        Ty::Bool
                    }
                    Op::And | Op::Or => {
                        if ta != Ty::Bool || tb != Ty::Bool {
                            self.errs += 1;
                        }
                        Ty::Bool
                    }
                };
                self.code.push(opc::BIN + op as u8);
                ty
            }
            Expr::Neg(a) => {
                if self.expr(a) != Ty::Int {
                    self.errs += 1;
                }
                self.code.push(opc::NEG);
                Ty::Int
            }
            Expr::Not(a) => {
                if self.expr(a) != Ty::Bool {
                    self.errs += 1;
                }
                self.code.push(opc::NOT);
                Ty::Bool
            }
        }
    }

    fn jump(&mut self, op: u8) -> usize {
        self.code.push(op);
        self.code.extend_from_slice(&[0; 4]);
        self.code.len() - 4
    }

    fn patch(&mut self, at: usize, target: usize) {
        let rel = (target as i64 - at as i64) as i32;
        self.code[at..at + 4].copy_from_slice(&rel.to_le_bytes());
    }

    fn cond(&mut self, e: u32) {
        if self.expr(e) != Ty::Bool {
            self.errs += 1;
        }
    }

    fn block(&mut self, (start, end): (u32, u32)) {
        let mark = self.log.len();
        for i in start..end {
            self.stmt(self.p.lists[i as usize]);
        }
        for name in self.log.drain(mark..) {
            self.syms.get_mut(name).unwrap().pop();
        }
    }

    fn stmt(&mut self, s: u32) {
        match self.p.stmts[s as usize] {
            Stmt::Let { name, mutable, init } => {
                let ty = self.expr(init);
                let slot = self.declare(self.text(name), ty, mutable);
                self.code.push(opc::STORE);
                varint(&mut self.code, slot as u64);
            }
            Stmt::Assign { name, val } => {
                let ty = self.expr(val);
                match self.lookup(self.text(name)) {
                    Some((slot, vty, mutable)) => {
                        if !mutable || vty != ty {
                            self.errs += 1;
                        }
                        self.code.push(opc::STORE);
                        varint(&mut self.code, slot as u64);
                    }
                    None => self.errs += 1,
                }
            }
            Stmt::If { cond, then_, els } => {
                self.cond(cond);
                let jz = self.jump(opc::JZ);
                self.block(then_);
                if els.0 != els.1 {
                    let jmp = self.jump(opc::JMP);
                    self.patch(jz, self.code.len());
                    self.block(els);
                    self.patch(jmp, self.code.len());
                } else {
                    self.patch(jz, self.code.len());
                }
            }
            Stmt::While { cond, body } => {
                let top = self.code.len();
                self.cond(cond);
                let jz = self.jump(opc::JZ);
                self.block(body);
                let back = self.jump(opc::JMP);
                self.patch(back, top);
                self.patch(jz, self.code.len());
            }
            Stmt::Ret(e) => {
                if self.expr(e) != Ty::Int {
                    self.errs += 1;
                }
                self.code.push(opc::RET);
            }
            Stmt::Expr(e) => {
                self.expr(e);
                self.code.push(opc::POP);
            }
        }
    }
}

/// Returns (checksum, error count).
fn compile_full(src: &str) -> (u64, u32) {
    let src = src.as_bytes();
    let mut toks = Vec::new();
    let lex_errs = lex(src, &mut toks);
    let mut p = Parser {
        toks: &toks,
        src,
        pos: 0,
        exprs: Vec::new(),
        args: Vec::new(),
        stmts: Vec::new(),
        lists: Vec::new(),
        params: Vec::new(),
        funcs: Vec::new(),
        errs: lex_errs,
    };
    p.file();

    let mut cg = Cg {
        p: &p,
        consts: Vec::new(),
        funcs: FastMap::default(),
        syms: FastMap::default(),
        log: Vec::new(),
        strs: FastMap::default(),
        code: Vec::new(),
        nslots: 0,
        errs: 0,
    };
    cg.fold();
    for (i, f) in p.funcs.iter().enumerate() {
        let arity = f.params.1 - f.params.0;
        if cg.funcs.insert(cg.text(f.name), (i as u32, arity)).is_some() {
            cg.errs += 1;
        }
    }
    for f in &p.funcs {
        cg.nslots = 0;
        let mark = cg.log.len();
        for i in f.params.0..f.params.1 {
            let name = cg.text(p.params[i as usize]);
            cg.declare(name, Ty::Int, false);
        }
        cg.block(f.body);
        for name in cg.log.drain(mark..) {
            cg.syms.get_mut(name).unwrap().pop();
        }
    }
    let errs = p.errs + cg.errs;
    let h = mix(fnv1a(&cg.code), toks.len() as u64);
    let h = mix(h, (p.exprs.len() as u64) << 32 | cg.strs.len() as u64);
    (mix(h, errs as u64), errs)
}

fn compile(src: &str) -> u64 {
    compile_full(src).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_code_compiles_cleanly() {
        for seed in 0..8 {
            let src = gen_source(seed, 120);
            let (_, errs) = compile_full(&src);
            assert_eq!(errs, 0, "seed {} produced errors:\n{}", seed, &src[..2000.min(src.len())]);
        }
    }

    #[test]
    fn detects_errors() {
        let (_, errs) = compile_full("fun f(a: Int): Int { val x: Int = 1; x = 2; return y + \"s\"; }");
        assert!(errs >= 2);
    }
}
