function call_gen(request_json, entropy, request)
  return {
    type = "campaign",
    campaign_json = [[
{
  "id":"river-oath","name":"The River Oath",
  "world":{
    "factions":{
      "tide-court":{"id":"tide-court","name":"Tide Court","tags":["river","keepers"],"claims":["ford"]},
      "ash-choir":{"id":"ash-choir","name":"Ash Choir","tags":["fire","invaders"],"claims":["glass-shrine"]}
    },
    "places":{
      "ford":{"id":"ford","name":"Oath Ford","tags":["village","river"],"map":"river-ford"},
      "glass-shrine":{"id":"glass-shrine","name":"Glass Shrine","tags":["ruin","fire"],"map":"glass-shrine-map"},
      "river-region":{"id":"river-region","name":"River March","tags":["region"],"map":"river-region"}
    },
    "characters":{
      "mara":{"id":"mara","name":"Mara Vale","tags":["warden","tide-court"],"faction":"tide-court","place":"ford"}
    },
    "routes":{
      "ford-shrine":{"id":"ford-shrine","from":"ford","to":"glass-shrine","tags":["road","contested"]},
      "region-ford":{"id":"region-ford","from":"river-region","to":"ford","tags":["river"]}
    },
    "laws":{
      "iron-remembers":{"id":"iron-remembers","name":"Iron Remembers","text":"Worked iron keeps the true name of its maker.","tags":["magic","names"],"parameters":{"identified_iron":"reveals_maker"}}
    },
    "history":[
      {"id":"choir-arrives","time":-8,"kind":"invasion","text":"The Ash Choir crossed the dry eastern bed.","participants":["ash-choir","tide-court"],"place":"glass-shrine","tags":["conflict"]},
      {"id":"oath-broken","time":-3,"kind":"schism","text":"A keeper broke the ford oath and hid the witness blade.","participants":["tide-court"],"place":"ford","tags":["secret","oath"]}
    ],
    "storylets":{
      "final-oath":{"key":"final-oath","entry":"At the Glass Shrine, the remembered name decides the river war.","tags":["finale","encounter"],"requirements":{"faction_tags":["river","fire"],"hidden_facts":["witness-blade.secret"],"world_laws":["iron-remembers"]},"roles":[{"key":"warden","tags":["warden"]}],"effects":[{"type":"fact","fact":{"id":"river-oath.restored","kind":"history","text":"The River Oath was spoken again.","tags":["river","oath"]}}]}
    }
  },
  "maps":[
    {"scale":"region","map":{"id":"river-region","name":"River March","width":7,"height":5,"default_ground":"grass","cells":[{"col":1,"row":2,"ground":"water"},{"col":2,"row":2,"ground":"water"},{"col":3,"row":2,"ground":"water"},{"col":4,"row":2,"ground":"water"},{"col":5,"row":2,"ground":"water"}],"spawn_zones":[],"transitions":[{"id":"to-ford","at":{"col":1,"row":1},"target_map":"river-ford","target_entry":"road"},{"id":"to-shrine","at":{"col":5,"row":3},"target_map":"glass-shrine-map","target_entry":"west"}],"encounter_anchors":[]}},
    {"scale":"local","map":{"id":"river-ford","name":"Oath Ford","width":8,"height":6,"default_ground":"grass","cells":[{"col":3,"row":0,"ground":"water"},{"col":3,"row":1,"ground":"water"},{"col":3,"row":2,"ground":"stone"},{"col":3,"row":3,"ground":"stone"},{"col":3,"row":4,"ground":"water"},{"col":3,"row":5,"ground":"water"}],"spawn_zones":[{"id":"party","cells":[{"col":0,"row":2},{"col":0,"row":3}]}],"transitions":[{"id":"to-region","at":{"col":7,"row":2},"target_map":"river-region","target_entry":"ford"}],"encounter_anchors":[{"id":"ford-ambush","at":{"col":4,"row":3},"tags":["ash-choir","skirmish"]}]}},
    {"scale":"local","map":{"id":"glass-shrine-map","name":"Glass Shrine","width":8,"height":6,"default_ground":"ash","cells":[{"col":4,"row":2,"ground":"glass","elevation":2},{"col":4,"row":3,"ground":"glass","elevation":2}],"spawn_zones":[{"id":"party","cells":[{"col":0,"row":2},{"col":0,"row":3}]}],"transitions":[{"id":"to-region","at":{"col":0,"row":5},"target_map":"river-region","target_entry":"shrine"}],"encounter_anchors":[{"id":"final-choir","at":{"col":5,"row":2},"tags":["ash-choir","finale"]}]}}
  ],
  "secrets":[{"id":"witness-blade.secret","text":"The witness blade names Mara's ancestor as the oathbreaker.","tags":["item:witness-blade","tide-court"],"reveal":"Identify"}],
  "rewards":[{"template":"demo:witness-blade","name":"Witness Blade","tags":["weapon","iron","quest"]}],
  "starting_map":"river-region","final_storylet":"final-oath"
}
]]
  }
end
