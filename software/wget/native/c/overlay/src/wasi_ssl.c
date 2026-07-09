/* In-guest TLS backend for GNU Wget on wasm32-wasip1, built on mbedTLS.

   GNU Wget has no upstream mbedTLS backend; its TLS abstraction is the four
   functions declared in src/ssl.h (ssl_init, ssl_cleanup, ssl_connect_wget,
   ssl_check_certificate). This file implements those four against mbedTLS,
   performing a real TLS handshake and X.509 chain + hostname verification
   entirely inside the guest -- the sidecar is a dumb ciphertext pipe.

   The already-connected TCP file descriptor handed to ssl_connect_wget is a
   real socket carried by the patched wasi-libc sysroot (host_net imports), so
   the mbedTLS BIO callbacks simply read()/write() that fd. On success the fd is
   registered with Wget's transport layer (fd_register_transport) so that all
   subsequent fd_read/fd_write/fd_peek calls flow through the TLS session.

   Trust configuration mirrors Linux Wget: certificates are verified against
   /etc/ssl/certs/ca-certificates.crt by default, --ca-certificate
   (opt.ca_cert) and --ca-directory (opt.ca_directory) override the trust
   anchors, --crl-file (opt.crl_file) adds a revocation list, and
   --no-check-certificate (opt.check_cert != CHECK_CERT_ON) downgrades a
   verification failure to a warning. A failed handshake or a rejected
   certificate makes the relevant function return false, so http.c reports
   CONSSLERR / VERIFCERTERR exactly as with the GnuTLS/OpenSSL backends.  */

#include "wget.h"

#include <assert.h>
#include <errno.h>
#include <string.h>
#include <unistd.h>
#include <xalloc.h>

#include <mbedtls/ctr_drbg.h>
#include <mbedtls/entropy.h>
#include <mbedtls/error.h>
#include <mbedtls/net_sockets.h>
#include <mbedtls/ssl.h>
#include <mbedtls/x509_crt.h>

#include "connect.h"
#include "log.h"
#include "ssl.h"
#include "url.h"
#include "utils.h"

/* Debian-shaped default trust store, seeded into the VM by the native
   bootstrap. Matches curl's compile-time CA bundle default and OpenSSL's
   OPENSSLDIR resolution on Debian. */
#ifndef WASI_TLS_DEFAULT_CA_BUNDLE
# define WASI_TLS_DEFAULT_CA_BUNDLE "/etc/ssl/certs/ca-certificates.crt"
#endif

/* The TLS transport singleton, defined after the I/O callbacks below. */
static struct transport_implementation wasi_tls_transport;

struct wasi_ssl_context
{
  mbedtls_ssl_context ssl;
  mbedtls_ssl_config conf;
  mbedtls_x509_crt cacert;
  mbedtls_x509_crl crl;
  bool have_crl;
  mbedtls_ctr_drbg_context ctr_drbg;
  mbedtls_entropy_context entropy;
  int fd;
  int last_err;                 /* last mbedTLS error, for errstr */
  unsigned char *peekbuf;       /* buffered-but-unconsumed plaintext */
  int peeklen;
  int peekcap;
};

/* mbedTLS BIO send/recv over the raw, already-connected socket fd. Sockets are
   blocking on wasip1 (Wget only flips them non-blocking on Windows), but we map
   EAGAIN/EINTR anyway so the handshake and I/O paths stay correct if a socket
   is ever non-blocking. */

static int
wasi_bio_send (void *arg, const unsigned char *buf, size_t len)
{
  struct wasi_ssl_context *ctx = arg;
  ssize_t n;

  do
    n = write (ctx->fd, buf, len);
  while (n < 0 && errno == EINTR);

  if (n >= 0)
    return (int) n;
  if (errno == EAGAIN || errno == EWOULDBLOCK)
    return MBEDTLS_ERR_SSL_WANT_WRITE;
  return MBEDTLS_ERR_NET_SEND_FAILED;
}

