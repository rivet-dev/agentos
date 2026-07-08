/* lib/curl_config.h.  Generated from curl_config.h.in by configure.  */
/* lib/curl_config.h.in.  Generated from configure.ac by autoheader.  */

/* Ignore c-ares deprecation warnings */
/* #undef CARES_NO_DEPRECATED */

/* to enable curl debug memory tracking */
/* #undef CURLDEBUG */

/* Location of default ca bundle */
#define CURL_CA_BUNDLE "/etc/ssl/certs/ca-certificates.crt"

/* define "1" to use built-in CA store of SSL library */
/* #undef CURL_CA_FALLBACK */

/* Location of default ca path */
/* #undef CURL_CA_PATH */

/* If safe CA bundle search is enabled */
/* #undef CURL_CA_SEARCH_SAFE */

/* Default SSL backend */
/* #undef CURL_DEFAULT_SSL_BACKEND */

/* disable alt-svc */
/* #undef CURL_DISABLE_ALTSVC */

/* to disable AWS sig support */
/* #undef CURL_DISABLE_AWS */

/* to disable basic authentication */
/* #undef CURL_DISABLE_BASIC_AUTH */

/* to disable bearer authentication */
/* #undef CURL_DISABLE_BEARER_AUTH */

/* disable local binding support */
/* #undef CURL_DISABLE_BINDLOCAL */

/* If unsafe CA bundle search in PATH on Windows is disabled */
/* #undef CURL_DISABLE_CA_SEARCH */

/* to disable cookies support */
/* #undef CURL_DISABLE_COOKIES */

/* to disable DICT */
/* #undef CURL_DISABLE_DICT */

/* to disable digest authentication */
/* #undef CURL_DISABLE_DIGEST_AUTH */

/* disable DoH */
/* #undef CURL_DISABLE_DOH */

/* to disable FILE */
/* #undef CURL_DISABLE_FILE */

/* disable form API */
/* #undef CURL_DISABLE_FORM_API */

/* to disable FTP */
/* #undef CURL_DISABLE_FTP */

/* to disable curl_easy_options */
/* #undef CURL_DISABLE_GETOPTIONS */

/* to disable Gopher */
/* #undef CURL_DISABLE_GOPHER */

/* disable headers-api */
/* #undef CURL_DISABLE_HEADERS_API */

/* disable alt-svc */
/* #undef CURL_DISABLE_HSTS */

/* to disable HTTP */
/* #undef CURL_DISABLE_HTTP */

/* disable HTTP authentication */
/* #undef CURL_DISABLE_HTTP_AUTH */

/* to disable IMAP */
/* #undef CURL_DISABLE_IMAP */

/* to disable IPFS */
/* #undef CURL_DISABLE_IPFS */

/* to disable kerberos authentication */
/* #undef CURL_DISABLE_KERBEROS_AUTH */

/* to disable LDAP */
#define CURL_DISABLE_LDAP 1

/* to disable LDAPS */
#define CURL_DISABLE_LDAPS 1

/* to disable --libcurl C code generation option */
/* #undef CURL_DISABLE_LIBCURL_OPTION */

/* disable mime API */
/* #undef CURL_DISABLE_MIME */

/* to disable MQTT */
/* #undef CURL_DISABLE_MQTT */

/* to disable negotiate authentication */
/* #undef CURL_DISABLE_NEGOTIATE_AUTH */

/* disable netrc parsing */
/* #undef CURL_DISABLE_NETRC */

/* to disable NTLM support */
/* #undef CURL_DISABLE_NTLM */

/* if the OpenSSL configuration won't be loaded automatically */
/* #undef CURL_DISABLE_OPENSSL_AUTO_LOAD_CONFIG */

/* disable date parsing */
/* #undef CURL_DISABLE_PARSEDATE */

/* to disable POP3 */
/* #undef CURL_DISABLE_POP3 */

/* disable progress-meter */
/* #undef CURL_DISABLE_PROGRESS_METER */

/* to disable proxies */
/* #undef CURL_DISABLE_PROXY */

/* to disable RTSP */
/* #undef CURL_DISABLE_RTSP */

/* disable SHA-512/256 hash algorithm */
/* #undef CURL_DISABLE_SHA512_256 */

/* disable DNS shuffling */
/* #undef CURL_DISABLE_SHUFFLE_DNS */

/* to disable SMB/CIFS */
/* #undef CURL_DISABLE_SMB */

