#!/usr/bin/env python3
"""Reads GARbro's scheme database (ArcFormats/Resources/Formats.dat: "GARbroDB", a version, then a
zlib stream of a .NET BinaryFormatter graph) into plain Python values, to look up per-game keys.

    garbro_db.py FORMATS.DAT TAG [GREP]   print the schemes of archive format TAG as JSON
                                          (only those whose JSON contains GREP)
    garbro_db.py FORMATS.DAT warc OUT NAME [HELPER]
                                          write ShiinaRio WARC scheme NAME for crates/shiina;
                                          HELPER (hex) replaces its helper key, which a trial's exe
                                          can have different from the full game's

Objects become dicts with "$class"; byte arrays become hex strings.
"""
import json
import struct
import sys
import zlib

# PrimitiveTypeEnum -> struct format (Char is one UTF-8 byte here; Decimal is a string)
PRIM = {1: "?", 2: "B", 3: "B", 6: "d", 7: "h", 8: "i", 9: "q", 10: "b", 11: "f", 12: "q", 13: "q", 14: "H", 15: "I", 16: "Q"}


class Ref:
    def __init__(self, id_):
        self.id = id_


class Reader:
    def __init__(self, data):
        self.d, self.p = data, 0
        self.objs = {}
        self.classes = {}

    def u8(self):
        v = self.d[self.p]
        self.p += 1
        return v

    def i32(self):
        v = struct.unpack_from("<i", self.d, self.p)[0]
        self.p += 4
        return v

    def string(self):
        n, shift = 0, 0
        while True:
            b = self.u8()
            n |= (b & 0x7F) << shift
            shift += 7
            if b < 0x80:
                break
        s = self.d[self.p : self.p + n].decode("utf-8")
        self.p += n
        return s

    def prim(self, t):
        if t == 18:
            return self.string()
        if t == 4:  # Decimal is a string
            return self.string()
        fmt = PRIM[t]
        v = struct.unpack_from("<" + fmt, self.d, self.p)[0]
        self.p += struct.calcsize(fmt)
        return v

    def class_info(self):
        id_ = self.i32()
        name = self.string()
        n = self.i32()
        members = [self.string() for _ in range(n)]
        return id_, name, members

    def member_types(self, n):
        bts = [self.u8() for _ in range(n)]
        extra = []
        for bt in bts:
            if bt == 0 or bt == 7:
                extra.append(self.u8())
            elif bt == 3:
                extra.append(self.string())
            elif bt == 4:
                extra.append((self.string(), self.i32()))
            else:
                extra.append(None)
        return list(zip(bts, extra))

    def values(self, name, members, types):
        obj = {"$class": name}
        for m, (bt, ex) in zip(members, types):
            if bt == 0:
                obj[m] = self.prim(ex)
            else:
                obj[m] = self.record()
        return obj

    def record(self):
        rt = self.u8()
        if rt == 0:  # header
            self.p += 16
            return self.record()
        if rt == 1:  # ClassWithId
            id_ = self.i32()
            meta = self.i32()
            name, members, types = self.classes[meta]
            obj = self.values(name, members, types)
            self.objs[id_] = obj
            return obj
        if rt in (2, 4, 5):  # SystemClassWithMembersAndTypes / ClassWithMembersAndTypes
            id_, name, members = self.class_info()
            types = self.member_types(len(members))
            if rt == 5:
                self.i32()  # library
            self.classes[id_] = (name, members, types)
            obj = self.values(name, members, types)
            self.objs[id_] = obj
            return obj
        if rt in (1, 3):
            raise ValueError("untyped classes")
        if rt == 6:  # BinaryObjectString
            id_ = self.i32()
            s = self.string()
            self.objs[id_] = s
            return s
        if rt == 7:  # BinaryArray
            id_ = self.i32()
            kind = self.u8()
            rank = self.i32()
            lengths = [self.i32() for _ in range(rank)]
            if kind in (3, 4, 5):
                [self.i32() for _ in range(rank)]
            bt, ex = self.member_types(1)[0]
            total = 1
            for n in lengths:
                total *= n
            arr = self.array_items(total, bt, ex)
            self.objs[id_] = arr
            return arr
        if rt == 8:  # MemberPrimitiveTyped
            return self.prim(self.u8())
        if rt == 9:  # MemberReference
            return Ref(self.i32())
        if rt == 10:  # ObjectNull
            return None
        if rt == 11:  # MessageEnd
            return StopIteration
        if rt == 12:  # BinaryLibrary
            self.i32()
            self.string()
            return self.record()
        if rt == 13:
            return ("null", self.u8())
        if rt == 14:
            return ("null", self.i32())
        if rt == 15:  # ArraySinglePrimitive
            id_ = self.i32()
            n = self.i32()
            t = self.u8()
            if t == 2:
                arr = self.d[self.p : self.p + n].hex()
                self.p += n
            else:
                arr = [self.prim(t) for _ in range(n)]
            self.objs[id_] = arr
            return arr
        if rt in (16, 17):  # ArraySingleObject / ArraySingleString
            id_ = self.i32()
            n = self.i32()
            arr = self.array_items(n, 2 if rt == 16 else 1, None)
            self.objs[id_] = arr
            return arr
        raise ValueError(f"record {rt} at {self.p - 1}")

    def array_items(self, n, bt, ex):
        out = []
        while len(out) < n:
            if bt == 0:
                out.append(self.prim(ex))
                continue
            v = self.record()
            if isinstance(v, tuple) and v[0] == "null":
                out += [None] * v[1]
            else:
                out.append(v)
        return out

    def resolve(self, v, seen=None):
        seen = seen or set()
        if isinstance(v, Ref):
            if v.id in seen:
                return f"<ref {v.id}>"
            return self.resolve(self.objs.get(v.id), seen | {v.id})
        if isinstance(v, dict):
            return {k: self.resolve(x, seen) for k, x in v.items()}
        if isinstance(v, list):
            return [self.resolve(x, seen) for x in v]
        return v


