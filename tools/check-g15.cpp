// SPDX-License-Identifier: GPL-3.0-or-later
// Drives contrib/source-health's g15.so the way the Source client does: dlopen,
// CreateInterface, the IG15 calls, and checks the feed file after each step.
// Usage: check-g15 MODULE.so [FAKE/client.so]   (a fresh XDG_STATE_HOME is made and removed)
// With the fake client (tools/fake-client.cpp) a singleplayer module's Battery hook is
// exercised: the patched slot must see the message, pass it on, and feed the shield.
#include <dlfcn.h>
#include <fcntl.h>
#include <fstream>
#include <iostream>
#include <string>
#include <cstdlib>
#include <cstring>
#include <unistd.h>

typedef void *G15_HANDLE;
struct IG15 {
    virtual void GetLCDSize(int &w, int &h) = 0;
    virtual bool Init(const char *name) = 0;
    virtual void Shutdown() = 0;
    virtual bool IsConnected() = 0;
    virtual G15_HANDLE AddText(int type, int size, int alignment, int maxLengthPixels) = 0;
    virtual G15_HANDLE AddIcon(void *icon, int sizeX, int sizeY) = 0;
    virtual void RemoveAndDestroyObject(G15_HANDLE hObject) = 0;
    virtual int SetText(G15_HANDLE handle, char const *text) = 0;
    virtual int SetOrigin(G15_HANDLE handle, int x, int y) = 0;
    virtual int SetVisible(G15_HANDLE handle, bool visible) = 0;
    virtual bool ButtonTriggered(int button) = 0;
    virtual void UpdateLCD(unsigned int dwTimestamp) = 0;
};

static std::string state;
static int failures = 0;

static std::string feed() {
    std::ifstream f(state + "/g13map/health");
    std::string line;
    std::getline(f, line);
    return line;
}

static void expect(const char *step, const std::string &want) {
    std::string got = feed();
    if (got == want) {
        std::cout << "ok   " << step << ": " << got << "\n";
    } else {
        std::cout << "FAIL " << step << ": got '" << got << "', want '" << want << "'\n";
        ++failures;
    }
}

