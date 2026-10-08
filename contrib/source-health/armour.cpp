// SPDX-License-Identifier: GPL-3.0-or-later
// Singleplayer only: armour from the Battery user message.
//
// The Source 2013 client keeps no armour on the player entity. The server sends a
// "Battery" user message (server/player.cpp: WRITE_SHORT(m_ArmorValue)) and only the
// battery HUD element keeps the number, so no LCD page token can reach it (found
// 2026-10-08 after eleven minutes of play with every field on the page). The engine
// hands every user message to IBaseClientDLL::DispatchUserMessage(type, bf_read&), the
// client's own interface, which this unit reaches through the client library's
// CreateInterface and patches: our function peeks at Battery messages and passes
// everything on untouched.
//
// This is a vtable patch inside the game's process, the technique anti-cheat looks
// for, so it is compiled into the singleplayer module only (g15-sp-*.so). The numbers
// are the 2013 branch's (Half-Life 2 and its episodes): VClient017, slot 36, message 15.
// Another branch needs its own and nothing here guesses.
#include <elf.h>
#include <fcntl.h>
#include <link.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

#include "armour.h"

namespace {

const char CLIENT_INTERFACE[] = "VClient017";
// IBaseClientDLL's virtuals in public/cdll_int.h, counted from Init (0): DispatchUserMessage.
const int DISPATCH_SLOT = 36;
// game/shared/hl2/hl2_usermessages.cpp registration order: "Battery", a short.
const int BATTERY_MESSAGE = 15;

// public/tier1/bitbuf.h bf_read, no vtable: the data pointer, byte count, bit count, cursor.
struct BitRead {
    const unsigned char *data;
    int bytes;
    int bits;
    int cur_bit;
};

typedef bool (*dispatch_fn)(void *self, int type, BitRead *msg);
typedef void *(*factory_fn)(const char *name, int *code);

dispatch_fn g_original = 0;
void (*g_on_armour)(int) = 0;
void (*g_on_log)(const char *) = 0;

// `count` bits at the cursor as the engine would read them: a little-endian bit stream.
bool peek(const BitRead *m, int count, unsigned *out) {
    if (!m || !m->data || count <= 0 || count > 32) return false;
    long start = m->cur_bit;
    if (start < 0 || start + count > (long)m->bits) return false;
    unsigned long long v = 0;
    long first = start / 8;
    long last = (start + count - 1) / 8;
    for (long b = last; b >= first; --b) v = (v << 8) | m->data[b];
    *out = (unsigned)((v >> (start % 8)) & ((1ull << count) - 1));
    return true;
}

bool hooked(void *self, int type, BitRead *msg) {
    unsigned value = 0;
    bool have = msg && peek(msg, 16, &value);
    if (g_on_log) {
        char line[96];
        snprintf(line, sizeof line, "type=%d bits=%d first16=%u", type,
                 msg ? msg->bits - msg->cur_bit : -1, have ? value : 0u);
        g_on_log(line);
    }
    if (type == BATTERY_MESSAGE && have && g_on_armour) g_on_armour((int)(short)value);
    return g_original ? g_original(self, type, msg) : false;
}

// The client library among the loaded objects, and its CreateInterface, found in its
// own dynamic symbol table: no dlopen/dlsym, which on a new glibc would bind this
// module to GLIBC_2.34 and keep it out of Steam's older runtimes.
struct Found {
    factory_fn factory;
    unsigned long lo, hi;   // the library's address range
};

unsigned long gnu_hash(const char *s) {
    unsigned long h = 5381;
    for (; *s; ++s) h = h * 33 + (unsigned char)*s;
    return h & 0xffffffffUL;
}

// CreateInterface in the object described by `info`, via DT_GNU_HASH or DT_HASH.
void *lookup(const dl_phdr_info *info) {
    const ElfW(Dyn) *dyn = 0;
    for (int i = 0; i < info->dlpi_phnum; ++i) {
        const ElfW(Phdr) &ph = info->dlpi_phdr[i];
        if (ph.p_type == PT_DYNAMIC) dyn = (const ElfW(Dyn) *)(info->dlpi_addr + ph.p_vaddr);
    }
    if (!dyn) return 0;
    const ElfW(Sym) *symtab = 0;
    const char *strtab = 0;
    const unsigned *hash = 0, *gnu = 0;
    for (; dyn->d_tag != DT_NULL; ++dyn) {
        // glibc relocates these entries in place on x86; a table below the load
        // address is still a file offset and gets the base added.
        unsigned long ptr = dyn->d_un.d_ptr;
        if (ptr && ptr < info->dlpi_addr) ptr += info->dlpi_addr;
        switch (dyn->d_tag) {
        case DT_SYMTAB: symtab = (const ElfW(Sym) *)ptr; break;
        case DT_STRTAB: strtab = (const char *)ptr; break;
        case DT_HASH: hash = (const unsigned *)ptr; break;
        case DT_GNU_HASH: gnu = (const unsigned *)ptr; break;
        }
    }
    if (!symtab || !strtab) return 0;
    const char name[] = "CreateInterface";
    const ElfW(Sym) *found = 0;
    if (gnu) {
        unsigned nbuckets = gnu[0], symoffset = gnu[1], bloom_size = gnu[2];
        const ElfW(Addr) *bloom = (const ElfW(Addr) *)(gnu + 4);
        const unsigned *buckets = (const unsigned *)(bloom + bloom_size);
        const unsigned *chain = buckets + nbuckets;
        unsigned h = (unsigned)gnu_hash(name);
        unsigned i = buckets[h % nbuckets];
        if (i >= symoffset) {
            for (;; ++i) {
                unsigned c = chain[i - symoffset];
                if ((c | 1) == (h | 1) && strcmp(strtab + symtab[i].st_name, name) == 0) {
                    found = &symtab[i];
                    break;
                }
                if (c & 1) break;
            }
        }
    } else if (hash) {
        unsigned nbuckets = hash[0];
        const unsigned *buckets = hash + 2, *chain = buckets + nbuckets;
        unsigned long h = 0;
        for (const char *s = name; *s; ++s) {
            h = (h << 4) + (unsigned char)*s;
            unsigned long g = h & 0xf0000000UL;
            if (g) h ^= g >> 24;
            h &= ~g;
        }
        for (unsigned i = buckets[h % nbuckets]; i; i = chain[i]) {
            if (strcmp(strtab + symtab[i].st_name, name) == 0) {
                found = &symtab[i];
                break;
            }
        }
    }
    if (!found || found->st_shndx == SHN_UNDEF || !found->st_value) return 0;
    return (void *)(info->dlpi_addr + found->st_value);
}

// The protection of the page holding `addr`, from the process map (PROT_* bits), or -1.
int page_protection(unsigned long addr) {
    int fd = open("/proc/self/maps", O_RDONLY);
    if (fd < 0) return -1;
    // One read of a procfs file returns at most about a page; a game's map runs to
    // hundreds of KB (found 2026-10-08: the package test failed once the harness's own
    // map passed 4 KB and the vtable line fell beyond the first read).
    static char maps[1 << 20];
    ssize_t got = 0;
    for (;;) {
        ssize_t r = read(fd, maps + got, sizeof maps - 1 - got);
        if (r <= 0) break;
        got += r;
        if (got >= (ssize_t)sizeof maps - 1) break;
    }
    close(fd);
    if (got <= 0) return -1;
    maps[got] = 0;
    for (char *line = maps; line && *line;) {
        char *nl = strchr(line, '\n');
        if (nl) *nl = 0;
        unsigned long a = 0, b = 0;
        const char *s = line;
        for (; *s && *s != '-'; ++s) a = a * 16 + (*s <= '9' ? *s - '0' : *s - 'a' + 10);
        if (*s == '-') ++s;
        for (; *s && *s != ' '; ++s) b = b * 16 + (*s <= '9' ? *s - '0' : *s - 'a' + 10);
        if (addr >= a && addr < b && *s == ' ') {
            const char *perm = s + 1;
            int prot = 0;
            if (perm[0] == 'r') prot |= PROT_READ;
            if (perm[1] == 'w') prot |= PROT_WRITE;
            if (perm[2] == 'x') prot |= PROT_EXEC;
            return prot;
        }
        line = nl ? nl + 1 : 0;
    }
    return -1;
}

int visit(dl_phdr_info *info, size_t, void *data) {
    Found *out = (Found *)data;
    const char *name = info->dlpi_name ? info->dlpi_name : "";
    size_t len = strlen(name);
    if (len < 10 || strcmp(name + len - 10, "/client.so") != 0) return 0;
    void *f = lookup(info);
    if (!f) return 0;
    out->factory = (factory_fn)f;
    out->lo = ~0ul;
    out->hi = 0;
    for (int i = 0; i < info->dlpi_phnum; ++i) {
        const ElfW(Phdr) &ph = info->dlpi_phdr[i];
        if (ph.p_type != PT_LOAD) continue;
        unsigned long a = info->dlpi_addr + ph.p_vaddr, b = a + ph.p_memsz;
        if (a < out->lo) out->lo = a;
        if (b > out->hi) out->hi = b;
    }
    return 1;
}

}  // namespace

