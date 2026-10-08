/* This file contains code for managing keys an profiles
 *
 */

// TODO: check out why g13.h needs to be included before libevdev
// clang-format off
#include "g13.hpp"
#include "g13_device.hpp"
#include "g13_keys.hpp"
#include "helper.hpp"
// clang-format on

namespace G13 {

void G13_Key::dump(std::ostream &o) const {
  o << G13_Manager::Instance()->FindG13KeyName(index()) << "(" << index()
    << ") : ";
  if (action()) {
    action()->dump(o);
  } else {
    o << "(no action)";
  }
}

// *************************************************************************

void G13_Key::ParseKey(const unsigned char *byte, G13_Device *g13) {
  bool key_is_down = byte[_index.offset] & _index.mask;
  g13->DispatchKey(_index.index, _action, key_is_down);
}

// *************************************************************************

} // namespace G13
