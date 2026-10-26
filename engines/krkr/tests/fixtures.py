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
Scripts.execStorage(global.KirikiriPolyfillRoot + "plugins/scriptsEx.tjs");
var keys = Scripts.getObjectKeys(%[z:1,a:2,syslay_logo2:3,syslay:4]);
if(keys.join(",") != "a,syslay,syslay_logo2,z") throw new Exception("scriptsEx key order");
var eq1=%[alpha:[1,2],beta:3], eq2=%[beta:3,alpha:[1,2]];
if(!Scripts.equalStruct(eq1,eq2)) throw new Exception("dictionary comparison order");
eq2.alpha[1]=4;
if(Scripts.equalStruct(eq1,eq2)) throw new Exception("nested comparison");
for(var n=0;n<3;n++) {
 global.testCalls=0;global.testExpected=[false,0,null][n];
 var result=Scripts.foreach([1,2,3],function(k,v) {global.testCalls++;return global.testExpected;});
 if(global.testCalls!=1 || result!==global.testExpected) throw new Exception("foreach false result");
}
var original=%[nested:[%[value:7]]];
var copied=Scripts.clone(original);
copied.nested[0].value=9;
if(original.nested[0].value!=7) throw new Exception("recursive clone");
class Cloneable { function clone() {return %[copied:true];} }
class Opaque {}
var opaque=new Opaque();
if(!Scripts.clone(new Cloneable()).copied || Scripts.clone(opaque)!==opaque)
 throw new Exception("class clone");
// A cached PIMG source keeps its own visibility until initOptions requests defaults.
class AffineSourceBMPBase {
 var resets=0;
 function initOptions() {resets++;}
}
class AffineSourcePSD extends AffineSourceBMPBase {
 var _psdlayer;
 var invalidations=0;
 function AffineSourcePSD() {
  _psdlayer=%[visibleInfo:%[background:0,logo:1],layvisible:%[logo:1]];
 }
 function calcUpdate() {invalidations++;}
}
Scripts.execStorage(global.KirikiriPolyfillRoot + "plugins/psbFile.tjs");
var cached=new AffineSourcePSD(), fresh=new AffineSourcePSD();
fresh.initOptions();
if(Scripts.getObjectCount(fresh._psdlayer.visibleInfo)!=0 ||
 Scripts.getObjectCount(fresh._psdlayer.layvisible)!=0 || fresh.resets!=1 || fresh.invalidations!=1 ||
 cached._psdlayer.visibleInfo.background!==0)
 throw new Exception("PIMG visibility reset");
var bitmap=new AffineSourceBMPBase();
bitmap.initOptions();
if(bitmap.resets!=1) throw new Exception("bitmap initializer");
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

# A hidden helper window created after the visible game must not steal pointer input.
POINTER = b'''
global.inputWindow = new Window();
inputWindow.setInnerSize(640,480);
global.inputLayer = new Layer(inputWindow,null);
inputLayer.setSize(640,480);
inputLayer.fillRect(0,0,640,480,0xff306090);
inputLayer.visible = true;
inputWindow.onMouseDown = function(x,y,button,shift) {
 if(button != mbLeft || x != 80 || y != 80)
  throw new Exception("bad pointer coordinates");
 Debug.message("VNRUN_KRKR_POINTER_OK");
} incontextof inputWindow;
inputWindow.visible = true;
global.hiddenHelper = new Window();
hiddenHelper.visible = false;
Debug.message("VNRUN_KRKR_POINTER_READY");
'''
folder = root / 'pointer'
folder.mkdir(parents=True, exist_ok=True)
(folder / 'data.bin').write_bytes(archive({'startup.tjs':POINTER},0))