static int
wasi_bio_recv (void *arg, unsigned char *buf, size_t len)
{
  struct wasi_ssl_context *ctx = arg;
  ssize_t n;

  do
    n = read (ctx->fd, buf, len);
  while (n < 0 && errno == EINTR);

  if (n >= 0)
    return (int) n;
  if (errno == EAGAIN || errno == EWOULDBLOCK)
    return MBEDTLS_ERR_SSL_WANT_READ;
  return MBEDTLS_ERR_NET_RECV_FAILED;
}

/* Global init. mbedTLS keeps no process-wide state we must set up, so this is
   a no-op that simply reports readiness, like the GnuTLS backend. */
bool
ssl_init (void)
{
  return true;
}

void
ssl_cleanup (void)
{
}

/* Map --secure-protocol onto the mbedTLS min/max TLS version knobs. mbedTLS
   3.6 only speaks TLS 1.2 and 1.3, so the legacy SSLv3/TLS1.0/1.1 selectors
   pin the floor at TLS 1.2 (the lowest still supported), matching how a modern
   Wget build behaves. */
static void
wasi_apply_secure_protocol (mbedtls_ssl_config *conf)
{
  switch (opt.secure_protocol)
    {
    case secure_protocol_tlsv1_3:
      mbedtls_ssl_conf_min_tls_version (conf, MBEDTLS_SSL_VERSION_TLS1_3);
      mbedtls_ssl_conf_max_tls_version (conf, MBEDTLS_SSL_VERSION_TLS1_3);
      break;
    case secure_protocol_tlsv1_2:
      mbedtls_ssl_conf_min_tls_version (conf, MBEDTLS_SSL_VERSION_TLS1_2);
      mbedtls_ssl_conf_max_tls_version (conf, MBEDTLS_SSL_VERSION_TLS1_2);
      break;
    case secure_protocol_tlsv1:
    case secure_protocol_tlsv1_1:
    case secure_protocol_sslv2:
    case secure_protocol_sslv3:
      /* Not supported by mbedTLS 3.6; fall back to the lowest available. */
      mbedtls_ssl_conf_min_tls_version (conf, MBEDTLS_SSL_VERSION_TLS1_2);
      break;
    case secure_protocol_auto:
    case secure_protocol_pfs:
    default:
      /* Library defaults: TLS 1.2 .. 1.3. */
      break;
    }
}

/* Load the trust anchors exactly like Linux Wget: --ca-certificate and
   --ca-directory when given, otherwise the seeded Debian bundle. Returns the
   number of certificates parsed (>= 0) or a negative mbedTLS error. */
static int
wasi_load_trust (struct wasi_ssl_context *ctx)
{
  int loaded = 0;
  int ret;

  if (opt.ca_cert)
    {
      ret = mbedtls_x509_crt_parse_file (&ctx->cacert, opt.ca_cert);
      if (ret < 0)
        return ret;
      loaded += 1;
    }

  if (opt.ca_directory && 0 != strcmp (opt.ca_directory, ""))
    {
      ret = mbedtls_x509_crt_parse_path (&ctx->cacert, opt.ca_directory);
      /* parse_path returns the number of files that failed to parse as a
         positive value; only a negative value is a hard error. */
      if (ret < 0)
        return ret;
      loaded += 1;
    }

  if (loaded == 0)
    {
      ret = mbedtls_x509_crt_parse_file (&ctx->cacert, WASI_TLS_DEFAULT_CA_BUNDLE);
      if (ret < 0)
        return ret;
      loaded += 1;
    }

  return loaded;
}

/* Perform the TLS handshake on FD and, on success, register the TLS transport
   so Wget's fd_* helpers use it. CONTINUE_SESSION (session resumption) is not
   supported here; http.c always passes NULL. Returns true on success. */
