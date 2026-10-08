#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# A Source engine LCD page that asks for every field a g15_dumpplayer dump lists, for
# hunting a value (armour, say) with contrib/source-health's log on. The first item is
# the health line, so the meter keeps working while the page is in. Usage:
#   tools/g15-probe-res.sh console.log > <mod>/custom/g13pad/resource/g15.res
# The dump: run the game with -g15 -condebug, type g15_dumpplayer in its console; the
# fields land in the mod folder's console.log. Only top-level names render (a.b and
# arrays do not), so those are left out.
set -eu
if [ "${1:-}" = --version ]; then printf '%s\n' 'g13pad-g15-probe-res 0.2.32'; exit 0; fi
dump=${1:?usage: g15-probe-res.sh DUMP.log}
printf '"Logitech G-15 Keyboard Layout"\n{\n\t"game"\t\t"g13pad field hunt"\n\t"chatlines"\t"1"\n'
printf '\t"page"\n\t{\n\t\t"titlepage"\t"1"\n'
printf '\t\t"static_text" { "size" "medium" "align" "left" "x" "0" "y" "0" "w" "160" "text" "G13 wait" }\n\t}\n'
printf '\t"page"\n\t{\n\t\t"requiresplayer"\t"1"\n'
printf '\t\t"static_text" { "size" "medium" "align" "left" "x" "0" "y" "0" "w" "160" "text" "G13 %%(localplayer)m_iHealth%% 100 %%(localplayer)m_lifeState%%" }\n'
awk '
  /^\(localplayer\)/       { section = "localplayer"; next }
  /^\(localteam\)/         { section = "localteam"; next }
  /^\(playerresource\)/    { section = ""; next }
  /^\(localplayerweapon\)/ { section = "localplayerweapon"; next }
  /^Other replacements:/   { section = "globals"; next }
  section == "globals" && /^'\''\(/ {
    name = $1; gsub(/^'\''|'\''$/, "", name)
    # The label without its parentheses: the engine strips every (...) group left after
    # the replacements, so "(x)=(x)" would render as "v=v".
    label = name; gsub(/[()]/, "", label)
    printf "\t\t\"static_text\" { \"size\" \"small\" \"align\" \"left\" \"x\" \"0\" \"y\" \"10\" \"w\" \"160\" \"text\" \"global.%s=%s\" }\n", label, name
    next
  }
  section != "" && section != "globals" && $1 ~ /^[A-Za-z_][A-Za-z0-9_]*$/ && !seen[section $1]++ {
    printf "\t\t\"static_text\" { \"size\" \"small\" \"align\" \"left\" \"x\" \"0\" \"y\" \"10\" \"w\" \"160\" \"text\" \"%s.%s=%%(%s)%s%%\" }\n", section, $1, section, $1
  }
' "$dump"
printf '\t}\n}\n'