/* to disable SMTP */
/* #undef CURL_DISABLE_SMTP */

/* to disable socketpair support */
/* #undef CURL_DISABLE_SOCKETPAIR */

/* to disable TELNET */
/* #undef CURL_DISABLE_TELNET */

/* to disable TFTP */
/* #undef CURL_DISABLE_TFTP */

/* to disable verbose strings */
/* #undef CURL_DISABLE_VERBOSE_STRINGS */

/* disable WebSockets */
/* #undef CURL_DISABLE_WEBSOCKETS */

/* Definition to make a library symbol externally visible. */
#define CURL_EXTERN_SYMBOL __attribute__((__visibility__("default")))

/* cpu-machine-OS */
#define CURL_OS "wasm32-unknown-wasi"

/* IP address type in sockaddr */
#define CURL_SA_FAMILY_T sa_family_t

/* built with multiple SSL backends */
/* #undef CURL_WITH_MULTI_SSL */

/* enable debug build options */
/* #undef DEBUGBUILD */

/* Define to the type of arg 2 for gethostname. */
#define GETHOSTNAME_TYPE_ARG2 size_t

/* Define to 1 if you have the alarm function. */
#define HAVE_ALARM 1

/* Define to 1 if you have the `arc4random' function. */
/* #undef HAVE_ARC4RANDOM */

/* Define to 1 if you have the <arpa/inet.h> header file. */
#define HAVE_ARPA_INET_H 1

/* Define to 1 if you have _Atomic support. */
#define HAVE_ATOMIC 1

/* Define to 1 if you have the basename function. */
#define HAVE_BASENAME 1

/* Define to 1 if bool is an available type. */
#define HAVE_BOOL_T 1

/* if BROTLI is in use */
#define HAVE_BROTLI 1

/* Define to 1 if you have the <brotli/decode.h> header file. */
#define HAVE_BROTLI_DECODE_H 1

/* Define to 1 if you have the __builtin_available function. */
/* #undef HAVE_BUILTIN_AVAILABLE */

/* Define to 1 if you have the clock_gettime function and monotonic timer. */
#define HAVE_CLOCK_GETTIME_MONOTONIC 1

/* Define to 1 if you have the clock_gettime function and raw monotonic timer.
   */
/* #undef HAVE_CLOCK_GETTIME_MONOTONIC_RAW */

/* Define to 1 if you have the closesocket function. */
/* #undef HAVE_CLOSESOCKET */

/* Define to 1 if you have the CloseSocket camel case function. */
/* #undef HAVE_CLOSESOCKET_CAMEL */

/* Define to 1 if you have the <crypto.h> header file. */
/* #undef HAVE_CRYPTO_H */

/* Define to 1 if you have the fseeko declaration */
#define HAVE_DECL_FSEEKO 1

/* Define to 1 if you have the declaration of `getpwuid_r', and to 0 if you
   don't. */
#define HAVE_DECL_GETPWUID_R 1

/* "Set if getpwuid_r() declaration is missing" */
/* #undef HAVE_DECL_GETPWUID_R_MISSING */

/* if you have <dirent.h> */
#define HAVE_DIRENT_H 1

/* Define to 1 if you have the <dlfcn.h> header file. */
#define HAVE_DLFCN_H 1

/* Define to 1 if you have the <err.h> header file. */
/* #undef HAVE_ERR_H */

/* Define to 1 if you have the `eventfd' function. */
/* #undef HAVE_EVENTFD */

/* Define to 1 if you have the fcntl function. */
#define HAVE_FCNTL 1

/* Define to 1 if you have the <fcntl.h> header file. */
#define HAVE_FCNTL_H 1

/* Define to 1 if you have a working fcntl O_NONBLOCK function. */
#define HAVE_FCNTL_O_NONBLOCK 1

/* Define to 1 if you have the `fnmatch' function. */
#define HAVE_FNMATCH 1

/* Define to 1 if you have the freeaddrinfo function. */
#define HAVE_FREEADDRINFO 1

/* Define to 1 if you have the `fseeko' function. */
#define HAVE_FSEEKO 1

/* Define to 1 if you have the fsetxattr function. */
/* #undef HAVE_FSETXATTR */

/* fsetxattr() takes 5 args */
/* #undef HAVE_FSETXATTR_5 */

/* fsetxattr() takes 6 args */
/* #undef HAVE_FSETXATTR_6 */

/* Define to 1 if you have the ftruncate function. */
#define HAVE_FTRUNCATE 1

