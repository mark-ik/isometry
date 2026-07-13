function call_gen(request_json, entropy, request)
    return {
        type = "local_map",
        map = {
            id = "demo:river-cache",
            name = "River Cache-" .. entropy,
            width = 8,
            height = 6,
            default_ground = "grass",
            cells = {
                { col = 3, row = 2, ground = "stone", elevation = 1 },
                { col = 4, row = 2, ground = "stone", elevation = 1 },
                { col = 2, row = 1, prop = "tree" },
                { col = 5, row = 4, prop = "tree" }
            },
            spawn_zones = {
                {
                    id = "party",
                    cells = {
                        { col = 0, row = 2 },
                        { col = 0, row = 3 }
                    }
                }
            },
            transitions = {
                {
                    id = "west-road",
                    at = { col = 7, row = 3 },
                    target_map = "demo:river-region",
                    target_entry = "cache-road"
                }
            },
            encounter_anchors = {
                {
                    id = "cache-guardian",
                    at = { col = 4, row = 3 },
                    tags = { "guardian", "river" }
                }
            }
        }
    }
end
