-- SPDX-License-Identifier: GPL-3.0-or-later
-- Read-only fortress and adventure overview for the G13 LCD. Requires DFHack.
--@module = true
local json = require('json')
local VERSION = '0.2.60'
local CARE_FLAGS = {'needs_healthcare', 'rq_diagnosis', 'rq_immobilize', 'rq_dressing',
    'rq_cleaning', 'rq_surgery', 'rq_suture', 'rq_setting', 'rq_traction', 'rq_crutch'}
enabled = enabled or false
session = session or os.time() * 1000
alert = alert or 0
detail_sequence = detail_sequence or 0

local function label(s)
    -- The LCD font is ASCII. Collapse unsupported bytes instead of emitting bad glyphs.
    s = tostring(s):upper():gsub('[^A-Z0-9_.%-]+', '_'):gsub('^_+', ''):gsub('_+$', '')
    return s:sub(1, 256) ~= '' and s:sub(1, 256) or 'UNKNOWN'
end

local SUPPLIES = {
    {key='soap', word='SOAP', request='rq_cleaning'},
    {key='thread', word='THREAD', request='rq_suture'},
    {key='cloth', word='CLOTH', request='rq_dressing'},
    {key='splints', word='SPLINTS', request='rq_immobilize', pieces=true},
    {key='crutches', word='CRUTCHES', request='rq_crutch', pieces=true},
    {key='powder', word='PLASTER', request='rq_setting'},
    {key='buckets', word='BUCKETS', request='rq_cleaning', pieces=true},
}

