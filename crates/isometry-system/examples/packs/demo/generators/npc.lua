-- A wandering NPC: a base creature from the bestiary, given a name.
--
-- The proposal's `key` is a bestiary slug (goblin, kobold, ...), which the host
-- lowers into that creature's stat block under the generated `name`. So a
-- generated "Skreek" is a real, fightable goblin -- statting reuses the whole
-- monster path, and the pack only decides flavor. Entropy is the host's single
-- tape draw; reroll = the next draw, so archetype and name are a fixed sequence
-- off the seed, not independent samples.
function call_gen(request_json, entropy, request)
    local archetypes = { "goblin", "kobold", "giant-spider", "wolf" }
    local names = {
        "Skreek", "Grix", "Molt", "Vane",
        "Threep", "Korga", "Nix", "Brak"
    }
    local temper = { "wary", "greedy", "lost", "hungry" }
    local a = (entropy % #archetypes) + 1
    local n = ((entropy // 4) % #names) + 1
    local t = ((entropy // 32) % #temper) + 1
    return {
        type = "npc",
        npc = {
            key = archetypes[a],
            name = names[n],
            tags = { "wandering", temper[t] }
        }
    }
end
