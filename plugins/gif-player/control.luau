--!nonstrict
local handlers = require("./panel.luau")(1, "control")
function onOpen(context) handlers.onOpen(context) end
function onClose() handlers.onClose() end
function onIpc(event, payload) handlers.onIpc(event, payload) end
function onFrameTick(deltaMs) handlers.onFrameTick(deltaMs) end
