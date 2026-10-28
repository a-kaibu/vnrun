-- values in and out
local co = coroutine.create(function(a, b)
  assert(coroutine.status(coroutine.running()) == "running")
  local c = coroutine.yield(a + b)
  local d, e = coroutine.yield(c * 2)
  return d + e, "done"
end)
assert(coroutine.status(co) == "suspended")
local ok, x = coroutine.resume(co, 1, 2)
assert(ok and x == 3)
ok, x = coroutine.resume(co, 10)
assert(ok and x == 20)
local ok2, s, t = coroutine.resume(co, 3, 4)
assert(ok2 and s == 7 and t == "done")
assert(coroutine.status(co) == "dead")
assert(not coroutine.resume(co))

-- yield from deep Lua calls, and through a tail call
local function wait(n) return coroutine.yield("wait", n) end
local function deep(n)
  if n == 0 then return wait(5) end
  local r = deep(n - 1)
  return r + 1
end
co = coroutine.create(function() return deep(10) end)
local _, w, n = coroutine.resume(co)
assert(w == "wait" and n == 5)
local _, r = coroutine.resume(co, 100)
assert(r == 110)

-- wrap as a generator
local function range(n)
  return coroutine.wrap(function()
    for i = 1, n do coroutine.yield(i) end
  end)
end
local sum = 0
for i in range(10) do sum = sum + i end
assert(sum == 55)

-- upvalues shared between threads while the coroutine is suspended
local get, set
co = coroutine.create(function()
  local v = 1
  get = function() return v end
  set = function(x) v = x end
  coroutine.yield()
  return v
end)
coroutine.resume(co)
assert(get() == 1)
set(42)
assert(get() == 42)
local _, final = coroutine.resume(co)
assert(final == 42)

-- nested coroutines and status "normal"
local outer
outer = coroutine.create(function()
  local inner = coroutine.create(function()
    assert(coroutine.status(outer) == "normal")
    coroutine.yield(1)
    return 2
  end)
  local _, a = coroutine.resume(inner)
  coroutine.yield(a)
  local _, b = coroutine.resume(inner)
  return b
end)
local _, a = coroutine.resume(outer)
assert(a == 1)
local _, b = coroutine.resume(outer)
assert(b == 2)

-- errors end the coroutine
co = coroutine.create(function() coroutine.yield(); error("boom") end)
coroutine.resume(co)
local okE, msg = coroutine.resume(co)
assert(not okE and string.find(msg, "boom"))
assert(coroutine.status(co) == "dead")

-- yield outside a coroutine
assert(not pcall(coroutine.yield))

-- a coroutine table of label functions, like the DebonosuWorks scripts
local labels = {}
labels.top = function() coroutine.yield("j", nil, "next") end
labels.next = function() coroutine.yield("e", "other", "top") end
local trail = {}
local label = "top"
for _ = 1, 2 do
  local c = coroutine.create(labels[label])
  local _, kind, _, l = coroutine.resume(c)
  trail[#trail + 1] = kind
  label = l
end
assert(trail[1] == "j" and trail[2] == "e")
