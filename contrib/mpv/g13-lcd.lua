-- SPDX-License-Identifier: GPL-3.0-or-later
-- Passive telemetry for g13map's mpv playback profile display. No input/OSD changes.
local mp = require 'mp'
local utils = require 'mp.utils'
local options = require 'mp.options'
local opts = { ['feed-dir'] = '' }
options.read_options(opts, 'g13-lcd')
local version = '0.2.76'
local base = os.getenv('XDG_STATE_HOME') or utils.join_path(os.getenv('HOME') or '', '.local/state')
local dir = opts['feed-dir'] ~= '' and opts['feed-dir'] or utils.join_path(base, 'g13map/mpv')
local pid = mp.get_property_number('pid')
local path = utils.join_path(dir, tostring(pid) .. '.json')
local result = utils.subprocess({args={'mkdir', '-p', '--', dir}, cancellable=false})
if result.status ~= 0 then mp.msg.error('Cannot create telemetry directory: ' .. dir); return end
local drops, previous = {}, {}
local function reset() drops = {}; previous = {} end
local function recent(name, now)
    local current = mp.get_property_number(name)
    if current == nil then return nil end
    if previous[name] and current >= previous[name] and current > previous[name] then
        drops[#drops + 1] = {name=name, at=now, count=current - previous[name]}
    end
    previous[name] = current
    local total = 0
    for _, event in ipairs(drops) do
        if event.name == name and now - event.at <= 1 then total = total + event.count end
    end
    return total
end
local failed = false
local function publish()
    local now = mp.get_time()
    local keep = {}
    for _, event in ipairs(drops) do if now - event.at <= 1 then keep[#keep+1] = event end end
    drops = keep
    local render_ms
    local passes = mp.get_property_native('vo-passes')
    if passes and passes.fresh and #passes.fresh > 0 then
        local sum = 0
        for _, pass in ipairs(passes.fresh) do sum = sum + (pass.avg or 0) end
        render_ms = sum / 1000000
    end
    local v = {
        version=1, build=version, pid=pid, written=os.time(),
        focused=mp.get_property_native('focused'), idle=mp.get_property_native('idle-active'),
        paused=mp.get_property_native('pause'), codec=mp.get_property('video-format'),
        height=mp.get_property_number('video-params/h'), hwdec=mp.get_property('hwdec-current'),
        decoder_drops=recent('decoder-frame-drop-count', now),
        display_drops=recent('frame-drop-count', now), avsync=mp.get_property_number('avsync'),
        buffer=mp.get_property_number('demuxer-cache-duration'), render_ms=render_ms,
        position=mp.get_property_number('playback-time'), duration=mp.get_property_number('duration'),
    }
    local file, err = io.open(path .. '.new', 'w')
    if file then
        local ok = file:write(utils.format_json(v)); local closed = file:close()
        if ok and closed then ok, err = os.rename(path .. '.new', path) else ok = nil end
        if ok then failed = false; return end
    end
    if not failed then mp.msg.error('Cannot write LCD telemetry: ' .. tostring(err)); failed = true end
end
mp.register_event('file-loaded', function() reset(); publish() end)
mp.register_event('seek', reset)
mp.register_event('end-file', reset)
mp.register_event('shutdown', function() os.remove(path); os.remove(path .. '.new') end)
mp.register_script_message('g13-lcd-version', function() mp.msg.info('g13-lcd ' .. version) end)
mp.add_periodic_timer(0.25, publish)
publish()
