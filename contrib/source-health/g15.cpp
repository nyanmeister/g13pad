// SPDX-License-Identifier: GPL-3.0-or-later
// The Source engine's Logitech LCD module, feeding the g13pad health meter.
//
// A Source 2013 client started with -g15 loads bin/g15.dll (bin/g15.so on Linux, which
// nobody shipped) through its own CreateInterface, reads resource/g15.res, and every
// g15_update_msec (250 ms) renders each text item of the current page with its
// %(localplayer)FIELD% tokens filled in and calls IG15::SetText with the result. The
// g15.res beside this file makes two items:
//     G13 wait                                  the title page: no player entity
//     G13 <health> <max> [<shield>] <life>      the player page
// where <life> is the player's m_lifeState, a character the engine prints raw: empty
// while alive, control bytes otherwise (found 2026-10-08 on CS:S and HL2; CS:S says
// health 1 for a dead or unspawned player, HL2 says 0). This module turns those lines
// into the meter's feed file, ~/.local/state/g13map/health: the state file, because
// Steam's container shares the home directory and not /run/user.
//
// Nothing here may need a glibc newer than the one in Steam's runtimes (sniper 2.31,
// the 32-bit scout older still): no libstdc++, no C23 aliases (sscanf, strtoul), no
// dladdr. tools/check-g15-symbols.sh enforces it; tools/check-g15.cpp drives the module
// through the interface the way the engine does.
//
// Two builds: g15-mp (multiplayer: this file alone, nothing touches the game's own
// code, for VAC-secured games) and g15-sp (singleplayer: plus armour.cpp, which hooks
// the Battery user message for the shield bar).
//
// Field hunting: while ~/.local/state/g13map/g15.log exists, every line the engine
// renders is appended to it as it changes (tools/g15-probe-res.sh makes a page that
// asks for every field a g15_dumpplayer dump lists). Remove the file to stop.
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <unistd.h>

#ifndef G13PAD_VERSION
#define G13PAD_VERSION "dev"
#endif
#ifdef G13_SINGLEPLAYER
#include "armour.h"
#define G13_VARIANT "sp"
#else
#define G13_VARIANT "mp"
#endif

extern "C" __attribute__((visibility("default"))) const char g13pad_g15_version[] =
    "g13pad-g15-" G13_VARIANT " " G13PAD_VERSION;

typedef void *G15_HANDLE;
enum G15ObjectType { G15_SCROLLING_TEXT, G15_STATIC_TEXT, G15_ICON, G15_PROGRESS_BAR, G15_UNKNOWN };
enum G15TextSize { G15_SMALL, G15_MEDIUM, G15_BIG };
#define G15_INTERFACE_VERSION "G15_INTERFACE_VERSION001"