/* Define to 1 if you have a working getaddrinfo function. */
#define HAVE_GETADDRINFO 1

/* Define to 1 if the getaddrinfo function is threadsafe. */
#define HAVE_GETADDRINFO_THREADSAFE 1

/* Define to 1 if you have the `geteuid' function. */
#define HAVE_GETEUID 1

/* Define to 1 if you have the gethostbyname function. */
#define HAVE_GETHOSTBYNAME 1

/* Define to 1 if you have the gethostbyname_r function. */
/* #undef HAVE_GETHOSTBYNAME_R */

/* gethostbyname_r() takes 3 args */
/* #undef HAVE_GETHOSTBYNAME_R_3 */

/* gethostbyname_r() takes 5 args */
/* #undef HAVE_GETHOSTBYNAME_R_5 */

/* gethostbyname_r() takes 6 args */
/* #undef HAVE_GETHOSTBYNAME_R_6 */

/* Define to 1 if you have the gethostname function. */
#define HAVE_GETHOSTNAME 1

/* Define to 1 if you have a working getifaddrs function. */
#define HAVE_GETIFADDRS 1

/* Define to 1 if you have the `getpass_r' function. */
/* #undef HAVE_GETPASS_R */

/* Define to 1 if you have the getpeername function. */
#define HAVE_GETPEERNAME 1

/* Define to 1 if you have the `getppid' function. */
#define HAVE_GETPPID 1

/* Define to 1 if you have the `getpwuid' function. */
#define HAVE_GETPWUID 1

/* Define to 1 if you have the `getpwuid_r' function. */
/* #undef HAVE_GETPWUID_R */

/* Define to 1 if you have the `getrlimit' function. */
#define HAVE_GETRLIMIT 1

/* Define to 1 if you have the getsockname function. */
#define HAVE_GETSOCKNAME 1

/* Define to 1 if you have the `gettimeofday' function. */
#define HAVE_GETTIMEOFDAY 1

/* Define to 1 if you have a working glibc-style strerror_r function. */
/* #undef HAVE_GLIBC_STRERROR_R */

/* Define to 1 if you have a working gmtime_r function. */
#define HAVE_GMTIME_R 1

/* if you have the function gnutls_srp_verifier */
/* #undef HAVE_GNUTLS_SRP */

/* if you have GSS-API libraries */
/* #undef HAVE_GSSAPI */

/* Define to 1 if you have the <gssapi/gssapi_generic.h> header file. */
/* #undef HAVE_GSSAPI_GSSAPI_GENERIC_H */

/* Define to 1 if you have the <gssapi/gssapi.h> header file. */
/* #undef HAVE_GSSAPI_GSSAPI_H */

/* Define to 1 if you have the <gssapi/gssapi_krb5.h> header file. */
/* #undef HAVE_GSSAPI_GSSAPI_KRB5_H */

/* if you have GNU GSS */
/* #undef HAVE_GSSGNU */

/* Define to 1 if you have the <hyper.h> header file. */
/* #undef HAVE_HYPER_H */

/* Define to 1 if you have the <idn2.h> header file. */
/* #undef HAVE_IDN2_H */

/* Define to 1 if you have the <ifaddrs.h> header file. */
#define HAVE_IFADDRS_H 1

/* Define to 1 if you have the `if_nametoindex' function. */
/* #undef HAVE_IF_NAMETOINDEX */

/* Define to 1 if you have a IPv6 capable working inet_ntop function. */
#define HAVE_INET_NTOP 1

/* Define to 1 if you have a IPv6 capable working inet_pton function. */
#define HAVE_INET_PTON 1

/* Define to 1 if you have the <inttypes.h> header file. */
#define HAVE_INTTYPES_H 1

/* Define to 1 if you have the ioctl function. */
#define HAVE_IOCTL 1

/* Define to 1 if you have the ioctlsocket function. */
/* #undef HAVE_IOCTLSOCKET */

/* Define to 1 if you have the IoctlSocket camel case function. */
/* #undef HAVE_IOCTLSOCKET_CAMEL */

/* Define to 1 if you have a working IoctlSocket camel case FIONBIO function.
   */
/* #undef HAVE_IOCTLSOCKET_CAMEL_FIONBIO */

/* Define to 1 if you have a working ioctlsocket FIONBIO function. */
/* #undef HAVE_IOCTLSOCKET_FIONBIO */

