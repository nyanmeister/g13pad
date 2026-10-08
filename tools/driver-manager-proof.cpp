// Exercises the actual manager/reader hotplug lifecycle. All USB and uinput are mocked.
#include "g13.hpp"
#include <atomic>
#include <cassert>
#include <chrono>
#include <cstdarg>
#include <csignal>
#include <iostream>
#include <fstream>
#include <stdexcept>
#include <thread>
#include <unistd.h>

static auto *fake_device = reinterpret_cast<libusb_device *>(0x1234);
static auto *fake_handle = reinterpret_cast<libusb_device_handle *>(0x5678);
static std::atomic<int> references{1}, opens{0}, closes{0}, readers{0}, reads{0};
static const auto owner = std::this_thread::get_id();
static std::thread notifications;

struct ManagerProbe : G13::G13_Manager {
  static void remove() { HotplugCallbackRemove(nullptr, fake_device, LIBUSB_HOTPLUG_EVENT_DEVICE_LEFT, nullptr); }
  static void insert() { HotplugCallbackInsert(nullptr, fake_device, LIBUSB_HOTPLUG_EVENT_DEVICE_ARRIVED, nullptr); }
};

extern "C" int __real_open(const char *, int, ...);
extern "C" int __wrap_open(const char *path, int flags, ...) {
  if (std::string(path) == "/dev/input/uinput" || std::string(path) == "/dev/uinput") return 123456;
  int mode = 0;
  if (flags & O_CREAT) { va_list args; va_start(args, flags); mode = va_arg(args, int); va_end(args); }
  return __real_open(path, flags, mode);
}
extern "C" int __real_access(const char *, int);
extern "C" int __wrap_access(const char *path, int mode) {
  if (std::string(path) == "/dev/input/uinput" || std::string(path) == "/dev/uinput") return 0;
  return __real_access(path, mode);
}
static int initialized_axes = 0;
extern "C" int __wrap_ioctl(int fd, unsigned long request, ...) {
  assert(fd == 123456);
  if (request == UI_ABS_SETUP) {
    va_list args;
    va_start(args, request);
    const auto *axis = va_arg(args, const uinput_abs_setup *);
    va_end(args);
    assert(axis->code == ABS_X || axis->code == ABS_Y);
    assert(axis->absinfo.value == 127 && axis->absinfo.minimum == 0 &&
           axis->absinfo.maximum == 255);
    initialized_axes |= 1 << axis->code;
  } else if (request == UI_DEV_CREATE) {
    assert(initialized_axes == 3 && "both axes must be neutral before discovery");
    initialized_axes = 0;
  }
  return 0;
}
extern "C" ssize_t __real_write(int, const void *, size_t);
extern "C" ssize_t __wrap_write(int fd, const void *data, size_t n) {
  if (fd != 123456) return __real_write(fd, data, n);
  assert(std::this_thread::get_id() == owner);
  return n;
}
extern "C" int libusb_init(libusb_context **ctx) { *ctx = reinterpret_cast<libusb_context *>(0x99); return 0; }
extern "C" void libusb_exit(libusb_context *) { assert(readers == 0); assert(references == 1); }
extern "C" int libusb_set_option(libusb_context *, libusb_option, ...) { return 0; }
extern "C" int libusb_has_capability(uint32_t) { return 1; }
extern "C" libusb_device *libusb_ref_device(libusb_device *dev) { assert(dev == fake_device); ++references; return dev; }
extern "C" void libusb_unref_device(libusb_device *dev) { assert(dev == fake_device); assert(--references >= 1); }
extern "C" int libusb_open(libusb_device *dev, libusb_device_handle **handle) {
  assert(std::this_thread::get_id() == owner);
  assert(dev == fake_device); ++references; ++opens; *handle = fake_handle; return 0;
}
extern "C" int libusb_set_auto_detach_kernel_driver(libusb_device_handle *, int) { return 0; }
extern "C" int libusb_claim_interface(libusb_device_handle *, int) { return 0; }
extern "C" int libusb_release_interface(libusb_device_handle *, int) { assert(readers == 0); return 0; }
extern "C" void libusb_close(libusb_device_handle *) { assert(readers == 0); ++closes; --references; }
extern "C" int libusb_hotplug_register_callback(libusb_context *ctx, int, int flags,
    int, int, int, libusb_hotplug_callback_fn callback, void *data,
    libusb_hotplug_callback_handle *handle) {
  static int next = 1;
  *handle = next++;
  if (flags & LIBUSB_HOTPLUG_ENUMERATE) callback(ctx, fake_device, LIBUSB_HOTPLUG_EVENT_DEVICE_ARRIVED, data);
  return 0;
}
extern "C" void libusb_hotplug_deregister_callback(libusb_context *, libusb_hotplug_callback_handle) {}
extern "C" int libusb_handle_events_timeout_completed(libusb_context *, timeval *, int *) { return 0; }
extern "C" int libusb_control_transfer(libusb_device_handle *, uint8_t, uint8_t,
    uint16_t, uint16_t, unsigned char *, uint16_t n, unsigned int) {
  assert(std::this_thread::get_id() == owner); return n;
}
extern "C" int libusb_interrupt_transfer(libusb_device_handle *, unsigned char endpoint,
    unsigned char *, int n, int *done, unsigned int timeout) {
  if (!(endpoint & LIBUSB_ENDPOINT_IN)) {
    assert(std::this_thread::get_id() == owner); *done = n; return 0;
  }
  ++readers;
  const int turn = ++reads;
  assert(std::this_thread::get_id() != owner);
  std::this_thread::sleep_for(std::chrono::milliseconds(timeout));
  *done = 0;
  if (turn == 1) {
    // Queue on the reader; never close its handle from inside a callback.
    ManagerProbe::remove();
    ManagerProbe::remove();
    ManagerProbe::insert();
    ManagerProbe::insert();
  } else if (turn == 3) {
    // Transfer failure precedes asynchronous unplug notification this time.
    notifications = std::thread([] {
      std::this_thread::sleep_for(std::chrono::milliseconds(20));
      ManagerProbe::remove();
      ManagerProbe::remove();
      ManagerProbe::insert();
      ManagerProbe::insert();
    });
    --readers;
    return LIBUSB_ERROR_NO_DEVICE;
  } else if (turn >= 5) {
    kill(getpid(), SIGTERM);
  }
  --readers;
  return LIBUSB_ERROR_TIMEOUT;
}

