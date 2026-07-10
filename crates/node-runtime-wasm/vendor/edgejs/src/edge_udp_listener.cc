#include "edge_udp_listener.h"

EdgeUdpListener::~EdgeUdpListener() {
  if (wrap_ != nullptr) wrap_->set_listener(nullptr);
}

EdgeUdpWrapBase::~EdgeUdpWrapBase() {
  set_listener(nullptr);
}

void EdgeUdpWrapBase::set_listener(EdgeUdpListener* listener) {
  if (listener_ != nullptr) listener_->wrap_ = nullptr;
  listener_ = listener;
  if (listener_ != nullptr) listener_->wrap_ = this;
}

EdgeUdpListener* EdgeUdpWrapBase::listener() const {
  return listener_;
}