/* Define to 1 if you have a working ioctl FIONBIO function. */
#define HAVE_IOCTL_FIONBIO 1

/* Define to 1 if you have a working ioctl SIOCGIFADDR function. */
/* #undef HAVE_IOCTL_SIOCGIFADDR */

/* Define to 1 if you have the <io.h> header file. */
/* #undef HAVE_IO_H */

/* Define to 1 if you have the lber.h header file. */
/* #undef HAVE_LBER_H */

/* Define to 1 if you have the ldap.h header file. */
/* #undef HAVE_LDAP_H */

/* Define to 1 if you have the `ldap_init_fd' function. */
/* #undef HAVE_LDAP_INIT_FD */

/* Use LDAPS implementation */
/* #undef HAVE_LDAP_SSL */

/* Define to 1 if you have the ldap_ssl.h header file. */
/* #undef HAVE_LDAP_SSL_H */

/* Define to 1 if you have the `ldap_url_parse' function. */
/* #undef HAVE_LDAP_URL_PARSE */

/* Define to 1 if you have the `brotlidec' library (-lbrotlidec). */
#define HAVE_LIBBROTLIDEC 1

/* Define to 1 if you have the <libgen.h> header file. */
#define HAVE_LIBGEN_H 1

/* Define to 1 if you have the `idn2' library (-lidn2). */
/* #undef HAVE_LIBIDN2 */

/* Define to 1 if you have the <libpsl.h> header file. */
/* #undef HAVE_LIBPSL_H */

/* Define to 1 if using LibreSSL. */
/* #undef HAVE_LIBRESSL */

/* Define to 1 if you have the <librtmp/rtmp.h> header file. */
/* #undef HAVE_LIBRTMP_RTMP_H */

/* Define to 1 if you have the `ssh' library (-lssh). */
/* #undef HAVE_LIBSSH */

/* Define to 1 if you have the `ssh2' library (-lssh2). */
/* #undef HAVE_LIBSSH2 */

/* Define to 1 if you have the `ssl' library (-lssl). */
/* #undef HAVE_LIBSSL */

/* Define to 1 if you have the `wolfssh' library (-lwolfssh). */
/* #undef HAVE_LIBWOLFSSH */

/* if zlib is available */
#define HAVE_LIBZ 1

/* Define to 1 if you have the `zstd' library (-lzstd). */
#define HAVE_LIBZSTD 1

/* Define to 1 if you have the <linux/tcp.h> header file. */
/* #undef HAVE_LINUX_TCP_H */

/* Define to 1 if you have the <locale.h> header file. */
#define HAVE_LOCALE_H 1

/* Define to 1 if the compiler supports the 'long long' data type. */
#define HAVE_LONGLONG 1

/* Define to 1 if you have the `mach_absolute_time' function. */
/* #undef HAVE_MACH_ABSOLUTE_TIME */

/* Define to 1 if you have the memrchr function or macro. */
/* #undef HAVE_MEMRCHR */

/* Define to 1 if you have the MSG_NOSIGNAL flag. */
#define HAVE_MSG_NOSIGNAL 1

/* Define to 1 if you have the <msh3.h> header file. */
/* #undef HAVE_MSH3_H */

/* Define to 1 if you have the <netdb.h> header file. */
#define HAVE_NETDB_H 1

/* Define to 1 if you have the <netinet/in6.h> header file. */
/* #undef HAVE_NETINET_IN6_H */

/* Define to 1 if you have the <netinet/in.h> header file. */
#define HAVE_NETINET_IN_H 1

/* Define to 1 if you have the <netinet/tcp.h> header file. */
#define HAVE_NETINET_TCP_H 1

/* Define to 1 if you have the <netinet/udp.h> header file. */
#define HAVE_NETINET_UDP_H 1

/* Define to 1 if you have the <net/if.h> header file. */
/* #undef HAVE_NET_IF_H */

/* Define to 1 if you have the <nghttp2/nghttp2.h> header file. */
/* #undef HAVE_NGHTTP2_NGHTTP2_H */

/* Define to 1 if you have the <nghttp3/nghttp3.h> header file. */
/* #undef HAVE_NGHTTP3_NGHTTP3_H */

/* Define to 1 if you have the <ngtcp2/ngtcp2_crypto.h> header file. */
/* #undef HAVE_NGTCP2_NGTCP2_CRYPTO_H */

/* Define to 1 if you have the <ngtcp2/ngtcp2.h> header file. */
/* #undef HAVE_NGTCP2_NGTCP2_H */

