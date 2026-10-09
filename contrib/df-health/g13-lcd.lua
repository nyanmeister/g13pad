-- SPDX-License-Identifier: GPL-3.0-or-later
-- Read-only fortress overview for the G13 LCD. Requires DFHack.
--@module = true
local json = require('json')
local VERSION = '0.2.55'
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

function snapshot()
    if not dfhack.isMapLoaded() or not dfhack.world.isFortressMode() then return nil end
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
        if (s.freeze == 'freezing' or s.freeze == 'frozen') and
                (previous.freeze ~= 'freezing' and previous.freeze ~= 'frozen') then new_alert = true end
    end
    if new_alert then alert = alert + 1 end
    previous = s
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
    clear_cache()
end
local function update()
    local now = dfhack.getTickCount()
    if last_write and now >= last_write and now - last_write < 1000 then return end
    local ok, err = pcall(function()
        if not last_refresh or now < last_refresh or now - last_refresh >= 2000 then
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
    local old_detail_state, old_detail_sequence = detail_state, detail_sequence
    local s = snapshot()
    -- A diagnostic must not consume events the running feeder has yet to publish.
    local old_previous, old_alert = previous, alert
    local output = s and line(s) or 'wait ttl 6'
    previous, alert = old_previous, old_alert
    detail_state, detail_sequence = old_detail_state, old_detail_sequence
    print(output)
elseif command == 'inspect' then
    local old_detail_state, old_detail_sequence = detail_state, detail_sequence
    local s = snapshot() or {state='wait'}
    detail_state, detail_sequence = old_detail_state, old_detail_sequence
    s.urgent = nil -- Numeric unit IDs otherwise serialize as a huge sparse JSON array.
    print(json.encode(s))
else
    qerror('Usage: g13-lcd start|stop|status|once|inspect|--version')
end
