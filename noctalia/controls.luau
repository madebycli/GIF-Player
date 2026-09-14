--!nonstrict
-- Runtime controls for active GIF overlays. Static launch defaults intentionally
-- stay in Settings -> Plugins so this panel mirrors Noctalia's own UX split.

local widgets = noctalia.state.get("widgets") or {}
local errorMessage = ""
local moveEditors = {}
local moveDraft = {}
local render

local function tr(key)
  return noctalia.tr(key)
end

local function command(args, callback, timeoutMs)
  local argv = { "gif-player" }
  for _, value in ipairs(args) do table.insert(argv, tostring(value)) end
  return noctalia.runAsync(argv, callback, timeoutMs or 6000)
end

local function decodeResult(result)
  local stdout = noctalia.string.trim(result.stdout or "")
  if stdout == "" then
    local stderr = noctalia.string.trim(result.stderr or "")
    return nil, stderr ~= "" and stderr or tr("common.command_failed")
  end
  local decoded, err = noctalia.json.decode(stdout)
  if decoded == nil then return nil, err end
  if type(decoded) == "table" and decoded.error ~= nil then return nil, tostring(decoded.error) end
  return decoded, nil
end

local function widgetAction(id, action, value, value2)
  local args = { "ipc", id, action }
  if value ~= nil then table.insert(args, tostring(value)) end
  if value2 ~= nil then table.insert(args, tostring(value2)) end
  command(args, function(result)
    local decoded, err = decodeResult(result)
    if decoded == nil then
      errorMessage = err or tr("common.command_failed")
    else
      errorMessage = ""
    end
    render()
  end)
end

local function globalAction(action)
  command({ action }, function(result)
    local stdout = noctalia.string.trim(result.stdout or "")
    if result.exitCode ~= 0 and stdout == "" then
      errorMessage = noctalia.string.trim(result.stderr or "")
      if errorMessage == "" then errorMessage = tr("common.command_failed") end
    else
      errorMessage = ""
    end
    render()
  end)
end

local function controlButton(text, glyph, onClick, selected, destructive)
  return ui.button({
    text = text,
    glyph = glyph,
    controlSize = "sm",
    variant = destructive and "destructive" or (selected and "primary" or "outline"),
    onClick = onClick,
  })
end

local function sliderRow(label, valueText, minValue, maxValue, step, value, onCommit)
  return ui.row({ gap = 8, align = "center" }, {
    ui.label({ text = label, width = 78, fontSize = 11, color = "on_surface_variant" }),
    ui.slider({
      min = minValue,
      max = maxValue,
      step = step,
      value = value,
      flexGrow = 1,
      controlSize = "sm",
      onDragEnd = function(v) onCommit(tonumber(v) or value) end,
    }),
    ui.label({ text = valueText, width = 58, fontSize = 11, textAlign = "end", color = "on_surface_variant" }),
  })
end

local function moveEditor(status)
  local id = tostring(status.id or "")
  if not moveEditors[id] then return nil end
  local draft = moveDraft[id]
  if type(draft) ~= "table" then
    draft = { x = tostring(status.x or 0), y = tostring(status.y or 0) }
    moveDraft[id] = draft
  end

  return ui.row({ gap = 8, align = "center" }, {
    ui.label({ text = "X", fontSize = 11, color = "on_surface_variant" }),
    ui.input({
      key = "move-x:" .. id,
      value = draft.x,
      width = 100,
      controlSize = "sm",
      onChange = function(v) draft.x = tostring(v or "") end,
    }),
    ui.label({ text = "Y", fontSize = 11, color = "on_surface_variant" }),
    ui.input({
      key = "move-y:" .. id,
      value = draft.y,
      width = 100,
      controlSize = "sm",
      submitOnEnter = true,
      onChange = function(v) draft.y = tostring(v or "") end,
      onSubmit = function()
        local x = tonumber(draft.x)
        local y = tonumber(draft.y)
        if x ~= nil and y ~= nil then
          widgetAction(id, "move", x, y)
          moveEditors[id] = nil
          moveDraft[id] = nil
          render()
        end
      end,
    }),
    ui.button({ text = tr("common.apply"), variant = "primary", controlSize = "sm", onClick = function()
      local x = tonumber(draft.x)
      local y = tonumber(draft.y)
      if x ~= nil and y ~= nil then
        widgetAction(id, "move", x, y)
        moveEditors[id] = nil
        moveDraft[id] = nil
        render()
      end
    end }),
    ui.button({ glyph = "x", variant = "ghost", controlSize = "sm", onClick = function()
      moveEditors[id] = nil
      moveDraft[id] = nil
      render()
    end }),
  })
