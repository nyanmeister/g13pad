#!/bin/sh
# Render docs/images/health-meter.gif from the meter's dump test.
#
#   G13MAP_METER_DUMP=DIR cargo test --lib meter::dump -- --ignored
#   tools/health-gif.sh DIR [OUT.gif]
#
# DIR holds demo-NNN.pbm (160x43, one per tick of the demo) and demo.colours (the
# backlight sent on each tick, R G B as the LEDs get it). Each frame is scaled 3x
# and tinted: the panel's background in the backlight's colour, lit pixels 55 % of
# the way from it to white; a dead panel is dark grey.
#
# The LED values are not what a monitor should show. The G13's red LED is weak
# next to its green and blue, so a value that is orange on the glass is nearly
# red on a monitor, and the saturated green of the glass is a sickly lime there
# (asked 2026-10-08). `display` maps each default band colour to how the glass
# looks; a colour not in the table is shown as is.
set -eu
dir=${1:?dump directory}
out=${2:-$(dirname "$0")/../docs/images/health-meter.gif}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

display() {
    # The user's own table (see src/glass.rs) wins when it names the colour.
    glass="${XDG_CONFIG_HOME:-$HOME/.config}/g13map/glass"
    if [ -r "$glass" ]; then
        found=$(awk -v led="$1" '$1" "$2" "$3 == led { print $4, $5, $6; exit }' "$glass")
        if [ -n "$found" ]; then echo "$found"; return; fi
    fi
    case "$1" in
    '0 255 0') echo 0 150 0 ;;      # green on the glass
    '255 48 0') echo 255 128 0 ;;   # orange on the glass
    *) echo "$1" ;;
    esac
}

i=0
while IFS= read -r led; do
    frame=$(printf '%s/demo-%03d.pbm' "$dir" "$i")
    if [ "$led" = '0 0 0' ]; then
        bg='rgb(32,32,32)' lit='rgb(106,106,106)'
    else
        # shellcheck disable=SC2046 # splitting is the point
        set -- $(display "$led")
        bg="rgb($1,$2,$3)"
        lit=$(printf 'rgb(%d,%d,%d)' $(($1 + (255 - $1) * 55 / 100)) \
            $(($2 + (255 - $2) * 55 / 100)) $(($3 + (255 - $3) * 55 / 100)))
    fi
    # P4 reads as black where the panel is dark, white where it is lit.
    magick "$frame" -filter point -resize 300% +level-colors "$bg,$lit" \
        "$(printf '%s/%03d.png' "$work" "$i")"
    i=$((i + 1))
done <"$dir/demo.colours"

magick -delay 10 -loop 0 "$work"/*.png -layers optimize "$out"
echo "$out: $i frames"
