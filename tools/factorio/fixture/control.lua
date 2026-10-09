-- SPDX-License-Identifier: GPL-3.0-or-later
-- TEST SCENARIO ONLY. This is never installed in the player's Factorio directory.
script.on_init(function()
  game.surfaces[1].request_to_generate_chunks({0, 0}, 2)
  game.surfaces[1].force_generate_chunk_requests()
end)
script.on_event(defines.events.on_player_created, function(event)
  local p = game.get_player(event.player_index)
  if not p.character then p.create_character() end
  p.teleport({0, 0}, game.surfaces[1])
  p.get_inventory(defines.inventory.character_armor).insert{name="power-armor-mk2", count=1}
  local grid = p.get_inventory(defines.inventory.character_armor)[1].grid
  grid.put{name="battery-mk2-equipment", position={0,0}}
  grid.put{name="energy-shield-mk2-equipment", position={2,0}}
  for _, prerequisite in pairs(p.force.technologies["military"].prerequisites) do prerequisite.researched = true end
  assert(p.force.add_research("military"), "military research was not queued")
  p.force.research_progress = 0.37
  storage.character = p.character
  storage.walls = {
    p.surface.create_entity{name="stone-wall", position={5,5}, force=p.force},
    p.surface.create_entity{name="stone-wall", position={6,5}, force=p.force}
  }
  storage.started = game.tick
end)
script.on_nth_tick(30, function()
  local p = game.players[1]
  if not p or not storage.started then return end
  local age = game.tick - storage.started
  local c = storage.character
  if age < 1800 and c and c.valid then
    c.health = age < 300 and 150 or 50
    local grid = c.grid
    for _, e in pairs(grid.equipment) do
      if e.name == "battery-mk2-equipment" then e.energy = e.max_energy * 0.25 end
      if e.name == "energy-shield-mk2-equipment" then e.shield = 30 end
    end
    p.force.research_progress = 0.37
  end
  if age >= 300 and age < 1500 then
    for _, wall in pairs(storage.walls) do p.add_alert(wall, defines.alert_type.entity_under_attack) end
  else
    p.remove_alert{type=defines.alert_type.entity_under_attack}
  end
  if age >= 900 and not storage.remote then
    p.set_controller{type=defines.controllers.remote, surface=p.surface, position={0,0}}
    storage.remote = true
  end
  if age >= 1200 and storage.remote and not storage.back then
    p.set_controller{type=defines.controllers.character, character=c}
    storage.back = true
  end
  if age >= 1800 and not storage.dead then
    c.die()
    storage.dead = true
  end
  helpers.write_file("expected.txt", string.format("age=%d health=%s shield=%s battery=%s research=37 attack=%d controller=%d\n", age, c and c.valid and tostring(c.health) or "dead", c and c.valid and tostring(c.grid.shield) or "none", c and c.valid and tostring(c.grid.available_in_batteries / c.grid.battery_capacity * 100) or "none", age >=300 and age <1500 and 2 or 0, p.controller_type), false, p.index)
end)
