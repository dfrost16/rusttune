#!/bin/bash

XDG_DATA_HOME=${XDG_DATA_HOME:-$HOME/.local/share}

if [ -d "/opt/system/Tools/PortMaster/" ]; then
  controlfolder="/opt/system/Tools/PortMaster"
elif [ -d "/opt/tools/PortMaster/" ]; then
  controlfolder="/opt/tools/PortMaster"
elif [ -d "$XDG_DATA_HOME/PortMaster/" ]; then
  controlfolder="$XDG_DATA_HOME/PortMaster"
else
  controlfolder="/roms/ports/PortMaster"
fi

source "$controlfolder/control.txt"
[ -f "${controlfolder}/mod_${CFW_NAME}.txt" ] && source "${controlfolder}/mod_${CFW_NAME}.txt"
get_controls

GAMEDIR="/$directory/ports/rusttune"
cd "$GAMEDIR" || exit 1

> "$GAMEDIR/log.txt" && exec > >(tee "$GAMEDIR/log.txt") 2>&1

export controlfolder
export RUSTTUNE_BASE_DIR="$GAMEDIR"
export SDL_GAMECONTROLLERCONFIG="$sdl_controllerconfig"
export SDL_GAMECONTROLLERCONFIG_FILE="$controlfolder/gamecontrollerdb.txt"
export LD_LIBRARY_PATH="/usr/lib:/usr/lib/aarch64-linux-gnu:$LD_LIBRARY_PATH"
export SDL_VIDEO_WAYLAND_WMCLASS="rusttune"
export SDL_APP_ID="rusttune"

chmod +x "$GAMEDIR/rusttune" 2>/dev/null || true
RUN_BIN="$GAMEDIR/rusttune"
if [ ! -x "$RUN_BIN" ]; then
  cp -f "$GAMEDIR/rusttune" /tmp/rusttune
  chmod +x /tmp/rusttune
  RUN_BIN="/tmp/rusttune"
fi

pm_platform_helper "$RUN_BIN"

"$RUN_BIN"

pm_finish
