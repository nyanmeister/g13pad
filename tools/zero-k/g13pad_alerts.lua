-- SPDX-License-Identifier: GPL-2.0-or-later
-- g13pad alerts: writes an off-screen attack/loss alert line for the G13 glass.
--
-- LuaUI can only write inside Spring's write directory (the Zero-K install folder), so this
-- widget writes g13pad's feed grammar to g13pad_alerts.txt there, and `g13map alert relay`
-- copies it to the pad's feed. Install with tools/zero-k/install.sh.
--
-- Events: a unit of ours damaged or destroyed while off screen (Spring.IsUnitInView), ranked
-- by its metal cost, the commander on top. Bearing is from the point the camera looks at to
-- the unit, degrees clockwise from the map's north (negative z).

function widget:GetInfo()
  return {
    name    = "G13 pad alerts",
    desc    = "Writes off-screen attack and loss alerts for g13pad",
    author  = "g13pad",
    date    = "2026-11-04",
    license = "GNU GPL, v2 or later",
    layer   = 0,
    enabled = true,
  }
end

local FILE = "g13pad_alerts.txt"
local HOLD = 8          -- seconds an alert stays on the glass
local KEEPALIVE = 2     -- seconds between quiet rewrites
local REPEAT_GAP = 1.0  -- seconds before the same unit may alert again
local AREA = 700        -- elmos: a fight this close to the current alert is the same fight
local AREA_HOLD = 10    -- seconds the same fight stays one alert (a worse loss still breaks in)

local spGetUnitPosition   = Spring.GetUnitPosition
local spIsUnitInView      = Spring.IsUnitInView
local spGetCameraPosition = Spring.GetCameraPosition
local spGetCameraDirection = Spring.GetCameraDirection
local spGetSpectatingState = Spring.GetSpectatingState
local spGetTimer          = Spring.GetTimer
local spDiffTimers        = Spring.DiffTimers

local localTeamID = Spring.GetLocalTeamID()
local current = nil      -- { sev = n, since = timer, x = elmos, z = elmos, label = s }
local lastUnit = {}      -- unitID -> timer of its last alert
local lastWrite = nil
local clock = nil

local function write(line)
  local f = io.open(FILE, "w")
  if f then
    f:write(line, "\n")
    f:close()
  end
end

-- The ground point under the camera's line of sight, else the camera itself.
local function lookAt()
  local cx, cy, cz = spGetCameraPosition()
  local dx, dy, dz = spGetCameraDirection()
  if cx and dy and dy < -0.05 then
    local t = -cy / dy
    return cx + dx * t, cz + dz * t
  end
  return cx or 0, cz or 0
end

local function bearingTo(x, z)
  local lx, lz = lookAt()
  local b = math.deg(math.atan2(x - lx, -(z - lz)))
  if b < 0 then b = b + 360 end
  return math.floor(b + 0.5) % 360
end

local function label(ud)
  local name = (ud.humanName or ud.name or "unit"):upper():gsub("[^A-Z0-9 ]", "")
  name = name:gsub("^%s+", "")
  if ud.customParams and ud.customParams.commtype then
    return "COM"
  end
  local first = name:match("^(%S+)")
  if first and #first <= 8 then
    return first
  end
  return name:sub(1, 8):gsub("%s+$", "")
end

local function severity(ud, lost)
  local sev
  if ud.customParams and ud.customParams.commtype then
    sev = 5
  else
    local cost = ud.metalCost or 0
    if cost >= 800 then sev = 4
    elseif cost >= 300 then sev = 3
    elseif cost >= 120 then sev = 2
    else sev = 1 end
  end
  if lost and sev < 5 then sev = sev + 1 end
  return sev
end

local function alert(unitID, unitDefID, lost)
  local ud = UnitDefs[unitDefID]
  if not ud then return end
  local now = spGetTimer()
  local sev = severity(ud, lost)
  if not lost then
    if sev < 2 then return end                -- cheap stuff being plinked is noise
    local t = lastUnit[unitID]
    if t and spDiffTimers(now, t) < REPEAT_GAP then return end
    -- A live alert of higher severity keeps the glass for a while.
    if current and current.sev > sev and spDiffTimers(now, current.since) < 3 then return end
  end
  local x, _, z = spGetUnitPosition(unitID)
  if not x then return end
  -- One fight, one alert: inside AREA of the current alert and within AREA_HOLD, only a
  -- higher severity (a bigger unit, or a loss where there were only hits) speaks again.
  if current and current.x and spDiffTimers(now, current.since) < AREA_HOLD then
    local dx, dz = x - current.x, z - current.z
    if dx * dx + dz * dz < AREA * AREA and sev <= current.sev then
      lastUnit[unitID] = now
      return
    end
  end
  lastUnit[unitID] = now
  current = { sev = sev, since = now, x = x, z = z }
  write(string.format("alert %d %s kind %s sev %d at %d ttl %d",
    bearingTo(x, z), label(ud):gsub(" ", "_"), lost and "lost" or "hit", sev,
    os.time() * 1000, HOLD))
  lastWrite = now
end

function widget:UnitDamaged(unitID, unitDefID, unitTeam, damage, paralyzer, weaponID, attackerID)
  if damage <= 0 or unitTeam ~= localTeamID then return end
  if spIsUnitInView(unitID) then return end
  alert(unitID, unitDefID, false)
end

function widget:UnitDestroyed(unitID, unitDefID, unitTeam)
  if unitTeam ~= localTeamID then return end
  if spIsUnitInView(unitID) then return end
  local ud = UnitDefs[unitDefID]
  if not ud or ((ud.metalCost or 0) < 120 and not (ud.customParams and ud.customParams.commtype)) then
    return
  end
  alert(unitID, unitDefID, true)
  lastUnit[unitID] = nil
end

function widget:Update(dt)
  local now = spGetTimer()
  if current and spDiffTimers(now, current.since) > HOLD then
    current = nil
  end
  if current == nil and (lastWrite == nil or spDiffTimers(now, lastWrite) >= KEEPALIVE) then
    write("quiet ttl " .. (KEEPALIVE * 3))
    lastWrite = now
  end
end

function widget:Initialize()
  if spGetSpectatingState() then
    widgetHandler:RemoveWidget()
    return
  end
  clock = spGetTimer()
  write("quiet ttl " .. (KEEPALIVE * 3))
  lastWrite = clock
end

function widget:PlayerChanged(playerID)
  if spGetSpectatingState() then
    write("off")
    widgetHandler:RemoveWidget()
    return
  end
  localTeamID = Spring.GetLocalTeamID()
end

function widget:Shutdown()
  write("off")
end
