#!/usr/bin/env python3
"""Writes typed call wrappers for xsystem4's HLL libraries (see shim/ffi.h).

    gen_hll.py OUT_DIR SOURCE.c... -- COMPILER [FLAGS...]

For each SOURCE (a src/hll/*.c file), the functions its HLL_LIBRARY table exports (`.fun = name`
after preprocessing) and those it swaps in with static_library_replace are looked up in clang's AST dump for their C type. OUT_DIR/hll_<name>.c then
includes SOURCE and adds one wrapper per function plus a table vnrun_wraps_<name> from function
address to wrapper; OUT_DIR/hll_wraps.c lists every table for ffi_call.
"""
import os
import re
import subprocess
import sys


def write(path, text):
    """Writes only changed files, so that the build recompiles only those."""
    try:
        with open(path) as fh:
            if fh.read() == text:
                return
    except FileNotFoundError:
        pass
    with open(path, "w") as fh:
        fh.write(text)

DECL = re.compile(r"^[|`]-FunctionDecl .*? (\w+) '([^']*)'")
FUN = re.compile(r"\.fun\s*=\s*(\w+)")


def split_top(s):
    """Splits a parameter list at the commas outside parentheses and brackets."""
    out, depth, cur = [], 0, ""
    for ch in s:
        if ch in "([":
            depth += 1
        elif ch in ")]":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def replace_calls(text):
    """The text of each static_library_replace call's last argument."""
    for m in re.finditer(r"\bstatic_library_replace\s*\(", text):
        depth, i = 1, m.end()
        while depth and i < len(text):
            depth += {"(": 1, ")": -1}.get(text[i], 0)
            i += 1
        args = split_top(text[m.end() : i - 1])
        if len(args) == 3:
            yield args[2]


def parse_type(t):
    """'ret (a, b)' -> ('ret', ['a', 'b']); the return type is before the outermost list."""
    depth = 0
    for i in range(len(t) - 1, -1, -1):
        if t[i] == ")":
            depth += 1
        elif t[i] == "(":
            depth -= 1
            if depth == 0:
                ret, params = t[:i].strip(), split_top(t[i + 1 : -1])
                if params == ["void"]:
                    params = []
                return ret, [p for p in params if p != "..."]
    raise ValueError(t)


def c_type(t):
    # Anonymous types cannot be named again; HLL code passes them as ints.
    return "int" if "unnamed" in t or "anonymous" in t else t


def main():
    sep = sys.argv.index("--")
    out_dir, sources, cc = sys.argv[1], sys.argv[2:sep], sys.argv[sep + 1 :]
    os.makedirs(out_dir, exist_ok=True)
    tables = []
    for src in sources:
        name = re.sub(r"\W", "_", os.path.splitext(os.path.basename(src))[0])
        pre = subprocess.run(cc + ["-E", src], check=True, capture_output=True).stdout.decode("utf-8", "replace")
        funs = [f for f in dict.fromkeys(FUN.findall(pre)) if f != "NULL"]
        if not funs:
            # A helper without a library table (iarray.c): compiled as it is.
            write(os.path.join(out_dir, f"hll_{name}.c"), f'#include "{os.path.abspath(src)}"\n')
            continue
        ast = subprocess.run(
            cc + ["-fsyntax-only", "-fno-color-diagnostics", "-Xclang", "-ast-dump", src],
            check=True,
            capture_output=True,
        ).stdout.decode("utf-8", "replace")
        types = {}
        for line in ast.splitlines():
            m = DECL.match(line)
            if m:
                types.setdefault(m.group(1), m.group(2))
        # Functions swapped in at run time: static_library_replace(lib, "name", fun).
        for call in replace_calls(pre):
            for ident in re.findall(r"\b[A-Za-z_]\w*\b", call):
                if ident in types and ident not in funs:
                    funs.append(ident)
        lines = [f'#include "{os.path.abspath(src)}"', '#include "ffi.h"', ""]
        entries = []
        for i, f in enumerate(funs):
            if f not in types:
                sys.exit(f"gen_hll: no declaration of {f} in {src}")
            ret, params = parse_type(types[f])
            ret = c_type(ret)
            body = []
            args = []
            for j, p in enumerate(params):
                body.append(f"\ttypedef __typeof__({c_type(p)}) t{j};")
                args.append(f"n > {j} ? *(t{j} *)a[{j}] : (t{j}){{0}}")
            call = f"{f}({', '.join(args)})"
            if ret == "void":
                body.append(f"\t{call};")
                body.insert(0, "\t(void)r;")
            else:
                body.append(f"\ttypedef __typeof__({ret}) rt;")
                body.append(f"\t*(rt *)r = {call};")
            if not params:
                body.insert(0, "\t(void)a; (void)n;")
            lines.append(f"static void vnrun_w{i}(void *r, void **a, unsigned n)\n{{\n" + "\n".join(body) + "\n}")
            entries.append(f"\t{{ (void *){f}, vnrun_w{i} }},")
        lines.append(f"\nconst struct vnrun_hll_wrap vnrun_wraps_{name}[] = {{")
        lines += entries
        lines.append("\t{ 0, 0 }\n};")
        write(os.path.join(out_dir, f"hll_{name}.c"), "\n".join(lines) + "\n")
        tables.append(name)
    text = '#include "ffi.h"\n'
    text += "".join(f"extern const struct vnrun_hll_wrap vnrun_wraps_{t}[];\n" for t in tables)
    text += "const struct vnrun_hll_wrap *const vnrun_hll_wraps[] = {\n"
    text += "".join(f"\tvnrun_wraps_{t},\n" for t in tables)
    text += "\t0\n};\n"
    write(os.path.join(out_dir, "hll_wraps.c"), text)


if __name__ == "__main__":
    main()