int main() {
  log4cpp::Category::getRoot().setPriority(log4cpp::Priority::CRIT);
  char directory[] = "/tmp/g13-manager-proof-XXXXXX";
  assert(mkdtemp(directory));
  const auto input = std::string(directory) + "/input";
  const auto output = std::string(directory) + "/output";
  auto *manager = G13::G13_Manager::Instance();
  manager->setStringConfigValue("pipe_in", input);
  manager->setStringConfigValue("pipe_out", output);
  assert(manager->Run() == EXIT_SUCCESS);
  notifications.join();
  assert(opens == 3 && closes == 3);
  assert(references == 1 && readers == 0);
  assert(access(input.c_str(), F_OK) != 0 && access(output.c_str(), F_OK) != 0);
  for (const auto &blocked : {input, output}) {
    { std::ofstream file(blocked); file << "preserved"; }
    try {
      manager->Run();
      assert(false && "unsafe command FIFO was accepted");
    } catch (const std::runtime_error &) {}
    assert(readers == 0 && opens == closes && references == 1);
    std::ifstream file(blocked); std::string text; file >> text;
    assert(text == "preserved");
    assert(unlink(blocked.c_str()) == 0);
    assert(access(input.c_str(), F_OK) != 0 && access(output.c_str(), F_OK) != 0);
  }
  assert(rmdir(directory) == 0);
  std::cout << "manager: enumerate, reader-thread unplug/replug, late unplug, duplicate notifications, SIGTERM passed\n";
  std::cout << "all USB readers joined before handle closure; FIFO cleanup and reference counts passed\n";
  std::cout << "failed input/output FIFO setup preserves foreign paths and releases resources\n";
}
