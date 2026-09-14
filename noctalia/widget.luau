--!nonstrict
-- Noctalia-native bar launcher. The bridge service owns daemon status.

local count = tonumber(noctalia.state.get("widget_count")) or 0
local online = noctalia.state.get("daemon_online") == true

local function render()
  local glyph = noctalia.getConfig("glyph")
  if type(glyph) ~= "string" or glyph == "" then glyph = "photo" end
  local showCount = noctalia.getConfig("show_count") ~= false

  barWidget.setGlyph(glyph)
  barWidget.setText(showCount and count > 0 and tostring(count) or "")
  if online then
    barWidget.setTooltip(count == 1 and "GIF Player · 1 overlay" or ("GIF Player · " .. tostring(count) .. " overlays"))
  else
    barWidget.setTooltip("GIF Player · daemon offline")
  end
end

function update()
  render()
end

function onClick()
  noctalia.togglePanel("madebycli/gif-player:picker")
end

function onRightClick()
  noctalia.togglePanel("madebycli/gif-player:controls")
end

function onMiddleClick()
  noctalia.openSettings()
end

noctalia.state.watch("widget_count", function(value)
  count = tonumber(value) or 0
  render()
end)

noctalia.state.watch("daemon_online", function(value)
  online = value == true
  render()
end)

render()
