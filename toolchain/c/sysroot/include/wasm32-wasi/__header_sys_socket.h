#ifndef __wasilibc___header_sys_socket_h
#define __wasilibc___header_sys_socket_h

#include <__wasi_snapshot.h>
#include <__struct_msghdr.h>
#include <__struct_sockaddr.h>
#include <__struct_sockaddr_storage.h>

#include <wasi/api.h>

#define SHUT_RD __WASI_SDFLAGS_RD
#define SHUT_WR __WASI_SDFLAGS_WR
#define SHUT_RDWR (SHUT_RD | SHUT_WR)

#ifdef __wasilibc_use_wasip2
#define MSG_DONTWAIT  0x0040
#define MSG_NOSIGNAL  0x4000
#define MSG_PEEK      0x0002
#define MSG_WAITALL   0x0100
#define MSG_TRUNC     0x0020

#define SOL_IP     0
#define SOL_TCP    6
#define SOL_UDP    17
#define SOL_IPV6   41

#define SOMAXCONN 128

#define SO_REUSEADDR 2
#define SO_ERROR 4
#define SO_SNDBUF 7
#define SO_RCVBUF 8
#define SO_KEEPALIVE 9
#define SO_ACCEPTCONN 30
#define SO_PROTOCOL 38
#define SO_DOMAIN 39
 
#if __LONG_MAX == 0x7fffffff
#define SO_RCVTIMEO     66
#define SO_SNDTIMEO     67
#else
#define SO_RCVTIMEO     20
#define SO_SNDTIMEO     21
#endif

#else // __wasilibc_use_wasip2
#define MSG_PEEK __WASI_RIFLAGS_RECV_PEEK
#define MSG_WAITALL __WASI_RIFLAGS_RECV_WAITALL
#define MSG_TRUNC __WASI_ROFLAGS_RECV_DATA_TRUNCATED
#endif // __wasilibc_use_wasip2

/*
 * secure-exec exposes a POSIX-style socket layer over host_net for WASI p1.
 * Keep the WASI-native domain/type values below, but expose the common
 * socket option and message flag constants that networking applications
 * expect to find in <sys/socket.h>.
 */
#ifndef MSG_DONTWAIT
#define MSG_DONTWAIT  0x0040
#endif

#ifndef MSG_NOSIGNAL
#define MSG_NOSIGNAL  0x4000
#endif

#ifndef SOL_IP
#define SOL_IP     0
#endif

#ifndef SOL_TCP
#define SOL_TCP    6
#endif

#ifndef SOL_UDP
#define SOL_UDP    17
#endif

#ifndef SOL_IPV6
#define SOL_IPV6   41
#endif

#ifndef SOMAXCONN
#define SOMAXCONN 128
#endif

#ifndef SO_REUSEADDR
#define SO_REUSEADDR 2
#endif

#ifndef SO_ERROR
#define SO_ERROR 4
#endif

#ifndef SO_SNDBUF
#define SO_SNDBUF 7
#endif

#ifndef SO_RCVBUF
#define SO_RCVBUF 8
#endif

#ifndef SO_KEEPALIVE
#define SO_KEEPALIVE 9
#endif

#ifndef SO_ACCEPTCONN
#define SO_ACCEPTCONN 30
#endif

#ifndef SO_PROTOCOL
#define SO_PROTOCOL 38
#endif

#ifndef SO_DOMAIN
#define SO_DOMAIN 39
#endif

#ifndef SO_RCVTIMEO
#if __LONG_MAX == 0x7fffffff
#define SO_RCVTIMEO     66
#define SO_SNDTIMEO     67
#else
#define SO_RCVTIMEO     20
#define SO_SNDTIMEO     21
#endif
#endif

#define SOCK_DGRAM __WASI_FILETYPE_SOCKET_DGRAM
#define SOCK_STREAM __WASI_FILETYPE_SOCKET_STREAM

#define SOCK_NONBLOCK (0x00004000)
#define SOCK_CLOEXEC (0x00002000)

#define SOL_SOCKET 0x7fffffff

#define SO_TYPE 3

#define PF_UNSPEC 0
#define PF_INET 1
#define PF_INET6 2

#define AF_UNSPEC PF_UNSPEC
#define AF_INET PF_INET
#define AF_INET6 PF_INET6
#define AF_UNIX 3

#ifdef __cplusplus
extern "C" {
#endif

#ifdef __cplusplus
}
#endif

#endif
