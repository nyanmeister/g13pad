// Real g13d code, synthetic USB reports: no USB device or uinput is opened.
// Usage: timing FIFO LOG idle|busy SECONDS
#include "g13.hpp"
#include <array>
#include <cassert>
#include <chrono>
#include <fstream>
#include <thread>
#include <poll.h>
#include <unistd.h>

static bool busy;
static std::ofstream frames;
static auto started = std::chrono::steady_clock::now();
extern "C" ssize_t __real_write(int, const void *, size_t);
extern "C" ssize_t __wrap_write(int fd, const void *buf, size_t n) {
  if (fd == 123456) return n;
  return __real_write(fd, buf, n);
}
extern "C" int libusb_control_transfer(libusb_device_handle *, uint8_t, uint8_t,
    uint16_t, uint16_t, unsigned char *, uint16_t n, unsigned int) { return n; }
extern "C" int libusb_interrupt_transfer(libusb_device_handle *, unsigned char endpoint,
    unsigned char *data, int n, int *done, unsigned int timeout) {
  if (endpoint & LIBUSB_ENDPOINT_IN) {
    std::this_thread::sleep_for(std::chrono::milliseconds(busy ? 2 : timeout));
    *done = 0;
    if (!busy) return LIBUSB_ERROR_TIMEOUT;
    const std::array<unsigned char, 8> report{0, 127, 127, 0, 0, 0, 0, 0};
    std::copy(report.begin(), report.end(), data);
    *done = report.size();
    return 0;
  }
  if (n == 992) {
    const auto elapsed = std::chrono::duration<double>(
        std::chrono::steady_clock::now() - started).count();
    frames << elapsed << ' ' << static_cast<unsigned>(data[32]) << '\n';
  }
  *done = n;
  return 0;
}
extern "C" int libusb_release_interface(libusb_device_handle *, int) { return 0; }
extern "C" void libusb_close(libusb_device_handle *) {}

class Device : public G13::G13_Device {
public:
  explicit Device(const char *fifo) : G13_Device(nullptr, nullptr, nullptr, 0) {
    m_uinput_fid = 123456;
    m_output_pipe_fid = -1;
    m_input_pipe_fid = open(fifo, O_RDWR | O_NONBLOCK);
    assert(m_input_pipe_fid >= 0);
  }
};

int main(int argc, char **argv) {
  assert(argc == 5);
  log4cpp::Category::getRoot().setPriority(log4cpp::Priority::CRIT);
  busy = std::string(argv[3]) == "busy";
  frames.open(argv[2]);
  Device device(argv[1]);
#ifdef G13_TIMING_THREADS
  device.StartInputReader();
#endif
  const auto end = started + std::chrono::seconds(std::stoi(argv[4]));
  while (std::chrono::steady_clock::now() < end) {
#ifdef G13_TIMING_THREADS
    pollfd fds[] = {{device.input_wake_fd(), POLLIN, 0}, {device.command_fd(), POLLIN, 0}};
    poll(fds, 2, 100);
    assert(device.ProcessInputReports() == 0);
#else
    device.ReadKeypresses();
#endif
    device.ReadCommandsFromPipe();
  }
}
