// Exercise the actual driver sources, with USB and input writes mocked.
// No RegisterContext call: no USB discovery, uinput device, or live G13 FIFO.
// USB transfers are stubbed; input_event writes are captured in memory.
#include "g13.hpp"
#include <array>
#include <cassert>
#include <cstring>
#include <cstdlib>
#include <fcntl.h>
#include <iostream>
#include <libevdev-1.0/libevdev/libevdev.h>
#include <unistd.h>
#ifdef G13PAD_FUZZ_BUILD
#include "GIT-VERSION.h"
extern "C" int LLVMFuzzerInitialize(int *argc, char ***argv) {
  if (*argc == 2 && std::strcmp((*argv)[1], "--version") == 0) {
    std::cout << "g13pad driver-fuzz " << GIT_VERSION << '\n';
    std::exit(0);
  }
  return 0;
}
#endif

static constexpr int capture_fd = 123456;
static std::array<bool, KEY_MAX + 1> down{};
static size_t key_events = 0;

extern "C" ssize_t __real_write(int, const void *, size_t);
extern "C" ssize_t __wrap_write(int fd, const void *buf, size_t n) {
  if (fd != capture_fd) return __real_write(fd, buf, n);
  assert(n == sizeof(input_event));
  input_event ev;
  std::memcpy(&ev, buf, n);
  if (ev.type == EV_KEY && ev.code <= KEY_MAX) {
    down[ev.code] = ev.value != 0;
    ++key_events;
  }
  return n;
}

extern "C" int libusb_control_transfer(libusb_device_handle *, uint8_t, uint8_t,
    uint16_t, uint16_t, unsigned char *, uint16_t n, unsigned int) { return n; }
extern "C" int libusb_interrupt_transfer(libusb_device_handle *, unsigned char,
    unsigned char *, int n, int *done, unsigned int) { *done = n; return 0; }
extern "C" int libusb_release_interface(libusb_device_handle *, int) { return 0; }
extern "C" void libusb_close(libusb_device_handle *) {}

class Device : public G13::G13_Device {
public:
  Device() : G13_Device(nullptr, nullptr, nullptr, 0) {
    m_uinput_fid = capture_fd;
    m_input_pipe_fid = m_output_pipe_fid = -1;
  }
  void report(bool held) {
    unsigned char r[8] = {0, 127, 127, 0, 0, 0, 0, 0};
    if (held) r[4] = 2; // G10, bit 9 in the key bitmap
    m_currentProfile->ParseKeys(r);
  }
  void top(bool held) {
    unsigned char r[8] = {0, 127, 127, 0, 0, 0, 0, 0};
    if (held) r[7] = 8;
    m_currentProfile->ParseKeys(r);
  }
  void joystick(unsigned char x, unsigned char y) {
    unsigned char r[8] = {0, x, y, 0, 0, 0, 0, 0};
    stick().ParseJoystick(r);
  }
  void pipe_read(const uint8_t *data, size_t n) {
    int fds[2];
    assert(pipe2(fds, O_NONBLOCK) == 0);
    assert(__real_write(fds[1], data, n) == static_cast<ssize_t>(n));
    m_input_pipe_fid = fds[0];
    ReadCommandsFromPipe();
    close(fds[0]);
    close(fds[1]);
    m_input_pipe_fid = -1;
  }
};

#ifdef G13_AUDIT_PROOF
static void check_zone_commands() {
  const auto no_held_keys = [] { for (bool held : down) assert(!held); };
  down.fill(false);
  Device d;
  d.Command("stickzone add SPARE1");
  d.Command("stickzone add SPARE2");
  d.Command("bind STICK_UP !stickzone add NEW");
  d.Command("stickzone bounds STICK_RIGHT 0 0 1 0.3");
  d.Command("bind STICK_RIGHT KEY_D");
  d.joystick(127, 50);
  assert(d.stick().zone("NEW") && "zone action did not add a zone");
  assert(down[KEY_D] && "growth skipped a later zone");
  d.Command("stickzone bounds NEW 0 0 1 0.3");
  d.Command("bind NEW KEY_A");
  assert(!down[KEY_A]);
  d.joystick(127, 50);
  assert(down[KEY_A] && "new zone did not join the next report");
  d.joystick(127, 127);
  no_held_keys();
  d.Command("stickmode ABSOLUTE");
  for (const char *victim : {"STICK_UP", "STICK_DOWN", "STICK_LEFT"}) {
    Device deleting;
    deleting.Command("stickzone bounds STICK_UP 0 0.7 1 1");
    deleting.Command("bind STICK_UP KEY_W");
    deleting.Command("stickzone bounds STICK_LEFT 0 0.7 1 1");
    deleting.Command("bind STICK_LEFT KEY_A");
    deleting.Command("stickzone bounds STICK_RIGHT 0 0.7 1 1");
    deleting.Command("bind STICK_RIGHT KEY_D");
    const std::string action = std::string("bind STICK_DOWN !stickzone del ") + victim;
    deleting.Command(action.c_str());
    deleting.joystick(127, 205);
    assert(!deleting.stick().zone(victim));
    assert(down[KEY_W] == (std::strcmp(victim, "STICK_UP") != 0));
    assert(down[KEY_A] == (std::strcmp(victim, "STICK_LEFT") != 0));
    assert(down[KEY_D] && "deletion skipped a surviving zone");
    deleting.Command("stickmode ABSOLUTE");
    no_held_keys();
  }
  {
    Device held;
    held.Command("bind STICK_UP KEY_LEFTSHIFT");
    held.joystick(127, 50);
    assert(down[KEY_LEFTSHIFT]);
    held.Command("stickzone del STICK_UP");
    no_held_keys();
    held.joystick(127, 50);
    no_held_keys();
  }
  {
    Device switching;
    switching.Command("bind STICK_UP !stickmode ABSOLUTE");
    switching.Command("stickzone bounds STICK_LEFT 0 0 1 0.3");
    switching.Command("bind STICK_LEFT KEY_LEFTSHIFT");
    switching.joystick(127, 50);
    no_held_keys();
    switching.joystick(127, 50);
    no_held_keys();
  }
  std::cout << "Zone commands grow/delete self/earlier/later zones and switch mode without stranded keys\n";
}

