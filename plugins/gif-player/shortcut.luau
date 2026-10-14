--!nonstrict
local function render()
    shortcut.setGlyph("movie")
    shortcut.setText(noctalia.tr("title"))
    shortcut.setActive(noctalia.state.get("connected") == true)
end
noctalia.state.watch("connected", render)
function update() noctalia.setUpdateInterval(60000); render() end
function onClick() noctalia.togglePanel("madebycli/gif-player:picker") end
render()
