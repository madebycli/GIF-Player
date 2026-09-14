--!nonstrict

local catalog = {}
local widgets = {}
local query = ""
local catalogLoading = false
local statusLoading = false
local lastError = ""
local maxVisible = 120

local render
local refreshCatalog
local refreshWidgets

local function configuredGifDir()
  local value = noctalia.getConfig("gif_dir")
  if type(value) == "string" and noctalia.string.trim(value) ~= "" then
    return noctalia.expandPath(value)
  end
  local dataHome = noctalia.getenv("XDG_DATA_HOME")
  if type(dataHome) == "string" and dataHome ~= "" then
    return dataHome .. "/gif-player/gifs"
  end
  return noctalia.expandPath("~/.local/share/gif-player/gifs")
end

local function commandArgs(extra)
  local argv = { "gif-player", "--gif-dir", configuredGifDir() }
  for _, value in ipairs(extra) do
    table.insert(argv, tostring(value))
  end
  return argv
end

local function run(extra, callback, timeoutMs)
  return noctalia.runAsync(commandArgs(extra), callback, timeoutMs or 8000)
end

local function decodeResult(result)
  local text = noctalia.string.trim(result.stdout or "")
  if text == "" then
    return nil, noctalia.string.trim(result.stderr or "")
  end
  return noctalia.json.decode(text)
end

local function setError(message)
  lastError = message or ""
  if render ~= nil then render() end
end

local function launchState()
  local scale = tonumber(noctalia.getConfig("launch_scale")) or 0.7
  local opacity = tonumber(noctalia.getConfig("launch_opacity")) or 1.0
  local speed = tonumber(noctalia.getConfig("launch_speed")) or 1.0
  local jumpRate = tonumber(noctalia.getConfig("launch_jump_rate")) or 6.0
  scale = math.max(0.1, math.min(scale, 5.0))
  opacity = math.max(0.05, math.min(opacity, 1.0))
  speed = math.max(0.1, math.min(speed, 10.0))
  jumpRate = math.max(0.5, math.min(jumpRate, 60.0))
  return {
    scale = scale,
    opacity = opacity,
    speed = speed,
    bouncing = noctalia.getConfig("launch_bounce") == true,
    jumping = noctalia.getConfig("launch_jump") == true,
    jump_rate = jumpRate,
  }
end

local function widgetAction(id, action, value)
  local args = { "ipc", id, action }
  if value ~= nil then table.insert(args, tostring(value)) end
  run(args, function(result)
    local decoded, err = decodeResult(result)
    if decoded == nil then
      setError(err ~= "" and err or "GIF Player command failed")
      return
    end
    if decoded.error ~= nil then
      setError(decoded.error)
      return
    end
    lastError = ""
    refreshWidgets()
  end)
end

local function launchGif(path)
  local encoded, encodeErr = noctalia.json.encode(launchState(), false)
  if encoded == nil then
    setError(encodeErr or "Could not encode launch state")
    return
  end
  run({ "run", path, "--state", encoded }, function(result)
    local decoded, err = decodeResult(result)
    if decoded == nil then
      setError(err ~= "" and err or "GIF Player failed to start")
      return
    end
    if decoded.error ~= nil then
      setError(decoded.error)
      return
    end

    lastError = ""
    if noctalia.getConfig("launch_locked") == false and decoded.id ~= nil then
      widgetAction(tostring(decoded.id), "unlock")
    else
      refreshWidgets()
    end
  end, 12000)
end

local function controlButton(text, glyph, callback, selected)
  return ui.button({
    text = text,
    glyph = glyph,
    controlSize = "sm",
    selected = selected == true,
    onClick = callback,
  })
end

