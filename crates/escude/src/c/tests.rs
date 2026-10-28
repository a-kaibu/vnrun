//! Compiles small programs and runs them, checking what they print.

use std::collections::HashMap;

use crate::c::compile::compile;
use crate::vm::{Host, Native, Vm};

struct Print(String);

impl Host for Print {
    fn native(&mut self, vm: &mut Vm, id: u16, a: &[u32]) -> Result<Native, String> {
        match vm.natives[id as usize].as_str() {
            "put" => {
                self.0.push_str(&format!("{} ", a[0] as i32));
                Ok(Native::Value(0))
            }
            "putf" => {
                self.0.push_str(&format!("{} ", f32::from_bits(a[0])));
                Ok(Native::Value(0))
            }
            "puts" => {
                let s = vm.cstr(a[0])?;
                self.0
                    .push_str(&format!("{} ", String::from_utf8_lossy(&s)));
                Ok(Native::Value(0))
            }
            n => Err(format!("no {n}")),
        }
    }
}

const HEADER: &str = "#trap void put(nValue);\n#trap void putf(fValue);\n#trap void puts(pszText);\n\
typedef unsigned char BYTE; typedef unsigned short WORD; typedef unsigned long DWORD;\n";

fn run(src: &str) -> String {
    let mut files = HashMap::new();
    files.insert(
        "t\\main.c".to_string(),
        format!("{HEADER}{src}").into_bytes(),
    );
    let mut load = |p: &str| files.get(p).cloned();
    let unit = crate::c::parse_program("t\\main.c", "t\\", &mut load).unwrap();
    let mut vm = Vm::new();
    let c = compile(&unit, &mut vm, None).unwrap();
    vm.call(c.start, &[]).unwrap();
    let mut host = Print(String::new());
    vm.run(&mut host, 1_000_000).unwrap();
    host.0.trim_end().to_string()
}

#[test]
fn arithmetic_and_loops() {
    assert_eq!(
        run(
            "void main(){ int i, s = 0; for(i = 0; i < 10; i++) s += i; put(s); put(-7 / 2); put(-7 % 2); }"
        ),
        "45 -3 -1"
    );
}

#[test]
fn pointer_to_pointer_steps() {
    let src = r#"
        int get_byte(BYTE** pp) { char ret = **pp; (*pp)++; return ret; }
        int get_dword(DWORD** pp) { int ret = **pp; (*pp)++; return ret; }
        BYTE data[16] = { 1, 0x10, 0, 0, 0, 2, 0xff };
        void main() {
            BYTE* p = data;
            put(get_byte(&p));
            put(get_dword(&p));
            put(get_byte(&p));
            put(get_byte(&p));
            put(p - data);
        }"#;
    assert_eq!(run(src), "1 16 2 -1 7");
}

#[test]
fn structs_and_switch() {
    let src = r#"
        typedef struct { BYTE mode; int time; int param; } F;
        F fs[2] = { { 1, 100, 5 }, { 2, 200, 6 } };
        int pick(int n) { switch(n) { case 1: return 10; case 2: case 3: return 20; default: return 0; } }
        void main() {
            F* f = fs;
            f++;
            put(f->time);
            put(sizeof(F));
            put(pick(1)); put(pick(3)); put(pick(9));
            { float x = 1.5f * 2; put((int)x); }
            puts("ok");
        }"#;
    assert_eq!(run(src), "200 9 10 20 0 3 ok");
}
