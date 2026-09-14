--!nonstrict
-- GIF picker with the original feature model expressed in Noctalia's native UI.
-- Rendering stays in the external daemon; this panel is a thin IPC client.

local PANEL_WIDTH = 920
local PANEL_PADDING = 28
local COLUMNS = 4
local COLUMN_GAP = 12
local ROW_GAP = 12
local PAGE_SIZE = 24
local TILE_RADIUS = 12

local catalog = {}
local filtered = {}
local profiles = {}
local widgets = {}
local query = ""
local category = ""
local page = 1
local loadingCatalog = false
local loadingProfiles = false
local errorMessage = ""
local hoveredPath = nil
local profileCreateOpen = false
local profileCreateName = ""
local renameTarget = nil
local renameName = ""

local render

local function tr(key)
  return noctalia.tr(key)
end

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

local function command(extra, callback, timeoutMs, withGifDir)
  local argv = { "gif-player" }
  if withGifDir == true then
    table.insert(argv, "--gif-dir")
    table.insert(argv, configuredGifDir())
  end
  for _, value in ipairs(extra) do
    table.insert(argv, tostring(value))
  end
  return noctalia.runAsync(argv, callback, timeoutMs or 8000)
end

local function decodeResult(result)
  local stdout = noctalia.string.trim(result.stdout or "")
  if stdout == "" then
    local stderr = noctalia.string.trim(result.stderr or "")
    return nil, stderr ~= "" and stderr or tr("common.command_failed")
  end
  local decoded, err = noctalia.json.decode(stdout)
  if decoded == nil then return nil, err end
  if type(decoded) == "table" and decoded.error ~= nil then
    return nil, tostring(decoded.error)
  end
  return decoded, nil
end

local function launchState()
  local scale = math.max(0.1, math.min(tonumber(noctalia.getConfig("launch_scale")) or 0.7, 5.0))
  local opacity = math.max(0.05, math.min(tonumber(noctalia.getConfig("launch_opacity")) or 1.0, 1.0))
  local speed = math.max(0.1, math.min(tonumber(noctalia.getConfig("launch_speed")) or 1.0, 10.0))
  local jumpRate = math.max(0.5, math.min(tonumber(noctalia.getConfig("launch_jump_rate")) or 6.0, 60.0))
  return {
    scale = scale,
    opacity = opacity,
    speed = speed,
    bouncing = noctalia.getConfig("launch_bounce") == true,
    jumping = noctalia.getConfig("launch_jump") == true,
    jump_rate = jumpRate,
  }
end

local function runningCount(path)
  local count = 0
  for _, status in ipairs(widgets) do
    if tostring(status.file or "") == path then count += 1 end
  end
  return count
end

