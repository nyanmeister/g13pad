// SPDX-License-Identifier: GPL-3.0-or-later
// ABI stand-in: the real client copies the table, then registers its HUD handlers.
#include <string.h>
using Message = int (*)(const char *, int, void *);
using Hook = int (*)(const char *, Message);
static void *engine[100];
static int calls;
static int message(const char *, int, void *) { return ++calls; }
extern "C" int Initialize(void **table, int version) {
    if (version != 7) return 0;
    memcpy(engine, table, sizeof engine);
    return 1;
}
extern "C" void HUD_Init() {
    Hook hook = (Hook)engine[18];
    hook("Health", message); hook("Battery", message); hook("ResetHUD", message);
}
extern "C" void HUD_Frame(double) {}
extern "C" void HUD_Shutdown() {}
extern "C" int original_calls() { return calls; }
extern "C" void F(void *table) {
    void **functions = (void **)table;
    functions[0] = (void *)Initialize;
    functions[1] = (void *)HUD_Init;
    functions[26] = (void *)HUD_Shutdown;
    functions[33] = (void *)HUD_Frame;
}
