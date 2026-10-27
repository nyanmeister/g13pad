-- SPDX-License-Identifier: GPL-3.0-or-later
-- G13Health: feeds the g13pad health meter from Deep Rock Galactic (a UE4SS Lua mod).
--
-- Writes one line to the meter's feed file, ~/.local/state/g13map/health on the host
-- (see docs/health-meter.md in g13pad):
--   HEALTH/BASE shield ARMOR/MAXARMOR ttl 15   with a local dwarf (0 health = down)
--   wait ttl 15                                 without one (menu, loading, spectating)
-- BASE is the dwarf's normal maximum with its perks: MaxHealth as the game reports it,
-- with any beer divided out. A beer is a temporary buff in the game instance's list; a
-- UStatTemporaryBuff modifies pawn stats, and a Red Rock Blaster multiplies MaxHealth by
-- 1.3 for the mission, so with one active the extra shows as over 100%, blue on the G13
-- (asked 2026-10-07; the first rule, "lowest MaxHealth seen", took the unperked rig pawn's
-- 110 as the base and read a perked 125 as 114%). DRG calls the shield "armor" inside.
--
-- Pawn tracking follows the pattern of the other Lua mod in this install: construction
-- events feed a candidate list, the poll only does cheap reads, no object scans in
-- steady state. Everything that touches UObjects runs on the game thread.

local config = {
	-- The feed file as the game sees it, e.g. "Z:\\home\\me\\.local\\state\\g13map\\health".
	-- nil: the first of G13MAP_FEED in the environment (a Steam launch option can set it),
	-- feed.txt beside this mod (install.sh writes it), then $XDG_STATE_HOME/g13map/health
	-- or ~/.local/state/g13map/health through Z:. The last only works where Wine still
	-- imports HOME: GE-Proton 11-7 did, 11-9 and later drop HOME and every XDG_* variable
	-- from the Windows environment (ntdll's is_special_env_var; found 2026-10-26, the meter
	-- had gone silent with "no HOME or XDG_STATE_HOME" in UE4SS.log).
	feed = nil,
	poll_ms = 200, -- how often health is read
	heartbeat_s = 5, -- rewrite the line even without a change, to keep the ttl alive
	ttl_s = 15, -- the line expires without a rewrite: a closed game is a meter gone
	base_max_health = nil, -- the 100% point; nil = MaxHealth with any beer divided out
	debug = false, -- print every write to UE4SS.log
}

-- ------------------------------------------------------------------ the feed file
-- The directory this script was loaded from, ending in its separator; nil when unknown.
local function scriptDir()
	local info = debug and debug.getinfo and debug.getinfo(1, "S")
	local src = info and info.source
	if type(src) ~= "string" then
		return nil
	end
	return src:match("^@(.*[\\/])")
end

-- The one non-blank line of feed.txt in the mod's folder (G13Health/feed.txt), or nil.
local function sidecarFeed()
	local dir = scriptDir()
	if not dir then
		return nil
	end
	local f = io.open(dir .. ".." .. dir:sub(-1) .. "feed.txt", "r")
	if not f then
		return nil
	end
	local text = f:read("a") or ""
	f:close()
	local path = text:match("^%s*(.-)%s*$")
	return path ~= "" and path or nil
end

