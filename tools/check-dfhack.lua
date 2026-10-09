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

local function unit(id)
    return {id=id, flags1={inactive=false}, flags2={killed=false}, job={},
        syndromes={active={}},
        counters={winded=0,stunned=0,unconscious=0,suffocation=0,pain=0,nausea=0,dizziness=0},
        counters2={exhaustion=0,paralysis=0,fever=0,hunger_timer=0,thirst_timer=0,sleepiness_timer=0},
        status2={limbs_stand_count=2,limbs_stand_max=2,limbs_grasp_count=2,limbs_grasp_max=2},
        body={blood_count=1000,blood_max=1000,infection_level=0,wounds={},
            components={body_part_status={}},
            body_plan={body_parts={{name_singular={[0]={value='left hand'}}}}}}}
end
local u=unit(1)
equal(env.adventure_condition(u,false).condition,'READY')
for _,case in ipairs({{1999,0,'RESTED'},{2000,1,'TIRED'},{3999,1,'TIRED'},
        {4000,2,'VERY_TIRED'},{6000,2,'EXHAUSTED'}}) do
    u.counters2.exhaustion=case[1]
    local v=env.adventure_condition(u,false)
    equal(v.severity,case[2]); equal(v.effort,case[3])
end
u=unit(1); u.counters.unconscious=-1
equal(env.adventure_condition(u,false).condition,'READY')
equal(env.adventure_condition(u,false).severity,0)
equal(env.adventure_condition(u,true).condition,'SLEEPING')
equal(u.counters.unconscious,-1)
for _,bad in ipairs({-2,0.5,'invalid',math.huge,0/0,1000000001}) do
    u.counters.unconscious=bad
    assert(not pcall(env.adventure_condition,u,false))
end
u.counters.unconscious=10
equal(env.adventure_condition(u,false).severity,3)
equal(env.adventure_condition(u,true).condition,'SLEEPING')
u.counters.suffocation=1
equal(env.adventure_condition(u,true).condition,'SUFFOCATING')
u=unit(1); u.body.blood_count=200
equal(env.adventure_condition(u,false).blood,20)
equal(env.adventure_condition(u,false).severity,1) -- Blood % is not an HP band.
u.body.blood_count=-26
equal(env.adventure_condition(u,false).blood,0)
equal(env.adventure_condition(u,false).condition,'BLOOD_LOSS')
equal(u.body.blood_count,-26) -- Observation must never change the game's counter.
u.flags2.killed=true
equal(env.adventure_condition(u,false).condition,'DEAD')
equal(env.adventure_condition(u,false).severity,4)
u.flags2.killed=false
for _,bad in ipairs({-1000000001,1000000001,0.5,'invalid',math.huge,0/0}) do
    u.body.blood_count=bad
    assert(not pcall(env.adventure_condition,u,false))
end
u.body.blood_count,u.body.blood_max=0,0
equal(env.adventure_condition(u,false).blood,nil)
u.status2.limbs_stand_count=1
equal(env.adventure_condition(u,false).condition,'WALK_IMPAIRED')
u=unit(1); u.counters.pain=-1
assert(not pcall(env.adventure_condition,u,false))
u=unit(1); u.body.wounds={{flags={infection=true},parts={{body_part_id=1,bleeding=2,impaired=1}}}}
local v=env.adventure_condition(u,false)
equal(v.condition,'INFECTION'); equal(v.wounds,1)
equal(v.details[1],'left hand - BLEEDING')
u=unit(1); u.body.components.body_part_status={{on_fire=true}}
equal(env.adventure_condition(u,false).condition,'BURNING')
u.flags2.killed=true
equal(env.adventure_condition(u,false).condition,'DEAD')

env.df.caste_raw_flags={HAS_BLOOD=69}
env.dfhack.units={casteFlagSet=function() return true end,
    getReadableName=function(v) return 'UNIT_' .. v.id end}
