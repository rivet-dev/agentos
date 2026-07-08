/***************************************************************************
 * curl_config.h - Hand-crafted configuration for WASI (wasm32-wasip1)
 *
 * This file configures libcurl 8.11.1 for compilation against our patched
 * WASI sysroot which provides POSIX socket APIs via host_net WASM imports.
 *
 * Key decisions:
 *   - HTTP and HTTPS (all other protocols disabled)
 *   - TLS via WASI host runtime (USE_WASI_TLS backend)
 *   - No threads (single-threaded WASM)
 *   - No proxy support
 *   - No cookies
 *   - Socket backend uses standard POSIX APIs from patched sysroot
 *
 * WASI sysroot quirk: sys/socket.h guards most SO_XXX, POLLPRI, and
 * getsockname constants behind __wasilibc_unmodified_upstream (wasip1
 * only exports a small subset via __header_sys_socket.h). We supply the
 * missing defines here so libcurl can use the full POSIX socket API.
 ***************************************************************************/

#ifndef HEADER_CURL_CONFIG_WASI_H
#define HEADER_CURL_CONFIG_WASI_H

/* ================================================================ */
/*               FEATURE DISABLES                                    */
/* ================================================================ */

/* Disable all protocols except HTTP */
#define CURL_DISABLE_DICT 1
#define CURL_DISABLE_FILE 1
#define CURL_DISABLE_FTP 1
#define CURL_DISABLE_GOPHER 1
#define CURL_DISABLE_IMAP 1
#define CURL_DISABLE_LDAP 1
#define CURL_DISABLE_LDAPS 1
#define CURL_DISABLE_MQTT 1
#define CURL_DISABLE_POP3 1
#define CURL_DISABLE_RTSP 1
#define CURL_DISABLE_SMB 1
#define CURL_DISABLE_SMTP 1
#define CURL_DISABLE_TELNET 1
#define CURL_DISABLE_TFTP 1

/* Disable features we don't need */
#define CURL_DISABLE_ALTSVC 1
#define CURL_DISABLE_COOKIES 1
#define CURL_DISABLE_DOH 1
#define CURL_DISABLE_HSTS 1
#define CURL_DISABLE_NETRC 1
#define CURL_DISABLE_PROXY 1
#define CURL_DISABLE_SOCKETPAIR 1

/* Disable authentication methods that need crypto */
#define CURL_DISABLE_DIGEST_AUTH 1
#define CURL_DISABLE_KERBEROS_AUTH 1
#define CURL_DISABLE_NEGOTIATE_AUTH 1
#define CURL_DISABLE_NTLM 1
#define CURL_DISABLE_AWS 1

/* Disable misc features */
#define CURL_DISABLE_VERBOSE_STRINGS 0
/* MIME and FORM_API enabled for multipart form uploads (-F) */
/* #undef CURL_DISABLE_MIME */
/* #undef CURL_DISABLE_FORM_API */
#define CURL_DISABLE_BINDLOCAL 1
#define CURL_DISABLE_PROGRESS_METER 1

/* TLS via host runtime (host_net WASM import) */
#define USE_WASI_TLS 1
#define CURL_DISABLE_OPENSSL_AUTO_LOAD_CONFIG 1

/* ================================================================ */
/*               SYSTEM HEADERS                                      */
/* ================================================================ */

