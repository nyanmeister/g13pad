-- SPDX-License-Identifier: GPL-3.0-or-later
-- Deterministic collector checks; actual DFHack userdata is checked in-game too.
if arg[1] == '--version' then print('g13pad-dfhack-check 1'); return end
local script = assert(arg[1], 'Usage: lua tools/check-dfhack.lua contrib/df-health/g13-lcd.lua')
local items = {NONE=-1, BAR=0, CLOTH=1, WOOD=2}
for key, value in pairs({NONE=-1, BAR=0, CLOTH=1, WOOD=2}) do items[value] = key end
local jobs = {StrangeMoodCrafter=10}
jobs[10] = 'StrangeMoodCrafter'
local env = setmetatable({
    dfhack_flags={module=true},
    df={item_type=items, job_type=jobs},
    dfhack={onStateChange={}, matinfo={decode=function(kind, index)
        if kind == 0 and index == 5 then
            return {toString=function() return 'iron' end, inorganic={flags={WAFERS=false}}}
        end
    end}},
    require=function(module) assert(module == 'json'); return {} end,
}, {__index=_G})
assert(loadfile(script, 't', env))()
assert(next(env.dfhack.onStateChange) == nil) -- Stopped imports cannot replace a live collector's callback.
local function equal(a, b) assert(a == b, tostring(a) .. ' ~= ' .. tostring(b)) end
local function supplies()
    local c = {}
    for _, key in ipairs({'soap','thread','cloth','splints','crutches','powder','buckets'}) do
        c['count_'..key], c['desired_'..key] = 0, 0
    end
    return c
end
local c = supplies()
equal(#env.hospital_status(c, {}).shortages, 0)
c.desired_soap, c.desired_crutches, c.count_crutches = 750, 5, 3
local h = env.hospital_status(c, {})
equal(#h.shortages, 2)
equal(h.urgent, false)
assert(h.message:find('SOAP EMPTY', 1, true) and h.message:find('CRUTCHES 3 OF 5', 1, true))
h = env.hospital_status(c, {rq_cleaning=2})
equal(h.urgent, true)
equal(h.shortages[1].requests, 2)
c.count_soap = 750
equal(env.hospital_status(c, {rq_cleaning=2}).shortages[1].supply, 'buckets')
-- A zero configured target does not hide empty supplies needed for current care.
c = supplies()
equal(#env.hospital_status(c, {rq_crutch=1}).shortages, 1)
c.count_soap = -1
assert(not pcall(env.hospital_status, c, {}))

local function requirement(kind, quantity, flags, material)
    return {item_type=kind, quantity=quantity, mat_type=material and 0 or -1,
        mat_index=material and 5 or -1, flags2=flags or {}}
end
local job = {job_type=10, job_items={elements={
    requirement(items.CLOTH, 20000, {silk=true}),
    requirement(items.BAR, 300, {}, true),
    requirement(items.NONE, 1, {body_part=true, bone=true}),
}}, items={{job_item_idx=1},{job_item_idx=2},{job_item_idx=3}}}
local m = env.mood_requirements(job, true)
equal(m.waiting, true)
assert(m.message:find('1 SILK CLOTH', 1, true) and m.message:find('1 IRON BARS', 1, true))
assert(not m.message:find('BONES', 1, true))
job.items[#job.items+1] = {job_item_idx=1}
job.items[#job.items+1] = {job_item_idx=2}
equal(env.mood_requirements(job, true).message, 'MAKING ARTIFACT')
equal(env.mood_requirements(job, false).message, 'NEEDS CRAFTSDWARF WORKSHOP')
job.job_items.elements[1].quantity = 0
assert(not pcall(env.mood_requirements, job, true))

local details = {
    {id='health', priority=100, text='URIST_INFECTION'},
    {id='hospital', priority=50, text='HOSPITAL_SOAP_EMPTY'},
    {id='mood', priority=105, text='MOOD_NEEDS_SILK'},
}
equal(env.choose_detail(details, 0), 'MOOD_NEEDS_SILK')
equal(env.choose_detail(details, 7999), 'MOOD_NEEDS_SILK')
equal(env.choose_detail(details, 8000), 'URIST_INFECTION')
equal(env.choose_detail(details, 16000), 'HOSPITAL_SOAP_EMPTY')
equal(env.choose_detail(details, 24000), 'MOOD_NEEDS_SILK')
details[1].text = string.rep('LONG_MOOD_', 20) -- Sorted mood is first now.
equal(env.choose_detail(details, 24001), details[1].text)
equal(env.choose_detail(details, 32002), details[1].text) -- Full marquee, not 8s truncation.
env.detail_state = nil
equal(env.choose_detail({details[2]}, 0), details[2].text)
local sequence = env.detail_state.sequence
equal(env.choose_detail({details[2]}, 8000), details[2].text)
equal(env.detail_state.sequence, sequence + 1) -- A lone message restarts its phase too.
env.detail_state = nil
local scrolling = {{id='scrolling', priority=0, text=string.rep('X', 30)}}
env.choose_detail(scrolling, 0)
equal(env.detail_state.until_ms, 3200) -- 1s start + 1s travel + 1s end + reader allowance.
local scrolling_sequence = env.detail_state.sequence
env.choose_detail(scrolling, 3199)
equal(env.detail_state.sequence, scrolling_sequence)
env.choose_detail(scrolling, 3200)
equal(env.detail_state.sequence, scrolling_sequence + 1)
env.dfhack.getTickCount = function() return 8000 end
env.previous, env.alert = nil, 0
local s = {pop=10, care=1, inf=1, walk=0, hand=0, limbs=0, rags=0, worn=0,
    season='winter',
    weather='clear', temp=10015, freeze='none', severity=2, urgent={[1]=100},
    details={{id='health', priority=100, text='INFECTION'}}}
assert(env.line(s):find('alert 0', 1, true)) -- Existing problems establish baseline.
s = setmetatable({urgent={[1]=100,['hospital:0']=110}}, {__index=s})
assert(env.line(s):find('alert 1', 1, true))
assert(env.line(s):find('alert 1', 1, true)) -- Persistence cannot repeatedly flash.
s = setmetatable({urgent={[1]=70}}, {__index=s})
assert(env.line(s):find('alert 1', 1, true)) -- Improvement is not a new event.
s = setmetatable({urgent={[1]=100,['mood:2']=105}}, {__index=s})
assert(env.line(s):find('alert 2', 1, true))
assert(env.line(s):find('phase %d+'))
assert(env.line(s):find('season winter', 1, true))
print('DFHack hospital, mood requirements, and detail rotation checks passed')