def load(path):
    data = open(path, "rb").read()
    assert data[:8] == b"GARbroDB"
    r = Reader(zlib.decompress(data[12:]))
    root = r.record()
    while r.record() is not StopIteration:
        pass
    return r, r.resolve(root)


def dictionary(d):
    """A serialized Dictionary`2 as a Python dict."""
    pairs = d.get("KeyValuePairs") or []
    return {p["key"]: p["value"] for p in pairs if p}


def write_warc(db, out, name, helper=None):
    """crates/shiina/src/warc.rs Scheme::parse: "SRWS", version, entry name size, then
    length-prefixed crypt key, helper key (u32s), region, decode bin, Shiina image, extra crypt."""
    scheme = next(
        s for s in dictionary(db["SchemeMap"])["WAR"]["KnownSchemes"] if s["<Name>k__BackingField"] == name
    )
    blob = lambda b: struct.pack("<I", len(b)) + b
    hexb = lambda h: bytes.fromhex(h or "")
    image = scheme["ShiinaImage"] or {}
    shiina = hexb(image.get("m_common"))[: image.get("m_common_length", 0)] + hexb(image.get("m_extra"))
    extra = (scheme.get("ExtraCrypt") or {}).get("$class", "").rsplit(".", 1)[-1]
    data = b"SRWS" + struct.pack("<II", scheme["<Version>k__BackingField"], scheme["EntryNameSize"])
    data += blob(hexb(scheme["CryptKey"]))
    helper_key = bytes.fromhex(helper) if helper else b"".join(struct.pack("<I", k) for k in scheme["HelperKey"] or [])
    data += blob(helper_key)
    data += blob(hexb(scheme["Region"])) + blob(hexb(scheme["DecodeBin"])) + blob(shiina)
    data += blob(extra.encode())
    open(out, "wb").write(data)


def main():
    path, tag = sys.argv[1], sys.argv[2]
    if tag == "warc":
        _, db = load(path)
        write_warc(db, sys.argv[3], sys.argv[4], sys.argv[5] if len(sys.argv) > 5 else None)
        return
    grep = sys.argv[3] if len(sys.argv) > 3 else None
    _, db = load(path)
    scheme = dictionary(db["SchemeMap"]).get(tag)
    text = json.dumps(scheme, ensure_ascii=False, indent=1)
    if grep is None:
        print(text)
        return
    for k, v in (scheme or {}).items():
        items = v if isinstance(v, list) else (dictionary(v).items() if isinstance(v, dict) and "KeyValuePairs" in v else [])
        for item in items:
            t = json.dumps(item, ensure_ascii=False)
            if grep in t:
                print(json.dumps(item, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