bool
ssl_connect_wget (int fd, const char *hostname, int *continue_session)
{
  struct wasi_ssl_context *ctx;
  int ret;

  (void) continue_session;

  DEBUGP (("Initiating SSL handshake (mbedTLS).\n"));

  ctx = xnew0 (struct wasi_ssl_context);
  ctx->fd = fd;

  mbedtls_ssl_init (&ctx->ssl);
  mbedtls_ssl_config_init (&ctx->conf);
  mbedtls_x509_crt_init (&ctx->cacert);
  mbedtls_x509_crl_init (&ctx->crl);
  mbedtls_ctr_drbg_init (&ctx->ctr_drbg);
  mbedtls_entropy_init (&ctx->entropy);

  ret = mbedtls_ctr_drbg_seed (&ctx->ctr_drbg, mbedtls_entropy_func,
                               &ctx->entropy,
                               (const unsigned char *) "agentos-wget-tls", 16);
  if (ret != 0)
    goto error;

  /* Load trust anchors. A failure to read the bundle is only fatal when the
     user asked us to verify; with --no-check-certificate we proceed with an
     empty trust store (verification is skipped in ssl_check_certificate). */
  ret = wasi_load_trust (ctx);
  if (ret < 0 && opt.check_cert == CHECK_CERT_ON)
    {
      char errbuf[128];
      mbedtls_strerror (ret, errbuf, sizeof errbuf);
      logprintf (LOG_NOTQUIET,
                 _("Could not load CA certificates: %s\n"), errbuf);
      goto error;
    }

  if (opt.crl_file)
    {
      ret = mbedtls_x509_crl_parse_file (&ctx->crl, opt.crl_file);
      if (ret != 0 && opt.check_cert == CHECK_CERT_ON)
        {
          char errbuf[128];
          mbedtls_strerror (ret, errbuf, sizeof errbuf);
          logprintf (LOG_NOTQUIET,
                     _("Could not load CRL from %s: %s\n"),
                     opt.crl_file, errbuf);
          goto error;
        }
      if (ret == 0)
        ctx->have_crl = true;
    }

  ret = mbedtls_ssl_config_defaults (&ctx->conf, MBEDTLS_SSL_IS_CLIENT,
                                     MBEDTLS_SSL_TRANSPORT_STREAM,
                                     MBEDTLS_SSL_PRESET_DEFAULT);
  if (ret != 0)
    goto error;

  wasi_apply_secure_protocol (&ctx->conf);

  /* Verify optionally: the handshake always completes and records the chain +
     hostname result, which ssl_check_certificate reads and turns into a
     pass/fail decision honoring opt.check_cert -- the same split the OpenSSL
     backend uses (SSL_VERIFY_NONE at handshake, manual check afterwards). */
  mbedtls_ssl_conf_authmode (&ctx->conf, MBEDTLS_SSL_VERIFY_OPTIONAL);
  mbedtls_ssl_conf_ca_chain (&ctx->conf, &ctx->cacert,
                             ctx->have_crl ? &ctx->crl : NULL);
  mbedtls_ssl_conf_rng (&ctx->conf, mbedtls_ctr_drbg_random, &ctx->ctr_drbg);

  ret = mbedtls_ssl_setup (&ctx->ssl, &ctx->conf);
  if (ret != 0)
    goto error;

  /* SNI + the name checked during verification. Covers DNS and IP-address
     SANs (mbedTLS matches an IP literal against iPAddress SAN entries). */
  ret = mbedtls_ssl_set_hostname (&ctx->ssl, hostname);
  if (ret != 0)
    goto error;

  mbedtls_ssl_set_bio (&ctx->ssl, ctx, wasi_bio_send, wasi_bio_recv, NULL);

  do
    {
      ret = mbedtls_ssl_handshake (&ctx->ssl);
      if (ret == MBEDTLS_ERR_SSL_WANT_READ)
        select_fd (fd, opt.read_timeout, WAIT_FOR_READ);
      else if (ret == MBEDTLS_ERR_SSL_WANT_WRITE)
        select_fd (fd, opt.read_timeout, WAIT_FOR_WRITE);
    }
  while (ret == MBEDTLS_ERR_SSL_WANT_READ || ret == MBEDTLS_ERR_SSL_WANT_WRITE);

  if (ret != 0)
    {
      char errbuf[128];
      mbedtls_strerror (ret, errbuf, sizeof errbuf);
      DEBUGP (("SSL handshake failed: %s\n", errbuf));
      logprintf (LOG_NOTQUIET, _("SSL handshake failed: %s\n"), errbuf);
      goto error;
    }

  /* Register FD with Wget's transport layer so fd_read/fd_write/fd_peek use
     our TLS callbacks from here on. */
  fd_register_transport (fd, &wasi_tls_transport, ctx);
  DEBUGP (("Handshake successful; TLS registered on socket %d\n", fd));

  return true;

 error:
  mbedtls_ssl_free (&ctx->ssl);
  mbedtls_ssl_config_free (&ctx->conf);
  mbedtls_x509_crt_free (&ctx->cacert);
  mbedtls_x509_crl_free (&ctx->crl);
  mbedtls_ctr_drbg_free (&ctx->ctr_drbg);
  mbedtls_entropy_free (&ctx->entropy);
  xfree (ctx);
  return false;
}