-- Use the hospital's own inventory and configured targets. Fortress-wide stocks
-- are not evidence that a hospital has the supplies, nor that a job can reach them.
function hospital_status(contents, demand)
    local shortages, messages, urgent = {}, {}, false
    for order, spec in ipairs(SUPPLIES) do
        local current, target = contents['count_' .. spec.key], contents['desired_' .. spec.key]
        assert(type(current) == 'number' and type(target) == 'number' and current >= 0 and target >= 0
            and current <= 1000000000 and target <= 1000000000,
            'Invalid hospital supply counters: ' .. spec.key)
        local patients = demand[spec.request] or 0
        if current < target or (current == 0 and patients > 0) then
            local empty = current == 0
            urgent = urgent or (empty and patients > 0)
            local message = empty and (spec.word .. ' EMPTY') or
                (spec.pieces and ('%s %d OF %d'):format(spec.word, current, target) or (spec.word .. ' LOW'))
            if patients > 0 then message = message .. (' - %d REQUESTS'):format(patients) end
            shortages[#shortages+1] = {supply=spec.key, current=current, target=target,
                requests=patients, empty=empty, message=message, order=order}
        end
    end
    table.sort(shortages, function(a, b)
        local ap = (a.empty and 2 or 0) + (a.requests > 0 and 1 or 0)
        local bp = (b.empty and 2 or 0) + (b.requests > 0 and 1 or 0)
        return ap > bp or (ap == bp and a.order < b.order)
    end)
    for _, shortage in ipairs(shortages) do messages[#messages+1] = shortage.message end
    return {shortages=shortages, message=table.concat(messages, ' - '), urgent=urgent}
end

local WORKSHOPS = {
    StrangeMoodCrafter='CRAFTSDWARF WORKSHOP', StrangeMoodJeweller='JEWELER WORKSHOP',
    StrangeMoodForge='METALSMITH FORGE', StrangeMoodMagmaForge='MAGMA FORGE',
    StrangeMoodCarpenter='CARPENTER WORKSHOP', StrangeMoodMason='STONEWORKER WORKSHOP',
    StrangeMoodBowyer='BOWYER WORKSHOP', StrangeMoodTanner='LEATHER WORKS',
    StrangeMoodWeaver='CLOTHIER SHOP', StrangeMoodGlassmaker='GLASS FURNACE',
    StrangeMoodMechanics='MECHANIC WORKSHOP',
}
function mood_material(item)
    local material = dfhack.matinfo.decode(item.mat_type, item.mat_index)
    local prefix = material and material:toString() or nil
    local kind = df.item_type[item.item_type]
    if kind == 'CLOTH' and not prefix then
        prefix = item.flags2.silk and 'SILK' or item.flags2.plant and 'PLANT FIBER' or
            item.flags2.yarn and 'YARN' or nil
    end
    if kind == 'NONE' then
        for _, part in ipairs({'bone', 'shell', 'horn', 'pearl', 'ivory_tooth'}) do
            if item.flags2[part] then
                kind = ({bone='BONES', shell='SHELLS', horn='HORNS', pearl='PEARLS', ivory_tooth='IVORY TEETH'})[part]
                break
            end
        end
    else
        kind = ({BOULDER='BOULDER', BLOCKS='BLOCKS', WOOD='LOGS', BAR='METAL BARS',
            SMALLGEM='CUT GEMS', ROUGH='ROUGH GEMS', SKIN_TANNED='LEATHER', CLOTH='CLOTH',
            REMAINS='REMAINS', CORPSE='CORPSE'})[kind] or kind
    end
    if material and item.item_type == df.item_type.BAR then
        kind = material.inorganic and material.inorganic.flags.WAFERS and 'WAFERS' or 'BARS'
    end
    return label((prefix and prefix .. ' ' or '') .. (kind or 'UNKNOWN MATERIAL')):gsub('_', ' ')
end

-- Same job-item indices and BAR/CLOTH quantity units as DFHack's showmood.
-- Report what has not been collected, without pretending stocks prove access.
function mood_requirements(job, has_workshop)
    assert(#job.job_items.elements <= 64 and #job.items <= 1024, 'Unbounded strange mood job')
    local requirements, missing = {}, {}
    for index, item in ipairs(job.job_items.elements) do
        local divisor = item.item_type == df.item_type.BAR and 150 or
            item.item_type == df.item_type.CLOTH and 10000 or 1
        assert(item.quantity > 0 and item.quantity <= 1000000000, 'Invalid mood quantity')
        local needed = item.quantity < divisor and item.quantity or math.ceil(item.quantity / divisor)
        local collected = 0
        for _, ref in ipairs(job.items) do
            if ref.job_item_idx == index then collected = collected + 1 end
        end
        local material = mood_material(item)
        requirements[#requirements+1] = {material=material, needed=needed, collected=collected}
        if collected < needed then
            missing[#missing+1] = ('%d %s'):format(needed-collected, material)
        end
    end
    local workshop = WORKSHOPS[df.job_type[job.job_type]]
    local message
    if not has_workshop and workshop then message = 'NEEDS ' .. workshop
    elseif #missing > 0 then message = 'NEEDS ' .. table.concat(missing, ' - ')
    elseif not has_workshop then message = 'SEEKING WORKSHOP'
    else message = 'MAKING ARTIFACT' end
    return {requirements=requirements, message=message,
        waiting=not has_workshop or #missing > 0, workshop=workshop}
end

local function add_detail(details, id, priority, message)
    details[#details+1] = {id=id, priority=priority, text=label(message)}
end
local function sort_details(details)
    table.sort(details, function(a,b)
        return a.priority > b.priority or (a.priority == b.priority and a.id < b.id)
    end)
end

-- Keep a message long enough to read its complete marquee, rather than resetting
-- it every snapshot. Rotate detail messages only; overview counts stay fixed.
function choose_detail(details, now)
    sort_details(details)
    local index
    if detail_state then
        for i, detail in ipairs(details) do if detail.id == detail_state.id then index = i; break end end
    end
    if not index then index = 1; detail_state = nil end
    local selected = details[index]
    if detail_state and selected.text == detail_state.text and now >= detail_state.until_ms then
        index = index % #details + 1
        selected, detail_state = details[index], nil
    end
    if not detail_state or selected.text ~= detail_state.text or now < detail_state.started then
        detail_sequence = detail_sequence + 1
        local travel = math.max(0, #selected.text * 6 - 160) * 50
        -- One second at each end, plus a small reader polling allowance. Static
        -- messages retain eight seconds to read; scrolling messages need no
        -- extra minimum that would extend their final pause.
        local hold = travel > 0 and travel + 2200 or 8000
        detail_state = {id=selected.id, text=selected.text, started=now, until_ms=now+hold,
            sequence=detail_sequence}
    end
    return selected.text
end

local function name(unit)
    return label(dfhack.df2utf(dfhack.units.getReadableName(unit)))
end

function classify(unit)
    local f = unit.health and unit.health.flags
    local care = false
    if f then for _, key in ipairs(CARE_FLAGS) do care = care or f[key] end end
    local infection = unit.body.infection_level > 0
    for _, wound in ipairs(unit.body.wounds) do
        infection = infection or wound.flags.infection
    end
    local sick = false
    for _, syndrome in ipairs(unit.syndromes.active) do
        sick = sick or syndrome.flags.is_sick or syndrome.flags.is_sick_low
    end
    local s = unit.status2
    local walk = s.limbs_stand_max > 0 and s.limbs_stand_count < s.limbs_stand_max
    local hand = s.limbs_grasp_max > 0 and s.limbs_grasp_count < s.limbs_grasp_max
    local worn, rags = false, false
    if not dfhack.units.isBaby(unit) then
        for _, inv in ipairs(unit.inventory) do
            local item = inv.item
            if inv.mode == df.inv_item_role_type.Worn and item:isClothing()
                    and item:getEffectiveArmorLevel() == 0 then
                local wear = item:getWear()
                worn = worn or wear >= 1
                rags = rags or wear >= 2
            end
        end
    end
    local reason, priority
    if infection then reason, priority = 'INFECTION', 100
    elseif sick then reason, priority = 'SICK_NEEDS_CARE', 95
    elseif f and f.rq_crutch then reason, priority = 'NEEDS_CRUTCH', 80
    elseif f and f.rq_cleaning then reason, priority = 'WOUND_NEEDS_CLEANING', 75
    elseif f and f.needs_healthcare then reason, priority = 'NEEDS_CARE', 70
    elseif care then reason, priority = 'NEEDS_TREATMENT', 70
    elseif (walk or hand) and unit.military.squad_id >= 0 then
        reason, priority = 'IMPAIRED_SOLDIER', 60
    elseif walk then reason, priority = 'WALKING_IMPAIRED', 40
    elseif hand then reason, priority = 'GRASPING_IMPAIRED', 35
    elseif rags then reason, priority = 'TATTERED_CLOTHES', 20
    end
    return {care=care or infection or sick, inf=infection, walk=walk, hand=hand,
        rags=rags, worn=worn, priority=priority or 0, reason=reason,
        urgent=infection or sick or (f and f.needs_healthcare) or false}
end

-- Cache a small grid of revealed outdoor columns. Never scan every map tile per frame.
points, point_cursor = points or {}, point_cursor or 0
local function clear_cache()
    points, point_cursor, env_cache, env_at = {}, 0, nil, nil
end
local function discover_points()
    local mx, my, mz = dfhack.maps.getTileSize()
    -- Revisit columns as terrain is revealed, roofed, or rebuilt. Cache at most 25
    -- coordinates and refresh only eight columns per environment callback.
    if point_cursor >= 25 then points, point_cursor = {}, 0 end
    -- Probe eight columns per callback, sweeping z in those columns only.
    for _ = 1, 8 do
        if point_cursor >= 25 then return end
        local x = math.min(mx - 1, math.floor((point_cursor % 5 + 0.5) * mx / 5))
        local y = math.min(my - 1, math.floor((math.floor(point_cursor / 5) + 0.5) * my / 5))
        point_cursor = point_cursor + 1
        for z = mz - 1, 0, -1 do
            local b = dfhack.maps.getTileBlock(x, y, z)
            if b then
                local d = b.designation[x % 16][y % 16]
                local shape = df.tiletype.attrs[b.tiletype[x % 16][y % 16]].shape
                if d.outside and not d.hidden and shape ~= df.tiletype_shape.EMPTY then
                    points[#points + 1] = {x=x, y=y, z=z}
                    break
                end
            end
        end
    end
end

function environment(now)
    if env_cache and env_at and now >= env_at and now - env_at < 5000 then return env_cache end
    discover_points()
    local result = {weather='unknown', temp='unknown', freeze='unknown'}
    local temp_on = df.global.d_init.feature.flags.TEMPERATURE
    local weather_on = df.global.d_init.feature.flags.WEATHER
    if not temp_on then result.freeze = 'off' end
    if not weather_on then result.weather = 'off' end
    local temps, weather, frozen = {}, {}, false
    for _, p in ipairs(points) do
        local b = dfhack.maps.getTileBlock(p)
        local x, y = p.x % 16, p.y % 16
        if b and b.designation[x][y].outside and not b.designation[x][y].hidden then
            if weather_on then
                local w = dfhack.maps.getCurrentWeather(p)
                weather[w] = true
            end
            if temp_on then
                local t = b.temperature_1[x][y]
                if t > 0 and t <= 60000 then temps[#temps + 1] = t end
                frozen = frozen or df.tiletype.attrs[b.tiletype[x][y]].material ==
                    df.tiletype_material.FROZEN_LIQUID
            end
        end
    end
    if weather_on then
        local kinds, w = 0
        for key in pairs(weather) do kinds, w = kinds + 1, key end
        if kinds > 1 then result.weather = 'mixed'
        elseif kinds == 1 then
            result.weather = ({[df.weather_type.None]='clear', [df.weather_type.Rain]='rain',
                [df.weather_type.Snow]='snow'})[w] or 'unknown'
        end
    end
    if #temps > 0 then
        table.sort(temps)
        result.temp = temps[math.floor((#temps + 1) / 2)]
        -- State is based on the coldest sampled tile, numeric overview on the median.
        if frozen then result.freeze = 'frozen'
        elseif temps[1] <= 10000 then result.freeze = 'freezing'
        elseif temps[1] <= 10004 then result.freeze = 'cold'
        else result.freeze = 'none' end
    end
    env_cache, env_at = result, now
    return result
end

-- Adventure mode has a controlled subject, so urgency follows conditions rather
-- than fortress totals or calendar colour. Blood is labelled, never synthetic HP.
local function bounded(n, maximum, what, minimum)
    assert(type(n) == 'number' and n >= (minimum or 0) and n <= maximum and n == math.floor(n),
        'Invalid adventure ' .. what)
    return n
end
function adventure_condition(unit, sleeping, bloodless)
    local c, c2, body, limbs = unit.counters, unit.counters2, unit.body, unit.status2
    local conditions, details, urgent = {}, {}, {}
    local function issue(severity, priority, word)
        conditions[#conditions+1] = {severity=severity, priority=priority, word=word}
        if severity >= 2 then urgent[word] = severity end
    end
    for _, key in ipairs({'winded','stunned','unconscious','suffocation','pain','nausea','dizziness'}) do
        -- The native signed KO counter can be -1 after waking from Adventure
        -- sleep. Accept that observed sentinel; only positive KO is unconscious.
        bounded(c[key], 1000000000, key, key == 'unconscious' and -1 or 0)
    end
    for _, key in ipairs({'exhaustion','paralysis','fever','hunger_timer','thirst_timer','sleepiness_timer'}) do
        bounded(c2[key], 1000000000, key)
    end
    -- Bleeding can overshoot zero (observed -26 on a killed party pet). Keep
    -- validating the signed native counter, but clamp only the displayed amount.
    local blood_count = body.blood_count
    assert(type(blood_count) == 'number' and blood_count >= -1000000000
        and blood_count <= 1000000000 and blood_count == math.floor(blood_count),
        'Invalid adventure blood')
    bounded(body.blood_max, 1000000000, 'blood maximum')
    for _, key in ipairs({'limbs_stand_count','limbs_stand_max','limbs_grasp_count','limbs_grasp_max'}) do
        bounded(limbs[key], 10000, key)
    end
    local walk = limbs.limbs_stand_count >= limbs.limbs_stand_max
    local hands = limbs.limbs_grasp_count >= limbs.limbs_grasp_max
    local blood = not bloodless and body.blood_max > 0 and
        math.max(0, math.min(100, math.floor(blood_count / body.blood_max * 100))) or nil
    local exhaustion = c2.exhaustion
    local effort = exhaustion >= 6000 and 'EXHAUSTED' or exhaustion >= 4000 and 'VERY_TIRED' or
        exhaustion >= 2000 and 'TIRED' or 'RESTED'
    if unit.flags2.killed then issue(4, 1000, 'DEAD') end
    if c.suffocation > 0 then issue(3, 990, 'SUFFOCATING') end
    if c.unconscious > 0 and not sleeping then issue(3, 980, 'UNCONSCIOUS') end
    if c2.paralysis > 0 then issue(2, 960, 'PARALYZED') end
    if c.stunned > 0 then issue(2, 950, 'STUNNED') end
    if c.winded > 0 then issue(2, 940, 'WINDED') end
    assert(#body.components.body_part_status <= 4096, 'Unbounded adventure body')
    for _, part in ipairs(body.components.body_part_status) do
        if part.on_fire then issue(3, 970, 'BURNING'); break end
    end
    local infection = bounded(body.infection_level, 1000000000, 'infection') > 0
    assert(#body.wounds <= 4096, 'Unbounded adventure wounds')
    local wounds, parts = 0, 0
    for _, wound in ipairs(body.wounds) do
        infection = infection or wound.flags.infection
        local descriptions, seen = {}, {}
        for _, part in ipairs(wound.parts) do
            parts = parts+1; assert(parts <= 16384, 'Unbounded adventure wound layers')
            local bp = body.body_plan.body_parts[part.body_part_id]
            local bpname = bp and bp.name_singular[0].value or 'BODY'
            local bleeding = bounded(part.bleeding, 1000000000, 'bleeding') > 0
            local impaired = bounded(part.impaired, 1000000000, 'wound impairment') > 0
            local word = bleeding and 'BLEEDING' or impaired and 'IMPAIRED' or 'INJURED'
            if bleeding then issue(1, 850, 'BLEEDING') end
            -- A wound is detailed without treating every scar as imminent death.
            if not seen[bpname] then
                seen[bpname]=true
                if #descriptions < 3 then descriptions[#descriptions+1] = bpname .. ' - ' .. word end
            end
        end
        wounds=wounds+1
        if #details < 4 then details[#details+1] = table.concat(descriptions, ' - ') end
    end
    if infection then issue(2, 920, 'INFECTION') end
    assert(#unit.syndromes.active <= 4096, 'Unbounded adventure syndromes')
    for _, syndrome in ipairs(unit.syndromes.active) do
        if syndrome.flags.is_sick or syndrome.flags.is_sick_low then issue(2, 915, 'SICK'); break end
    end
    if c2.fever > 0 then issue(2, 910, 'FEVER') end
    if c.pain >= 100 then issue(1, 800, 'PAIN') end
    if c.nausea > 0 then issue(1, 790, 'NAUSEATED') end
    if c.dizziness > 0 then issue(1, 780, 'DIZZY') end
    if not bloodless and body.blood_max > 0 and body.blood_count < body.blood_max then issue(1, 770, 'BLOOD_LOSS') end
    if exhaustion >= 4000 then issue(2, 700, effort)
    elseif exhaustion >= 2000 then issue(1, 700, effort) end
    if not walk then issue(1, 600, 'WALK_IMPAIRED') end
    if not hands then issue(1, 590, 'HAND_IMPAIRED') end
    if wounds > 0 then issue(1, 500, 'WOUNDED') end
    -- These are Adventure-specific need breakpoints, kept separate from exertion.
    if c2.thirst_timer >= 172800 then issue(1, 400, 'THIRSTY') end
    if c2.hunger_timer >= 172800 then issue(1, 390, 'HUNGRY') end
    if c2.sleepiness_timer >= 172800 and not sleeping then issue(1, 380, 'SLEEPY') end
    table.sort(conditions, function(a,b)
        return a.severity > b.severity or (a.severity == b.severity and a.priority > b.priority)
    end)
    local highest = conditions[1]
    return {severity=highest and highest.severity or 0, condition=highest and highest.word or
        (sleeping and 'SLEEPING' or 'READY'), effort=effort, blood=blood, wounds=wounds,
        walk=walk, hands=hands, urgent=urgent, details=details, conditions=conditions}
end

function adventure_party(interactions, player_id, resolve)
    local allies, pets, seen = {0,0,0}, {0,0,0}, {}
    local details, urgent = {}, {}
    for _, spec in ipairs({{'party_core_members',allies,'ALLY'}, {'party_extra_members',allies,'ALLY'},
            {'party_pets',pets,'PET'}, {'party_extra_pets',pets,'PET'}}) do
        local list, tally, role = interactions[spec[1]], spec[2], spec[3]
        assert(#list <= 10000, 'Unbounded adventure party')
        for _, id in ipairs(list) do
            if not seen[id] then
                seen[id]=true
                local unit = resolve(id)
                if not unit or unit.id ~= player_id then
                    tally[2]=tally[2]+1
                    if not unit or (unit.flags1.inactive and not unit.flags2.killed) then
                        tally[3]=tally[3]+1
                    else
                        local sleeping = unit.job.current_job and unit.job.current_job.job_type == df.job_type.Sleep
                        local c = adventure_condition(unit, sleeping,
                            not dfhack.units.casteFlagSet(unit.race, unit.caste, df.caste_raw_flags.HAS_BLOOD))
                        if c.severity > 0 then
                            tally[1]=tally[1]+1
                            add_detail(details, role .. ':' .. unit.id, c.severity*100+(role=='ALLY' and 10 or 5),
                                role .. ' ' .. name(unit) .. ' - ' .. c.condition)
                        end
                        for word, severity in pairs(c.urgent) do urgent[unit.id .. ':' .. word]=severity end
                    end
                end
            end
        end
    end
    sort_details(details)
    -- One companion and one pet can remain visible even with many affected pets.
    local selected = {}
    for _, role in ipairs({'ALLY:','PET:'}) do
        for _, d in ipairs(details) do
            if d.id:sub(1,#role)==role then selected[#selected+1]=d; break end
        end
    end
    return allies, pets, selected, urgent
end

function adventure_snapshot()
    local unit = dfhack.world.getAdventurer()
    if not unit and adventure_unit then
        local last = df.unit.find(adventure_unit)
        -- Preserve an observed death on the end screen, never a stale healthy unit.
        if last and last.flags2.killed then unit=last end
    end
    if not unit or (unit.flags1.inactive and not unit.flags2.killed) then return nil end
    if adventure_unit ~= unit.id or adventure_view ~= 'local' then
        adventure_unit=unit.id
        adventure_view='local'
        session=session+1
        previous, detail_state, recovery = nil, nil, nil
    end
    local sleeping = df.global.adventure.sleeping ~= 0 or
        (unit.job.current_job and unit.job.current_job.job_type == df.job_type.Sleep)
    local s = adventure_condition(unit, sleeping,
        not dfhack.units.casteFlagSet(unit.race, unit.caste, df.caste_raw_flags.HAS_BLOOD))
    s.mode, s.unit, s.name = 'adventure', unit.id, name(unit)
    -- A newly observed functional loss deserves an immediate warning; established
    -- impairment stays a caution instead of permanently implying a crisis.
    if previous then
        local word = previous.walk and not s.walk and 'WALK_IMPAIRED' or
            previous.hands and not s.hands and 'HAND_IMPAIRED' or nil
        if word then
            s.urgent[word]=2
            if s.severity < 2 then s.severity,s.condition=2,word end
        end
    end
    local details = {}
    for i=1,math.min(4,#s.conditions) do
        add_detail(details, 'player:' .. s.conditions[i].word, 20+s.conditions[i].severity*100,
            'YOU - ' .. s.conditions[i].word)
    end
    for i, description in ipairs(s.details) do
        if description ~= '' then add_detail(details, 'wound:' .. i, 110, description) end
    end
    if not s.walk or not s.hands then
        add_detail(details, 'function', 120, ('WORKING LIMBS - STANDING %d OF %d - GRASPING %d OF %d')
            :format(unit.status2.limbs_stand_count, unit.status2.limbs_stand_max,
                unit.status2.limbs_grasp_count, unit.status2.limbs_grasp_max))
    end
    local party_details, party_urgent
    s.allies, s.pets, party_details, party_urgent = adventure_party(df.global.adventure.interactions, unit.id,
        function(id)
            local hf = df.historical_figure.find(id)
            return hf and df.unit.find(hf.unit_id)
        end)
    local urgent = {}
    for word, severity in pairs(s.urgent) do urgent[unit.id .. ':' .. word]=severity end
    for id, severity in pairs(party_urgent) do urgent[id]=severity end
    s.urgent=urgent
    for _, d in ipairs(party_details) do details[#details+1]=d end
    if s.allies[3]+s.pets[3] > 0 then
        add_detail(details,'unavailable',10, ('PARTY STATUS UNKNOWN - %d ALLIES - %d PETS')
            :format(s.allies[3],s.pets[3]))
    end
    if #details==0 then add_detail(details,'clear',0,'NO_ACTIVE_CONDITIONS') end
    -- New serious events interrupt a long benign marquee immediately.
    sort_details(details)
    if previous then
        for id, severity in pairs(urgent) do
            if severity > (previous.urgent[id] or 0) then
                detail_state=nil
                local who,word=id:match('^(%d+):(.+)$')
                for _,d in ipairs(details) do
                    if d.id=='ALLY:' .. who or d.id=='PET:' .. who or
                            (tonumber(who)==unit.id and (d.id=='player:' .. word or d.id=='function')) then
                        d.priority=2000+severity*100
                    end
                end
            end
        end
    end
    local now=dfhack.getTickCount()
    -- Immediate escalation; require two stable seconds before stepping down.
    if recovery and s.severity < recovery.severity then
        if recovery.target ~= s.severity or recovery.condition ~= s.condition then
            recovery.target, recovery.condition, recovery.since=s.severity,s.condition,now
        end
        if now-recovery.since < 2000 then
            s.severity,s.condition=recovery.severity,recovery.word
        else recovery={severity=s.severity,word=s.condition} end
    else recovery={severity=s.severity,word=s.condition} end
    s.details=details
    s.detail=choose_detail(details,now)
    return s
end

-- Travel unloads all local units. The army retains live needs and identities,
-- but no current blood/wound/limb data; never present cached health as current.
function travel_needs(member)
    local needs = {}
    for i,spec in ipairs({{'hunger_timer','eats'}, {'thirst_timer','drinks'}, {'sleepiness_timer','sleeps'}}) do
        local value=bounded(member[spec[1]],1000000000,spec[1])
        local applies=member.flags[spec[2]]
        assert(type(applies)=='boolean','Invalid travel need flag')
        needs[i]=not applies and 2 or value >= 172800 and 1 or 0
    end
    return needs
end

function travel_party(interactions, player_hfid, present)
    local allies,pets,seen=0,0,{}
    for _,spec in ipairs({{'party_core_members','ALLY'}, {'party_extra_members','ALLY'},
            {'party_pets','PET'}, {'party_extra_pets','PET'}}) do
        local list=interactions[spec[1]]
        assert(#list<=10000,'Unbounded travel party')
        for _,id in ipairs(list) do
            bounded(id,2147483647,'travel party identity')
            if id~=player_hfid and not seen[id] and present[id] then
                seen[id]=true
                if spec[2]=='ALLY' then allies=allies+1 else pets=pets+1 end
            end
        end
    end
    return allies,pets
end

function travel_snapshot()
    local adv=df.global.adventure
    local nemesis=df.nemesis_record.find(adv.player_id)
    local army=df.army.find(adv.player_army_id)
    if not nemesis or not nemesis.figure or not army or not army.flags[df.army_flags.player] then return nil end
    local member,present=nil,{}
    assert(#army.members<=10000,'Unbounded travelling army')
    for _,m in ipairs(army.members) do
        if m.nemesis_id==nemesis.id then member=m end
        local n=df.nemesis_record.find(m.nemesis_id)
        if n and n.figure then present[n.figure.id]=true end
    end
    if not member then return nil end -- A transition/loading screen is not travel data.
    local id=bounded(nemesis.unit_id,2147483647,'travelling subject')
    if adventure_unit~=id or adventure_view~='travel' then
        adventure_unit,adventure_view=id,'travel'
        session=session+1
        previous,detail_state,recovery=nil,nil,nil
    end
    local needs=travel_needs(member)
    local activity='WALKING'
    for _,spec in ipairs({{'working','WORKING'}, {'composing','COMPOSING'},
            {'sneaking','SNEAKING'}, {'waiting','WAITING'}, {'sleeping','SLEEPING'}}) do
        if army.flags[df.army_flags[spec[1]]] then activity=spec[2] end
    end
    if member.flags.is_sleeping then activity='SLEEPING' end
    if member.flags.on_watch and activity~='SLEEPING' then activity='ON_WATCH' end
    local allies,pets=travel_party(adv.interactions,nemesis.figure.id,present)
    local details={}
    for i,word in ipairs({'HUNGRY - FOOD DUE','THIRSTY - WATER DUE','SLEEPY - REST DUE'}) do
        if needs[i]==1 then add_detail(details,'need:' .. i,100,word) end
    end
    add_detail(details,'health',10,'HEALTH UNAVAILABLE WHILE TRAVELLING')
    for _,key in ipairs({'x','y','z'}) do
        local value=army.pos[key]
        assert(type(value)=='number' and math.abs(value)<=1000000 and value==math.floor(value),
            'Invalid travel position')
    end
    add_detail(details,'position',0,('WORLD POSITION - X %d Y %d Z %d'):format(army.pos.x,army.pos.y,army.pos.z))
    sort_details(details)
    return {mode='travel',unit=id,name=label(dfhack.df2utf(dfhack.translation.translateName(nemesis.figure.name))),
        needs=needs,activity=activity,allies=allies,pets=pets,details=details,urgent={}}
end

function snapshot()
    if not dfhack.isWorldLoaded() then return nil end
    if dfhack.world.isAdventureMode() then
        if dfhack.isMapLoaded() then return adventure_snapshot() end
        return travel_snapshot()
    end
    if not dfhack.isMapLoaded() then return nil end
    if not dfhack.world.isFortressMode() then return nil end
    local s = {pop=0, care=0, inf=0, walk=0, hand=0, limbs=0, rags=0, worn=0,
        units={}, urgent={}, hospitals={}, moods={}}
    s.season = assert(({[0]='spring', [1]='summer', [2]='autumn', [3]='winter'})[df.global.cur_season],
        'Invalid calendar season')
    local details, hospital_details, mood_details, demand = {}, {}, {}, {}
    local best = 0
    for _, unit in ipairs(dfhack.units.getCitizens(false, true)) do
        s.pop = s.pop + 1
        local c = classify(unit)
        if c.walk or c.hand then s.limbs = s.limbs + 1 end
        for _, request in ipairs(CARE_FLAGS) do
            if unit.health and unit.health.flags[request] then
                demand[request] = (demand[request] or 0) + 1
            end
        end
        for _, key in ipairs({'care','inf','walk','hand','rags','worn'}) do
            if c[key] then s[key] = s[key] + 1 end
        end
        if c.priority > best then
            best = c.priority
            s.health_detail = label(name(unit) .. '_-_' .. c.reason)
        end
        if c.urgent then s.urgent[unit.id] = c.priority end
        if c.priority > 0 then
            s.units[#s.units + 1] = {id=unit.id, name=name(unit), status=c}
        end
        local job = unit.job.current_job
        if job and job.job_type >= df.job_type.StrangeMoodCrafter and
                job.job_type <= df.job_type.StrangeMoodMechanics then
            local m = mood_requirements(job, dfhack.job.getHolder(job) ~= nil)
            m.id, m.name = unit.id, name(unit)
            s.moods[#s.moods+1] = m
            add_detail(mood_details, 'mood:' .. unit.id, m.waiting and 105 or 10,
                'MOOD ' .. name(unit) .. ' - ' .. m.message)
            if m.waiting then s.urgent['mood:' .. unit.id] = 105 end
        end
    end
    if s.health_detail then add_detail(details, 'health', best, s.health_detail) end
    local active_hospitals = 0
    for _, hospital in ipairs(dfhack.world.getCurrentSite().buildings) do
        if df.abstract_building_hospitalst:is_instance(hospital) and not hospital.flags.DOES_NOT_EXIST then
            local active = false
            for _, id in ipairs(hospital.contents.building_ids) do
                local zone = df.building.find(id)
                if zone and df.building_civzonest:is_instance(zone) and zone.spec_sub_flag.active then active = true end
            end
            if active then
                active_hospitals = active_hospitals + 1
                local h = hospital_status(hospital.contents, demand)
                h.id, h.name = hospital.id, label(dfhack.df2utf(dfhack.translation.translateName(hospital.name, true)))
                s.hospitals[#s.hospitals+1] = h
                if #h.shortages > 0 then
                    add_detail(hospital_details, 'hospital:' .. hospital.id, h.urgent and 110 or 50,
                        'HOSPITAL - ' .. h.message .. ' - ' .. h.name)
                    if h.urgent then s.urgent['hospital:' .. hospital.id] = 110 end
                end
            end
        end
    end
    if active_hospitals == 0 then
        add_detail(hospital_details, 'hospital:none', s.care > 0 and 110 or 50, 'NO ACTIVE HOSPITAL')
        if s.care > 0 then s.urgent['hospital:none'] = 110 end
    end
    -- Reserve space for both additions; dozens of patients/moods cannot crowd one out.
    for _, group in ipairs({hospital_details, mood_details}) do
        sort_details(group)
        for i=1, math.min(3, #group) do details[#details+1] = group[i] end
    end
    local e = environment(dfhack.getTickCount())
    s.weather, s.temp, s.freeze = e.weather, e.temp, e.freeze
    if e.freeze == 'freezing' or e.freeze == 'frozen' or e.freeze == 'cold' then
        add_detail(details, 'environment', 50,
            ({freezing='SURFACE_FREEZING', frozen='ICE_ON_SURFACE', cold='SURFACE_NEAR_FREEZING'})[e.freeze])
    end
    if #details == 0 then add_detail(details, 'clear', 0,
        e.temp == 'unknown' and 'OUTSIDE_TEMPERATURE_UNKNOWN' or 'NO_CARE_ALERTS') end
    s.detail = choose_detail(details, dfhack.getTickCount())
    s.details = details
    s.severity = next(s.urgent) and 2 or
        ((s.care > 0 or s.limbs > 0 or s.rags > 0 or #hospital_details > 0 or e.freeze == 'cold' or e.freeze == 'freezing' or e.freeze == 'frozen') and 1 or 0)
    return s
end

function line(s)
    s.detail = choose_detail(s.details, dfhack.getTickCount())
    local new_alert = false
    if previous then
        for id, priority in pairs(s.urgent) do
            if not previous.urgent[id] or priority > previous.urgent[id] then new_alert = true end
        end
        if s.mode ~= 'adventure' and s.mode ~= 'travel' and (s.freeze == 'freezing' or s.freeze == 'frozen') and
                (previous.freeze ~= 'freezing' and previous.freeze ~= 'frozen') then new_alert = true end
    end
    if new_alert then alert = alert + 1 end
    previous = s
    if s.mode == 'travel' then
        return ('travel 1 session %d unit %d name %s food %d water %d rest %d activity %s allies %d pets %d phase %d detail %s ttl 6\n')
            :format(session,s.unit,s.name,s.needs[1],s.needs[2],s.needs[3],s.activity,
                s.allies,s.pets,detail_state.sequence,s.detail)
    end
    if s.mode == 'adventure' then
        return ('adv 1 session %d unit %d name %s condition %s effort %s blood %s wounds %d walk %d hand %d allies %d/%d/%d pets %d/%d/%d severity %d alert %d phase %d detail %s ttl 6\n')
            :format(session,s.unit,s.name,s.condition,s.effort,s.blood or 'na',s.wounds,
                s.walk and 1 or 0,s.hands and 1 or 0,table.unpack({
                    s.allies[1],s.allies[2],s.allies[3],s.pets[1],s.pets[2],s.pets[3],
                    s.severity,alert,detail_state.sequence,s.detail}))
    end
    return ('fort 2 session %d pop %d care %d inf %d walk %d hand %d rags %d worn %d weather %s temp %s freeze %s severity %d alert %d detail %s limbs %d phase %d season %s ttl 6\n')
        :format(session, s.pop, s.care, s.inf, s.walk, s.hand, s.rags, s.worn,
            s.weather, tostring(s.temp), s.freeze, s.severity, alert, s.detail, s.limbs, detail_state.sequence,
            assert(s.season, 'Missing calendar season'))
end

local function feed_path()
    local custom = os.getenv('G13MAP_HEALTH_FILE')
    if custom and custom ~= '' then return custom end
    local state = os.getenv('XDG_STATE_HOME')
    if not state or state == '' then
        local home = os.getenv('HOME')
        if not home or home == '' then error('No HOME/XDG_STATE_HOME: set G13MAP_HEALTH_FILE') end
        state = home .. '/.local/state'
    end
    return state .. '/g13map/health'
end
local function publish(text)
    local path = feed_path()
    local parent = path:match('^(.*)/[^/]+$')
    if parent then dfhack.filesystem.mkdir_recursive(parent) end
    local tmp = path .. '.dfhack-' .. tostring(session) .. '.new'
    local f = assert(io.open(tmp, 'w'))
    local ok, err = f:write(text)
    local closed, close_err = f:close()
    if not ok or not closed then os.remove(tmp); error(err or close_err) end
    local renamed, rename_err = os.rename(tmp, path)
    if not renamed then os.remove(tmp); error(rename_err) end
    last_published = text
end
local function reset()
    session = session + 1
    alert, previous, last_snapshot, last_refresh, last_write = 0, nil, nil, nil, nil
    detail_state, detail_sequence = nil, 0
    adventure_unit, adventure_view, recovery = nil, nil, nil
    clear_cache()
end
local function update()
    local now = dfhack.getTickCount()
    if last_write and now >= last_write and now - last_write < 1000 then return end
    local ok, err = pcall(function()
        local interval = dfhack.world.isAdventureMode() and 1000 or 2000
        if not last_refresh or now < last_refresh or now - last_refresh >= interval then
            last_snapshot, last_refresh = snapshot(), now
        end
        publish(last_snapshot and line(last_snapshot) or 'wait ttl 6\n')
    end)
    if not ok then
        -- Never fabricate all-clear data after an API/version mismatch.
        pcall(publish, 'wait ttl 6\n')
        if tostring(err) ~= last_error then dfhack.printerr('g13-lcd: ' .. tostring(err)) end
        last_error = tostring(err)
        last_snapshot, last_refresh = nil, nil
    else last_error = nil end
    last_write = now
end

function start()
    if enabled then return end
    enabled = true
    reset()
    dfhack.onStateChange.g13LCD = function(code)
        if code == SC_MAP_LOADED or code == SC_MAP_UNLOADED or code == SC_WORLD_UNLOADED then
            reset()
            if enabled then update() end
        end
    end
    local function tick()
        update()
        if enabled then timer = dfhack.timeout(1, 'frames', tick) end
    end
    tick()
end
function stop()
    enabled = false
    if timer then dfhack.timeout_active(timer, nil); timer = nil end
    -- Do not clear a newer feed from a different game.
    local f = io.open(feed_path(), 'r')
    if f then
        local current = f:read(1024)
        f:close()
        if last_published and current == last_published then publish('off\n') end
    end
end
local function diagnostic(action)
    local saved = {session=session, unit=adventure_unit, view=adventure_view, previous=previous, alert=alert,
        state=detail_state, sequence=detail_sequence, recovery=recovery}
    -- Recovery mutates its table in place; diagnostics work on an independent copy.
    if recovery then
        local copy={}; for k,v in pairs(recovery) do copy[k]=v end; recovery=copy
    end
    local ok, result = pcall(function() return action(snapshot()) end)
    session, adventure_unit, previous, alert = saved.session, saved.unit, saved.previous, saved.alert
    adventure_view=saved.view
    detail_state, detail_sequence, recovery = saved.state, saved.sequence, saved.recovery
    assert(ok, result)
    return result
end
if dfhack_flags.module then return end
local args = {...}
local command = args[1] or 'status'
if command == '--version' then print('g13-lcd ' .. VERSION)
elseif command == 'start' then start()
elseif command == 'stop' then stop()
elseif command == 'status' then
    print('g13-lcd ' .. VERSION .. (enabled and ' running' or ' stopped') .. ' -> ' .. feed_path())
    if last_error then print('Last error: ' .. last_error) end
elseif command == 'once' then
    print(diagnostic(function(s) return s and line(s) or 'wait ttl 6' end))
elseif command == 'inspect' then
    local s = diagnostic(function(s) return s or {state='wait'} end)
    s.urgent = nil -- Numeric unit IDs otherwise serialize as a huge sparse JSON array.
    print(json.encode(s))
else
    qerror('Usage: g13-lcd start|stop|status|once|inspect|--version')
end