/* if you have an old MIT Kerberos version, lacking GSS_C_NT_HOSTBASED_SERVICE
   */
/* #undef HAVE_OLD_GSSMIT */

/* if you have opendir */
#define HAVE_OPENDIR 1

/* Define to 1 if you have the <openssl/crypto.h> header file. */
/* #undef HAVE_OPENSSL_CRYPTO_H */

/* Define to 1 if you have the <openssl/err.h> header file. */
/* #undef HAVE_OPENSSL_ERR_H */

/* Define to 1 if you have the <openssl/pem.h> header file. */
/* #undef HAVE_OPENSSL_PEM_H */

/* Define to 1 if you have the <openssl/rsa.h> header file. */
/* #undef HAVE_OPENSSL_RSA_H */

/* if you have the functions SSL_CTX_set_srp_username and
   SSL_CTX_set_srp_password */
/* #undef HAVE_OPENSSL_SRP */

/* Define to 1 if you have the <openssl/ssl.h> header file. */
/* #undef HAVE_OPENSSL_SSL_H */

/* Define to 1 if you have the <openssl/x509.h> header file. */
/* #undef HAVE_OPENSSL_X509_H */

/* Define to 1 if you have the <pem.h> header file. */
/* #undef HAVE_PEM_H */

/* Define to 1 if you have the `pipe' function. */
#define HAVE_PIPE 1

/* Define to 1 if you have the `poll' function. */
#define HAVE_POLL 1

/* Define to 1 if you have the <poll.h> header file. */
#define HAVE_POLL_H 1

/* Define to 1 if you have a working POSIX-style strerror_r function. */
#define HAVE_POSIX_STRERROR_R 1

/* Define to 1 if you have the <proto/bsdsocket.h> header file. */
/* #undef HAVE_PROTO_BSDSOCKET_H */

/* if you have <pthread.h> */
/* #undef HAVE_PTHREAD_H */

/* Define to 1 if you have the <pwd.h> header file. */
#define HAVE_PWD_H 1

/* Define to 1 if you have the `quiche_conn_set_qlog_fd' function. */
/* #undef HAVE_QUICHE_CONN_SET_QLOG_FD */

/* Define to 1 if you have the <quiche.h> header file. */
/* #undef HAVE_QUICHE_H */

/* Define to 1 if you have the recv function. */
#define HAVE_RECV 1

/* Define to 1 if you have the <rsa.h> header file. */
/* #undef HAVE_RSA_H */

/* Define to 1 if you have the `sched_yield' function. */
#define HAVE_SCHED_YIELD 1

/* Define to 1 if you have the select function. */
#define HAVE_SELECT 1

/* Define to 1 if you have the send function. */
#define HAVE_SEND 1

/* Define to 1 if you have the `sendmmsg' function. */
/* #undef HAVE_SENDMMSG */

/* Define to 1 if you have the `sendmsg' function. */
/* #undef HAVE_SENDMSG */

/* Define to 1 if you have the <setjmp.h> header file. */
#define HAVE_SETJMP_H 1

/* Define to 1 if you have the `setlocale' function. */
#define HAVE_SETLOCALE 1

/* Define to 1 if you have the `setmode' function. */
/* #undef HAVE_SETMODE */

/* Define to 1 if you have the `setrlimit' function. */
#define HAVE_SETRLIMIT 1

/* Define to 1 if you have the sigaction function. */
#define HAVE_SIGACTION 1

/* Define to 1 if you have the siginterrupt function. */
/* #undef HAVE_SIGINTERRUPT */

/* Define to 1 if you have the signal function. */
#define HAVE_SIGNAL 1

/* Define to 1 if you have the sigsetjmp function or macro. */
/* #undef HAVE_SIGSETJMP */

/* Define to 1 if you have the `snprintf' function. */
#define HAVE_SNPRINTF 1

/* Define to 1 if struct sockaddr_in6 has the sin6_scope_id member */
#define HAVE_SOCKADDR_IN6_SIN6_SCOPE_ID 1

/* Define to 1 if you have the socket function. */
#define HAVE_SOCKET 1

/* Define to 1 if you have the socketpair function. */
/* #undef HAVE_SOCKETPAIR */

/* Define to 1 if you have the `SSL_ech_set1_echconfig' function. */
/* #undef HAVE_SSL_ECH_SET1_ECHCONFIG */

/* Define to 1 if you have the <ssl.h> header file. */
/* #undef HAVE_SSL_H */

