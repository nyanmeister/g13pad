//
// Created by khampf on 07-05-2020.
//

#include "g13_device.hpp"
#include "g13.hpp"
#include "g13_fonts.hpp"
#include "g13_log.hpp"
#include "g13_manager.hpp"
#include "g13_profile.hpp"
#include "g13_stick.hpp"
#include "logo.hpp"
#include <fstream>
#include <unistd.h>
#include <sys/eventfd.h>
#include <stdexcept>
#include <sys/stat.h>

namespace G13 {
// *************************************************************************

G13_Device::G13_Device(libusb_device *dev, libusb_context *ctx,
                       libusb_device_handle *handle, int m_id)
    : m_lcd(*this), m_stick(*this), device(dev), handle(handle),
      m_id_within_manager(m_id), m_uinput_fid(-1), m_ctx(ctx) {
  m_currentProfile = std::make_shared<G13_Profile>(*this, "default");
  m_profiles["default"] = m_currentProfile;

  for (bool &key : keys) {
    key = false;
  }

  lcd().image_clear();

  InitFonts();
  InitCommands();
}

// *************************************************************************

std::string G13_Device::DescribeLibusbErrorCode(int code) {
  /*
    auto description = std::string(libusb_error_name(code)) + " (" +
    std::to_string(code) + ") - " +
                       std::string(libusb_strerror((libusb_error)code));
  */
  auto description = std::string(libusb_strerror((libusb_error)code));
  return std::move(description);
  // return "unknown error";
}

int G13CreateFifo(const char *fifo_name) {
  if (mkfifo(fifo_name, 0660) < 0 && errno != EEXIST) return -1;
  struct stat before {};
  if (lstat(fifo_name, &before) < 0) return -1;
  if (!S_ISFIFO(before.st_mode) || before.st_uid != geteuid()) {
    errno = EACCES;
    return -1;
  }
  const int fd = open(fifo_name, O_RDWR | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC);
  if (fd < 0) return -1;
  struct stat opened {};
  int error = 0;
  if (fstat(fd, &opened) < 0) {
    error = errno;
  } else if (!S_ISFIFO(opened.st_mode) || opened.st_uid != geteuid()
      || opened.st_dev != before.st_dev || opened.st_ino != before.st_ino) {
    error = EACCES;
  } else if (fchmod(fd, 0660) < 0) {
    error = errno;
  }
  if (error) {
    close(fd);
    errno = error;
    return -1;
  }
  return fd;
}

int G13CreateUinput(G13_Device *g13) {
  struct uinput_user_dev uinp {};
  struct input_event event {};
  const char *dev_uinput_fname =
      access("/dev/input/uinput", F_OK) == 0
          ? "/dev/input/uinput"
          : access("/dev/uinput", F_OK) == 0 ? "/dev/uinput" : nullptr;
  if (!dev_uinput_fname) {
    G13_ERR("Could not find an uinput device");
    return -1;
  }
  if (access(dev_uinput_fname, W_OK) != 0) {
    G13_ERR(dev_uinput_fname << " doesn't grant write permissions");
    return -1;
  }
  int ufile = open(dev_uinput_fname, O_WRONLY | O_NDELAY);
  if (ufile <= 0) {
    G13_ERR("Could not open uinput");
    return -1;
  }
  memset(&uinp, 0, sizeof(uinp));
  char name[] = "G13";
  strncpy(uinp.name, name, sizeof(name));
  uinp.id.version = 1;
  uinp.id.bustype = BUS_USB;
  uinp.id.product = G13_PRODUCT_ID;
  uinp.id.vendor = G13_VENDOR_ID;
  uinp.absmin[ABS_X] = 0;
  uinp.absmin[ABS_Y] = 0;
  uinp.absmax[ABS_X] = 0xff;
  uinp.absmax[ABS_Y] = 0xff;
  //  uinp.absfuzz[ABS_X] = 4;
  //  uinp.absfuzz[ABS_Y] = 4;
  //  uinp.absflat[ABS_X] = 0x80;
  //  uinp.absflat[ABS_Y] = 0x80;

  ioctl(ufile, UI_SET_EVBIT, EV_KEY);
  ioctl(ufile, UI_SET_EVBIT, EV_ABS);
  /*  ioctl(ufile, UI_SET_EVBIT, EV_REL);*/
  ioctl(ufile, UI_SET_MSCBIT, MSC_SCAN);
  ioctl(ufile, UI_SET_ABSBIT, ABS_X);
  ioctl(ufile, UI_SET_ABSBIT, ABS_Y);
  /*  ioctl(ufile, UI_SET_RELBIT, REL_X);
   ioctl(ufile, UI_SET_RELBIT, REL_Y);*/
  for (int i = 0; i < G13::UINPUT_KEY_CODES; i++) {
    ioctl(ufile, UI_SET_KEYBIT, i);
  }

  // Mouse buttons
  for (int i = 0x110; i < 0x118; i++) {
    ioctl(ufile, UI_SET_KEYBIT, i);
  }
  ioctl(ufile, UI_SET_KEYBIT, BTN_THUMB);

  int retcode = write(ufile, &uinp, sizeof(uinp));
  if (retcode < 0) {
    G13_ERR("Could not write to uinput device (" << retcode << ")");
    close(ufile);
    return -1;
  }
  // The legacy descriptor cannot specify a current value. Set it before the
  // device is exposed: zero on a 0..255 axis looks like a fully held stick.
  for (auto code : {ABS_X, ABS_Y}) {
    struct uinput_abs_setup axis {};
    axis.code = code;
    axis.absinfo.value = 127;
    axis.absinfo.maximum = 255;
    if (ioctl(ufile, UI_ABS_SETUP, &axis) < 0) {
      G13_ERR("Could not initialize G13 stick axis");
      close(ufile);
      return -1;
    }
  }
  retcode = ioctl(ufile, UI_DEV_CREATE);
  if (retcode) {
    G13_ERR("Error creating uinput device for G13");
    close(ufile);
    return -1;
  }
  return ufile;
}

// *************************************************************************

void G13_Device::SendEvent(int type, int code, int val) {
  memset(&m_event, 0, sizeof(m_event));
  gettimeofday(&m_event.time, nullptr);
  m_event.type = type;
  m_event.code = code;
  m_event.value = val;
  write(m_uinput_fid, &m_event, sizeof(m_event));
}

void G13_Device::DispatchKey(int key, const G13_ActionPtr &action, bool down) {
  if (!update(key, down)) return;
  if (down) {
    pressed_actions[key] = action;
    // Keep it alive if an in-daemon command replaces its own binding.
    auto pressed = pressed_actions[key];
    if (pressed) pressed->act(*this, true);
  } else {
    auto pressed = std::move(pressed_actions[key]);
    if (pressed) pressed->act(*this, false);
  }
}

void G13_Device::OutputPipeWrite(const std::string &out) const {
  write(m_output_pipe_fid, out.c_str(), out.size());
}

void G13_Device::SetModeLeds(int leds) {
  unsigned char usb_data[] = {5, 0, 0, 0, 0};
  usb_data[1] = leds;
  int error = libusb_control_transfer(
      handle, LIBUSB_REQUEST_TYPE_CLASS | LIBUSB_RECIPIENT_INTERFACE, 9, 0x305,
      0, usb_data, 5, 1000);
  if (error != 5) {
    G13_ERR("Problem setting mode LEDs: " + DescribeLibusbErrorCode(error));
    return;
  }
}

void G13_Device::SetKeyColor(int red, int green, int blue) {
  int error;
  unsigned char usb_data[] = {5, 0, 0, 0, 0};
  usb_data[1] = red;
  usb_data[2] = green;
  usb_data[3] = blue;

  error = libusb_control_transfer(
      handle, LIBUSB_REQUEST_TYPE_CLASS | LIBUSB_RECIPIENT_INTERFACE, 9, 0x307,
      0, usb_data, 5, 1000);
  if (error != 5) {
    G13_ERR("Problem changing color: " + DescribeLibusbErrorCode(error));
    return;
  }
  m_backlight[0] = red;
  m_backlight[1] = green;
  m_backlight[2] = blue;
  if (m_report_seen) WriteKeyState(m_last_report); // the state file carries the colour
}

/*! reads and processes key state report from G13
 *
 */
int G13_Device::ReadKeypresses() {
  unsigned char buffer[G13_REPORT_SIZE];
  int size = 0;
  int error =
      libusb_interrupt_transfer(handle, LIBUSB_ENDPOINT_IN | G13_KEY_ENDPOINT,
                                buffer, G13_REPORT_SIZE, &size, 100);

  if (error && error != LIBUSB_ERROR_TIMEOUT) {
    G13_ERR("Error while reading keys: " << DescribeLibusbErrorCode(error));
    if (error == LIBUSB_ERROR_NO_DEVICE || error == LIBUSB_ERROR_IO) {
      G13_DBG("Giving libusb a nudge");
      libusb_handle_events(m_ctx);
    }
  }
  if (size == G13_REPORT_SIZE) {
    ProcessInputReport(buffer);
  }
  return 0;
}

void G13_Device::ProcessInputReport(unsigned char *buffer) {
  parse_joystick(buffer);
  m_currentProfile->ParseKeys(buffer);
  SendEvent(EV_SYN, SYN_REPORT, 0);
  WriteKeyState(buffer);
}

std::string G13KeyStateText(const unsigned char *report, const int backlight[3]) {
  std::string text = "stick " + std::to_string(report[1]) + " " +
                     std::to_string(report[2]) + "\nbacklight " +
                     std::to_string(backlight[0]) + " " + std::to_string(backlight[1]) + " " +
                     std::to_string(backlight[2]) + "\nkeys";
  for (size_t i = 0; i < G13_NUM_KEYS; ++i) {
    if (!(report[3 + i / 8] & (1u << (i % 8)))) continue;
    const std::string name = G13_KEY_STRINGS[i];
    // Firmware state bits share the bitmap; only LIGHT among the non-parsed is a key.
    if (name.rfind("UNDEF", 0) == 0 || name == "LIGHT_STATE" || name == "LIGHT2" ||
        name == "MISC_TOGGLE")
      continue;
    text += ' ';
    text += name;
  }
  text += '\n';
  return text;
}

bool G13ReplaceFile(const std::string &path, const std::string &content) {
  const std::string temporary = path + ".tmp";
  const int fd = open(temporary.c_str(),
                      O_WRONLY | O_CREAT | O_TRUNC | O_NOFOLLOW | O_CLOEXEC, 0640);
  if (fd < 0) return false;
  size_t done = 0;
  while (done < content.size()) {
    const ssize_t n = write(fd, content.data() + done, content.size() - done);
    if (n < 0 && errno == EINTR) continue;
    if (n <= 0) break;
    done += n;
  }
  close(fd);
  if (done == content.size() && rename(temporary.c_str(), path.c_str()) == 0) return true;
  unlink(temporary.c_str());
  return false;
}

void G13_Device::WriteKeyState(const unsigned char *report) {
  if (report != m_last_report) {
    memcpy(m_last_report, report, sizeof(m_last_report));
    m_report_seen = true;
  }
  if (m_keys_file_name.empty()) return;
  std::string text = G13KeyStateText(report, m_backlight);
  if (text == m_keys_last) return;
  if (!G13ReplaceFile(m_keys_file_name, text) && m_keys_last.empty()) {
    G13_ERR("cannot write " << m_keys_file_name << ": " << strerror(errno));
  }
  m_keys_last = text; // a failure is reported once, not per key press
}

void G13_Device::StartInputReader() {
  if (m_inputThread.joinable()) return;
  m_inputWake = eventfd(0, EFD_NONBLOCK | EFD_CLOEXEC);
  if (m_inputWake < 0) throw std::runtime_error("Cannot create G13 input wake fd");
  m_inputStopping.store(false);
  {
    std::lock_guard<std::mutex> lock(m_inputMutex);
    m_inputReports.clear();
    m_inputError = 0;
  }
  try {
    m_inputThread = std::thread(&G13_Device::InputLoop, this);
  } catch (...) {
    close(m_inputWake);
    m_inputWake = -1;
    throw;
  }
}

void G13_Device::StopInputReader() {
  {
    // Under the lock, or the reader can check the flag, lose this notify and wait forever.
    std::lock_guard<std::mutex> lock(m_inputMutex);
    m_inputStopping.store(true);
  }
  m_inputSpace.notify_all();
  if (m_inputThread.joinable()) m_inputThread.join();
  if (m_inputWake >= 0) close(m_inputWake);
  m_inputWake = -1;
}

void G13_Device::InputLoop() {
  while (!m_inputStopping.load()) {
    InputReport report{};
    int size = 0;
    // An idle reader can sleep: it no longer holds up the LCD/FIFO event loop.
    const int error = libusb_interrupt_transfer(handle,
        LIBUSB_ENDPOINT_IN | G13_KEY_ENDPOINT, report.data(), report.size(), &size, 100);
    if (m_inputStopping.load()) break;
    if (error && error != LIBUSB_ERROR_TIMEOUT) {
      G13_ERR("G13 input reader stopped: " << DescribeLibusbErrorCode(error));
      {
        std::lock_guard<std::mutex> lock(m_inputMutex);
        m_inputError = error;
      }
      const uint64_t wake = 1;
      (void)write(m_inputWake, &wake, sizeof(wake));
      break;
    }
    if (size != G13_REPORT_SIZE) continue;
    {
      std::unique_lock<std::mutex> lock(m_inputMutex);
      // Bounded storage, preserving down/up order; never overwrite a release report.
      m_inputSpace.wait(lock, [this] {
        return m_inputStopping.load() || m_inputReports.size() < 256;
      });
      if (m_inputStopping.load()) break;
      m_inputReports.push_back(report);
    }
    const uint64_t wake = 1;
    (void)write(m_inputWake, &wake, sizeof(wake));
  }
}

int G13_Device::ProcessInputReports() {
  uint64_t wakes;
  if (m_inputWake >= 0) (void)read(m_inputWake, &wakes, sizeof(wakes));
  std::deque<InputReport> reports;
  int error;
  {
    std::lock_guard<std::mutex> lock(m_inputMutex);
    // Bound each dispatch turn so continuous input cannot starve the LCD FIFO.
    for (size_t i = 0; i < 32 && !m_inputReports.empty(); ++i) {
      reports.push_back(m_inputReports.front());
      m_inputReports.pop_front();
    }
    if (!m_inputReports.empty()) {
      const uint64_t wake = 1;
      (void)write(m_inputWake, &wake, sizeof(wake));
    }
    error = m_inputError;
  }
  m_inputSpace.notify_one();
  for (auto &report : reports) ProcessInputReport(report.data());
  return error;
}

void G13_Device::ReadConfigFile(const std::string &filename) {
  std::ifstream s(filename);

  G13_OUT("reading configuration from " << filename);
  if (s.fail()) G13_LOG(log4cpp::Priority::ERROR << strerror(errno));
  else while (s.good()) {
    // grab a line
    char buf[1024];
    buf[0] = 0;
    buf[sizeof(buf) - 1] = 0;
    s.getline(buf, sizeof(buf) - 1);

    // strip comment
    char *comment = strchr(buf, '#');
    if (comment) {
      comment--;
      while (comment > buf && isspace(*comment))
        comment--;
      comment++;
      *comment = 0;
    }

    // send it
    if (buf[0]) {
      G13_OUT("  cfg: " << buf);
      Command(buf);
    }
  }
}

void G13_Device::ReadCommandsFromPipe() {
  fd_set set;
  FD_ZERO(&set);
  FD_SET(m_input_pipe_fid, &set);
  struct timeval tv {};
  tv.tv_sec = 0;
  tv.tv_usec = 0;
  int ret = select(m_input_pipe_fid + 1, &set, nullptr, nullptr, &tv);
  if (ret > 0) {
    unsigned char buf[1024 * 1024];
    memset(buf, 0, 1024 * 1024);
    ret = read(m_input_pipe_fid, buf, 1024 * 1024);
    G13_LOG(log4cpp::Priority::DEBUG << "read " << ret << " characters");

    if (ret ==
        960) { // TODO probably image, for now, don't test, just assume image
      lcd().Image(buf, ret);
    } else {
      if (ret <= 0) return;
      std::string buffer(reinterpret_cast<const char *>(buf), ret);
      auto lines = Helper::split<std::vector<std::string>>(
          buffer, "\n\r", Helper::split::no_empties);

      for (auto &cmd : lines) {
        auto command_comment = Helper::split<std::vector<std::string>>(
            cmd, "#", Helper::split::no_empties);

        if (!command_comment.empty() && command_comment[0] != std::string("")) {
          while (!command_comment[0].empty() &&
                 isspace(static_cast<unsigned char>(command_comment[0].back())))
            command_comment[0].pop_back();
          if (command_comment[0] != std::string("")) {
            G13_OUT("command: " << command_comment[0]);
            Command(command_comment[0].c_str());
          }
        }
      }
    }
  }
}

FontPtr G13_Device::SwitchToFont(const std::string &name) {
  FontPtr rv = pFonts[name];
  if (rv) {
    m_currentFont = rv;
  }
  return rv;
}

void G13_Device::SwitchToProfile(const std::string &name) {
  m_currentProfile = Profile(name);
}

ProfilePtr G13_Device::Profile(const std::string &name) {
  ProfilePtr rv = m_profiles[name];
  if (!rv) {
    rv = std::make_shared<G13_Profile>(*m_currentProfile, name);
    m_profiles[name] = rv;
  }
  return rv;
}

G13_ActionPtr G13_Device::MakeAction(const std::string &action) {
  if (action.empty()) {
    throw G13_CommandException("empty action string");
  }
  if (action[0] == '>') {
    return G13_ActionPtr(new G13_Action_PipeOut(*this, &action[1]));
  } else if (action[0] == '!') {
    return G13_ActionPtr(new G13_Action_Command(*this, &action[1]));
  } else {
    return G13_ActionPtr(new G13_Action_Keys(*this, action));
  }
  // UNREACHABLE: throw G13_CommandException("can't create action for " +
  // action);
}

// *************************************************************************

void G13_Device::Dump(std::ostream &o, int detail) {
  o << "G13 id=" << id_within_manager() << std::endl;
  o << "   input_pipe_name=" << Helper::repr(m_input_pipe_name) << std::endl;
  o << "   output_pipe_name=" << Helper::repr(m_output_pipe_name) << std::endl;
  o << "   current_profile=" << m_currentProfile->name() << std::endl;
  o << "   current_font=" << m_currentFont->name() << std::endl;

  if (detail > 0) {
    o << "STICK" << std::endl;
    stick().dump(o);
    if (detail == 1) {
      m_currentProfile->dump(o);
    } else {
      for (auto &_profile : m_profiles) {
        _profile.second->dump(o);
      }
    }
  }
}

struct commandAdder {
  commandAdder(G13_Device::CommandFunctionTable &t, const char *name)
      : _t(t), _name(name) {}