local function widgetCard(status)
  local id = tostring(status.id or "?")
  local file = tostring(status.file or "")
  local scale = tonumber(status.scale) or 1.0
  local opacity = tonumber(status.opacity) or 1.0
  local speed = tonumber(status.speed) or 1.0
  local jumpRate = tonumber(status.jump_rate) or 6.0
  local filename = file:match("([^/]+)$") or file

  return ui.column({
    gap = 8,
    padding = 10,
    fill = "surface_variant/0.35",
    radius = 10,
  }, {
    ui.row({ gap = 8, align = "center" }, {
      ui.label({ text = id, fontWeight = "bold", flexGrow = 1, maxLines = 1 }),
      ui.label({ text = filename, fontSize = 11, color = "on_surface_variant", maxWidth = 260, maxLines = 1 }),
      controlButton(status.locked and "Unlock" or "Lock", status.locked and "lock-open" or "lock", function()
        widgetAction(id, status.locked and "unlock" or "lock")
      end),
      controlButton(status.paused and "Play" or "Pause", status.paused and "player-play" or "player-pause", function()
        widgetAction(id, status.paused and "play" or "pause")
      end),
      controlButton("Close", "x", function() widgetAction(id, "quit") end),
    }),

    ui.row({ gap = 8, align = "center" }, {
      controlButton(status.bouncing and "Stop bounce" or "Bounce", "arrows-random", function()
        widgetAction(id, status.bouncing and "stop-bounce" or "bounce")
      end, status.bouncing),
      controlButton("Hop", "arrow-up", function() widgetAction(id, "hop") end),
      controlButton(status.jumping and "Auto jump off" or "Auto jump", "jump-rope", function()
        widgetAction(id, "jump", status.jumping and "off" or "on")
      end, status.jumping),
      controlButton("Reset", "restore", function() widgetAction(id, "reset") end),
      controlButton("Flip H", "flip-horizontal", function() widgetAction(id, "flip", "toggle-h") end, status.flip_h),
      controlButton("Flip V", "flip-vertical", function() widgetAction(id, "flip", "toggle-v") end, status.flip_v),
    }),

    ui.row({ gap = 8, align = "center" }, {
      ui.label({ text = string.format("Scale %.2fx", scale), width = 90, fontSize = 11 }),
      ui.slider({ min = 0.1, max = 5.0, step = 0.05, value = scale, flexGrow = 1, onDragEnd = function(value)
        widgetAction(id, "scale", value)
      end }),
      ui.label({ text = string.format("Opacity %d%%", math.floor(opacity * 100 + 0.5)), width = 96, fontSize = 11 }),
      ui.slider({ min = 0.05, max = 1.0, step = 0.05, value = opacity, flexGrow = 1, onDragEnd = function(value)
        widgetAction(id, "opacity", value)
      end }),
    }),

    ui.row({ gap = 8, align = "center" }, {
      ui.label({ text = string.format("Speed %.2fx", speed), width = 90, fontSize = 11 }),
      ui.slider({ min = 0.1, max = 10.0, step = 0.1, value = speed, flexGrow = 1, onDragEnd = function(value)
        widgetAction(id, "speed", value)
      end }),
      ui.label({ text = string.format("Jump ~%.1fs", jumpRate), width = 96, fontSize = 11 }),
      ui.slider({ min = 0.5, max = 60.0, step = 0.5, value = jumpRate, flexGrow = 1, onDragEnd = function(value)
        widgetAction(id, "jump-rate", value)
      end }),
    }),

    ui.row({ gap = 6, align = "center" }, {
      ui.label({ text = "Snap", fontSize = 11 }),
      controlButton("TL", nil, function() widgetAction(id, "corner", "tl") end),
      controlButton("TR", nil, function() widgetAction(id, "corner", "tr") end),
      controlButton("Center", nil, function() widgetAction(id, "corner", "center") end),
      controlButton("BL", nil, function() widgetAction(id, "corner", "bl") end),
      controlButton("BR", nil, function() widgetAction(id, "corner", "br") end),
    }),
  })
end

local function matchesQuery(item)
  local needle = string.lower(noctalia.string.trim(query))
  if needle == "" then return true end
  local haystack = string.lower(tostring(item.relative or item.name or item.path or ""))
  return string.find(haystack, needle, 1, true) ~= nil
end