/* Define to 1 if you have the `SSL_set0_wbio' function. */
/* #undef HAVE_SSL_SET0_WBIO */

/* Define to 1 if you have the `SSL_set1_ech_config_list' function. */
/* #undef HAVE_SSL_SET1_ECH_CONFIG_LIST */

/* Define to 1 if you have the `SSL_set_quic_use_legacy_codepoint' function.
   */
/* #undef HAVE_SSL_SET_QUIC_USE_LEGACY_CODEPOINT */

/* Define to 1 if you have the <stdatomic.h> header file. */
#define HAVE_STDATOMIC_H 1

/* Define to 1 if you have the <stdbool.h> header file. */
#define HAVE_STDBOOL_H 1

/* Define to 1 if you have the <stdint.h> header file. */
#define HAVE_STDINT_H 1

/* Define to 1 if you have the <stdio.h> header file. */
#define HAVE_STDIO_H 1

/* Define to 1 if you have the <stdlib.h> header file. */
#define HAVE_STDLIB_H 1

/* Define to 1 if you have the strcasecmp function. */
#define HAVE_STRCASECMP 1

/* Define to 1 if you have the strcmpi function. */
/* #undef HAVE_STRCMPI */

/* Define to 1 if you have the strdup function. */
#define HAVE_STRDUP 1

/* Define to 1 if you have the strerror_r function. */
#define HAVE_STRERROR_R 1

/* Define to 1 if you have the stricmp function. */
/* #undef HAVE_STRICMP */

/* Define to 1 if you have the <strings.h> header file. */
#define HAVE_STRINGS_H 1

/* Define to 1 if you have the <string.h> header file. */
#define HAVE_STRING_H 1

/* Define to 1 if you have the <stropts.h> header file. */
#define HAVE_STROPTS_H 1

/* Define to 1 if you have the strtok_r function. */
#define HAVE_STRTOK_R 1

/* Define to 1 if you have the strtoll function. */
#define HAVE_STRTOLL 1

/* if struct sockaddr_storage is defined */
#define HAVE_STRUCT_SOCKADDR_STORAGE 1

/* Define to 1 if you have the timeval struct. */
#define HAVE_STRUCT_TIMEVAL 1

/* Define to 1 if suseconds_t is an available type. */
#define HAVE_SUSECONDS_T 1

/* Define to 1 if you have the <sys/eventfd.h> header file. */
#define HAVE_SYS_EVENTFD_H 1

/* Define to 1 if you have the <sys/filio.h> header file. */
/* #undef HAVE_SYS_FILIO_H */

/* Define to 1 if you have the <sys/ioctl.h> header file. */
#define HAVE_SYS_IOCTL_H 1

/* Define to 1 if you have the <sys/param.h> header file. */
#define HAVE_SYS_PARAM_H 1

/* Define to 1 if you have the <sys/poll.h> header file. */
#define HAVE_SYS_POLL_H 1

/* Define to 1 if you have the <sys/resource.h> header file. */
#define HAVE_SYS_RESOURCE_H 1

/* Define to 1 if you have the <sys/select.h> header file. */
#define HAVE_SYS_SELECT_H 1

/* Define to 1 if you have the <sys/socket.h> header file. */
#define HAVE_SYS_SOCKET_H 1

/* Define to 1 if you have the <sys/sockio.h> header file. */
/* #undef HAVE_SYS_SOCKIO_H */

/* Define to 1 if you have the <sys/stat.h> header file. */
#define HAVE_SYS_STAT_H 1

/* Define to 1 if you have the <sys/time.h> header file. */
#define HAVE_SYS_TIME_H 1

/* Define to 1 if you have the <sys/types.h> header file. */
#define HAVE_SYS_TYPES_H 1

/* Define to 1 if you have the <sys/un.h> header file. */
#define HAVE_SYS_UN_H 1

/* Define to 1 if you have the <sys/utime.h> header file. */
/* #undef HAVE_SYS_UTIME_H */

/* Define to 1 if you have the <sys/wait.h> header file. */
#define HAVE_SYS_WAIT_H 1

/* Define to 1 if you have the <sys/xattr.h> header file. */
/* #undef HAVE_SYS_XATTR_H */

/* Define to 1 if you have the <termios.h> header file. */
#define HAVE_TERMIOS_H 1

/* Define to 1 if you have the <termio.h> header file. */
/* #undef HAVE_TERMIO_H */