#define HAVE_ARPA_INET_H 1
#define HAVE_ERRNO_H 1
#define HAVE_FCNTL_H 1
#define HAVE_INTTYPES_H 1
#define HAVE_NETDB_H 1
#define HAVE_NETINET_IN_H 1
#define HAVE_NETINET_TCP_H 1
#define HAVE_POLL_H 1
/* #undef HAVE_PWD_H */
/* #undef HAVE_SETJMP_H */
/* #undef HAVE_SIGNAL_H */
#define HAVE_STDBOOL_H 1
#define HAVE_STDINT_H 1
#define HAVE_STDLIB_H 1
#define HAVE_STRING_H 1
#define HAVE_STRINGS_H 1
#define HAVE_SYS_IOCTL_H 1
#define HAVE_SYS_PARAM_H 1
#define HAVE_SYS_RESOURCE_H 1
#define HAVE_SYS_SELECT_H 1
#define HAVE_SYS_SOCKET_H 1
#define HAVE_SYS_STAT_H 1
#define HAVE_SYS_TIME_H 1
#define HAVE_SYS_TYPES_H 1
#define HAVE_SYS_UIO_H 1
/* #undef HAVE_SYS_UN_H */
#define HAVE_SYS_UTSNAME_H 1
#define HAVE_TIME_H 1
#define HAVE_UNISTD_H 1

/* Headers we do NOT have in WASI */
/* #undef HAVE_IFADDRS_H */
/* #undef HAVE_NET_IF_H */
/* #undef HAVE_SYS_WAIT_H */
/* #undef HAVE_TERMIO_H */
/* #undef HAVE_TERMIOS_H */
/* #undef HAVE_SYS_FILIO_H */
/* #undef HAVE_SYS_SOCKIO_H */
/* #undef HAVE_PTHREAD_H */
/* #undef HAVE_LIBGEN_H */

/* ================================================================ */
/*               TYPES AND SIZES                                     */
/* ================================================================ */

#define SIZEOF_INT 4
#define SIZEOF_SHORT 2
#define SIZEOF_LONG 4
#define SIZEOF_SIZE_T 4
#define SIZEOF_CURL_OFF_T 8
#define SIZEOF_OFF_T 8
#define SIZEOF_TIME_T 8

#define HAVE_LONGLONG 1
#define HAVE_BOOL_T 1
#define HAVE_STRUCT_TIMEVAL 1
#define HAVE_STRUCT_SOCKADDR_STORAGE 1
#define HAVE_SA_FAMILY_T 1
#define HAVE_SOCKADDR_IN6_SIN6_SCOPE_ID 1

/* Socket type */
#define CURL_TYPEOF_CURL_SOCKLEN_T socklen_t
#define HAVE_SOCKLEN_T 1

/* ================================================================ */
/*               FUNCTIONS                                           */
/* ================================================================ */

/* Standard C/POSIX functions */
#define HAVE_BASENAME 1
#define HAVE_CLOCK_GETTIME 1
#define HAVE_FCNTL 1
#define HAVE_FCNTL_O_NONBLOCK 1
#define HAVE_FREEADDRINFO 1
#define HAVE_GETADDRINFO 1
#define HAVE_GETHOSTNAME 1
#define HAVE_GETPEERNAME 1
#define HAVE_GETSOCKNAME 1
#define HAVE_GMTIME_R 1
#define HAVE_INET_NTOP 1
#define HAVE_INET_PTON 1
#define HAVE_MEMRCHR 1
#define HAVE_POLL 1
#define HAVE_POLL_FINE 1
#define HAVE_RECV 1
#define HAVE_SELECT 1
#define HAVE_SEND 1
#define HAVE_SETSOCKOPT 1
#define HAVE_SNPRINTF 1
#define HAVE_SOCKET 1
#define HAVE_STRCASECMP 1
#define HAVE_STRDUP 1
#define HAVE_STRTOLL 1
#define HAVE_FSTAT 1
#define HAVE_FTRUNCATE 1
/* #undef HAVE_UTIME */
/* #undef HAVE_UTIMES */
/* #undef HAVE_GETPWUID */
/* #undef HAVE_GETPWUID_R */

/* strerror_r: POSIX-style on WASI */
#define HAVE_STRERROR_R 1
#define HAVE_POSIX_STRERROR_R 1