/* --- Wget transport implementation over the TLS session --- */

static int
wasi_tls_read (int fd, char *buf, int bufsize, void *arg, double timeout)
{
  struct wasi_ssl_context *ctx = arg;
  int ret;

  /* Serve any peeked-but-unconsumed plaintext first. */
  if (ctx->peeklen > 0)
    {
      int n = ctx->peeklen < bufsize ? ctx->peeklen : bufsize;
      memcpy (buf, ctx->peekbuf, n);
      if (n < ctx->peeklen)
        memmove (ctx->peekbuf, ctx->peekbuf + n, ctx->peeklen - n);
      ctx->peeklen -= n;
      return n;
    }

  if (timeout == -1)
    timeout = opt.read_timeout;
  if (timeout && mbedtls_ssl_get_bytes_avail (&ctx->ssl) == 0)
    {
      int sel = select_fd (fd, timeout, WAIT_FOR_READ);
      if (sel <= 0)
        return sel; /* 0 = timeout, -1 = error; matches Wget's expectation */
    }

  do
    ret = mbedtls_ssl_read (&ctx->ssl, (unsigned char *) buf, bufsize);
  while (ret == MBEDTLS_ERR_SSL_WANT_READ || ret == MBEDTLS_ERR_SSL_WANT_WRITE);

  if (ret == MBEDTLS_ERR_SSL_PEER_CLOSE_NOTIFY)
    return 0;
  if (ret < 0)
    {
      ctx->last_err = ret;
      return -1;
    }
  return ret;
}

static int
wasi_tls_write (int fd _GL_UNUSED, char *buf, int bufsize, void *arg)
{
  struct wasi_ssl_context *ctx = arg;
  int written = 0;

  while (written < bufsize)
    {
      int ret = mbedtls_ssl_write (&ctx->ssl,
                                   (const unsigned char *) buf + written,
                                   bufsize - written);
      if (ret == MBEDTLS_ERR_SSL_WANT_READ || ret == MBEDTLS_ERR_SSL_WANT_WRITE)
        continue;
      if (ret < 0)
        {
          ctx->last_err = ret;
          return -1;
        }
      written += ret;
    }
  return written;
}

static int
wasi_tls_poll (int fd, double timeout, int wait_for, void *arg)
{
  struct wasi_ssl_context *ctx = arg;

  if ((wait_for & WAIT_FOR_READ)
      && (ctx->peeklen > 0 || mbedtls_ssl_get_bytes_avail (&ctx->ssl) > 0))
    return 1;
  if (timeout == -1)
    timeout = opt.read_timeout;
  return select_fd (fd, timeout, wait_for);
}

