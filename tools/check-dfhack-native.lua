-- SPDX-License-Identifier: GPL-3.0-or-later
-- Run with DFHack: lua --file /path/to/tools/check-dfhack-native.lua g13-lcd
local script = ({...})[1] or 'g13-lcd'
if script == '--version' then print('g13pad-dfhack-native-check 1'); return end
assert(dfhack.isMapLoaded() and dfhack.world.isFortressMode(), 'Load a fortress first')
local collector = reqscript(script)
local job, items, refs = df.job:new(), {}, {}
local function item(kind, quantity, flags, mat_type, mat_index)
    local v = df.job_item:new()
    items[#items+1] = v
    v.item_type, v.quantity = kind, quantity
    v.mat_type, v.mat_index = mat_type or -1, mat_index or -1
    for key, value in pairs(flags or {}) do v.flags2[key] = value end
    job.job_items.elements:insert('#', v)
end
local function collected(index)
    local v = df.job_item_ref:new()
    refs[#refs+1] = v
    v.job_item_idx = index
    job.items:insert('#', v)
end
local ok, err = pcall(function()
    local iron
    for index, raw in ipairs(df.global.world.raws.inorganics.all) do
        if raw.id == 'IRON' then iron = index; break end
    end
    assert(iron, 'IRON raw unavailable')
    job.job_type = df.job_type.StrangeMoodWeaver
    item(df.item_type.CLOTH, 20000, {silk=true})
    item(df.item_type.BAR, 300, {}, 0, iron)
    item(df.item_type.NONE, 2, {body_part=true, bone=true})
    collected(0); collected(1); collected(2)
    local m = collector.mood_requirements(job, true)
    assert(m.message == 'NEEDS 1 SILK CLOTH - 1 IRON BARS - 1 BONES', m.message)
    print('Native mood materials, BAR/CLOTH units, and zero-based collection indices: PASS')
    collected(0); collected(1); collected(2)
    assert(collector.mood_requirements(job, true).message == 'MAKING ARTIFACT')
    assert(collector.mood_requirements(job, false).message == 'NEEDS CLOTHIER SHOP')
    print('Collected materials and unclaimed workshop: PASS')
    local s = collector.snapshot()
    assert(s and s.pop >= s.limbs and s.limbs >= math.max(s.walk, s.hand)
        and s.limbs <= s.walk + s.hand)
    assert(#s.details <= 8)
    assert(s.season == ({[0]='spring', [1]='summer', [2]='autumn', [3]='winter'})[df.global.cur_season])
    print('Native calendar season: ' .. s.season)
    print(('Read-only live snapshot: %d people, %d hospitals, %d moods, %d details')
        :format(s.pop, #s.hospitals, #s.moods, #s.details))
    for _, hospital in ipairs(s.hospitals) do
        print('Hospital ' .. hospital.name .. ': ' .. hospital.message)
    end
end)
-- These allocations were never linked into world.jobs or a unit. Release the
-- pointers explicitly after clearing the vectors; do not touch existing objects.
job.items:resize(0)
job.job_items.elements:resize(0)
for _, v in ipairs(refs) do v:delete() end
for _, v in ipairs(items) do v:delete() end
job:delete()
assert(ok, err)
print('DFHack native read-only checks passed; no world data changed')