end

local function widgetCard(status)
  local id = tostring(status.id or "?")
  local file = tostring(status.file or "")
  local filename = file:match("([^/]+)$") or file
  local scale = tonumber(status.scale) or 1.0
  local opacity = tonumber(status.opacity) or 1.0
  local speed = tonumber(status.speed) or 1.0
  local jumpRate = tonumber(status.jump_rate) or 6.0
  local x = tonumber(status.x) or 0
  local y = tonumber(status.y) or 0

  local children = {
    ui.row({ gap = 10, align = "center" }, {
      ui.image({ path = file, width = 54, height = 54, fit = "contain", radius = 8 }),
      ui.column({ gap = 2, flexGrow = 1 }, {
        ui.label({ text = id, fontWeight = "bold", maxLines = 1 }),
        ui.label({ text = filename, fontSize = 10, color = "on_surface_variant", maxLines = 1 }),
        ui.label({ text = string.format("%d, %d · %s", x, y, status.locked and tr("controls.locked") or tr("controls.editing")), fontSize = 10, color = "on_surface_variant" }),
      }),
      controlButton(status.locked and tr("controls.unlock") or tr("controls.lock"), status.locked and "lock-open" or "lock", function()
        widgetAction(id, status.locked and "unlock" or "lock")
      end, not status.locked, false),
      controlButton(status.paused and tr("controls.play") or tr("controls.pause"), status.paused and "player-play" or "player-pause", function()
        widgetAction(id, status.paused and "play" or "pause")
      end, status.paused, false),
      ui.button({ glyph = "trash", variant = "destructive", controlSize = "sm", tooltip = tr("controls.close"), onClick = function()
        widgetAction(id, "quit")
      end }),
    }),

    sliderRow(tr("controls.scale"), string.format("%.2fx", scale), 0.1, 5.0, 0.05, scale, function(v)
      widgetAction(id, "scale", v)
    end),
    sliderRow(tr("controls.opacity"), string.format("%d%%", math.floor(opacity * 100 + 0.5)), 0.05, 1.0, 0.05, opacity, function(v)
      widgetAction(id, "opacity", v)
    end),
    sliderRow(tr("controls.speed"), string.format("%.2fx", speed), 0.1, 10.0, 0.1, speed, function(v)
      widgetAction(id, "speed", v)
    end),
    sliderRow(tr("controls.jump_rate"), string.format("%.1fs", jumpRate), 0.5, 60.0, 0.5, jumpRate, function(v)
      widgetAction(id, "jump-rate", v)
    end),

    ui.row({ gap = 6, align = "center" }, {
      controlButton(status.bouncing and tr("controls.stop_bounce") or tr("controls.bounce"), "arrows-random", function()
        widgetAction(id, status.bouncing and "stop-bounce" or "bounce")
      end, status.bouncing, false),
      controlButton(tr("controls.hop"), "arrow-up", function() widgetAction(id, "hop") end, false, false),
      controlButton(status.jumping and tr("controls.auto_jump_off") or tr("controls.auto_jump"), "jump-rope", function()
        widgetAction(id, "jump")
      end, status.jumping, false),
      controlButton(tr("controls.flip_h"), "flip-horizontal", function() widgetAction(id, "flip", "toggle-h") end, status.flip_h, false),
      controlButton(tr("controls.flip_v"), "flip-vertical", function() widgetAction(id, "flip", "toggle-v") end, status.flip_v, false),
      controlButton(tr("controls.reset"), "restore", function() widgetAction(id, "reset") end, false, false),
    }),

    ui.row({ gap = 6, align = "center" }, {
      ui.label({ text = tr("controls.snap"), fontSize = 11, color = "on_surface_variant" }),
      ui.button({ text = "TL", variant = "ghost", controlSize = "sm", onClick = function() widgetAction(id, "corner", "tl") end }),
      ui.button({ text = "TR", variant = "ghost", controlSize = "sm", onClick = function() widgetAction(id, "corner", "tr") end }),
      ui.button({ text = tr("controls.center"), variant = "ghost", controlSize = "sm", onClick = function() widgetAction(id, "corner", "center") end }),
      ui.button({ text = "BL", variant = "ghost", controlSize = "sm", onClick = function() widgetAction(id, "corner", "bl") end }),
      ui.button({ text = "BR", variant = "ghost", controlSize = "sm", onClick = function() widgetAction(id, "corner", "br") end }),
      ui.spacer({ flexGrow = 1 }),
      ui.button({ text = tr("controls.position"), glyph = "arrows-move", variant = "outline", controlSize = "sm", onClick = function()
        moveEditors[id] = not moveEditors[id]
        if not moveEditors[id] then moveDraft[id] = nil end
        render()
      end }),
    }),
  }

  local editor = moveEditor(status)
  if editor ~= nil then table.insert(children, editor) end

  return ui.column({
    key = id,
    gap = 9,
    padding = 12,
    fill = "surface_variant/0.30",
    radius = 12,
    border = "outline_variant",
    borderWidth = 1,
  }, children)