/* Define this if time_t is unsigned */
/* #undef HAVE_TIME_T_UNSIGNED */

/* Define to 1 if you have the <unicode/uidna.h> header file. */
/* #undef HAVE_UNICODE_UIDNA_H */

/* Define to 1 if you have the <unistd.h> header file. */
#define HAVE_UNISTD_H 1

/* Define to 1 if you have the `utime' function. */
#define HAVE_UTIME 1

/* Define to 1 if you have the `utimes' function. */
#define HAVE_UTIMES 1

/* Define to 1 if you have the <utime.h> header file. */
#define HAVE_UTIME_H 1

/* Define to 1 if you have the <uv.h> header file. */
/* #undef HAVE_UV_H */

/* Define to 1 if you have the <wolfssh/ssh.h> header file. */
/* #undef HAVE_WOLFSSH_SSH_H */

/* if you have wolfSSL_BIO_new */
/* #undef HAVE_WOLFSSL_BIO */

/* Define to 1 if you have the `wolfSSL_CTX_GenerateEchConfig' function. */
/* #undef HAVE_WOLFSSL_CTX_GENERATEECHCONFIG */

/* if you have wolfSSL_DES_ecb_encrypt */
/* #undef HAVE_WOLFSSL_DES_ECB_ENCRYPT */

/* if you have wolfSSL_BIO_set_shutdown */
/* #undef HAVE_WOLFSSL_FULL_BIO */

/* Define to 1 if you have the `wolfSSL_get_peer_certificate' function. */
/* #undef HAVE_WOLFSSL_GET_PEER_CERTIFICATE */

/* Define to 1 if you have the `wolfSSL_UseALPN' function. */
/* #undef HAVE_WOLFSSL_USEALPN */

/* Define this symbol if your OS supports changing the contents of argv */
/* #undef HAVE_WRITABLE_ARGV */

/* Define to 1 if you have the <x509.h> header file. */
/* #undef HAVE_X509_H */

/* if libzstd is in use */
#define HAVE_ZSTD 1

/* Define to 1 if you have the <zstd.h> header file. */
#define HAVE_ZSTD_H 1

/* Define to 1 if you have the `_setmode' function. */
/* #undef HAVE__SETMODE */

/* Define to the sub-directory where libtool stores uninstalled libraries. */
#define LT_OBJDIR ".libs/"

/* Define to 1 if you need the lber.h header file even with ldap.h */
/* #undef NEED_LBER_H */

/* Define to 1 if _REENTRANT preprocessor symbol must be defined. */
/* #undef NEED_REENTRANT */

/* Define to 1 if _THREAD_SAFE preprocessor symbol must be defined. */
/* #undef NEED_THREAD_SAFE */

/* Name of package */
#define PACKAGE "curl"

/* Define to the address where bug reports for this package should be sent. */
#define PACKAGE_BUGREPORT "a suitable curl mailing list: https://curl.se/mail/"

/* Define to the full name of this package. */
#define PACKAGE_NAME "curl"

/* Define to the full name and version of this package. */
#define PACKAGE_STRING "curl -"

/* Define to the one symbol short name of this package. */
#define PACKAGE_TARNAME "curl"

/* Define to the home page for this package. */
#define PACKAGE_URL ""

/* Define to the version of this package. */
#define PACKAGE_VERSION "-"

/* Size of curl_off_t in number of bytes */
#define SIZEOF_CURL_OFF_T 8

/* Size of curl_socket_t in number of bytes */
#define SIZEOF_CURL_SOCKET_T 4

/* Size of int in number of bytes */
#define SIZEOF_INT 4

/* Size of long in number of bytes */
#define SIZEOF_LONG 4

/* Size of long long in number of bytes */
/* #undef SIZEOF_LONG_LONG */

/* Size of off_t in number of bytes */
#define SIZEOF_OFF_T 8

/* Size of size_t in number of bytes */
#define SIZEOF_SIZE_T 4

/* Size of time_t in number of bytes */
#define SIZEOF_TIME_T 8

/* Define to 1 if all of the C90 standard headers exist (not just the ones
   required in a freestanding environment). This macro is provided for
   backward compatibility; new code need not use it. */
#define STDC_HEADERS 1

/* if AmiSSL is in use */
/* #undef USE_AMISSL */

/* if AppleIDN */
/* #undef USE_APPLE_IDN */

/* Define to enable c-ares support */
/* #undef USE_ARES */

