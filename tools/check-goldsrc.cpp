// SPDX-License-Identifier: GPL-3.0-or-later
// Run under LD_PRELOAD with fake-goldsrc/client.so; verifies the loader boundary,
// unchanged HUD delivery, truncation handling, reset, disconnect and heartbeat.
#include <assert.h>
#include <dlfcn.h>
#include <fstream>
#include <iostream>
#include <string>
#include <cstring>
#include <cstdlib>
#include <unistd.h>
using Message = int (*)(const char *, int, void *);
static Message health, battery, reset;
static const char *level = "maps/test.bsp";
static int spectator;
static int hook(const char *name, Message callback) {
    if (!strcmp(name, "Health")) health = callback;
    if (!strcmp(name, "Battery")) battery = callback;
    if (!strcmp(name, "ResetHUD")) reset = callback;
    return 91;
}
static const char *get_level() { return level; }
static int get_spectator() { return spectator; }
static std::string feed() {
    std::ifstream f(getenv("G13MAP_HEALTH_FILE"));
    std::string line; std::getline(f, line); return line;
}
static void expect(const char *want) {
    const auto got = feed();
    if (got != want) { std::cerr << "got '" << got << "', want '" << want << "'\n"; exit(1); }
}
int main(int argc, char **argv) {
    if (argc >= 2 && !strcmp(argv[1], "--version")) {
        std::cout << "g13pad-check-goldsrc 1\n"; return 0;
    }
    if (argc == 3 && !strcmp(argv[1], "--lookup")) {
        void *library = dlopen(argv[2], RTLD_NOW | RTLD_LOCAL);
        assert(library);
        auto check = (int (*)())dlsym(library, "check_lookup");
        assert(check && check() == 1);
        std::cout << "Caller-relative RTLD_NEXT and RTLD_DEFAULT passed\n";
        return 0;
    }
    assert(argc >= 2 && argc <= 4);
    char dir[] = "/tmp/g13pad-goldsrc-XXXXXX";
    assert(mkdtemp(dir));
    const std::string path = std::string(dir) + "/health";
    setenv("G13MAP_HEALTH_FILE", path.c_str(), 1);
    void *client = dlopen(argv[1], RTLD_NOW);
    if (!client) { std::cerr << dlerror() << '\n'; return 1; }
    auto initialize = (int (*)(void **, int))dlsym(client, "Initialize");
    auto init = (void (*)())dlsym(client, "HUD_Init");
    auto frame = (void (*)(double))dlsym(client, "HUD_Frame");
    auto shutdown = (void (*)())dlsym(client, "HUD_Shutdown");
    auto calls = (int (*)())dlsym(client, "original_calls");
    if (argc > 2 && !strcmp(argv[2], "table")) {
        void *functions[43] = {};
        auto exports = (void (*)(void *))dlsym(client, "F");
        assert(exports);
        exports(functions);
        initialize = (int (*)(void **, int))functions[0];
        init = (void (*)())functions[1];
        shutdown = (void (*)())functions[26];
        frame = (void (*)(double))functions[33];
    }
    assert(initialize && init && frame && shutdown && calls);
    void *table[100] = {};
    table[18] = (void *)hook; table[74] = (void *)get_level; table[88] = (void *)get_spectator;
    assert(initialize(table, 6) == 0);
    assert(initialize(table, 7) == 1);
    assert(table[18] == (void *)hook);
    init();
    expect("wait ttl 3");
    unsigned char hp[] = {100, 0}, ap[] = {75, 0};
    assert(health("Health", 1, hp) == 1);
    assert(battery("Battery", 2, ap) == 2);
    expect("100/100 shield 75/100 ttl 3");
    assert(battery("Battery", 1, ap) == 3); // malformed data still reaches HUD
    assert(health("Health", 0, nullptr) == 4);
    expect("100/100 shield 75/100 ttl 3");
    hp[0] = 44; hp[1] = 1; health("Health", 2, hp);
    expect("300/100 shield 75/100 ttl 3");
    hp[0] = hp[1] = 255; health("Health", 2, hp);
    expect("0/100 shield 0/100 ttl 3");
    reset("ResetHUD", 0, nullptr); expect("0/100 shield 0/100 ttl 3");
    usleep(550000); frame(0);
    expect("wait ttl 3");
    hp[0] = 87; hp[1] = 0; health("Health", 1, hp);
    expect("87/100 shield 0/100 ttl 3");
    unlink(path.c_str()); usleep(1100000); frame(1);
    expect("87/100 shield 0/100 ttl 3"); // no new messages: TTL kept alive
    spectator = 1; frame(2); expect("wait ttl 3");
    spectator = 0; health("Health", 1, hp);
    level = ""; frame(3); expect("wait ttl 3");
    shutdown(); expect("wait ttl 3");
    assert(calls() == 9);
    unlink(path.c_str()); rmdir(dir);
    std::cout << "GoldSource loader, passthrough, health/armour, reset, spectator, disconnect and heartbeat passed\n";
}
