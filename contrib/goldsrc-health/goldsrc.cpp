// SPDX-License-Identifier: GPL-3.0-or-later
// Native GoldSource client observer. ABI slots are from Valve's APIProxy.h,
// cl_enginefunc_t (interface 7). No SDK implementation is copied here.
// Preload only into a local singleplayer game, never a secured multiplayer client.
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

// Bind functions moved into libc in 2.34 to their old libdl/librt versions.
// Scout carries those versions; the host linker otherwise requests 2.34.
#ifdef __i386__
__asm__(".symver dladdr,dladdr@GLIBC_2.0");
__asm__(".symver dlvsym,dlvsym@GLIBC_2.1");
__asm__(".symver clock_gettime,clock_gettime@GLIBC_2.2");
#else
__asm__(".symver dladdr,dladdr@GLIBC_2.2.5");
__asm__(".symver dlvsym,dlvsym@GLIBC_2.2.5");
__asm__(".symver clock_gettime,clock_gettime@GLIBC_2.2.5");
#endif

#ifndef G13PAD_VERSION
#define G13PAD_VERSION "dev"
#endif
#define EXPORT __attribute__((visibility("default")))
extern "C" EXPORT const char g13pad_goldsrc_version[] = "g13pad-goldsrc " G13PAD_VERSION;

namespace {
using Message = int (*)(const char *, int, void *);
using Hook = int (*)(const char *, Message);
using Lookup = void *(*)(void *, const char *);
using Initialize = int (*)(void **, int);
Initialize original_init;
void (*original_frame)(double);
void (*original_shutdown)();
void (*original_exports)(void *);
Hook original_hook;
const char *(*level_name)();
int (*spectate_only)();
Message health_handler, battery_handler, reset_handler;
int health, armour;
bool known;
char last[128];
long long last_write;
long long zero_until;

long long now_ms() {
    timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t)) return 0;
    return (long long)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

bool write_feed(const char *line) {
    char path[4096];
    const char *override_path = getenv("G13MAP_HEALTH_FILE");
    const char *xdg = getenv("XDG_STATE_HOME"), *home = getenv("HOME");
    int n;
    if (override_path && *override_path) n = snprintf(path, sizeof path, "%s", override_path);
    else if (xdg && *xdg) n = snprintf(path, sizeof path, "%s/g13map/health", xdg);
    else if (home && *home) n = snprintf(path, sizeof path, "%s/.local/state/g13map/health", home);
    else return false;
    if (n <= 0 || (size_t)n >= sizeof path || path[0] != '/') return false;
    for (char *p = path + 1; *p; ++p) {
        if (*p != '/') continue;
        *p = 0;
        int result = mkdir(path, 0700);
        *p = '/';
        if (result && errno != EEXIST) return false;
    }
    char tmp[4200];
    snprintf(tmp, sizeof tmp, "%s.goldsrc-%ld-XXXXXX", path, (long)getpid());
    int fd = mkstemp(tmp);
    if (fd < 0) return false;
    size_t len = strlen(line), written = 0;
    bool ok = true;
    while (written < len) {
        ssize_t count = write(fd, line + written, len - written);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) { ok = false; break; }
        written += (size_t)count;
    }
    if (close(fd)) ok = false;
    if (ok) ok = rename(tmp, path) == 0;
    if (!ok) unlink(tmp);
    return ok;
}

void emit(bool force = false) {
    char line[128];
    long long now = now_ms();
    if (now < zero_until) snprintf(line, sizeof line, "0/100 shield 0/100 ttl 3\n");
    else if (known) snprintf(line, sizeof line, "%d/100 shield %d/100 ttl 3\n", health, armour);
    else snprintf(line, sizeof line, "wait ttl 3\n");
    if (!force && !strcmp(last, line) && now - last_write < 1000) return;
    if (write_feed(line)) { strcpy(last, line); last_write = now; }
}

int on_health(const char *name, int size, void *data) {
    // Vanilla is BYTE; some mods extend it to a little-endian signed SHORT.
    if (data && (size == 1 || size == 2)) {
        const unsigned char *p = (const unsigned char *)data;
        int value = p[0];
        if (size == 2) { value |= p[1] << 8; if (value & 0x8000) value -= 65536; }
        if (value <= 0 && known && health > 0) zero_until = now_ms() + 500;
        health = value < 0 ? 0 : value;
        known = true;
        emit();
    }
    return health_handler ? health_handler(name, size, data) : 0;
}

int on_battery(const char *name, int size, void *data) {
    if (data && size == 2) {
        const unsigned char *p = (const unsigned char *)data;
        int value = p[0] | (p[1] << 8);
        armour = value & 0x8000 ? 0 : value;
        emit();
    }
    return battery_handler ? battery_handler(name, size, data) : 0;
}

