//! Types and syntax trees of ACPX C (a 32-bit C: int, long and pointers are 4 bytes, double is
//! float).

use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Void,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    Ptr(Box<Ty>),
    /// element and count (0 when not given)
    Array(Box<Ty>, u32),
    Struct(usize),
    Func(Rc<FuncTy>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FuncTy {
    pub ret: Ty,
    pub params: Vec<Ty>,
    pub variadic: bool,
    /// the parameters were declared (not `f()`)
    pub known: bool,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub ty: Ty,
    pub off: u32,
}

#[derive(Debug, Clone, Default)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<Field>,
    pub size: u32,
    pub align: u32,
    pub complete: bool,
    pub union: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Types {
    pub structs: Vec<StructDef>,
}

impl Types {
    pub fn size(&self, t: &Ty) -> u32 {
        match t {
            Ty::Void => 1,
            Ty::I8 | Ty::U8 => 1,
            Ty::I16 | Ty::U16 => 2,
            Ty::I32 | Ty::U32 | Ty::F32 | Ty::Ptr(_) | Ty::Func(_) => 4,
            Ty::Array(e, n) => self.size(e) * n,
            Ty::Struct(i) => self.structs[*i].size,
        }
    }

    pub fn align(&self, t: &Ty) -> u32 {
        match t {
            Ty::Array(e, _) => self.align(e),
            Ty::Struct(i) => self.structs[*i].align.max(1),
            Ty::Void => 1,
            t => self.size(t),
        }
    }

    pub fn field(&self, s: usize, name: &str) -> Option<&Field> {
        self.structs[s].fields.iter().find(|f| f.name == name)
    }

    /// Lays out a struct's fields. ACPX packs them: no padding (a .mot file's 26-byte
    /// pattern records are a WORD then a RECT).
    pub fn layout(&mut self, s: usize, fields: Vec<(String, Ty)>, union: bool) {
        let mut out = Vec::new();
        let (mut off, mut size, align) = (0u32, 0u32, 1u32);
        for (name, ty) in fields {
            let sz = self.size(&ty);
            let at = if union { 0 } else { off };
            out.push(Field { name, ty, off: at });
            off = at + sz;
            size = size.max(off);
        }
        let size = size.div_ceil(align) * align;
        let d = &mut self.structs[s];
        d.fields = out;
        d.size = size;
        d.align = align;
        d.complete = true;
        d.union = union;
    }
}

impl Ty {
    pub fn is_float(&self) -> bool {
        *self == Ty::F32
    }

    pub fn is_ptr(&self) -> bool {
        matches!(self, Ty::Ptr(_) | Ty::Array(..) | Ty::Func(_))
    }

    pub fn is_int(&self) -> bool {
        matches!(
            self,
            Ty::I8 | Ty::U8 | Ty::I16 | Ty::U16 | Ty::I32 | Ty::U32
        )
    }

    pub fn is_scalar(&self) -> bool {
        self.is_int() || self.is_float() || matches!(self, Ty::Ptr(_))
    }

    pub fn unsigned(&self) -> bool {
        matches!(
            self,
            Ty::U8 | Ty::U16 | Ty::U32 | Ty::Ptr(_) | Ty::Array(..) | Ty::Func(_)
        )
    }

    /// What a pointer or array points at.
    pub fn target(&self) -> Option<&Ty> {
        match self {
            Ty::Ptr(t) | Ty::Array(t, _) => Some(t),
            _ => None,
        }
    }

    pub fn ptr(self) -> Ty {
        Ty::Ptr(Box::new(self))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UOp {
    Neg,
    Plus,
    BitNot,
    Not,
    Deref,
    Addr,
    PreInc,
    PreDec,
    PostInc,
    PostDec,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Shl,
    Shr,
    And,
    Or,
    Xor,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    LAnd,
    LOr,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(u32, Ty),
    Float(f32),
    Str(Vec<u8>),
    Name(String),
    Unary(UOp, Box<Expr>),
    Bin(BOp, Box<Expr>, Box<Expr>),
    Assign(Option<BOp>, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Comma(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    /// object, field, through a pointer (->)
    Member(Box<Expr>, String, bool),
    Cast(Ty, Box<Expr>),
    SizeofTy(Ty),
    SizeofE(Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum Init {
    Expr(Expr),
    List(Vec<Init>),
}

#[derive(Debug, Clone)]
pub struct Decl {
    pub name: String,
    pub ty: Ty,
    pub init: Option<Init>,
    pub is_static: bool,
    pub is_extern: bool,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr, u32),
    Decls(Vec<Decl>, u32),
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>, u32),
    While(Expr, Box<Stmt>, u32),
    Do(Box<Stmt>, Expr, u32),
    For(
        Option<Box<Stmt>>,
        Option<Expr>,
        Option<Expr>,
        Box<Stmt>,
        u32,
    ),
    Switch(Expr, Box<Stmt>, u32),
    Case(Expr, u32),
    Default,
    Break(u32),
    Continue(u32),
    Return(Option<Expr>, u32),
    Goto(String, u32),
    Label(String),
    Empty,
}

#[derive(Debug, Clone)]
pub struct Func {
    pub name: String,
    pub ty: Rc<FuncTy>,
    pub params: Vec<(String, Ty)>,
    pub body: Stmt,
    pub file: String,
}

#[derive(Debug, Clone)]
pub enum Item {
    Func(Func),
    Var(Decl),
}

/// A parsed program: its items, types and the engine's built-in functions it declares.
#[derive(Debug, Clone, Default)]
pub struct Unit {
    pub items: Vec<Item>,
    pub types: Types,
    /// built-in functions: name, type
    pub traps: Vec<(String, Rc<FuncTy>)>,
    /// functions declared but not defined here, with their types
    pub protos: Vec<(String, Rc<FuncTy>)>,
}
