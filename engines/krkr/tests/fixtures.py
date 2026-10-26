"""Generate small, original XP3 fixtures for the browser smoke test."""
import pathlib
import struct
import sys
import zlib

MAGIC = bytes.fromhex("5850330d0a200a1a8b6701")
STARTUP = b'''// Original vnrun regression scenario.
Scripts.execStorage(global.KirikiriPolyfillRoot + "KAGParser.tjs");
global.f = %[ready:true];
var parser = new KAGParser();
parser.loadScenario("condition.ks");
var tag = parser.getNextTag();
if(tag.tagname != "image" || tag.storage != "first") throw new Exception("trailing condition");
tag = parser.getNextTag();
if(tag.tagname != "image" || tag.storage != "second" || tag.page != "fore") throw new Exception("following attributes");
Debug.message("VNRUN_KRKR_SMOKE_OK");
var window = new Window();
window.visible = true;
'''
SCENARIO = b'@image storage=first cond=global.f.bg === void\n@image storage=second cond=global.f.ready page=fore\n'

def chunk(tag, data):
    return tag + struct.pack('<Q', len(data)) + data

def crypt(data, hash_, period):
    value = (hash_ ^ 0x12345678) & 0x7fffffff
    value |= (value << 31) & 0xffffffff
    key = []
    for _ in range(32):
        key.append(value & 255)
        value = (((value & 0xfffffffe) << 23) | (value >> 8)) & 0xffffffff
    return bytes(v ^ key[i % period] for i, v in enumerate(data))

def archive(files, period):
    content = bytearray(MAGIC + bytes(8))
    index = bytearray()
    for name, data in files.items():
        hash_ = zlib.adler32(data)
        encoded = crypt(data, hash_, period) if period else data
        pos = len(content)
        # Compressed entries also exercise the real stream decompression path.
        stored = zlib.compress(encoded)
        content.extend(stored)
        info = struct.pack('<IQQH', 0x80000000 if period else 0, len(data), len(stored), len(name)) + name.encode('utf-16le')
        entry = chunk(b'info', info) + chunk(b'segm', struct.pack('<IQQQ', 1, pos, len(data), len(stored))) + chunk(b'adlr', struct.pack('<I', hash_))
        index.extend(chunk(b'File', entry))
    struct.pack_into('<Q', content, 11, len(content))
    packed = zlib.compress(index)
    content.extend(b'\1' + struct.pack('<QQ', len(packed), len(index)) + packed)
    return content

root = pathlib.Path(sys.argv[1])
for name, period, ext in [('mado',31,'.png'),('akabei',32,'.png'),('tlg',32,'.tlg'),('plaintext',0,'.png')]:
    header = bytes.fromhex('89504e470d0a1a0a0000000d49484452') if ext == '.png' else b'TLG6.0\0raw\x1a'
    startup = STARTUP
    if not period:
        startup = b'\xef\xbb\xbf' + ('// 日本語の設定。' * 100 + '\n').encode() + startup
    files = {'a'+ext: header+b'original synthetic image A'*10, 'b'+ext: header+b'original synthetic image B'*10,
             'startup.tjs':startup, 'condition.ks':SCENARIO}
    folder = root / name
    folder.mkdir(parents=True, exist_ok=True)
    (folder / 'data.bin').write_bytes(archive(files,period))
