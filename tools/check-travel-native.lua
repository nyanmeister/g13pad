-- SPDX-License-Identifier: GPL-3.0-or-later
-- Detached need-counter checks plus read-only current travel snapshot.
local script=({...})[1] or 'g13-lcd'
if script=='--version' then print('g13pad-travel-native-check 1'); return end
local collector=reqscript(script)
local keys={'session','adventure_unit','adventure_view','previous','alert','detail_state','detail_sequence','recovery'}
local saved={}
for _,key in ipairs(keys) do saved[key]=collector[key] end
local member=df.army_nemesisst:new()
local ok,err=pcall(function()
    member.flags.eats,member.flags.drinks,member.flags.sleeps=true,true,true
    member.hunger_timer,member.thirst_timer,member.sleepiness_timer=172799,172800,0
    assert(table.concat(collector.travel_needs(member),'/')=='0/1/0')
    member.flags.drinks=false
    assert(table.concat(collector.travel_needs(member),'/')=='0/2/0')
    member.sleepiness_timer=172800
    assert(table.concat(collector.travel_needs(member),'/')=='0/2/1')
    print('Detached native travel need flags and boundaries: PASS')
    assert(dfhack.world.isAdventureMode() and dfhack.isWorldLoaded() and not dfhack.isMapLoaded(),
        'Enter Adventure travel view for the live snapshot check')
    local s=collector.travel_snapshot()
    assert(s and s.mode=='travel')
    local n=df.nemesis_record.find(df.global.adventure.player_id)
    assert(s.unit==n.unit_id)
    print('LIVE',collector.line(s))
end)
member:delete()
for _,key in ipairs(keys) do collector[key]=saved[key] end
assert(ok,err)
print('Native travel checks passed; no world objects changed')