local function catalogRows()
  local rows = {}
  local shown = 0
  for _, item in ipairs(catalog) do
    if matchesQuery(item) then
      shown = shown + 1
      if shown <= maxVisible then
        local path = tostring(item.path or "")
        local relative = tostring(item.relative or item.name or path)
        table.insert(rows, ui.row({ gap = 10, align = "center", padding = 6 }, {
          ui.image({ path = path, width = 48, height = 48, radius = 6, fit = "contain" }),
          ui.label({ text = relative, flexGrow = 1, maxLines = 1 }),
          controlButton("Play", "player-play", function() launchGif(path) end),
        }))
      end
    end
  end
  if shown == 0 then
    table.insert(rows, ui.label({ text = catalogLoading and "Loading GIF collection..." or "No matching GIFs", color = "on_surface_variant" }))
  elseif shown > maxVisible then
    table.insert(rows, ui.label({ text = string.format("Showing %d of %d matches. Use search to narrow the list.", maxVisible, shown), color = "on_surface_variant", fontSize = 11 }))
  end
  return rows
end

render = function()
  local children = {
    ui.row({ gap = 8, align = "center" }, {
      ui.label({ text = "GIF Player", fontSize = 20, fontWeight = "bold", flexGrow = 1 }),
      controlButton("Edit all", "edit", function()
        run({ "edit" }, function(_) refreshWidgets() end)
      end),
      controlButton("Lock all", "lock", function()
        run({ "lock" }, function(_) refreshWidgets() end)
      end),
      controlButton("Stop all", "player-stop", function()
        run({ "stop-all" }, function(_) refreshWidgets() end)
      end),
      controlButton("Settings", "settings", function() noctalia.openSettings() end),
    }),
  }

  if lastError ~= "" then
    table.insert(children, ui.box({ fill = "error_container", radius = 8, padding = 8 }, {
      ui.label({ text = lastError, color = "on_error_container", maxLines = 3 }),
    }))
  end

  table.insert(children, ui.label({ text = "Active overlays", fontWeight = "bold" }))
  if #widgets == 0 then
    table.insert(children, ui.label({ text = statusLoading and "Checking daemon..." or "No overlays running", color = "on_surface_variant" }))
  else
    for _, status in ipairs(widgets) do
      table.insert(children, widgetCard(status))
    end
  end

  table.insert(children, ui.separator({ spacing = 6 }))
  table.insert(children, ui.row({ gap = 8, align = "center" }, {
    ui.label({ text = "GIF collection", fontWeight = "bold", flexGrow = 1 }),
    ui.label({ text = configuredGifDir(), fontSize = 10, color = "on_surface_variant", maxWidth = 360, maxLines = 1 }),
    controlButton("Refresh", "refresh", function() refreshCatalog() end),
  }))
  table.insert(children, ui.input({
    placeholder = "Search GIFs...",
    value = query,
    onChange = function(value)
      query = tostring(value or "")
      render()
    end,
  }))
  table.insert(children, ui.scroll({ flexGrow = 1, gap = 4 }, catalogRows()))

  panel.render(ui.column({ gap = 10, padding = 4 }, children))
end

refreshCatalog = function()
  if catalogLoading then return end
  catalogLoading = true
  render()
  run({ "catalog" }, function(result)
    catalogLoading = false
    local decoded, err = decodeResult(result)
    if decoded == nil then
      catalog = {}
      setError(err ~= "" and err or "Could not read GIF collection")
      return
    end
    if decoded.error ~= nil then
      catalog = {}
      setError(decoded.error)
      return
    end
    catalog = decoded.gifs or {}
    lastError = ""
    render()
  end, 15000)
end

refreshWidgets = function()
  if statusLoading then return end
  statusLoading = true
  run({ "list", "--json" }, function(result)
    statusLoading = false
    local decoded, _err = decodeResult(result)
    if decoded ~= nil and decoded.ok then
      widgets = decoded.widgets or {}
    else
      widgets = {}
    end
    render()
  end, 5000)
end

function onOpen(_context)
  noctalia.setUpdateInterval(1500)
  render()
  refreshCatalog()
  refreshWidgets()
end

function update()
  refreshWidgets()
end