namespace {

// Seconds a feed line stays valid: a closed game is a meter gone within this.
const char FEED_TTL[] = "5";
// An unchanged line is rewritten this often, to keep the ttl alive.
const long KEEP_ALIVE_MS = 2000;
// After a death the feed says 0 this long (the watcher latches its dark flatline from
// one read), then waits until the next spawn.
const long DEATH_HOLD_MS = 1000;

long now_ms() {
    timeval tv;
    gettimeofday(&tv, 0);
    return tv.tv_sec * 1000L + tv.tv_usec / 1000;
}

// $XDG_STATE_HOME/g13map, else $HOME/.local/state/g13map, made if missing.
bool state_dir(char *out, size_t n) {
    const char *xdg = getenv("XDG_STATE_HOME");
    const char *home = getenv("HOME");
    int len;
    if (xdg && *xdg)
        len = snprintf(out, n, "%s/g13map", xdg);
    else if (home && *home)
        len = snprintf(out, n, "%s/.local/state/g13map", home);
    else
        return false;
    if (len <= 0 || (size_t)len >= n) return false;
    // mkdir every prefix; existing ones are fine.
    for (char *p = out + 1; *p; ++p) {
        if (*p != '/') continue;
        *p = 0;
        mkdir(out, 0755);
        *p = '/';
    }
    return mkdir(out, 0755) == 0 || errno == EEXIST;
}

// Writes the feed atomically: beside, then renamed over.
bool write_feed(const char *line) {
    char dir[1024], tmp[1100], path[1100];
    if (!state_dir(dir, sizeof dir)) return false;
    snprintf(path, sizeof path, "%s/health", dir);
    snprintf(tmp, sizeof tmp, "%s/health.new", dir);
    int fd = open(tmp, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) return false;
    size_t len = strlen(line);
    bool ok = write(fd, line, len) == (ssize_t)len && write(fd, "\n", 1) == 1;
    close(fd);
    return ok && rename(tmp, path) == 0;
}

// The hunt log, only while the file already exists (touch it to start, remove it to stop).
int open_log() {
    char dir[1024], path[1100];
    if (!state_dir(dir, sizeof dir)) return -1;
    snprintf(path, sizeof path, "%s/g15.log", dir);
    if (access(path, F_OK) != 0) return -1;
    return open(path, O_WRONLY | O_APPEND);
}

void log_line(int fd, const char *what, const char *text) {
    if (fd < 0) return;
    char buf[1200];
    timeval tv;
    gettimeofday(&tv, 0);
    int n = snprintf(buf, sizeof buf, "%ld.%03ld %s ", (long)tv.tv_sec, (long)tv.tv_usec / 1000, what);
    // Control bytes (a raw m_lifeState) shown as \xNN so the log stays a text file.
    for (const char *p = text; *p && n < (int)sizeof buf - 6; ++p) {
        unsigned char c = (unsigned char)*p;
        if (c < 0x20 || c == 0x7f)
            n += snprintf(buf + n, sizeof buf - n, "\\x%02x", c);
        else
            buf[n++] = (char)c;
    }
    buf[n++] = '\n';
    write(fd, buf, n);
}

bool numeric(const char *s, size_t n) {
    if (n == 0) return false;
    size_t i = (s[0] == '-') ? 1 : 0;
    if (i == n) return false;
    for (; i < n; ++i)
        if (s[i] < '0' || s[i] > '9') return false;
    return true;
}

long to_long(const char *s, size_t n) {
    long v = 0;
    bool neg = n && s[0] == '-';
    for (size_t i = neg ? 1 : 0; i < n && v < 100000000L; ++i) v = v * 10 + (s[i] - '0');
    return neg ? -v : v;
}

struct Reading {
    bool wait, alive, has_shield;
    long health, max, shield;
};

// "G13 wait" or "G13 HEALTH MAX [SHIELD] [LIFE]". Any other text (another item, a page
// of someone's own res) is not a reading.
bool parse(const char *text, Reading &r) {
    r.wait = false; r.alive = true; r.has_shield = false; r.health = 0; r.max = 100; r.shield = 0;
    const char *p = text;
    int index = 0;
    while (*p) {
        while (*p == ' ' || *p == '\t') ++p;
        if (!*p) break;
        const char *start = p;
        while (*p && *p != ' ' && *p != '\t') ++p;
        size_t n = p - start;
        switch (index++) {
        case 0:
            if (n != 3 || memcmp(start, "G13", 3) != 0) return false;
            break;
        case 1:
            if (n == 4 && memcmp(start, "wait", 4) == 0) { r.wait = true; return true; }
            if (!numeric(start, n)) return false;
            r.health = to_long(start, n);
            break;
        case 2:
            if (!numeric(start, n)) return false;
            r.max = to_long(start, n);
            break;
        default:
            if (numeric(start, n)) {
                if (!r.has_shield) { r.has_shield = true; r.shield = to_long(start, n); }
            } else {
                r.alive = false;   // the life state, printed raw: anything but empty
            }
        }
    }
    if (index < 3) return false;
    if (r.max <= 0) r.max = 100;
    if (r.health < 0) r.health = 0;
    if (r.shield < 0) r.shield = 0;
    return true;
}

struct Feeder {
    char last[160];
    long last_ms;
    bool alive_seen;
    long died_ms;
    // The shield from the singleplayer hook, until the next wait (a map change).
    bool hook_shield;
    long shield;

    void reset() { last[0] = 0; last_ms = 0; alive_seen = false; died_ms = -1; hook_shield = false; shield = 0; }

    void set_shield(long value) {
        hook_shield = true;
        shield = value < 0 ? 0 : value;
    }

    void emit(const char *line, bool keep_ttl) {
        char full[160];
        if (keep_ttl)
            snprintf(full, sizeof full, "%s ttl %s", line, FEED_TTL);
        else
            snprintf(full, sizeof full, "%s", line);
        long now = now_ms();
        if (strcmp(full, last) == 0 && now - last_ms < KEEP_ALIVE_MS) return;
        if (write_feed(full)) {
            snprintf(last, sizeof last, "%s", full);
            last_ms = now;
        }
    }

