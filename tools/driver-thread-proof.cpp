// Actual driver + synthetic USB; no discovery, real input device or uinput.
#include "g13.hpp"
#include <array>
#include <atomic>
#include <cassert>
#include <chrono>
#include <condition_variable>
#include <deque>
#include <iostream>
#include <mutex>
#include <poll.h>
#include <thread>
#include <unistd.h>

using Clock = std::chrono::steady_clock;
using Report = std::array<unsigned char, 8>;
static const auto owner = std::this_thread::get_id();
static std::mutex usb_mutex;
static std::condition_variable usb_changed;
static std::deque<Report> usb_reports;
static std::atomic<bool> disconnected{false}, input_waiting{false};
static std::atomic<size_t> acquired{0};
static std::array<bool, KEY_MAX + 1> down{};
static size_t key_events = 0, lcd_frames = 0;

extern "C" ssize_t __real_write(int, const void *, size_t);
extern "C" ssize_t __wrap_write(int fd, const void *buf, size_t n) {
  if (fd != 123456) return __real_write(fd, buf, n);
  assert(std::this_thread::get_id() == owner); // no profile dispatch on the USB reader
  assert(n == sizeof(input_event));
  input_event event;
  std::memcpy(&event, buf, n);
  if (event.type == EV_KEY && event.code <= KEY_MAX) {
    down[event.code] = event.value != 0;
    ++key_events;
  }
  return n;
}
extern "C" int libusb_control_transfer(libusb_device_handle *, uint8_t, uint8_t,
    uint16_t, uint16_t, unsigned char *, uint16_t n, unsigned int) {
  assert(std::this_thread::get_id() == owner);
  return n;
}
extern "C" int libusb_interrupt_transfer(libusb_device_handle *, unsigned char endpoint,
    unsigned char *data, int n, int *done, unsigned int timeout) {
  *done = 0;
  if (!(endpoint & LIBUSB_ENDPOINT_IN)) {
    assert(std::this_thread::get_id() == owner);
    assert(n == 992);
    ++lcd_frames;
    *done = n;
    return 0;
  }
  std::unique_lock<std::mutex> lock(usb_mutex);
  input_waiting.store(true);
  usb_changed.wait_for(lock, std::chrono::milliseconds(timeout), [] {
    return !usb_reports.empty() || disconnected.load();
  });
  input_waiting.store(false);
  if (disconnected.load()) return LIBUSB_ERROR_NO_DEVICE;
  if (usb_reports.empty()) return LIBUSB_ERROR_TIMEOUT;
  const auto report = usb_reports.front();
  usb_reports.pop_front();
  std::copy(report.begin(), report.end(), data);
  *done = report.size();
  ++acquired;
  return 0;
}
extern "C" int libusb_release_interface(libusb_device_handle *, int) { return 0; }
extern "C" void libusb_close(libusb_device_handle *) {}

class Device : public G13::G13_Device {
  int writer = -1;
public:
  Device() : G13_Device(nullptr, nullptr, nullptr, 0) {
    m_uinput_fid = 123456;
    m_output_pipe_fid = -1;
    int fds[2];
    assert(pipe2(fds, O_NONBLOCK | O_CLOEXEC) == 0);
    m_input_pipe_fid = fds[0];
    writer = fds[1];
  }
  ~Device() { close(writer); }
  void pump(int timeout = 100) {
    pollfd fds[] = {{input_wake_fd(), POLLIN, 0}, {command_fd(), POLLIN, 0}};
    assert(poll(fds, 2, timeout) >= 0);
    assert(ProcessInputReports() == 0);
    ReadCommandsFromPipe();
  }
  void frame(unsigned char value) {
    const std::array<unsigned char, 960> data = [value] {
      std::array<unsigned char, 960> bytes{}; bytes.fill(value); return bytes;
    }();
    assert(__real_write(writer, data.data(), data.size()) == 960);
  }
};

static void enqueue(bool held) {
  std::lock_guard<std::mutex> lock(usb_mutex);
  usb_reports.push_back({0, 127, 127, 0, static_cast<unsigned char>(held ? 2 : 0), 0, 0, 0});
  usb_changed.notify_one();
}

template<class Predicate> static void until(Predicate done) {
  const auto deadline = Clock::now() + std::chrono::seconds(2);
  while (!done()) {
    assert(Clock::now() < deadline);
    std::this_thread::sleep_for(std::chrono::milliseconds(1));
  }
}

int main() {
  log4cpp::Category::getRoot().setPriority(log4cpp::Priority::CRIT);
  {
    Device d;
    d.Command("bind G10 KEY_LEFTCTRL");
    d.StartInputReader();
    until([] { return input_waiting.load(); });
    // A frame is serviced while the input reader is blocked in a 100 ms USB wait.
    const auto t0 = Clock::now();
    d.frame(77);
    d.pump();
    const auto latency = std::chrono::duration<double, std::milli>(Clock::now()-t0).count();
    assert(lcd_frames == 1 && latency < 50);
    std::cout << "idle LCD dispatch: " << latency << " ms (< 100 ms input wait)\n";
    enqueue(true);
    while (!down[KEY_LEFTCTRL]) d.pump();
    d.Command("bind G10 KEY_A");
    enqueue(false);
    while (down[KEY_LEFTCTRL]) d.pump();
    assert(!down[KEY_A]);
    std::cout << "held Ctrl -> rebind A -> release: no stranded modifier\n";
    // Flood > queue capacity, retaining every press/release under backpressure.
    d.Command("bind G10 KEY_B");
    const size_t before = key_events;
    for (size_t i = 0; i < 2048; ++i) enqueue(i % 2 == 0);
    const auto deadline = Clock::now() + std::chrono::seconds(5);
    size_t frame_count = lcd_frames;
    while (key_events < before + 2048) {
      assert(Clock::now() < deadline);
      d.frame(88);
      d.pump();
    }
    assert(key_events == before + 2048 && !down[KEY_B]);
    assert(lcd_frames > frame_count);
    std::cout << "2048 alternating key reports delivered in order; LCD serviced during flood\n";
    const auto stop_start = Clock::now();
    d.StopInputReader();
    assert(Clock::now()-stop_start < std::chrono::milliseconds(250));
    d.StartInputReader(); // restart initializes wake fd/queue/failure state
    until([] { return input_waiting.load(); });
    disconnected.store(true);
    usb_changed.notify_all();
    pollfd fd{d.input_wake_fd(), POLLIN, 0};
    assert(poll(&fd, 1, 500) == 1);
    assert(d.ProcessInputReports() == LIBUSB_ERROR_NO_DEVICE);
    d.StopInputReader();
    disconnected.store(false);
    std::cout << "stop/restart and USB disconnect wakeup passed\n";
  }
  {
    // Stop a worker blocked on the full report queue, with no consumer.
    Device d;
    for (size_t i = 0; i < 512; ++i) enqueue(i % 2 == 0);
    const auto first = acquired.load();
    d.StartInputReader();
    until([first] { return acquired.load() >= first + 257; });
    const auto start = Clock::now();
    d.StopInputReader();
    assert(Clock::now()-start < std::chrono::milliseconds(250));
    std::lock_guard<std::mutex> lock(usb_mutex);
    usb_reports.clear();
    std::cout << "full-queue shutdown passed\n";
  }
}