int on_reset(const char *name, int size, void *data) {
    known = false; health = armour = 0;
    emit();
    return reset_handler ? reset_handler(name, size, data) : 0;
}

int hook(const char *name, Message callback) {
    Message observer = callback;
    if (name && callback) {
        if (!strcmp(name, "Health")) { health_handler = callback; observer = on_health; }
        else if (!strcmp(name, "Battery")) { battery_handler = callback; observer = on_battery; }
        else if (!strcmp(name, "ResetHUD")) { reset_handler = callback; observer = on_reset; }
    }
    return original_hook(name, observer);
}

int initialize(void **engine, int version) {
    if (!engine || version != 7 || !engine[18]) return original_init(engine, version);
    original_hook = (Hook)engine[18];
    level_name = (const char *(*)())engine[74];
    spectate_only = (int (*)())engine[88];
    health_handler = battery_handler = reset_handler = nullptr;
    known = false; health = armour = 0; last[0] = 0; zero_until = 0;
    // Initialize copies this table into the mod. Restore the engine's table after
    // that copy; only the client's copied registration callback remains wrapped.
    engine[18] = (void *)hook;
    int result = original_init(engine, version);
    engine[18] = (void *)original_hook;
    if (result) { fprintf(stderr, "%s: client initialized\n", g13pad_goldsrc_version); emit(true); }
    return result;
}

void frame(double time) {
    original_frame(time);
    const char *level = level_name ? level_name() : nullptr;
    if (!level || !*level || (spectate_only && spectate_only())) {
        known = false; health = armour = 0;
    }
    emit();
}

void shutdown() {
    if (original_shutdown) original_shutdown();
    known = false; health = armour = 0;
    emit(true);
    // TTL handles abnormal exit; avoid deleting another game's newer feed.
}

void exports(void *table) {
    original_exports(table);
    if (!table) return;
    // Steam clients supply all exports in one call to F, rather than individual
    // dlsym calls. These are cldll_func_t's init, shutdown and frame slots.
    void **functions = (void **)table;
    if (functions[0]) { original_init = (Initialize)functions[0]; functions[0] = (void *)initialize; }
    if (functions[26]) { original_shutdown = (void (*)())functions[26]; functions[26] = (void *)shutdown; }
    if (functions[33]) { original_frame = (void (*)(double))functions[33]; functions[33] = (void *)frame; }
}

bool client_symbol(void *symbol) {
    Dl_info info;
    if (!symbol || !dladdr(symbol, &info) || !info.dli_fname) return false;
    const char *base = strrchr(info.dli_fname, '/');
    return !strcmp(base ? base + 1 : info.dli_fname, "client.so");
}
}

extern "C" EXPORT void *dlsym(void *handle, const char *name) noexcept {
    // Resolve libc's versioned lookup without calling our own dlsym, then use
    // it to find the next UNVERSIONED interposer. Steam's overlay must receive
    // lookups so it can initialize its own loader/close hooks.
#ifdef __i386__
    Lookup real_lookup = (Lookup)dlvsym(RTLD_NEXT, "dlsym", "GLIBC_2.0");
#else
    Lookup real_lookup = (Lookup)dlvsym(RTLD_NEXT, "dlsym", "GLIBC_2.2.5");
#endif
    if (!real_lookup) return nullptr;
    Lookup lookup = (Lookup)real_lookup(RTLD_NEXT, "dlsym");
    if (!lookup) return nullptr;
    // Preserve the caller for every unrelated symbol, even on concrete handles.
    // Keep both another interposer and libc's caller-relative scopes intact.
    if (handle == RTLD_NEXT || handle == RTLD_DEFAULT ||
        (strcmp(name, "F") && strcmp(name, "Initialize") &&
         strcmp(name, "HUD_Frame") && strcmp(name, "HUD_Shutdown")))
        return lookup(handle, name);
    void *symbol = lookup(handle, name);
    if (getenv("G13MAP_GOLDSRC_DEBUG") && (!strcmp(name, "Initialize") || !strcmp(name, "HUD_Frame") || !strcmp(name, "F"))) {
        Dl_info info = {};
        dladdr(symbol, &info);
        fprintf(stderr, "goldsrc lookup %s: %s\n", name, info.dli_fname ? info.dli_fname : "unknown");
    }
    if (!client_symbol(symbol)) return symbol;
    if (!strcmp(name, "F")) { original_exports = (void (*)(void *))symbol; return (void *)exports; }
    if (!strcmp(name, "Initialize")) { original_init = (Initialize)symbol; return (void *)initialize; }
    if (!strcmp(name, "HUD_Frame")) { original_frame = (void (*)(double))symbol; return (void *)frame; }
    if (!strcmp(name, "HUD_Shutdown")) { original_shutdown = (void (*)())symbol; return (void *)shutdown; }
    return symbol;
}
