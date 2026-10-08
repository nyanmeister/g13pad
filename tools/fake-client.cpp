// SPDX-License-Identifier: GPL-3.0-or-later
// A stand-in for a Source client library, for testing the singleplayer armour hook:
// exports CreateInterface("VClient017") returning an object whose vtable slot 36 is a
// DispatchUserMessage that records what it was called with. Built as client.so.
#include <cstring>

struct BitRead { const unsigned char *data; int bytes; int bits; int cur_bit; };

static int g_calls = 0;
static int g_last_type = -1;

static bool dispatch(void *, int type, BitRead *) {
    ++g_calls;
    g_last_type = type;
    return true;
}
static void stub() {}

// 40 slots; slot 36 is DispatchUserMessage as in public/cdll_int.h (VClient017).
static void *g_vtable[40];
static void **g_object = g_vtable;

extern "C" __attribute__((visibility("default"))) void *CreateInterface(const char *name, int *code) {
    bool ok = name && std::strcmp(name, "VClient017") == 0;
    if (code) *code = ok ? 0 : 1;
    if (!ok) return 0;
    // A fixed singleton, as a real client returns: a patch on its vtable must survive
    // later CreateInterface calls.
    static bool made = false;
    if (!made) {
        for (int i = 0; i < 40; ++i) g_vtable[i] = (void *)&stub;
        g_vtable[36] = (void *)&dispatch;
        made = true;
    }
    return &g_object;
}
extern "C" __attribute__((visibility("default"))) int fake_client_calls() { return g_calls; }
extern "C" __attribute__((visibility("default"))) int fake_client_last_type() { return g_last_type; }
