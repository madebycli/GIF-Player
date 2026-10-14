--!nonstrict
local nonce = 0
local function render()
    local state = noctalia.state.get("snapshot") or {}
    local connected = noctalia.state.get("connected") == true
    local widgets = connected and (state.widgets or {}) or {}
    local playing = false
    for _, widget in ipairs(widgets) do if not widget.paused then playing = true end end
    local nodes = {ui.glyph({name = playing and "movie" or "player-pause", size = 18})}
    if #widgets > 0 then table.insert(nodes, ui.label({text = tostring(#widgets), fontSize = 11, color = "primary"})) end
    barWidget.render(ui.row({gap = 3, align = "center"}, nodes))
    barWidget.setTooltip(noctalia.tr("title") .. " · " .. noctalia.trp("active", #widgets))
end
noctalia.state.watch("snapshot", render)
noctalia.state.watch("connected", render)
function update()
    noctalia.setUpdateInterval(60000)
    render()
end
function onClick()
    noctalia.togglePanel("madebycli/gif-player:control")
end
function onRightClick()
    noctalia.togglePanel("madebycli/gif-player:picker")
end
function onIpc(event, payload)
    if event == "picker" or event == "control" then
        noctalia.togglePanel("madebycli/gif-player:" .. event)
    elseif event == "pause" or event == "play" or event == "lock" or event == "unlock" or event == "show" or event == "hide" then
        nonce += 1
        noctalia.state.set("command", { source = "bar", nonce = nonce, command = { action = event, id = "*" } })
    end
end
render()