  commandAdder(G13_Device::CommandFunctionTable &t, const char *name,
               G13_Device::COMMAND_FUNCTION f)
      : _t(t), _name(name) {
    _t[_name] = std::move(f);
  }

  G13_Device::CommandFunctionTable &_t;
  std::string _name;

  commandAdder &operator+=(G13_Device::COMMAND_FUNCTION f) {
    _t[_name] = std::move(f);
    return *this;
  };
};

void G13_Device::InitCommands() {
  using Helper::advance_ws;
  // const char *remainder;

  commandAdder add_out(_command_table, "out", [this](const char *remainder) {
    lcd().WriteString(remainder);
  });

  commandAdder add_pos(_command_table, "pos", [this](const char *remainder) {
    int row, col;
    if (sscanf(remainder, "%i %i", &row, &col) == 2) {
      lcd().WritePos(row, col);
    } else {
      G13_ERR("bad pos : " << remainder);
    }
  });

  commandAdder add_bind(_command_table, "bind", [this](const char *remainder) {
    std::string keyname;
    advance_ws(remainder, keyname);
    std::string action = remainder;
    try {
      if (auto key = m_currentProfile->FindKey(keyname)) {
        key->set_action(MakeAction(action));
      } else if (auto stick_key = m_stick.zone(keyname)) {
        stick_key->set_action(MakeAction(action));
      } else {
        G13_ERR("bind key " << keyname << " unknown");
        return;
      }
      G13_LOG(log4cpp::Priority::DEBUG << "bind " << keyname << " [" << action
                                       << "]");
    } catch (const std::exception &ex) {
      G13_ERR("bind " << keyname << " " << action << " failed : " << ex.what());
    }
  });

  commandAdder add_profile(
      _command_table, "profile",
      [this](const char *remainder) { SwitchToProfile(remainder); });

  commandAdder add_font(_command_table, "font", [this](const char *remainder) {
    SwitchToFont(remainder);
  });

  commandAdder add_mod(_command_table, "mod", [this](const char *remainder) {
    SetModeLeds(atoi(remainder));
  });

  commandAdder add_textmode(
      _command_table, "textmode",
      [this](const char *remainder) { lcd().text_mode = atoi(remainder); });

  commandAdder add_rgb(_command_table, "rgb", [this](const char *remainder) {
    int red, green, blue;
    if (sscanf(remainder, "%i %i %i", &red, &green, &blue) == 3) {
      SetKeyColor(red, green, blue);
    } else {
      G13_ERR("rgb bad format: <" << remainder << ">");
    }
  });

  commandAdder add_stickmode(
      _command_table, "stickmode", [this](const char *remainder) {
        std::string mode = remainder;
        // A sorted container's index is not a stick_mode_t. KEYS used to select
        // CALNORTH; RELATIVE has no implementation or enum value in this driver.
        const std::map<std::string, G13::stick_mode_t> modes = {
            {"ABSOLUTE", STICK_ABSOLUTE}, {"KEYS", STICK_KEYS},
            {"CALCENTER", STICK_CALCENTER}, {"CALBOUNDS", STICK_CALBOUNDS},
            {"CALNORTH", STICK_CALNORTH}};
        auto found = modes.find(mode);
        if (found != modes.end()) {
          m_stick.set_mode(found->second);
          return;
        }
        G13_ERR("unknown stick mode : <" << mode << ">");
      });

  commandAdder add_stickzone(
      _command_table, "stickzone", [this](const char *remainder) {
        std::string operation, zonename;
        advance_ws(remainder, operation);
        advance_ws(remainder, zonename);
        if (operation == "add") {
          /* G13_StickZone* zone = */
          m_stick.zone(zonename, true);
        } else {
          G13_StickZone *zone = m_stick.zone(zonename);
          if (!zone) {
            throw G13_CommandException("unknown stick zone");
          }
          if (operation == "action") {
            zone->set_action(MakeAction(remainder));
          } else if (operation == "bounds") {
            double x1, y1, x2, y2;
            if (sscanf(remainder, "%lf %lf %lf %lf", &x1, &y1, &x2, &y2) != 4) {
              throw G13_CommandException("bad bounds format");
            }
            zone->set_bounds(G13_ZoneBounds(x1, y1, x2, y2));

          } else if (operation == "del") {
            m_stick.RemoveZone(*zone);
          } else {
            G13_ERR("unknown stickzone operation: <" << operation << ">");
          }
        }
      });

  commandAdder add_dump(_command_table, "dump", [this](const char *remainder) {
    std::string target;
    advance_ws(remainder, target);
    if (target == "all") {
      Dump(std::cout, 3);
    } else if (target == "current") {
      Dump(std::cout, 1);
    } else if (target == "summary") {
      Dump(std::cout, 0);
    } else {
      G13_ERR("unknown dump target: <" << target << ">");
    }
  });

  commandAdder add_log_level(_command_table, "log_level",
                             [this](const char *remainder) {
                               std::string level;
                               advance_ws(remainder, level);
                               G13_Manager::Instance()->SetLogLevel(level);
                             });

  commandAdder add_refresh(
      _command_table, "refresh",
      [this](const char *remainder) { lcd().image_send(); });

  commandAdder add_clear(_command_table, "clear",
                         [this](const char *remainder) {
                           lcd().image_clear();
                           lcd().image_send();
                         });
}

void G13_Device::Command(char const *str) {
  const char *remainder = str;

  try {
    using Helper::advance_ws;

    std::string cmd;
    advance_ws(remainder, cmd);
    if (cmd.empty()) return; // a blank line (clients pad 960-byte batches with one)

    auto i = _command_table.find(cmd);
    if (i == _command_table.end()) {
      G13_ERR("unknown command : " << cmd);
    } else {
      COMMAND_FUNCTION f = i->second;
      f(remainder);
    }
  } catch (const std::exception &ex) {
    G13_ERR("command failed : " << ex.what());
  }
}

void G13_Device::RegisterContext(libusb_context *libusbContext) {
  m_ctx = libusbContext;
  // State files first: the logo LcdInit sends and the colour set below are state too.
  m_keys_file_name = G13_Manager::Instance()->MakePipeName(this, true) + "_keys";
  m_lcd_file_name = G13_Manager::Instance()->MakePipeName(this, true) + "_lcd";
  const unsigned char idle[8] = {0, 128, 128, 0, 0, 0, 0, 0}; // until the pad reports
  WriteKeyState(idle);

  int leds = 0;
  int red = 0;
  int green = 0;
  int blue = 255;
  LcdInit();

  SetModeLeds(leds);
  SetKeyColor(red, green, blue);

  m_uinput_fid = G13CreateUinput(this);
  if (m_uinput_fid < 0) {
    // Without it every key press is written to nowhere while the service looks healthy.
    throw std::runtime_error("failed creating the uinput keyboard (is /dev/uinput there and group-accessible?)");
  }
  m_input_pipe_name = G13_Manager::Instance()->MakePipeName(this, true);
  m_input_pipe_fid = G13CreateFifo(m_input_pipe_name.c_str());
  if (m_input_pipe_fid == -1) {
    throw std::runtime_error("failed opening input FIFO: " + m_input_pipe_name);
  }
  m_output_pipe_name = G13_Manager::Instance()->MakePipeName(this, false);
  m_output_pipe_fid = G13CreateFifo(m_output_pipe_name.c_str());
  if (m_output_pipe_fid == -1) {
    throw std::runtime_error("failed opening output FIFO: " + m_output_pipe_name);
  }
}

void G13_Device::Cleanup() {
  // Join before closing libusb handles, FIFOs or uinput, including on unplug/shutdown.
  StopInputReader();
  SetKeyColor(0, 0, 0);
  // Never unlink a substituted path or one we failed to open.
  const auto unlink_owned = [](const std::string &path, int fd) {
    struct stat opened {}, current {};
    if (fd >= 0 && fstat(fd, &opened) == 0 && lstat(path.c_str(), &current) == 0
        && S_ISFIFO(current.st_mode) && current.st_uid == geteuid()
        && opened.st_dev == current.st_dev && opened.st_ino == current.st_ino)
      unlink(path.c_str());
  };
  unlink_owned(m_input_pipe_name, m_input_pipe_fid);
  unlink_owned(m_output_pipe_name, m_output_pipe_fid);
  // A state file is ours only while it is a plain file we own (the directory is the
  // daemon's; this guards a substituted path all the same).
  for (const std::string &path : {m_keys_file_name, m_lcd_file_name}) {
    struct stat state_file {};
    if (!path.empty() && lstat(path.c_str(), &state_file) == 0 && S_ISREG(state_file.st_mode)
        && state_file.st_uid == geteuid())
      unlink(path.c_str());
  }
  ioctl(m_uinput_fid, UI_DEV_DESTROY);
  close(m_uinput_fid);
  if (m_input_pipe_fid >= 0) close(m_input_pipe_fid);
  if (m_output_pipe_fid >= 0) close(m_output_pipe_fid);
  libusb_release_interface(handle, 0);
  libusb_close(handle);
}

G13_Device::~G13_Device() {
  Cleanup();
}

// libusb_device_handle *G13_Device::Handle() const { return handle; }

libusb_device *G13_Device::Device() const { return device; }

} // namespace G13