/* Functions NOT available in WASI */
/* #undef HAVE_FORK */
/* #undef HAVE_PIPE */
/* #undef HAVE_SOCKETPAIR */
/* #undef HAVE_SIGACTION */
/* #undef HAVE_SIGNAL */
/* #undef HAVE_SIGSETJMP */
/* #undef HAVE_ALARM */
/* #undef HAVE_GETIFADDRS */
/* #undef HAVE_IF_NAMETOINDEX */
/* #undef HAVE_IOCTL */
/* #undef HAVE_SENDMSG */
/* #undef HAVE_RECVMSG */

/* Thread/process functions not available */
/* #undef USE_THREADS_POSIX */

/* ================================================================ */
/*               NETWORKING                                          */
/* ================================================================ */

/* Use POSIX socket API (provided by our patched sysroot via host_net) */
#define USE_RECV 1
#define USE_SEND 1
#define RECV_TYPE_ARG1 int
#define RECV_TYPE_ARG2 void *
#define RECV_TYPE_ARG3 size_t
#define RECV_TYPE_ARG4 int
#define RECV_TYPE_RETV ssize_t
#define SEND_TYPE_ARG1 int
#define SEND_TYPE_ARG2 const void *
#define SEND_TYPE_ARG3 size_t
#define SEND_TYPE_ARG4 int
#define SEND_QUAL_ARG2
#define SEND_TYPE_RETV ssize_t

/* MSG_NOSIGNAL not available in WASI — undef so libcurl uses 0 as send() flag */
/* #undef HAVE_MSG_NOSIGNAL */

/* IP versions */
/* #undef USE_IPV6 */

/* DNS */
#define HAVE_GETADDRINFO_THREADSAFE 1
#define USE_SYNC_DNS 1

/* ================================================================ */
/*  WASI SOCKET CONSTANT FIXUPS                                      */
/*  The WASI wasip1 sysroot guards most socket options behind        */
/*  __wasilibc_unmodified_upstream. Supply the missing POSIX          */
/*  constants so libcurl can work.                                   */
/* ================================================================ */

#ifndef SO_KEEPALIVE
#define SO_KEEPALIVE 9
#endif

#ifndef SO_REUSEADDR
#define SO_REUSEADDR 2
#endif

#ifndef TCP_NODELAY
#define TCP_NODELAY 1
#endif

#ifndef TCP_KEEPIDLE
#define TCP_KEEPIDLE 4
#endif

#ifndef TCP_KEEPINTVL
#define TCP_KEEPINTVL 5
#endif

#ifndef TCP_KEEPCNT
#define TCP_KEEPCNT 6
#endif

#ifndef IPPROTO_TCP
#define IPPROTO_TCP 6
#endif

#ifndef POLLPRI
#define POLLPRI 0x002
#endif

#ifndef POLLRDNORM
#define POLLRDNORM 0x040
#endif

#ifndef POLLWRNORM
#define POLLWRNORM 0x100
#endif

#ifndef POLLRDBAND
#define POLLRDBAND 0x080
#endif

/* ================================================================ */
/*               MISC                                                */
/* ================================================================ */

/* OS identification */
#define OS "wasm32-wasip1"
#define CURL_OS "wasm32-wasip1"

/* Large file support (WASI has 64-bit off_t natively) */
/* #undef USE_WIN32_LARGE_FILES */

/* Variadic macros */
#define HAVE_VARIADIC_MACROS_C99 1
#define HAVE_VARIADIC_MACROS_GCC 1

/* No async DNS resolver */
/* #undef USE_ARES */

/* Version strings */
#define CURL_CA_BUNDLE ""
#define CURL_CA_PATH ""

/* Disable features that need process/signal support */
/* #undef USE_UNIX_SOCKETS */
/* #undef HAVE_LINUX_TCP_H */

/* No GSS-API */
/* #undef HAVE_GSSAPI */

/* Compile as building the library */
#define BUILDING_LIBCURL 1
#define HAVE_CONFIG_H 1

/* Suppress some warnings */
#define CURL_HIDDEN_SYMBOLS 0
#define CURL_EXTERN_SYMBOL

#endif /* HEADER_CURL_CONFIG_WASI_H */
