function call_gen(args_json, entropy, request)
    local name = "River Blade"
    if request.locks.culture ~= nil and request.locks.culture.value == "river-clans" then
        name = "River-Clan Blade"
    end
    return {
        type = "item",
        item = {
            template = "demo:river-blade",
            name = name .. "-" .. entropy,
            tags = { "fixture" }
        }
    }
end
