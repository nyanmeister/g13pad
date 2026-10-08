//
// Created by khampf on 13-05-2020.
//

#ifndef G13_G13_MANAGER_HPP
#define G13_G13_MANAGER_HPP

#include "g13.hpp"
#include "g13_action.hpp"
#include "g13_device.hpp"
#include "g13_keys.hpp"
#include "g13_log.hpp"
#include "g13_manager.hpp"
#include <libusb-1.0/libusb.h>
#include <csignal>
#include <deque>
#include <mutex>

#define CONTROL_DIR std::string("/tmp/")

/*!
 * top level class, holds what would otherwise be in global variables
 */
namespace G13 {
class G13_Manager {
private:
  G13_Manager();

  // declarations
  static volatile std::sig_atomic_t running;
  static int wake_fd;
  static std::mutex hotplug_mutex;
  static std::deque<std::pair<libusb_device *, libusb_hotplug_event>> hotplug_events;
  static void QueueHotplug(libusb_device *, libusb_hotplug_event);
  static void ProcessHotplug();
  static void WaitForActivity();
  static std::map<std::string, std::string> stringConfigValues;
  static libusb_context *libusbContext;
  static std::vector<G13::G13_Device *> g13s;
  static libusb_hotplug_callback_handle hotplug_cb_handle[3];
  static std::map<G13_KEY_INDEX, std::string> g13_key_to_name;
  static std::map<std::string, G13_KEY_INDEX> g13_name_to_key;
  static std::map<LINUX_KEY_VALUE, std::string> input_key_to_name;
  static std::map<std::string, LINUX_KEY_VALUE> input_name_to_key;
  static libusb_device **devs;
  static std::string logoFilename;
  static const int class_id;

public:
  static G13_Manager *
  Instance(); // Singleton pattern instead of passing references around

  // static const std::string &getLogoFilename();
  static void setLogoFilename(const std::string &logoFilename);

  [[nodiscard]] static int FindG13KeyValue(const std::string &keyname);

  [[nodiscard]] static std::string FindG13KeyName(int v);

  [[nodiscard]] static G13::LINUX_KEY_VALUE
  FindInputKeyValue(const std::string &keyname);

  [[nodiscard]] static std::string FindInputKeyName(G13::LINUX_KEY_VALUE v);

  static int Run();

  [[nodiscard]] static std::string
  getStringConfigValue(const std::string &name);

  static void setStringConfigValue(const std::string &name,
                                   const std::string &value);

  static std::string MakePipeName(G13::G13_Device *d, bool is_input);

  static void start_logging();

  [[maybe_unused]] static void
  SetLogLevel(log4cpp::Priority::PriorityLevel lvl);

  static void SetLogLevel(const std::string &level);

protected:
  static void InitKeynames();

  static void DisplayKeys();

  static void DiscoverG13s(libusb_device **devs, ssize_t count);

  static void Cleanup();

  static void SignalHandler(int);

  static void SetupDevice(G13::G13_Device *g13);

  static int LIBUSB_CALL HotplugCallbackEnumerate(struct libusb_context *ctx,
                                                  struct libusb_device *dev,
                                                  libusb_hotplug_event event,
                                                  void *user_data);

  static int LIBUSB_CALL HotplugCallbackInsert(struct libusb_context *ctx,
                                               struct libusb_device *dev,
                                               libusb_hotplug_event event,
                                               void *user_data);

  static int LIBUSB_CALL HotplugCallbackRemove(struct libusb_context *ctx,
                                               struct libusb_device *dev,
                                               libusb_hotplug_event event,
                                               void *user_data);

  static int OpenAndAddG13(libusb_device *dev);

  static void ArmHotplugCallbacks();
};
} // namespace G13

#endif // G13_G13_MANAGER_HPP