/* if BearSSL is enabled */
/* #undef USE_BEARSSL */

/* if ECH support is available */
/* #undef USE_ECH */

/* if GnuTLS is enabled */
/* #undef USE_GNUTLS */

/* GSASL support enabled */
/* #undef USE_GSASL */

/* enable HTTPS RR support */
/* #undef USE_HTTPSRR */

/* if hyper is in use */
/* #undef USE_HYPER */

/* Define if you want to enable IPv6 support */
#define USE_IPV6 1

/* if libpsl is in use */
/* #undef USE_LIBPSL */

/* if librtmp is in use */
/* #undef USE_LIBRTMP */

/* if libssh is in use */
/* #undef USE_LIBSSH */

/* if libssh2 is in use */
/* #undef USE_LIBSSH2 */

/* if libuv is in use */
/* #undef USE_LIBUV */

/* if mbedTLS is enabled */
#define USE_MBEDTLS 1

/* if msh3 is in use */
/* #undef USE_MSH3 */

/* if nghttp2 is in use */
/* #undef USE_NGHTTP2 */

/* if nghttp3 is in use */
/* #undef USE_NGHTTP3 */

/* if ngtcp2 is in use */
/* #undef USE_NGTCP2 */

/* if ngtcp2_crypto_boringssl is in use */
/* #undef USE_NGTCP2_CRYPTO_BORINGSSL */

/* if ngtcp2_crypto_gnutls is in use */
/* #undef USE_NGTCP2_CRYPTO_GNUTLS */

/* if ngtcp2_crypto_quictls is in use */
/* #undef USE_NGTCP2_CRYPTO_QUICTLS */

/* if ngtcp2_crypto_wolfssl is in use */
/* #undef USE_NGTCP2_CRYPTO_WOLFSSL */

/* if ngtcp2 + nghttp3 is in use */
/* #undef USE_NGTCP2_H3 */

/* Use OpenLDAP-specific code */
/* #undef USE_OPENLDAP */

/* if OpenSSL is in use */
/* #undef USE_OPENSSL */

/* if openssl quic + nghttp3 is in use */
/* #undef USE_OPENSSL_H3 */

/* if openssl QUIC is in use */
/* #undef USE_OPENSSL_QUIC */

/* if quiche is in use */
/* #undef USE_QUICHE */

/* if Rustls is enabled */
/* #undef USE_RUSTLS */

/* to enable Windows native SSL/TLS support */
/* #undef USE_SCHANNEL */

/* enable Secure Transport */
/* #undef USE_SECTRANSP */

/* if you want POSIX threaded DNS lookup */
/* #undef USE_THREADS_POSIX */

/* if you want Win32 threaded DNS lookup */
/* #undef USE_THREADS_WIN32 */

/* Use TLS-SRP authentication */
/* #undef USE_TLS_SRP */

/* Use Unix domain sockets */
#define USE_UNIX_SOCKETS 1

/* Define to 1 if you are building a Windows target with crypto API support.
   */
/* #undef USE_WIN32_CRYPTO */

/* Define to 1 if you have the `normaliz' (WinIDN) library (-lnormaliz). */
/* #undef USE_WIN32_IDN */

/* Define to 1 if you are building a Windows target with large file support.
   */
/* #undef USE_WIN32_LARGE_FILES */

/* Use Windows LDAP implementation */
/* #undef USE_WIN32_LDAP */

/* Define to 1 if you are building a Windows target without large file
   support. */
/* #undef USE_WIN32_SMALL_FILES */

/* to enable SSPI support */
/* #undef USE_WINDOWS_SSPI */

/* if wolfSSH is in use */
/* #undef USE_WOLFSSH */

/* if wolfSSL is enabled */
/* #undef USE_WOLFSSL */

/* Version number of package */
#define VERSION "-"

/* Define to 1 if OS is AIX. */
#ifndef _ALL_SOURCE
/* #  undef _ALL_SOURCE */
#endif

/* Number of bits in a file offset, on hosts where this is settable. */
/* #undef _FILE_OFFSET_BITS */

/* Define for large files, on AIX-style hosts. */
/* #undef _LARGE_FILES */

/* Define to empty if `const' does not conform to ANSI C. */
/* #undef const */

/* Type to use in place of in_addr_t when system does not provide it. */
/* #undef in_addr_t */

/* Define to `unsigned int' if <sys/types.h> does not define. */
/* #undef size_t */

/* the signed version of size_t */
/* #undef ssize_t */