local function refreshFilter()
  filtered = {}
  local needle = noctalia.string.trim(query)
  for _, item in ipairs(catalog) do
    local itemCategory = tostring(item.category or "")
    local categoryOk = category == "" or itemCategory == category
    local searchOk = true
    if needle ~= "" then
      local haystack = tostring(item.relative or item.name or item.path or "")
      searchOk = noctalia.fuzzyScore(needle, haystack) ~= nil
    end
    if categoryOk and searchOk then table.insert(filtered, item) end
  end
  local maxPage = math.max(1, math.ceil(#filtered / PAGE_SIZE))
  if page > maxPage then page = maxPage end
end

local function categories()
  local seen = {}
  local result = { tr("picker.all_categories") }
  for _, item in ipairs(catalog) do
    local value = tostring(item.category or "")
    if value ~= "" and not seen[value] then
      seen[value] = true
      table.insert(result, value)
    end
  end
  table.sort(result, function(a, b)
    if a == tr("picker.all_categories") then return true end
    if b == tr("picker.all_categories") then return false end
    return string.lower(a) < string.lower(b)
  end)
  return result
end

local function selectedCategoryIndex(options)
  if category == "" then return 0 end
  for index, label in ipairs(options) do
    if label == category then return index - 1 end
  end
  return 0
end

local function loadCatalog()
  if loadingCatalog then return end
  loadingCatalog = true
  render()
  command({ "catalog" }, function(result)
    loadingCatalog = false
    local decoded, err = decodeResult(result)
    if decoded == nil then
      catalog = {}
      errorMessage = err or tr("picker.catalog_failed")
      refreshFilter()
      render()
      return
    end
    catalog = type(decoded.gifs) == "table" and decoded.gifs or {}
    errorMessage = ""
    refreshFilter()
    render()
  end, 12000, true)
end

local function loadProfiles()
  if loadingProfiles then return end
  loadingProfiles = true
  command({ "profiles", "list" }, function(result)
    loadingProfiles = false
    local decoded, err = decodeResult(result)
    if decoded == nil then
      profiles = {}
      if errorMessage == "" then errorMessage = err or tr("picker.profiles_failed") end
      render()
      return
    end
    profiles = type(decoded.profiles) == "table" and decoded.profiles or {}
    render()
  end, 5000, false)
end

local function runProfile(args, successMessage)
  command(args, function(result)
    local decoded, err = decodeResult(result)
    if decoded == nil then
      errorMessage = err or tr("common.command_failed")
      render()
      return
    end
    errorMessage = ""
    if successMessage ~= nil then noctalia.notify(tr("title"), successMessage) end
    loadProfiles()
  end, 15000, false)
end

local function launchGif(path)
  local stateJson, err = noctalia.json.encode(launchState())
  if stateJson == nil then
    errorMessage = err or tr("common.command_failed")
    render()
    return
  end

  command({ "run", path, "--state", stateJson }, function(result)
    local decoded, decodeErr = decodeResult(result)
    if decoded == nil then
      errorMessage = decodeErr or tr("picker.launch_failed")
      render()
      return
    end

    errorMessage = ""
    if noctalia.getConfig("launch_locked") == false and decoded.id ~= nil then
      command({ "ipc", tostring(decoded.id), "unlock" }, function(_unlockResult) end, 5000, false)
    end
    render()
  end, 12000, true)
end

local function totalPages()
  return math.max(1, math.ceil(#filtered / PAGE_SIZE))
end

local function columnWidth()
  return math.floor(((PANEL_WIDTH - PANEL_PADDING) - COLUMN_GAP * (COLUMNS - 1)) / COLUMNS)
end

local function tilePreviewHeight()
  return math.floor(columnWidth() * 0.72)
end

local function gifTile(item)
  local path = tostring(item.path or "")
  local label = tostring(item.name or item.relative or path)
  local count = runningCount(path)
  local hovered = hoveredPath == path
  local borderColor = count > 0 and "primary" or (hovered and "outline_variant" or "outline")
  local borderWidth = count > 0 and 3 or 1

  local preview = ui.image({
    key = "preview:" .. path,
    path = path,
    width = columnWidth(),
    height = tilePreviewHeight(),
    fit = "contain",
    radius = TILE_RADIUS,
    border = borderColor,
    borderWidth = borderWidth,
    onClick = function() launchGif(path) end,
    onHover = function(state)
      hoveredPath = state == "true" and path or nil
      render()
    end,
  })

  local captionChildren = {
    ui.label({
      text = label,
      fontSize = 11,
      color = count > 0 and "primary" or "on_surface_variant",
      maxLines = 1,
      maxWidth = columnWidth() - 36,
      flexGrow = 1,
      textAlign = "center",
    }),
  }
  if count > 0 then
    table.insert(captionChildren, ui.label({
      text = "×" .. tostring(count),
      fontSize = 10,
      color = "primary",
    }))
  end

  return ui.column({ key = path, gap = 5, flexGrow = 1, align = "stretch" }, {
    preview,
    ui.row({ gap = 4, align = "center", justify = "center" }, captionChildren),
  })
end

local function grid()
  if loadingCatalog and #catalog == 0 then
    return ui.column({ flexGrow = 1, align = "center", justify = "center", gap = 10 }, {
      ui.glyph({ name = "loader-2", size = 32, color = "on_surface_variant" }),
      ui.label({ text = tr("picker.loading"), color = "on_surface_variant" }),
    })
  end
  if #filtered == 0 then
    return ui.column({ flexGrow = 1, align = "center", justify = "center", gap = 8 }, {
      ui.glyph({ name = "photo-off", size = 40, color = "on_surface_variant" }),
      ui.label({ text = tr("picker.empty"), color = "on_surface_variant", textAlign = "center" }),
      ui.label({ text = configuredGifDir(), fontSize = 10, color = "on_surface_variant", textAlign = "center" }),
    })
  end

  local first = (page - 1) * PAGE_SIZE + 1
  local last = math.min(first + PAGE_SIZE - 1, #filtered)
  local rows = {}
  local row = {}
  for index = first, last do
    table.insert(row, gifTile(filtered[index]))
    if #row == COLUMNS or index == last then
      while #row < COLUMNS do table.insert(row, ui.spacer({})) end
      table.insert(rows, ui.row({ gap = COLUMN_GAP, align = "stretch" }, row))
      row = {}
    end
  end
  return ui.column({ gap = ROW_GAP, align = "stretch" }, rows)
end

local function sortedProfileNames()
  local names = {}
  for name, _ in pairs(profiles) do table.insert(names, name) end
  table.sort(names, function(a, b) return string.lower(a) < string.lower(b) end)
  return names
end

local function openProfileMenu(name)
  panel.openContextMenu({
    items = {
      { id = "apply", label = tr("picker.profile_apply") },
      { id = "overwrite", label = tr("picker.profile_overwrite") },
      { kind = "separator" },
      { id = "rename", label = tr("picker.profile_rename") },
      { id = "delete", label = tr("picker.profile_delete") },
    },
    onActivate = "onProfileMenuAction",
    context = name,
  })
end

local function profileRow()
  local children = {
    ui.label({ text = tr("picker.setups"), fontSize = 11, color = "on_surface_variant" }),
  }
  for _, name in ipairs(sortedProfileNames()) do
    local profile = profiles[name]
    local count = type(profile) == "table" and type(profile.widgets) == "table" and #profile.widgets or 0
    table.insert(children, ui.button({
      key = "profile:" .. name,
      text = name,
      variant = "outline",
      controlSize = "sm",
      tooltip = tostring(count) .. " GIF(s)",
      onClick = function() runProfile({ "profiles", "apply", name }, tr("picker.profile_applied")) end,
      onRightClick = function() openProfileMenu(name) end,
    }))
  end
  table.insert(children, ui.button({
    glyph = profileCreateOpen and "x" or "plus",
    variant = "ghost",
    controlSize = "sm",
    tooltip = tr("picker.profile_save"),
    onClick = function()
      profileCreateOpen = not profileCreateOpen
      profileCreateName = ""
      render()
    end,
  }))
  return ui.row({ gap = 6, align = "center" }, children)
end

local function profileEditor()
  if renameTarget ~= nil then
    return ui.row({ gap = 8, align = "center" }, {
      ui.input({
        key = "rename:" .. tostring(renameTarget),
        value = renameTarget,
        placeholder = tr("picker.profile_name"),
        focus = true,
        submitOnEnter = true,
        flexGrow = 1,
        onChange = function(value) renameName = tostring(value or "") end,
        onSubmit = function()
          local name = noctalia.string.trim(renameName ~= "" and renameName or renameTarget)
          if name ~= "" and name ~= renameTarget then
            runProfile({ "profiles", "rename", renameTarget, name }, tr("picker.profile_renamed"))
          end
          renameTarget = nil
          renameName = ""
          render()
        end,
      }),
      ui.button({ text = tr("common.cancel"), variant = "ghost", onClick = function()
        renameTarget = nil
        renameName = ""
        render()
      end }),
    })
  end

  if profileCreateOpen then
    return ui.row({ gap = 8, align = "center" }, {
      ui.input({
        key = "new-profile",
        value = "",
        placeholder = tr("picker.profile_name"),
        focus = true,
        submitOnEnter = true,
        flexGrow = 1,
        onChange = function(value) profileCreateName = tostring(value or "") end,
        onSubmit = function()
          local name = noctalia.string.trim(profileCreateName)
          if name ~= "" then
            runProfile({ "profiles", "save", name }, tr("picker.profile_saved"))
            profileCreateOpen = false
            profileCreateName = ""
            render()
          end
        end,
      }),
      ui.button({ text = tr("common.save"), variant = "primary", onClick = function()
        local name = noctalia.string.trim(profileCreateName)
        if name ~= "" then
          runProfile({ "profiles", "save", name }, tr("picker.profile_saved"))
          profileCreateOpen = false
          profileCreateName = ""
          render()
        end
      end }),
    })
  end
  return nil
end

render = function()
  widgets = noctalia.state.get("widgets") or widgets
  refreshFilter()
  local options = categories()
  local header = ui.row({ gap = 8, align = "center" }, {
    ui.label({ text = tr("picker.title"), fontSize = 16, fontWeight = "bold", flexGrow = 1 }),
    ui.button({ glyph = "adjustments-horizontal", variant = "ghost", tooltip = tr("controls.title"), onClick = function()
      panel.close()
      noctalia.togglePanel("madebycli/gif-player:controls")
    end }),
    ui.button({ glyph = "settings", variant = "ghost", tooltip = tr("common.settings"), onClick = function() noctalia.openSettings() end }),
    ui.button({ glyph = "close", variant = "ghost", onClick = function() panel.close() end }),
  })

  local children = { header, profileRow() }
  local editor = profileEditor()
  if editor ~= nil then table.insert(children, editor) end

  table.insert(children, ui.row({ gap = 8, align = "center" }, {
    ui.input({
      key = "search",
      placeholder = tr("picker.search"),
      focus = true,
      flexGrow = 1,
      onChange = function(value)
        query = tostring(value or "")
        page = 1
        refreshFilter()
        render()
      end,
    }),
    ui.select({
      key = "category",
      options = options,
      selectedIndex = selectedCategoryIndex(options),
      width = 190,
      onChange = function(index, label)
        category = tonumber(index) == 0 and "" or tostring(label or "")
        page = 1
        refreshFilter()
        render()
      end,
    }),
    ui.button({ glyph = "refresh", variant = "ghost", tooltip = tr("common.refresh"), onClick = function()
      loadCatalog()
      loadProfiles()
    end }),
  }))

  if errorMessage ~= "" then
    table.insert(children, ui.box({ fill = "error_container", radius = 10, padding = 10 }, {
      ui.label({ text = errorMessage, color = "on_error_container", maxLines = 3 }),
    }))
  end

  table.insert(children, ui.scroll({ flexGrow = 1, gap = ROW_GAP }, { grid() }))
  table.insert(children, ui.row({ gap = 8, align = "center", justify = "space_between" }, {
    ui.label({ text = tostring(#filtered) .. " / " .. tostring(#catalog), fontSize = 11, color = "on_surface_variant" }),
    ui.row({ gap = 4, align = "center" }, {
      ui.button({ glyph = "chevron-left", variant = "ghost", enabled = page > 1, onClick = function()
        if page > 1 then page -= 1; render() end
      end }),
      ui.label({ text = tostring(page) .. " / " .. tostring(totalPages()), color = "on_surface_variant" }),
      ui.button({ glyph = "chevron-right", variant = "ghost", enabled = page < totalPages(), onClick = function()
        if page < totalPages() then page += 1; render() end
      end }),
    }),
    ui.label({ text = configuredGifDir(), fontSize = 10, color = "on_surface_variant", maxWidth = 360, maxLines = 1 }),
  }))

  panel.render(ui.column({ flexGrow = 1, gap = 12, align = "stretch" }, children))
end

function onProfileMenuAction(actionId, profileName)
  local name = tostring(profileName or "")
  if actionId == "apply" then
    runProfile({ "profiles", "apply", name }, tr("picker.profile_applied"))
  elseif actionId == "overwrite" then
    runProfile({ "profiles", "overwrite", name }, tr("picker.profile_overwritten"))
  elseif actionId == "rename" then
    renameTarget = name
    renameName = name
    profileCreateOpen = false
    render()
  elseif actionId == "delete" then
    runProfile({ "profiles", "delete", name }, tr("picker.profile_deleted"))
  end
end

function onOpen(_context)
  widgets = noctalia.state.get("widgets") or {}
  loadCatalog()
  loadProfiles()
  render()
end

function onClose()
  hoveredPath = nil
end

noctalia.state.watch("widgets", function(value)
  widgets = type(value) == "table" and value or {}
  render()
end)