namespace g13 {

const char *armour_hook_install(void (*on_armour)(int), void (*on_log)(const char *)) {
    if (g_original) return "already installed";
    Found f = {0, 0, 0};
    if (!dl_iterate_phdr(&visit, &f) || !f.factory) return "no client.so with a CreateInterface in this process";
    unsigned long lo = f.lo, hi = f.hi;
    int code = 1;
    void *client = f.factory(CLIENT_INTERFACE, &code);
    if (!client || code != 0) return "client.so is not VClient017";
    void **vtable = *(void ***)client;
    if (!vtable) return "no vtable";
    unsigned long slot = (unsigned long)vtable[DISPATCH_SLOT];
    if (slot < lo || slot >= hi) return "slot 36 does not point into client.so";
    long page = sysconf(_SC_PAGESIZE);
    void *entry = &vtable[DISPATCH_SLOT];
    void *page_start = (void *)((unsigned long)entry & ~(unsigned long)(page - 1));
    // The vtable sits in relro in a real client; whatever it was, it goes back that way.
    int before = page_protection((unsigned long)entry);
    if (before < 0) return "vtable page not in the process map";
    if (!(before & PROT_WRITE) && mprotect(page_start, page, before | PROT_WRITE) != 0)
        return "vtable page would not open";
    g_on_armour = on_armour;
    g_on_log = on_log;
    g_original = (dispatch_fn)vtable[DISPATCH_SLOT];
    vtable[DISPATCH_SLOT] = (void *)&hooked;
    if (!(before & PROT_WRITE)) mprotect(page_start, page, before);
    return 0;
}

}  // namespace g13
