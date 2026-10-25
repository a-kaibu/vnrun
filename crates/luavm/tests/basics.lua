local function eq(a, b, what) if a ~= b then error(what .. ": got " .. tostring(a) .. ", want " .. tostring(b)) end end
eq(1 + 2, 3, "add")
eq(7 % 3, 1, "mod")
eq(-7 % 3, 2, "neg mod")
eq(2 ^ 10, 1024, "pow")
eq(#"abc", 3, "len")
eq("a" .. "b" .. 1, "ab1", "concat")
local t = {1, 2, 3, x = 4}
eq(#t, 3, "#t")
local s = 0
for i, v in ipairs(t) do s = s + v * i end
eq(s, 14, "ipairs")
local n = 0
for k, v in pairs(t) do n = n + 1 end
eq(n, 4, "pairs")
for i = 10, 1, -3 do s = s + i end
eq(s, 14 + 10 + 7 + 4 + 1, "numeric for")
local function counter()
  local c = 0
  return function() c = c + 1; return c end
end
local c1 = counter(); c1(); eq(c1(), 2, "closure")
local fs = {}
for i = 1, 3 do fs[i] = function() return i end end
eq(fs[1]() + fs[3](), 4, "loop upvalues")
local function va(...) return select("#", ...), ... end
local a, b, c = va(5, nil, 7)
eq(a, 3, "select #"); eq(c, nil, "vararg 2")
eq(select(2, "x", "y", "z"), "y", "select")
eq(string.format("%d %s %5.2f %x %02d", 3, "hi", 1.5, 255, 7), "3 hi  1.50 ff 07", "format")
eq(("abc"):upper(), "ABC", "string method")
eq(string.find("hello world", "o w"), 5, "find plain")
eq(string.match("key=val", "(%w+)=(%w+)"), "key", "match")
eq(select(2, string.gsub("a b c", "%s", "_")), 2, "gsub count")
eq((string.gsub("abc", "%w", "%0%0")), "aabbcc", "gsub %0")
local words = {}
for w in string.gmatch("one two three", "%a+") do words[#words + 1] = w end
eq(table.concat(words, ","), "one,two,three", "gmatch")
local sorted = {5, 2, 8, 1}
table.sort(sorted); eq(table.concat(sorted, " "), "1 2 5 8", "sort")
table.sort(sorted, function(x, y) return x > y end); eq(sorted[1], 8, "sort cmp")
table.insert(sorted, 1, 0); eq(sorted[1], 0, "insert"); eq(table.remove(sorted), 1, "remove")
local Class = {}
Class.__index = Class
function Class.new(v) return setmetatable({v = v}, Class) end
function Class:get() return self.v end
eq(Class.new(9):get(), 9, "method")
local V = setmetatable({}, {__add = function(x, y) return 42 end, __call = function(self, x) return x * 2 end})
eq(V + 1, 42, "__add"); eq(V(4), 8, "__call")
local ok, e = pcall(function() error("boom") end)
eq(ok, false, "pcall"); assert(tostring(e):find("boom"), "pcall message")
local ok2, e2 = pcall(function() local x = nil; return x.y end)
eq(ok2, false, "index nil")
eq(tostring(nil), "nil", "tostring nil")
eq(tonumber("0x10"), 16, "tonumber hex"); eq(tonumber("  12  "), 12, "tonumber spaces")
eq(tonumber("z", 36), 35, "tonumber base")
eq(math.floor(3.7), 3, "floor"); eq(math.max(1, 5, 3), 5, "max")
local x = 0
while true do x = x + 1; if x > 5 then break end end
eq(x, 6, "while break")
repeat x = x - 1 until x == 0
eq(x, 0, "repeat")
local deep = {a = {b = {c = "d"}}}
eq(deep.a.b.c, "d", "nested")
local long = {}
for i = 1, 120 do long[i] = i end
local unpack = table.unpack or unpack
local big = {unpack(long)}
eq(#big, 120, "setlist")
local tc = {1, 2, 3, (function() return 4, 5, 6 end)()}
eq(#tc, 6, "multret in constructor")
local function tail(n) if n == 0 then return "done" end return tail(n - 1) end
eq(tail(100), "done", "tail call")
eq(3 == 3.0, true, "int float eq")
eq(("%s"):format(true), "true", "format bool")
eq(not nil, true, "not")
eq(1 < 2 and "y" or "n", "y", "and/or")
