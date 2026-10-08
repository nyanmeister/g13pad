// SPDX-License-Identifier: GPL-3.0-or-later
// Private-display fixture; C XKB headers provide an independent oracle for Rust's FFI.
#include <X11/Xlib.h>
#include <X11/XKBlib.h>
#include <cstdio>
#include <cstring>
int main(int argc, char **argv) {
    if (argc == 2 && !std::strcmp(argv[1], "--version")) {
        std::puts("g13pad-xkb-group fixture 1");
        return 0;
    }
    if (argc != 2 || std::strlen(argv[1]) != 1 || argv[1][0] < '0' || argv[1][0] > '3') return 2;
    Display *display = XOpenDisplay(nullptr);
    if (!display) return 3;
    const unsigned int wanted = argv[1][0] - '0';
    const bool locked = XkbLockGroup(display, XkbUseCoreKbd, wanted);
    XSync(display, False);
    XkbStateRec state{};
    const bool correct = XkbGetState(display, XkbUseCoreKbd, &state) == Success && state.group == wanted;
    XCloseDisplay(display);
    return locked && correct ? 0 : 4;
}
