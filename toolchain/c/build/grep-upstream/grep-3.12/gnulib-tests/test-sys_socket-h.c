/* Test of <sys/socket.h> substitute.
   Copyright (C) 2007, 2009-2025 Free Software Foundation, Inc.

   This program is free software: you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published by
   the Free Software Foundation, either version 3 of the License, or
   (at your option) any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

/* Written by Bruno Haible <bruno@clisp.org>, 2007.  */

#include <config.h>

#include <sys/socket.h>

/* POSIX mandates that AF_UNSPEC shall be 0.  */
static_assert (AF_UNSPEC == 0);

/* Check that the 'socklen_t' type is defined.  */
socklen_t t1;

/* Check that the 'size_t' and 'ssize_t' types are defined.  */
size_t t2;
ssize_t t3;

/* Check that the 'struct sockaddr' type is defined.  */
struct sockaddr t4;

/* Check that the 'struct sockaddr_storage' type is defined.  */
struct sockaddr_storage t5;

/* Check that 'struct iovec' is defined.  */
struct iovec io;

/* Check that a minimal set of 'struct msghdr' is defined.  */
struct msghdr msg;

#ifdef SCM_RIGHTS
/* Check that the 'struct cmsghdr' type is defined.  */
struct cmsghdr cmsg;
/* Check constant cmsg_type values.  */
int cmsg_types[] = { SCM_RIGHTS };
#endif

/* Check that the 'struct linger' type is defined.  */
struct linger li;

/* Check constant level arguments.  */
int levels[] = { SOL_SOCKET };

/* Check constant backlog arguments.  */
int backlogs[] = { SOMAXCONN };

#include <errno.h>

#include "intprops.h"
#include "macros.h"

/* POSIX requires that 'socklen_t' is an integer type with a width of at
   least 32 bits.  */
static_assert (32 <= TYPE_WIDTH (socklen_t));

/* POSIX requires that sa_family_t is an unsigned integer type.  */
static_assert (! TYPE_SIGNED (sa_family_t));


int
main (void)
{
  struct sockaddr_storage x;
  sa_family_t i;

  /* Check some errno values.  */
  switch (ENOTSOCK)
    {
    case ENOTSOCK:
    case EADDRINUSE:
    case ENETRESET:
    case ECONNABORTED:
    case ECONNRESET:
    case ENOTCONN:
    case ESHUTDOWN:
      break;
    }

  /* Check that the supported socket types have distinct values.  */
  switch (0)
    {
    case SOCK_DGRAM:
#ifdef SOCK_RAW
    case SOCK_RAW:
#endif
    case SOCK_SEQPACKET:
    case SOCK_STREAM:
    default:
      break;
    }
  int socket_types = 0;
  socket_types |= SOCK_DGRAM;
#ifdef SOCK_RAW
  socket_types |= SOCK_RAW;
#endif
  socket_types |= SOCK_SEQPACKET;
  socket_types |= SOCK_STREAM;

  /* Check socket creation flags.  */
#ifdef SOCK_NONBLOCK
  ASSERT ((SOCK_NONBLOCK & socket_types) == 0);
#endif
#ifdef SOCK_CLOEXEC
  ASSERT ((SOCK_CLOEXEC & socket_types) == 0);
#endif
#ifdef SOCK_CLOFORK
  ASSERT ((SOCK_CLOFORK & socket_types) == 0);
#endif
#if defined SOCK_NONBLOCK && defined SOCK_CLOEXEC
  static_assert ((SOCK_NONBLOCK & SOCK_CLOEXEC) == 0);
#endif
#if defined SOCK_NONBLOCK && defined SOCK_CLOFORK
  static_assert ((SOCK_NONBLOCK & SOCK_CLOFORK) == 0);
#endif
#if defined SOCK_CLOEXEC && defined SOCK_CLOFORK
  static_assert ((SOCK_CLOEXEC & SOCK_CLOFORK) == 0);
#endif

  /* Check that the option_name constants have distinct values.  */
  switch (0)
    {
    case SO_ACCEPTCONN:
    case SO_BROADCAST:
    case SO_DEBUG:
#ifdef SO_DOMAIN
    case SO_DOMAIN:
#endif
    case SO_DONTROUTE:
    case SO_ERROR:
    case SO_KEEPALIVE:
    case SO_LINGER:
    case SO_OOBINLINE:
#ifdef SO_PROTOCOL
    case SO_PROTOCOL:
#endif
    case SO_RCVBUF:
    case SO_RCVLOWAT:
    case SO_RCVTIMEO:
    case SO_REUSEADDR:
    case SO_SNDBUF:
    case SO_SNDLOWAT:
    case SO_SNDTIMEO:
    case SO_TYPE:
    default:
      break;
    }

  /* Check the msg_flags constants have distinct values.  */
  switch (0)
    {
#ifdef MSG_CMSG_CLOEXEC
    case MSG_CMSG_CLOEXEC:
#endif
#ifdef MSG_CMSG_CLOFORK
    case MSG_CMSG_CLOFORK:
#endif
    case MSG_DONTROUTE:
#ifdef MSG_EOR
    case MSG_EOR:
#endif
    case MSG_OOB:
#ifdef MSG_NOSIGNAL
    case MSG_NOSIGNAL:
#endif
    case MSG_PEEK:
#ifdef MSG_TRUNC
    case MSG_CTRUNC:
    case MSG_TRUNC:
#endif
#ifdef MSG_WAITALL
    case MSG_WAITALL:
#endif
    default:
      break;
    }

  /* Check that each supported address family has a distinct value.  */
  switch (0)
    {
    case AF_UNSPEC:
#if HAVE_IPV4
    case AF_INET:
#endif
#if HAVE_IPV6
    case AF_INET6:
#endif
#if HAVE_UNIXSOCKET
    case AF_UNIX:
#endif
    default:
      break;
    }

  /* Check that the shutdown type macros are defined to distinct values.  */
#if HAVE_SHUTDOWN
  switch (0)
    {
    case SHUT_RD:
    case SHUT_WR:
    case SHUT_RDWR:
    default:
      break;
    }
#endif

  x.ss_family = 42;
  i = 42;
  msg.msg_iov = &io;

  return (x.ss_family - i
          + msg.msg_namelen + msg.msg_iov->iov_len + msg.msg_iovlen)
         + test_exit_status;
}
