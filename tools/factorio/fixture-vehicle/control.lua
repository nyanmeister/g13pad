-- SPDX-License-Identifier: GPL-3.0-or-later
-- TEST SCENARIO ONLY. Never install this in the player's Factorio directories.
script.on_init(function()
  game.surfaces[1].request_to_generate_chunks({0, 0}, 3)
  game.surfaces[1].force_generate_chunk_requests()
end)
script.on_event(defines.events.on_player_created, function(event)
  local p = game.get_player(event.player_index)
  if not p.character then p.create_character() end
  p.teleport({0, 0}, game.surfaces[1])
  p.get_inventory(defines.inventory.character_armor).insert{name="power-armor-mk2", count=1}
  local grid = p.character.grid
  grid.put{name="battery-mk2-equipment", position={0,0}}
  grid.put{name="energy-shield-mk2-equipment", position={2,0}}
  for _, prerequisite in pairs(p.force.technologies["advanced-oil-processing"].prerequisites) do prerequisite.researched = true end
  assert(p.force.add_research("advanced-oil-processing"))
  p.force.research_progress = 0.37
  storage.character = p.character
  storage.started = game.tick
  storage.stage = -1
end)
local stages = {
  {name="foot"},
  {name="car", health=0.5},
  {name="car", health=0.2},
  {name="foot"},
  {name="tank", health=0.6, passenger=true},
  {name="car", health=0.75, quality="rare"},
  {name="spidertron", health=0.5, quality="legendary"},
  {name="spidertron", health=0.3, quality="legendary", remote=true},
  {name="foot"},
  {name="locomotive", health=0.4},
  {name="foot"},
  {name="dead"}
}
script.on_nth_tick(30, function()
  local p = game.players[1]
  if not p or not storage.started then return end
  local age = game.tick - storage.started
  local index = math.min(math.floor(age / 180) + 1, #stages)
  local spec = stages[index]
  local c = storage.character
  if storage.stage ~= index then
    storage.stage = index
    if p.controller_type == defines.controllers.remote then
      p.set_controller{type=defines.controllers.character, character=c}
    end
    p.driving = false
    if storage.vehicle and storage.vehicle.valid then storage.vehicle.destroy() end
    storage.vehicle = nil
    if spec.name == "dead" then
      c.die()
    elseif spec.name ~= "foot" then
      local position = {3, 0}
      if spec.name == "locomotive" then
        position = {30, 6}
        for y=0,12,2 do p.surface.create_entity{name="straight-rail", position={30,y}, direction=defines.direction.north, force=p.force} end
      end
      local vehicle = p.surface.create_entity{name=spec.name, position=position, force=p.force, quality=spec.quality or "normal"}
      assert(vehicle, "vehicle creation failed")
      if spec.passenger then vehicle.set_passenger(p) else vehicle.set_driver(p) end
      vehicle.health = vehicle.max_health * spec.health
      storage.vehicle = vehicle
      if spec.remote then p.set_controller{type=defines.controllers.remote, surface=p.surface, position=position} end
    end
  end
  if c and c.valid then
    c.health = 150
    for _, e in pairs(c.grid.equipment) do
      if e.name == "battery-mk2-equipment" then e.energy = e.max_energy * 0.25 end
      if e.name == "energy-shield-mk2-equipment" then e.shield = 30 end
    end
  end
  local v = storage.vehicle
  if v and v.valid then v.health = v.max_health * spec.health end
  p.force.research_progress = 0.37
  helpers.write_file("expected-vehicle.txt", string.format("age=%d stage=%d name=%s vehicle=%s/%s quality=%s pilot=%s controller=%d driving=%s\n", age, index, spec.name, v and v.valid and tostring(v.health) or "none", v and v.valid and tostring(v.max_health) or "none", spec.quality or "normal", c and c.valid and tostring(c.health) or "dead", p.controller_type, tostring(p.driving)), true, p.index)
end)