    void text(const char *s) {
        Reading r;
        if (!parse(s, r)) return;
        char line[96];
        if (r.wait) {
            alive_seen = false;
            died_ms = -1;
            hook_shield = false;
            emit("wait", true);
        } else if (r.alive) {
            alive_seen = true;
            died_ms = -1;
            if (r.has_shield)
                snprintf(line, sizeof line, "%ld/%ld shield %ld", r.health, r.max, r.shield);
            else if (hook_shield)
                snprintf(line, sizeof line, "%ld/%ld shield %ld", r.health, r.max, shield);
            else
                snprintf(line, sizeof line, "%ld/%ld", r.health, r.max);
            emit(line, true);
        } else if (alive_seen) {
            // A death: the drop to zero first, then the wait for the next spawn.
            long now = now_ms();
            if (died_ms < 0) died_ms = now;
            emit(now - died_ms < DEATH_HOLD_MS ? "0" : "wait", true);
        } else {
            // Never alive on this page: a team menu, a spectator, a respawn room.
            emit("wait", true);
        }
    }

    void off() {
        emit("off", false);
        reset();
    }
};

// One concrete class carries the engine's IG15 vtable in its declared order (see
// src/public/g15/ig15.h in the Source SDK). An abstract base would need
// __cxa_pure_virtual from the libstdc++ this module does without.
const int MAX_LOGGED = 256;

#ifdef G13_SINGLEPLAYER
void on_armour(int value);
void on_usermsg(const char *line);
#endif

struct Lcd {
    Feeder feeder;
    long handles;
    bool up;
    int log_fd;
    // The last text per handle, so the log carries changes only.
    char seen[MAX_LOGGED][256];

    // A zero-initialised global would read log_fd as 0, and closing that is closing
    // the game's stdin (the 32-bit client then never finished a map load, 2026-10-08).
    Lcd() : handles(0), up(false), log_fd(-1) { feeder.reset(); memset(seen, 0, sizeof seen); }

    void close_log() {
        if (log_fd > 2) close(log_fd);
        log_fd = -1;
    }

    void start() {
        feeder.reset();
        handles = 0;
        up = false;
        close_log();
        memset(seen, 0, sizeof seen);
    }

    virtual void GetLCDSize(int &w, int &h) { w = 160; h = 43; }
    virtual bool Init(const char *name) {
        feeder.reset();
        up = true;
        if (log_fd < 0) log_fd = open_log();
        log_line(log_fd, "init", name ? name : "");
#ifdef G13_SINGLEPLAYER
        const char *why = g13::armour_hook_install(&on_armour, &on_usermsg);
        log_line(log_fd, "armour hook", why ? why : "installed");
#endif
        return true;
    }
    virtual void Shutdown() {
        if (up) feeder.off();
        up = false;
        log_line(log_fd, "shutdown", "");
        close_log();
    }
    virtual bool IsConnected() { return true; }
    virtual G15_HANDLE AddText(G15ObjectType, G15TextSize, int, int) { return (G15_HANDLE)(++handles); }
    virtual G15_HANDLE AddIcon(void *, int, int) { return (G15_HANDLE)(++handles); }
    virtual void RemoveAndDestroyObject(G15_HANDLE) {}
    virtual int SetText(G15_HANDLE handle, const char *text) {
        if (!text) return 0;
        feeder.text(text);
        long i = (long)handle;
        if (log_fd >= 0 && i > 0 && i < MAX_LOGGED && strncmp(seen[i], text, sizeof seen[i] - 1) != 0) {
            snprintf(seen[i], sizeof seen[i], "%s", text);
            char what[24];
            snprintf(what, sizeof what, "text[%ld]", i);
            log_line(log_fd, what, text);
        }
        return 0;
    }
    virtual int SetOrigin(G15_HANDLE, int, int) { return 0; }
    virtual int SetVisible(G15_HANDLE, bool) { return 0; }
    virtual bool ButtonTriggered(int) { return false; }
    virtual void UpdateLCD(unsigned int) {}
};

Lcd g_lcd;

#ifdef G13_SINGLEPLAYER
void on_armour(int value) { g_lcd.feeder.set_shield(value); }
void on_usermsg(const char *line) { log_line(g_lcd.log_fd, "usermsg", line); }
#endif

}  // namespace

extern "C" __attribute__((visibility("default"))) void *CreateInterface(const char *name, int *code) {
    bool ok = name && strcmp(name, G15_INTERFACE_VERSION) == 0;
    if (code) *code = ok ? 0 : 1;
    if (!ok) return 0;
    g_lcd.start();
    return &g_lcd;
}
