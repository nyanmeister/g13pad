// SPDX-License-Identifier: GPL-3.0-or-later
// Singleplayer only: the Battery user message hook (armour.cpp).
#ifndef G13_ARMOUR_H
#define G13_ARMOUR_H
namespace g13 {
// Patches the client's DispatchUserMessage; `on_armour` gets every Battery value,
// `on_log` a line per user message (for the hunt log; may be null). Returns null when
// installed, else why not. Idempotent.
const char *armour_hook_install(void (*on_armour)(int), void (*on_log)(const char *));
}
#endif