int main(int argc, char **argv) {
    if (argc == 2 && std::strcmp(argv[1], "--version") == 0) {
        std::cout << "g13pad-check-g15\n";
        return 0;
    }
    if (argc != 2 && argc != 3) {
        std::cerr << "usage: check-g15 MODULE.so [FAKE/client.so]\n";
        return 2;
    }
    void *fake = 0;
    if (argc == 3) {
        fake = dlopen(argv[2], RTLD_NOW | RTLD_GLOBAL);
        if (!fake) { std::cerr << "fake client: " << dlerror() << "\n"; return 2; }
    }
    char tmpl[] = "/tmp/g13pad-g15-XXXXXX";
    const char *dir = mkdtemp(tmpl);
    if (!dir) return 2;
    state = dir;
    setenv("XDG_STATE_HOME", dir, 1);

    void *lib = dlopen(argv[1], RTLD_NOW);
    if (!lib) {
        std::cerr << "dlopen: " << dlerror() << "\n";
        return 1;
    }
    typedef void *(*factory_t)(const char *, int *);
    factory_t factory = (factory_t)dlsym(lib, "CreateInterface");
    if (!factory) {
        std::cerr << "CreateInterface is not exported\n";
        return 1;
    }
    const char *version = (const char *)dlsym(lib, "g13pad_g15_version");
    std::cout << (version ? version : "(no version string)") << "\n";
    if (!version) ++failures;

    int code = 7;
    if (factory("SOMETHING_ELSE", &code) != 0 || code != 1) {
        std::cout << "FAIL an unknown interface must give null and code 1\n";
        ++failures;
    }
    IG15 *lcd = (IG15 *)factory("G15_INTERFACE_VERSION001", &code);
    if (!lcd || code != 0) {
        std::cout << "FAIL the G15 interface was not returned\n";
        return 1;
    }
    int w = 0, h = 0;
    lcd->GetLCDSize(w, h);
    if (w != 160 || h != 43) { std::cout << "FAIL size " << w << "x" << h << "\n"; ++failures; }
    if (!lcd->Init("Half-Life 2") || !lcd->IsConnected()) { std::cout << "FAIL init/connected\n"; ++failures; }
    G15_HANDLE title = lcd->AddText(1, 1, 1, 160);
    G15_HANDLE line = lcd->AddText(1, 1, 1, 160);
    if (!title || !line || title == line) { std::cout << "FAIL handles\n"; ++failures; }
    lcd->SetOrigin(title, 0, 0);
    lcd->SetVisible(title, true);
    lcd->UpdateLCD(1);

    lcd->SetText(title, "G13 wait");
    expect("title page", "wait ttl 5");
    lcd->SetText(line, "G13 1 100 \x02\x01");
    expect("unspawned (CS:S team menu)", "wait ttl 5");
    lcd->SetText(line, "G13 100 100 ");
    expect("alive", "100/100 ttl 5");
    lcd->SetText(line, "G13 70 100 ");
    expect("hurt", "70/100 ttl 5");
    lcd->SetText(line, "G13 70 100 30 ");
    expect("with a shield", "70/100 shield 30 ttl 5");
    lcd->SetText(line, "G13 -5 100 \x01");
    expect("just died (negative health)", "0 ttl 5");
    usleep(1200 * 1000);
    lcd->SetText(line, "G13 1 100 \x02");
    expect("dead a second later", "wait ttl 5");
    lcd->SetText(line, "G13 100 100 ");
    expect("respawned", "100/100 ttl 5");
    lcd->SetText(line, "G13 0 100 ");
    expect("zero health without a life byte", "0/100 ttl 5");
    lcd->SetText(line, "Score/Deaths/Ping");
    expect("someone else's item is ignored", "0/100 ttl 5");
    lcd->SetText(title, "G13 wait");
    expect("back to the menu", "wait ttl 5");
    lcd->RemoveAndDestroyObject(line);
    lcd->Shutdown();
    expect("shutdown", "off");

    // The hunt log: nothing without the file, every changed line with it.
    std::string logpath = state + "/g13map/g15.log";
    if (std::ifstream(logpath)) { std::cout << "FAIL log written without the file\n"; ++failures; }
    std::ofstream(logpath).close();
    lcd->Init("Half-Life 2");
    G15_HANDLE probe = lcd->AddText(1, 0, 1, 160);
    lcd->SetText(probe, "ARMOR=");
    lcd->SetText(probe, "ARMOR=");
    lcd->SetText(probe, "ARMOR=35");
    lcd->SetText(line, "G13 100 100 \x02");
    lcd->Shutdown();
    {
        std::ifstream f(logpath);
        std::string all((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
        // Five lines of ours; a singleplayer build adds one "armour hook" line.
        int lines = 0;
        size_t at = 0;
        while ((at = all.find('\n', at)) != std::string::npos) { ++lines; ++at; }
        if (all.find("armour hook") != std::string::npos) --lines;
        bool ok = all.find("init Half-Life 2") != std::string::npos
            && all.find("ARMOR=\n") != std::string::npos
            && all.find("ARMOR=35\n") != std::string::npos
            && all.find("G13 100 100 \\x02") != std::string::npos
            && all.find("shutdown") != std::string::npos && lines == 5;
        if (ok) std::cout << "ok   hunt log: 5 lines, changes only\n";
        else { std::cout << "FAIL hunt log:\n" << all; ++failures; }
    }

    if (fake) {
        // The singleplayer hook: Init patched slot 36 of the fake client's vtable
        // (the log says so), a Battery message yields the shield, everything is passed on.
        typedef void *(*factory_t)(const char *, int *);
        factory_t cf = (factory_t)dlsym(fake, "CreateInterface");
        int (*calls)() = (int (*)())dlsym(fake, "fake_client_calls");
        int (*last_type)() = (int (*)())dlsym(fake, "fake_client_last_type");
        int c = 1;
        void **object = (void **)cf("VClient017", &c);
        void **vtable = (void **)*object;
        struct BitRead { const unsigned char *data; int bytes; int bits; int cur_bit; };
        typedef bool (*dispatch_t)(void *, int, BitRead *);
        lcd->Init("Half-Life 2");   // installs the hook (idempotent)
        {
            std::ifstream f(logpath);
            std::string all((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
            // The first Init of this run installed it before the log existed.
            if (all.find("armour hook installed") == std::string::npos
                && all.find("armour hook already installed") == std::string::npos) {
                std::cout << "FAIL hook not installed:\n" << all; ++failures;
            }
        }
        dispatch_t patched = (dispatch_t)vtable[36];
        lcd->SetText(line, "G13 100 100 ");
        expect("alive, no shield yet", "100/100 ttl 5");
        // Battery = 42 as a 16-bit little-endian short at bit 0.
        unsigned char payload[4] = {42, 0, 0, 0};
        BitRead m = {payload, 4, 16, 0};
        bool r = patched(object, 15, &m);
        if (!r || calls() != 1 || last_type() != 15) { std::cout << "FAIL not passed on (" << calls() << ", " << last_type() << ")\n"; ++failures; }
        lcd->SetText(line, "G13 100 100 ");
        expect("after a Battery message", "100/100 shield 42 ttl 5");
        // 300 at bit offset 3 inside a longer message: cursor honoured.
        unsigned char shifted[4] = {(unsigned char)((300 << 3) & 0xff), (unsigned char)(300 >> 5), 0, 0};
        BitRead m2 = {shifted, 4, 32, 3};
        patched(object, 15, &m2);
        lcd->SetText(line, "G13 90 100 ");
        expect("a second value at a bit offset", "90/100 shield 300 ttl 5");
        // Another message type changes nothing; the res's own shield wins over the hook's.
        BitRead m3 = {payload, 4, 16, 0};
        patched(object, 7, &m3);
        lcd->SetText(line, "G13 90 100 ");
        expect("other messages ignored", "90/100 shield 300 ttl 5");
        lcd->SetText(line, "G13 90 100 5 ");
        expect("page shield beats hook shield", "90/100 shield 5 ttl 5");
        lcd->SetText(title, "G13 wait");
        lcd->SetText(line, "G13 100 100 ");
        expect("a map change forgets the shield", "100/100 ttl 5");
        if (calls() != 3) { std::cout << "FAIL calls " << calls() << "\n"; ++failures; }
        lcd->Shutdown();
    }

    // The game's own descriptors must survive the module: stdin, stdout, stderr.
    for (int fd = 0; fd < 3; ++fd) {
        if (fcntl(fd, F_GETFD) == -1) { std::cout << "FAIL descriptor " << fd << " was closed\n"; ++failures; }
    }
    dlclose(lib);
    std::string rm = "rm -rf '" + state + "'";
    if (std::system(rm.c_str()) != 0) ++failures;
    std::cout << (failures ? "FAILED\n" : "all checks passed\n");
    return failures ? 1 : 0;
}