int main() {
  log4cpp::Category::getRoot().setPriority(log4cpp::Priority::CRIT);
  check_zone_commands();
  Device d;
  d.Command("stickmode ABSOLUTE");
  d.Command("bind STICK_LEFT KEY_LEFT");
  d.Command("stickmode KEYS");
  d.joystick(10, 127);
  assert(down[KEY_LEFT] && "stickmode KEYS failed to emit a directional key");
  d.joystick(127, 127);
  assert(!down[KEY_LEFT]);
  d.joystick(10, 127);
  assert(down[KEY_LEFT]);
  d.Command("stickmode ABSOLUTE");
  assert(!down[KEY_LEFT] && "mode exit stranded a directional key");
  const auto absolute_events = key_events;
  d.joystick(10, 127);
  assert(key_events == absolute_events);
  d.Command("stickmode KEYS");
  d.Command("stickmode RELATIVE"); // unsupported: preserve the valid current mode
  d.joystick(10, 127);
  assert(down[KEY_LEFT]);
  d.joystick(127, 127);
  assert(!down[KEY_LEFT]);
  for (const auto &point : std::array<std::array<int, 3>, 4>{{
      {KEY_LEFT, 10, 127}, {KEY_RIGHT, 245, 127},
      {KEY_UP, 127, 50}, {KEY_DOWN, 127, 205}}}) {
    d.joystick(point[1], point[2]);
    assert(down[point[0]] && "parsed KEYS mode did not emit a direction");
    d.joystick(127, 127);
    assert(!down[point[0]]);
  }
  std::cout << "Parsed ABSOLUTE -> KEYS emits/releases all four directions\n";
  d.Command("stickmode ABSOLUTE");
  d.Command("bind STICK_UP KEY_W");
  d.Command("bind STICK_DOWN KEY_S");
  d.Command("bind STICK_LEFT KEY_A");
  d.Command("bind STICK_RIGHT KEY_D");
  d.Command("bind STICK_PAGEUP KEY_RESERVED");
  d.Command("bind STICK_PAGEDOWN KEY_RESERVED");
  d.Command("stickzone bounds STICK_UP 0 0 1 0.3");
  d.Command("stickzone bounds STICK_DOWN 0 0.7 1 1");
  d.Command("stickmode KEYS");
  for (const auto &point : std::array<std::array<int, 2>, 8>{{
      {10, 0}, {127, 0}, {245, 0}, {245, 127},
      {245, 255}, {127, 255}, {10, 255}, {10, 127}}}) {
    d.joystick(point[0], point[1]);
    assert(down[KEY_W] == (point[1] == 0));
    assert(down[KEY_S] == (point[1] == 255));
    assert(down[KEY_A] == (point[0] == 10));
    assert(down[KEY_D] == (point[0] == 245));
    assert(!down[KEY_V]);
    d.joystick(127, 127);
    assert(!down[KEY_W] && !down[KEY_A] && !down[KEY_S] && !down[KEY_D]);
  }
  d.Command("stickmode ABSOLUTE");
  std::cout << "Eight-way WASD reaches extremes/diagonals, releases at centre and emits no V\n";
  d.Command("bind G10 KEY_LEFTCTRL");
  d.report(true);
  assert(down[KEY_LEFTCTRL]);
  d.Command("bind G10 KEY_A"); // DRG -> default while G10 held
  d.report(false);
  std::cout << "G10 Ctrl down, rebind to A, release: Ctrl still down = "
            << down[KEY_LEFTCTRL] << '\n';
  const bool stuck_key = down[KEY_LEFTCTRL];
  d.Command("bind TOP MEXTRA");
  d.top(true);
  assert(down[BTN_EXTRA]);
  d.Command("bind TOP KEY_RESERVED");
  d.top(false);
  assert(!down[BTN_EXTRA]);
  const auto before = key_events;
  d.top(true);
  d.top(false);
  assert(key_events == before && "unbound TOP emitted an event");
  std::cout << "Held analog TOP releases, then KEY_RESERVED emits no events\n";
  down.fill(false);
  d.Command("bind STICK_UP KEY_LEFTSHIFT");
  auto *zone = d.stick().zone("STICK_UP");
  zone->test(G13::G13_ZoneCoord(0.5, 0.2));
  assert(down[KEY_LEFTSHIFT]);
  d.Command("bind STICK_UP KEY_V");
  zone->test(G13::G13_ZoneCoord(0.5, 0.5));
  std::cout << "Stick Shift down, rebind to V, return to center: Shift still down = "
            << down[KEY_LEFTSHIFT] << '\n';
  // Every KEY_* code the virtual keyboard enables resolves by its libevdev name, with
  // or without the prefix, and a code outside that range does not. The editor's key
  // table comes from the same header, so anything it offers binds.
  auto *manager = G13::G13_Manager::Instance();
  int named = 0;
  for (int code = 0; code < G13::UINPUT_KEY_CODES; ++code) {
    const char *name = libevdev_event_code_get_name(EV_KEY, code);
    if (!name || std::strncmp(name, "KEY_", 4) != 0) continue;
    assert(manager->FindInputKeyValue(name) == code);
    assert(manager->FindInputKeyValue(name + 4) == code);
    ++named;
  }
  assert(named >= 240 && "libevdev names fewer keys than the editor's table");
  assert(manager->FindInputKeyValue("KEY_OK") == G13::BAD_KEY_VALUE); // 0x160: not enabled
  assert(manager->FindInputKeyValue("NOSUCHKEY") == G13::BAD_KEY_VALUE);
  d.Command("bind G10 KEY_VOLUMEUP");
  d.report(true);
  assert(down[KEY_VOLUMEUP] && "a media key from the full table did not emit");
  d.report(false);
  assert(!down[KEY_VOLUMEUP]);
  d.Command("");
  d.Command("   ");
  std::cout << named << " libevdev key names bind, KEY_VOLUMEUP among them; blank lines ignored\n";
  return stuck_key || down[KEY_LEFTSHIFT] ? 1 : 0;
}
#else
extern "C" int LLVMFuzzerTestOneInput(const uint8_t *data, size_t n) {
  if (!n || n > 4096) return 0;
  log4cpp::Category::getRoot().setPriority(log4cpp::Priority::CRIT);
  down.fill(false);
  Device d;
#ifdef G13_AUDIT_STATE
  d.Command("bind STICK_LEFT KEY_LEFT");
  d.Command("bind STICK_RIGHT KEY_RIGHT");
  d.Command("bind STICK_UP KEY_UP");
  d.Command("bind STICK_DOWN KEY_DOWN");
  static const char *actions[] = {
      "bind G10 KEY_LEFTCTRL", "bind G10 KEY_A", "bind G10 KEY_LEFTSHIFT",
      "bind G10 KEY_LEFTCTRL+KEY_LEFTALT+KEY_F", "bind G10 KEY_RESERVED",
      "bind G10 KEY_A KEY_B", "bind G10 >M1;"};
  for (size_t i = 0; i < n; ++i) {
    switch (data[i] % 10) {
      case 0: d.report(true); break;
      case 1: d.report(false); break;
      case 2: d.Command(actions[(data[i] / 4) % 7]); break;
      case 3: d.Command(data[i] & 4 ? "profile game" : "profile default"); break;
      case 4: d.Command(data[i] & 8 ? "stickmode KEYS" : "stickmode ABSOLUTE"); break;
      case 5: d.joystick(data[i], data[(i + 1) % n]); break;
      case 6: d.Command(data[i] & 8 ? "bind STICK_UP KEY_UP" : "bind STICK_UP KEY_RESERVED"); break;
      case 7: d.Command(data[i] & 8 ? "bind STICK_UP !stickzone add FUZZ" : "bind STICK_UP !stickzone del STICK_UP"); break;
      case 8: d.Command(data[i] & 8 ? "bind STICK_DOWN !stickzone del STICK_UP" : "bind STICK_DOWN !stickmode ABSOLUTE"); break;
      case 9: d.Command("stickzone add STICK_UP");
              d.Command("stickzone bounds STICK_UP 0 0.1 1 0.3"); break;
    }
  }
  d.report(false);
  d.Command("stickmode ABSOLUTE");
  for (size_t i = 1; i < down.size(); ++i) {
    if (down[i]) {
      std::cerr << "stuck emitted key " << i << '\n';
      std::abort();
    }
  }
#else
  d.pipe_read(data, n);
#endif
  return 0;
}
#endif
