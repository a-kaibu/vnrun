//! The stack machine's instructions and compiled functions.

/// A value on the stack is 32 bits: an int, a pointer or a float's bits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Push(u32),
    Pop,
    Dup,
    /// the frame's address plus an offset
    Fp(u32),
    Ld8s,
    Ld8u,
    Ld16s,
    Ld16u,
    Ld32,
    /// [addr, value] -> [value]
    St8,
    St16,
    St32,
    /// [dst, src] -> [dst], copying n bytes
    Copy(u32),
    /// [addr] -> [], zeroing n bytes
    Zero(u32),
    Add,
    Sub,
    Mul,
    DivS,
    DivU,
    ModS,
    ModU,
    And,
    Or,
    Xor,
    Shl,
    ShrS,
    ShrU,
    Neg,
    BitNot,
    LNot,
    Eq,
    Ne,
    LtS,
    LtU,
    LeS,
    LeU,
    GtS,
    GtU,
    GeS,
    GeU,
    FAdd,
    FSub,
    FMul,
    FDiv,
    FMod,
    FNeg,
    FEq,
    FNe,
    FLt,
    FLe,
    FGt,
    FGe,
    I2F,
    U2F,
    F2I,
    F2U,
    Sx8,
    Zx8,
    Sx16,
    Zx16,
    /// a float to 0 or 1 (for conditions)
    FBool,
    Jmp(u32),
    Jz(u32),
    Jnz(u32),
    /// function index, argument count
    Call(u32, u8),
    /// [args..., fnptr] -> [result]
    CallInd(u8),
    /// built-in id, argument count
    Native(u16, u8),
    /// [value] -> returns it
    Ret,
    /// [addr] -> [old value], adding delta to a value of `width` bytes (signed)
    IncDec {
        width: u8,
        signed: bool,
        delta: i32,
        post: bool,
    },
    FIncDec {
        delta: i8,
        post: bool,
    },
    /// [a, b] -> [b, a]
    Swap,
}

/// Where a function's code came from, for error messages.
#[derive(Debug, Clone, Default)]
pub struct Function {
    pub name: String,
    pub code: Vec<Op>,
    /// source line of each instruction
    pub lines: Vec<u32>,
    pub file: String,
    /// bytes of parameters and locals
    pub frame: u32,
    /// each parameter: offset in the frame and size (a struct is copied in)
    pub params: Vec<(u32, u32)>,
    pub defined: bool,
}

/// Function pointers are numbers above this (the index added).
pub const FN_BASE: u32 = 0xF000_0000;
/// Built-in functions taken as pointers.
pub const NATIVE_BASE: u32 = 0xFF00_0000;
