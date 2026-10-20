--!nonstrict
-- Effective host settings only: never derive a second palette from the wallpaper.
local M = {}
local viewportWidth, viewportHeight
function M.openViewport()
    viewportWidth, viewportHeight = nil, nil
end
function M.appearance()
    local shell = noctalia.getSetting("shell") or {}
    local a = noctalia.getSetting("accessibility") or {}
    return {shell = shell, ui_scale = a.ui_scale or 1}
end
function M.card(extra)
    local shell = (M.appearance()).shell
    local p = shell.panel or {}
    local opacity = p.transparency_mode == "glass" and 0.65 or p.transparency_mode == "soft" and 0.88 or 1
    local props = {align = "stretch", gap = 12, padding = 14,
        radius = 12 * (shell.corner_radius_scale or 1), fill = "surface_variant/" .. tostring(opacity),
        border = p.borders == false and "outline/0" or "outline", borderWidth = p.borders == false and 0 or 1}
    for k,v in pairs(extra or {}) do props[k] = v end
    return props
end
function M.style()
    local host = M.appearance()
    local shell = host.shell
    local p = shell.panel or {}
    local mode = p.transparency_mode or "solid"
    return {corner_radius_scale=shell.corner_radius_scale or 1, button_borders=shell.button_borders ~= false,
        input_borders=shell.input_borders ~= false, panel_borders=p.borders ~= false,
        panel_border_width=p.border_width or 1, panel_opacity=mode == "glass" and 0.55 or mode == "soft" and 0.80 or 1,
        card_opacity=mode == "glass" and 0.65 or mode == "soft" and 0.88 or 1,
        window_translucent=shell.settings_window_translucent == true,
        show_close_buttons=shell.show_close_buttons ~= false, ui_scale=host.ui_scale}
end
function M.borderColor()
    local p = ((M.appearance()).shell).panel or {}
    local color = p.border_color or "outline"
    return string.sub(color,1,1) == "#" and color or noctalia.getColor(color)
end
function M.dimensions(preferredWidth, preferredHeight)
    local outputs = noctalia.outputs() or {}
    local output = outputs[1]
    local focused = noctalia.focusedOutputName()
    for _, candidate in ipairs(outputs) do if candidate.name == focused then output = candidate; break end end
    local scale = (M.appearance()).ui_scale
    -- outputs() reports logical output coordinates, already accounting for monitor scaling.
    local w = output and output.width or preferredWidth
    local h = output and output.height or preferredHeight
    local panelWidth = math.min(preferredWidth * scale, w - 16)
    local panelHeight = math.min(preferredHeight * scale, h - 16)
    -- Noctalia can retain a smaller configured surface after output enlargement. Keep an
    -- open panel within its opening extent; the next open negotiates the larger surface.
    viewportWidth = math.min(viewportWidth or panelWidth, panelWidth)
    viewportHeight = math.min(viewportHeight or panelHeight, panelHeight)
    return math.max(1, viewportWidth / scale - 28), math.max(1, viewportHeight / scale - 28)
end
return M
