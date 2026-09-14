--!nonstrict

local pending = false
local widgets = {}
local lastError = ""

local function run(args, callback)
  local argv = { "gif-player" }
  for _, value in ipairs(args) do
    table.insert(argv, tostring(value))
  end
  return noctalia.runAsync(argv, callback, 5000)
end

local function render()
  barWidget.setGlyph("photo")
  if #widgets > 0 then
    barWidget.setText(tostring(#widgets))
  else
    barWidget.setText("")
  end

  local tooltip = {
    { key = "GIF Player", value = #widgets == 1 and "1 widget" or (tostring(#widgets) .. " widgets") },
  }
  if lastError ~= "" then
    table.insert(tooltip, { key = "Status", value = lastError })
  end
  barWidget.setTooltip(tooltip)
end

local function refresh()
  if pending then return end
  pending = true
  run({ "list", "--json" }, function(result)
    pending = false
    if result.exitCode ~= 0 and noctalia.string.trim(result.stdout or "") == "" then
      widgets = {}
      lastError = "daemon offline"
      render()
      return
    end

    local decoded, err = noctalia.json.decode(result.stdout or "")
    if decoded == nil then
      widgets = {}
      lastError = err or "invalid status"
      render()
      return
    end

    if decoded.ok then
      widgets = decoded.widgets or {}
      lastError = ""
    else
      widgets = {}
      lastError = decoded.error or "daemon offline"
    end
    render()
  end)
end

function update()
  noctalia.setUpdateInterval(2000)
  render()
  refresh()
end

function onClick()
  noctalia.togglePanel("madebycli/gif-player:manager")
end

function onRightClick()
  noctalia.openSettings()
end

function onMiddleClick()
  local anyUnlocked = false
  for _, status in ipairs(widgets) do
    if status.locked == false then
      anyUnlocked = true
      break
    end
  end
  run({ anyUnlocked and "lock" or "edit" }, function(_result)
    refresh()
  end)
end
