#ifndef EDGE_UDP_LISTENER_H_
#define EDGE_UDP_LISTENER_H_

#include <cstddef>

#include <uv.h>

#include "node_api.h"

class EdgeUdpWrapBase;

class EdgeUdpSendWrap {
 public:
  virtual ~EdgeUdpSendWrap() = default;
  virtual napi_value object(napi_env env) const = 0;

  uv_udp_send_t req{};
  uv_buf_t* bufs = nullptr;
  size_t nbufs = 0;
  size_t msg_size = 0;
  bool have_callback = false;
};

class EdgeUdpListener {
 public:
  virtual ~EdgeUdpListener();

  virtual uv_buf_t OnAlloc(size_t suggested_size) = 0;
  virtual void OnRecv(ssize_t nread,
                      const uv_buf_t& buf,
                      const sockaddr* addr,
                      unsigned int flags) = 0;
  virtual EdgeUdpSendWrap* CreateSendWrap(size_t msg_size) = 0;
  virtual void OnSendDone(EdgeUdpSendWrap* wrap, int status) = 0;
  virtual void OnAfterBind() {}

  EdgeUdpWrapBase* udp() const { return wrap_; }

 protected:
  EdgeUdpWrapBase* wrap_ = nullptr;

  friend class EdgeUdpWrapBase;
};

class EdgeUdpWrapBase {
 public:
  virtual ~EdgeUdpWrapBase();

  virtual int RecvStart() = 0;
  virtual int RecvStop() = 0;
  virtual ssize_t Send(uv_buf_t* bufs,
                       size_t nbufs,
                       const sockaddr* addr) = 0;

  void set_listener(EdgeUdpListener* listener);
  EdgeUdpListener* listener() const;

 private:
  EdgeUdpListener* listener_ = nullptr;
};

#endif  // EDGE_UDP_LISTENER_H_