env.dfhack.df2utf=function(v) return v end
local members={[1]=unit(1),[2]=unit(2),[3]=unit(3),[4]=unit(4)}
members[2].counters.stunned=1
members[4].flags1.inactive=true
local party={party_core_members={1,2},party_extra_members={2,5},party_pets={3,4},party_extra_pets={3}}
local allies,pets,party_details,urgent=env.adventure_party(party,1,function(id) return members[id] end)
equal(table.concat(allies,'/'),'1/2/1'); equal(table.concat(pets,'/'),'0/2/1')
equal(#party_details,1); equal(urgent['2:STUNNED'],2)
allies=env.adventure_party(party,2,function(id) return members[id] end)
equal(table.concat(allies,'/'),'0/2/1') -- Controlled party switch excludes the new subject.

-- Exercise snapshot recovery and mode/control changes without a real world.
local now, player = 0, members[1]
env.dfhack.getTickCount=function() return now end
env.dfhack.world={getAdventurer=function() return player end}
env.df.global={adventure={sleeping=0,interactions=party}}
env.df.historical_figure={find=function(id) return {unit_id=id} end}
env.df.unit={find=function(id) return members[id] end}
env.previous,env.adventure_unit,env.recovery,env.detail_state=nil,nil,nil,nil
local snap=env.adventure_snapshot()
equal(snap.condition,'READY'); equal(snap.severity,0)
assert(env.line(snap):find('adv 1',1,true))
player.counters.stunned=1; now=1000
snap=env.adventure_snapshot(); equal(snap.severity,2)
assert(env.line(snap):find('alert 3',1,true)) -- Earlier fortress fixtures left alert at2.
player.counters.stunned=0; now=2000
snap=env.adventure_snapshot(); equal(snap.severity,2); equal(snap.condition,'STUNNED')
now=3999; equal(env.adventure_snapshot().severity,2)
now=4000; equal(env.adventure_snapshot().severity,0)
player=members[2]; snap=env.adventure_snapshot()
equal(snap.unit,2); equal(env.previous,nil)
print('DFHack adventure fatigue, blood, sleep, wounds, party and recovery checks passed')
-- New limb loss briefly warns orange, then established impairment settles yellow.
player=members[1]; env.adventure_unit=nil; env.previous=nil; env.recovery=nil
now=5000; env.line(env.adventure_snapshot())
player.status2.limbs_grasp_count=1; now=6000
snap=env.adventure_snapshot(); equal(snap.severity,2); equal(snap.condition,'HAND_IMPAIRED')
env.line(snap)
now=7000; equal(env.adventure_snapshot().severity,2)
now=9000; equal(env.adventure_snapshot().severity,1)
player=unit(1); player.body.blood_count=0
local bloodless=env.adventure_condition(player,false,true)
equal(bloodless.blood,nil); equal(bloodless.severity,0)
print('New impairment recovery and bloodless physiology checks passed')
members[4].flags2.killed=true
members[4].body.blood_count=-26
allies,pets,party_details,urgent=env.adventure_party(party,1,function(id) return members[id] end)
equal(table.concat(pets,'/'),'1/2/0') -- Known death is not an unknown off-map pet.
equal(urgent['4:DEAD'],4)
player=members[1]
snap=env.adventure_snapshot()
equal(table.concat(snap.pets,'/'),'1/2/0')
assert(env.line(snap):find('adv 1',1,true)) -- A depleted pet must not blank the player feed.
player=nil; members[1].flags2.killed=true; env.adventure_unit=1
snap=env.adventure_snapshot(); equal(snap.condition,'DEAD'); equal(snap.severity,4)
members[1].flags2.killed=false
equal(env.adventure_snapshot(),nil) -- No stale living subject after unloading/control loss.
print('Known party death and controlled-character death fallback checks passed')

-- Travel has no loaded units: identify the subject through its nemesis and army.
local m={nemesis_id=7,hunger_timer=0,thirst_timer=172800,sleepiness_timer=172799,
    flags={eats=true,drinks=true,sleeps=true,is_sleeping=false,on_watch=false}}
equal(table.concat(env.travel_needs(m),'/'),'0/1/0')
m.flags.drinks=false
equal(table.concat(env.travel_needs(m),'/'),'0/2/0')
m.sleepiness_timer=172800
equal(table.concat(env.travel_needs(m),'/'),'0/2/1')
m.hunger_timer=-1; assert(not pcall(env.travel_needs,m)); m.hunger_timer=0
local records={[7]={id=7,unit_id=1,figure={id=101,name='ELANA'}},
    [8]={id=8,unit_id=2,figure={id=102,name='ALLY'}},
    [9]={id=9,unit_id=3,figure={id=103,name='PET'}}}
local other={nemesis_id=8,hunger_timer=0,thirst_timer=0,sleepiness_timer=0,
    flags={eats=false,drinks=false,sleeps=false,is_sleeping=false,on_watch=false}}
local army={pos={x=280,y=231,z=0},members={m,other,{nemesis_id=9}},flags={}}
env.df.army_flags={player=1,working=2,composing=3,sneaking=4,waiting=5,sleeping=6}
army.flags[1]=true
env.df.nemesis_record={find=function(id) return records[id] end}
env.df.army={find=function(id) return id==10 and army or nil end}
env.dfhack.translation={translateName=function(v) return v end}
env.df.global.adventure={player_id=7,player_army_id=10,sleeping=0,interactions={
    party_core_members={101,102},party_extra_members={102,104},party_pets={103},party_extra_pets={103}}}
local map_loaded,world_loaded=false,true
env.dfhack.isWorldLoaded=function() return world_loaded end
env.dfhack.isMapLoaded=function() return map_loaded end
env.dfhack.world.isAdventureMode=function() return true end
env.previous=nil
snap=env.snapshot()
equal(snap.mode,'travel'); equal(snap.unit,1); equal(snap.name,'ELANA')
equal(snap.allies,1); equal(snap.pets,1); equal(snap.activity,'WALKING')
equal(table.concat(snap.needs,'/'),'0/2/1')
local travel_line=env.line(snap)
assert(travel_line:find('travel 1',1,true) and not travel_line:find('blood',1,true))
local travel_session=env.session
army.flags[4]=true; equal(env.snapshot().activity,'SNEAKING')
m.flags.on_watch=true; equal(env.snapshot().activity,'ON_WATCH')
m.flags.is_sleeping=true; equal(env.snapshot().activity,'SLEEPING')
env.df.global.adventure.player_id=8
snap=env.snapshot() -- No stale controlled character when switching during travel.
equal(snap.unit,2); equal(snap.name,'ALLY'); assert(env.session>travel_session)
equal(table.concat(snap.needs,'/'),'2/2/2')
env.df.global.adventure.player_id=999; equal(env.snapshot(),nil)
env.df.global.adventure.player_id=7
army.flags[1]=false; equal(env.snapshot(),nil); army.flags[1]=true
local saved_member=m.nemesis_id; m.nemesis_id=999
equal(env.snapshot(),nil); m.nemesis_id=saved_member
env.line(env.snapshot()); travel_session=env.session
local travel_alert=env.alert
map_loaded=true; player=unit(1)
player.counters.unconscious=-1 -- Actual post-sleep return must retain a valid feed.
env.df.historical_figure.find=function(id)
    for _,r in pairs(records) do if r.figure.id==id then return {unit_id=r.unit_id} end end
end
snap=env.snapshot(); equal(snap.mode,'adventure'); equal(snap.condition,'READY')
assert(env.session>travel_session)
env.line(snap); equal(env.alert,travel_alert) -- Returning to local view establishes a baseline.
world_loaded=false; equal(env.snapshot(),nil)
print('Travel needs, physiological flags, membership, identity and map-transition checks passed')