end

render = function()
  widgets = noctalia.state.get("widgets") or widgets
  local online = noctalia.state.get("daemon_online") == true
  local children = {
    ui.row({ gap = 8, align = "center" }, {
      ui.label({ text = tr("controls.title"), fontSize = 16, fontWeight = "bold", flexGrow = 1 }),
      ui.button({ glyph = "photo-search", variant = "ghost", tooltip = tr("picker.title"), onClick = function()
        panel.close()
        noctalia.togglePanel("madebycli/gif-player:picker")
      end }),
      ui.button({ glyph = "settings", variant = "ghost", tooltip = tr("common.settings"), onClick = function() noctalia.openSettings() end }),
      ui.button({ glyph = "close", variant = "ghost", onClick = function() panel.close() end }),
    }),
    ui.row({ gap = 8, align = "center" }, {
      ui.button({ text = tr("controls.edit_all"), glyph = "edit", variant = "outline", onClick = function() globalAction("edit") end }),
      ui.button({ text = tr("controls.lock_all"), glyph = "lock", variant = "outline", onClick = function() globalAction("lock") end }),
      ui.button({ text = tr("controls.stop_all"), glyph = "player-stop", variant = "destructive", onClick = function() globalAction("stop-all") end }),
      ui.spacer({ flexGrow = 1 }),
      ui.label({ text = online and tr("controls.online") or tr("controls.offline"), color = online and "primary" or "error", fontSize = 11 }),
    }),
  }

  if errorMessage ~= "" then
    table.insert(children, ui.box({ fill = "error_container", radius = 10, padding = 10 }, {
      ui.label({ text = errorMessage, color = "on_error_container", maxLines = 3 }),
    }))
  end

  local cards = {}
  if #widgets == 0 then
    table.insert(cards, ui.column({ flexGrow = 1, align = "center", justify = "center", gap = 8 }, {
      ui.glyph({ name = "photo-off", size = 40, color = "on_surface_variant" }),
      ui.label({ text = tr("controls.empty"), color = "on_surface_variant" }),
      ui.button({ text = tr("controls.open_picker"), glyph = "photo-search", variant = "primary", onClick = function()
        panel.close()
        noctalia.togglePanel("madebycli/gif-player:picker")
      end }),
    }))
  else
    for _, status in ipairs(widgets) do table.insert(cards, widgetCard(status)) end
  end
  table.insert(children, ui.scroll({ flexGrow = 1, gap = 10 }, cards))
  panel.render(ui.column({ flexGrow = 1, gap = 12, align = "stretch" }, children))
end

function onOpen(_context)
  widgets = noctalia.state.get("widgets") or {}
  render()
end

noctalia.state.watch("widgets", function(value)
  widgets = type(value) == "table" and value or {}
  render()
end)

noctalia.state.watch("daemon_online", function(_value)
  render()
end)