static int
wasi_tls_peek (int fd, char *buf, int bufsize, void *arg, double timeout)
{
  struct wasi_ssl_context *ctx = arg;
  int n;

  /* Mirror recv(MSG_PEEK)/SSL_peek semantics: preview data without consuming
     it, returning as soon as *some* data is available -- one TLS record's
     worth. Never try to fill BUFSIZE (that would block on a keep-alive
     connection once the whole response fits in one record). Wget's
     fd_read_hunk consumes each preview and re-peeks for the next record, so
     returning one chunk at a time is sufficient and cannot deadlock.

     Data that mbedtls_ssl_read pulls off the wire here is retained in peekbuf
     so wasi_tls_read drains it first -- i.e. the "peek" does not lose it. */
  if (ctx->peeklen == 0)
    {
      int ret;

      if (ctx->peekcap < bufsize)
        {
          ctx->peekbuf = xrealloc (ctx->peekbuf, bufsize);
          ctx->peekcap = bufsize;
        }

      if (timeout == -1)
        timeout = opt.read_timeout;
      if (timeout && mbedtls_ssl_get_bytes_avail (&ctx->ssl) == 0)
        {
          int sel = select_fd (fd, timeout, WAIT_FOR_READ);
          if (sel <= 0)
            return sel; /* 0 = timeout, -1 = error */
        }

      do
        ret = mbedtls_ssl_read (&ctx->ssl, ctx->peekbuf, bufsize);
      while (ret == MBEDTLS_ERR_SSL_WANT_READ
             || ret == MBEDTLS_ERR_SSL_WANT_WRITE);

      if (ret == MBEDTLS_ERR_SSL_PEER_CLOSE_NOTIFY)
        return 0;
      if (ret < 0)
        {
          ctx->last_err = ret;
          return -1;
        }
      ctx->peeklen = ret;
    }

  n = ctx->peeklen < bufsize ? ctx->peeklen : bufsize;
  if (n > 0)
    memcpy (buf, ctx->peekbuf, n);
  return n;
}

static const char *
wasi_tls_errstr (int fd _GL_UNUSED, void *arg)
{
  struct wasi_ssl_context *ctx = arg;
  static char errbuf[160];

  if (ctx->last_err == 0)
    return NULL;
  mbedtls_strerror (ctx->last_err, errbuf, sizeof errbuf);
  return errbuf;
}

static void
wasi_tls_close (int fd, void *arg)
{
  struct wasi_ssl_context *ctx = arg;

  mbedtls_ssl_close_notify (&ctx->ssl);
  mbedtls_ssl_free (&ctx->ssl);
  mbedtls_ssl_config_free (&ctx->conf);
  mbedtls_x509_crt_free (&ctx->cacert);
  mbedtls_x509_crl_free (&ctx->crl);
  mbedtls_ctr_drbg_free (&ctx->ctr_drbg);
  mbedtls_entropy_free (&ctx->entropy);
  xfree (ctx->peekbuf);
  xfree (ctx);

  close (fd);
  DEBUGP (("Closed %d/SSL (mbedTLS)\n", fd));
}

static struct transport_implementation wasi_tls_transport = {
  wasi_tls_read, wasi_tls_write, wasi_tls_poll,
  wasi_tls_peek, wasi_tls_errstr, wasi_tls_close
};

/* Verify the peer certificate against the configured trust anchors and check
   that it matches HOST. Reads the verify result recorded during the handshake.
   Returns false only when verification failed AND the user requested checking
   (opt.check_cert == CHECK_CERT_ON); otherwise it warns and returns true,
   matching Wget's --no-check-certificate semantics. */
bool
ssl_check_certificate (int fd, const char *host)
{
  struct wasi_ssl_context *ctx = fd_transport_context (fd);
  uint32_t flags;
  bool success;
  const char *severity = opt.check_cert ? _("ERROR") : _("WARNING");

  if (!ctx)
    return opt.check_cert != CHECK_CERT_ON;

  flags = mbedtls_ssl_get_verify_result (&ctx->ssl);
  success = (flags == 0);

  if (!success)
    {
      char vbuf[512];
      int n = mbedtls_x509_crt_verify_info (vbuf, sizeof vbuf, "  ", flags);
      if (n < 0)
        {
          vbuf[0] = '\0';
          n = 0;
        }

      logprintf (LOG_NOTQUIET,
                 _("%s: cannot verify %s's certificate:\n"),
                 severity, quote (host));
      if (n > 0)
        logprintf (LOG_NOTQUIET, "%s", vbuf);

      if (opt.check_cert == CHECK_CERT_ON)
        logprintf (LOG_NOTQUIET,
                   _("To connect to %s insecurely, use `--no-check-certificate'.\n"),
                   quote (host));
    }
  else
    DEBUGP (("X509 certificate successfully verified and matches host %s\n",
             quote (host)));

  return opt.check_cert == CHECK_CERT_ON ? success : true;
}

/*
 * vim: tabstop=2 shiftwidth=2 softtabstop=2
 */
