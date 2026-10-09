-- SPDX-License-Identifier: GPL-3.0-or-later
-- Detached native fixtures; never change or link anything into the live party.
local script=({...})[1] or 'g13-lcd'
if script=='--version' then print('g13pad-adventure-native-check 1'); return end
assert(dfhack.world.isAdventureMode() and dfhack.isMapLoaded(),'Load an adventure first')
local collector=reqscript(script)
local player=assert(dfhack.world.getAdventurer())
local unit,wound,part=df.unit:new(),df.unit_wound:new(),df.unit_wound_layerst:new()
local ok,err=pcall(function()
    unit.id=123456
    unit.body.blood_max,unit.body.blood_count=1000,1000
    unit.status2.limbs_stand_max,unit.status2.limbs_stand_count=2,2
    unit.status2.limbs_grasp_max,unit.status2.limbs_grasp_count=2,2
    assert(collector.adventure_condition(unit,false).condition=='READY')
    unit.body.blood_count=-26
    unit.flags2.killed=true
    local depleted=collector.adventure_condition(unit,false)
    assert(depleted.condition=='DEAD' and depleted.severity==4 and depleted.blood==0)
    assert(unit.body.blood_count==-26)
    unit.body.blood_count=1000
    unit.flags2.killed=false
    for _,case in ipairs({{2000,1,'TIRED'},{4000,2,'VERY_TIRED'},{6000,2,'EXHAUSTED'}}) do
        unit.counters2.exhaustion=case[1]
        local c=collector.adventure_condition(unit,false)
        assert(c.severity==case[2] and c.effort==case[3])
    end
    unit.counters2.exhaustion=0
    unit.counters.unconscious=-1
    assert(collector.adventure_condition(unit,false).condition=='READY')
    assert(collector.adventure_condition(unit,true).condition=='SLEEPING')
    assert(unit.counters.unconscious==-1)
    unit.counters.unconscious=1
    assert(collector.adventure_condition(unit,false).condition=='UNCONSCIOUS')
    assert(collector.adventure_condition(unit,true).condition=='SLEEPING')
    unit.counters.suffocation=1
    assert(collector.adventure_condition(unit,true).condition=='SUFFOCATING')
    unit.counters.unconscious,unit.counters.suffocation=0,0
    unit.body.body_plan=player.body.body_plan -- Read-only raw; clear reference before delete.
    part.body_part_id,part.bleeding,part.impaired=0,10,1
    wound.parts:insert('#',part); unit.body.wounds:insert('#',wound)
    local c=collector.adventure_condition(unit,false)
    assert(c.condition=='BLEEDING' and c.details[1]:find('BLEEDING',1,true))
    wound.flags.infection=true
    assert(collector.adventure_condition(unit,false).condition=='INFECTION')
    print('Detached native fatigue, sleep/KO, suffocation and wound-name fixtures: PASS')
    local s=collector.snapshot()
    assert(s and s.mode=='adventure' and s.unit==player.id)
    local line=collector.line(s)
    assert(line:find('adv 1',1,true) and not line:find('season',1,true))
    print('LIVE',line)
    print(('Read-only adventure party: %d allies (%d unavailable), %d pets (%d unavailable)')
        :format(s.allies[2],s.allies[3],s.pets[2],s.pets[3]))
end)
unit.body.body_plan=nil
unit.body.wounds:resize(0); wound.parts:resize(0)
part:delete(); wound:delete(); unit:delete()
assert(ok,err)
print('Native adventure checks passed; no world objects changed')