-- The path and where it came from; nil, reason without one.
local function feedPath()
	if config.feed then
		return config.feed, "config"
	end
	local env = os.getenv("G13MAP_FEED")
	if env and env ~= "" then
		return env, "G13MAP_FEED"
	end
	local sidecar = sidecarFeed()
	if sidecar then
		return sidecar, "feed.txt"
	end
	-- Steam's container gives the game a private /run/user; the home directory is
	-- shared, so the feed goes to the meter's state-directory file. Wine builds that
	-- hide HOME still name the host home: WINE_HOST_HOME as a Unix path, WINEHOMEDIR as
	-- an NT path (\??\Z:\home\me, or \??\unix\home\me on older builds).
	local dir, source = os.getenv("XDG_STATE_HOME"), "XDG_STATE_HOME"
	if not (dir and dir:sub(1, 1) == "/") then
		dir, source = nil, nil
		local home = os.getenv("HOME")
		if home and home:sub(1, 1) == "/" then
			dir, source = home .. "/.local/state", "HOME"
		end
	end
	if not dir then
		local home = os.getenv("WINE_HOST_HOME")
		if home and home:sub(1, 1) == "/" then
			dir, source = home .. "/.local/state", "WINE_HOST_HOME"
		end
	end
	if dir then
		return "Z:" .. dir:gsub("/", "\\") .. "\\g13map\\health", source
	end
	local nt = os.getenv("WINEHOMEDIR") or ""
	local home = nt:match("^\\%?%?\\unix(\\.+)$")
	if home then
		return "Z:" .. home .. "\\.local\\state\\g13map\\health", "WINEHOMEDIR"
	end
	home = nt:match("^\\%?%?\\(%a:\\.*)$")
	if home then
		return home .. "\\.local\\state\\g13map\\health", "WINEHOMEDIR"
	end
	return nil, "no feed.txt beside the mod and no host home in the environment: run contrib/drg-health/install.sh, or set config.feed"
end

local feed, feedSource = feedPath()
local warned = false
local function writeFeed(line)
	if not feed then
		if not warned then
			print(string.format("[G13Health] nowhere to write: %s\n", feedSource))
			warned = true
		end
		return false
	end
	local f, err = io.open(feed, "w")
	if not f then
		if not warned then
			print(string.format("[G13Health] cannot write %s: %s\n", feed, tostring(err)))
			warned = true
		end
		return false
	end
	f:write(line, "\n")
	f:close()
	if config.debug then
		print(string.format("[G13Health] %s\n", line))
	end
	return true
end

-- ------------------------------------------------------------ pawn tracking (events)
local cachedPawn = nil
local candidates = {}

local function noteCandidate(obj)
	candidates[#candidates + 1] = obj
	if #candidates > 16 then
		table.remove(candidates, 1)
	end
end

local eventsOk = pcall(function()
	NotifyOnNewObject("/Script/FSD.PlayerCharacter", noteCandidate)
end)

ExecuteInGameThread(function()
	local all = FindAllOf("PlayerCharacter")
	if all then
		for _, p in ipairs(all) do
			noteCandidate(p)
		end
	end
end)

local function getMyPawn()
	if cachedPawn then
		local ok, good = pcall(function()
			return cachedPawn:IsValid() and cachedPawn:IsLocallyControlled()
		end)
		if ok and good then
			return cachedPawn
		end
		cachedPawn = nil
	end
	for i = #candidates, 1, -1 do
		local c = candidates[i]
		local okV, valid = pcall(function()
			return c:IsValid()
		end)
		if not okV or not valid then
			table.remove(candidates, i)
		else
			local okL, loc = pcall(function()
				return c:IsLocallyControlled()
			end)
			if okL and loc then
				cachedPawn = c
				return c
			end
		end
	end
	return nil
end

local pawnlessTicks = 0
local function getMyPawnByScan()
	local all = FindAllOf("PlayerCharacter")
	if all then
		for _, p in ipairs(all) do
			local ok, loc = pcall(function()
				return p:IsLocallyControlled()
			end)
			if ok and loc and p:IsValid() then
				cachedPawn = p
				return p
			end
		end
	end
	return nil
end

-- ------------------------------------------------------------------ the reading
local reported = false
local lastBase = nil

-- The MaxHealth stat asset (GameData.Stats.MaxHealth) and the game instance, found once.
local maxHealthStat = nil
local gameInstance = nil
local function statName(stat)
	local ok, name = pcall(function()
		return stat:GetFullName()
	end)
	return ok and name or nil
end
local function findStat()
	if maxHealthStat then
		local ok, valid = pcall(function()
			return maxHealthStat:IsValid()
		end)
		if ok and valid then
			return maxHealthStat
		end
		maxHealthStat = nil
	end
	pcall(function()
		local gd = FindFirstOf("GameData")
		if gd and gd:IsValid() then
			local s = gd.Stats.MaxHealth
			if s and s:IsValid() then
				maxHealthStat = s
			end
		end
	end)
	return maxHealthStat
end

-- What active beers do to MaxHealth: a multiplier and an addend (1, 0 without any).
local function beerEffect()
	local mult, add = 1.0, 0.0
	local stat = findStat()
	if not stat then
		return mult, add
	end
	local want = statName(stat)
	pcall(function()
		if not (gameInstance and gameInstance:IsValid()) then
			gameInstance = FindFirstOf("FSDGameInstance")
		end
		local buffs = gameInstance.TemporaryBuffs
		for i = 1, #buffs do
			local buff = buffs[i]
			pcall(function()
				buff.ModifiedStats:ForEach(function(key, value)
					local k, v = key:get(), value:get()
					if statName(k) == want and type(v) == "number" then
						-- ValueModificationType: 0 multiplicative, 1 additive
						local kind = k.ValueModificationType
						if kind == 1 then
							add = add + v
						elseif v > 0 then
							mult = mult * (v >= 1 and v or 1 + v)
						end
					end
				end)
			end)
		end
	end)
	return mult, add
end

-- health, max, armor, maxArmor, dead; nil without a usable component
local function readHealth(pawn)
	local ok, r = pcall(function()
		local hc = pawn.HealthComponent
		if not hc or not hc:IsValid() then
			return nil
		end
		return {
			health = hc:GetHealth(),
			max = hc.MaxHealth,
			pct = hc:GetHealthPct(),
			armor = hc:GetArmor(),
			maxArmor = hc:GetMaxArmor(),
			dead = hc:IsDead(),
		}
	end)
	if ok and r and type(r.health) == "number" then
		return r
	end
	return nil
end

local function line(pawn)
	if not pawn then
		return string.format("wait ttl %d", config.ttl_s)
	end
	local r = readHealth(pawn)
	if not r then
		return string.format("wait ttl %d", config.ttl_s)
	end
	local base = config.base_max_health
	if not base and r.max and r.max > 0 then
		local mult, add = beerEffect()
		base = (r.max - add) / mult
		if base ~= lastBase then
			lastBase = base
			print(string.format("[G13Health] 100%% point: MaxHealth %.1f, beer x%.3f +%.1f -> base %.1f\n",
				r.max, mult, add, base))
		end
	end
	if not reported then
		reported = true
		print(string.format(
			"[G13Health] first reading: health %.1f, MaxHealth %.1f, pct %.3f, armor %.1f of %.1f\n",
			r.health, r.max or -1, r.pct or -1, r.armor or -1, r.maxArmor or -1))
	end
	if not base or base <= 0 then
		return string.format("wait ttl %d", config.ttl_s)
	end
	local health = (r.dead or r.health <= 0) and 0 or r.health
	local maxArmor = (r.maxArmor and r.maxArmor > 0) and r.maxArmor or 1
	local armor = r.armor or 0
	if armor < 0 then
		armor = 0
	end
	return string.format("%.1f/%.1f shield %.1f/%.1f ttl %d", health, base, armor, maxArmor, config.ttl_s)
end

-- --------------------------------------------------------------------- the loop
local lastLine = nil
local lastWrite = 0
LoopAsync(config.poll_ms, function()
	ExecuteInGameThread(function()
		local pawn = getMyPawn()
		if not pawn then
			pawnlessTicks = pawnlessTicks + 1
			-- events missing or quiet for 15 s: one scan per 5 s is allowed
			if (not eventsOk or pawnlessTicks >= 75) and pawnlessTicks % 25 == 0 then
				pawn = getMyPawnByScan()
			end
		else
			pawnlessTicks = 0
		end
		local l = line(pawn)
		local now = os.time()
		if l ~= lastLine or now - lastWrite >= config.heartbeat_s then
			if writeFeed(l) then
				lastLine = l
				lastWrite = now
			end
		end
	end)
	return false -- keep looping
end)

print(string.format("[G13Health] loaded; feed %s (%s), poll %d ms, ttl %d s (pawn events: %s)\n",
	tostring(feed), feedSource, config.poll_ms, config.ttl_s, eventsOk and "ok" or "unavailable, scan fallback"))
