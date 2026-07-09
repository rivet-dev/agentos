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
#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <strings.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <xalloc.h>

#include <mbedtls/ctr_drbg.h>
#include <mbedtls/entropy.h>
#include <mbedtls/error.h>
#include <mbedtls/net_sockets.h>
#include <mbedtls/pk.h>
#include <mbedtls/ssl.h>
#include <mbedtls/ssl_ciphersuites.h>
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
  mbedtls_ssl_session session;
  bool have_session;
  mbedtls_x509_crt cacert;
  mbedtls_x509_crl crl;
  bool have_crl;
  mbedtls_x509_crt client_cert;
  mbedtls_pk_context client_key;
  int *ciphersuites;
  mbedtls_ctr_drbg_context ctr_drbg;
  mbedtls_entropy_context entropy;
  int fd;
  int last_err;                 /* last mbedTLS error, for errstr */
  unsigned char *peekbuf;       /* buffered-but-unconsumed plaintext */
  int peeklen;
  int peekcap;
};

/* mbedTLS BIO send/recv over the raw, already-connected socket fd. TLS sockets
   are made nonblocking before the handshake so every WANT_READ/WANT_WRITE can
   be driven by readiness with Wget's Linux timeout semantics. */

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

static double
wasi_monotonic_seconds (void)
{
  struct timespec now;

  if (clock_gettime (CLOCK_MONOTONIC, &now) < 0)
    return -1;
  return (double) now.tv_sec + (double) now.tv_nsec / 1000000000.0;
}

/* Wait for the readiness mbedTLS requested. TIMEOUT == 0 means no timeout,
   matching Wget's command-line timeout semantics (select_fd itself uses zero
   as an immediate poll). START makes repeated WANT_* retries share one budget. */
static int
wasi_wait_fd (int fd, double timeout, double start, int wait_for)
{
  if (timeout == 0)
    {
      struct pollfd pfd;
      int ret;

      pfd.fd = fd;
      pfd.events = 0;
      pfd.revents = 0;
      if (wait_for & WAIT_FOR_READ)
        pfd.events |= POLLIN;
      if (wait_for & WAIT_FOR_WRITE)
        pfd.events |= POLLOUT;

      do
        ret = poll (&pfd, 1, -1);
      while (ret < 0 && errno == EINTR);
      return ret;
    }
  else
    {
      double now = wasi_monotonic_seconds ();
      double remaining;
      int ret;

      if (now < 0)
        return -1;
      remaining = timeout - (now - start);
      if (remaining <= 0)
        {
          errno = ETIMEDOUT;
          return -1;
        }

      ret = select_fd (fd, remaining, wait_for);
      if (ret == 0)
        {
          errno = ETIMEDOUT;
          return -1;
        }
      return ret;
    }
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
wasi_cipher_canonical_name (const char *input, char *output, size_t output_size)
{
  const char *p = input;
  size_t written = 0;

  if (0 == strncasecmp (p, "TLS", 3))
    {
      p += 3;
      /* mbedTLS spells TLS 1.3 suites TLS1-3-..., while OpenSSL uses the
         standard TLS_AES_... form. Ignore both prefixes for comparison. */
      if (p[0] == '1' && (p[1] == '-' || p[1] == '_') && p[2] == '3')
        p += 3;
    }

  while (*p && written + 1 < output_size)
    {
      if (0 == strncasecmp (p, "WITH", 4))
        {
          p += 4;
          continue;
        }
      if (isalnum ((unsigned char) *p))
        output[written++] = toupper ((unsigned char) *p);
      ++p;
    }
  output[written] = '\0';
}

static bool
wasi_cipher_name_matches (const char *name, const char *selector)
{
  char name_key[160];
  char selector_key[160];
  char rsa_selector_key[164];

  wasi_cipher_canonical_name (name, name_key, sizeof name_key);
  wasi_cipher_canonical_name (selector, selector_key, sizeof selector_key);
  if (0 == strcmp (name_key, selector_key))
    return true;

  /* OpenSSL omits the RSA key-exchange prefix in names such as
     AES128-GCM-SHA256; mbedTLS uses TLS-RSA-WITH-AES-128-GCM-SHA256. */
  if (strlen (selector_key) + 4 < sizeof rsa_selector_key)
    {
      strcpy (rsa_selector_key, "RSA");
      strcat (rsa_selector_key, selector_key);
      if (0 == strcmp (name_key, rsa_selector_key))
        return true;
    }
  return false;
}

static bool
wasi_cipher_is_tls13 (int id)
{
  const char *name = mbedtls_ssl_get_ciphersuite_name (id);
  return name && 0 == strncmp (name, "TLS1-3-", 7);
}

static bool
wasi_cipher_matches_class (int id, const char *selector)
{
  const char *name = mbedtls_ssl_get_ciphersuite_name (id);
  const mbedtls_ssl_ciphersuite_t *info =
    mbedtls_ssl_ciphersuite_from_id (id);

  if (!name || !info)
    return false;
  if (0 == strcasecmp (selector, "ALL")
      || 0 == strcasecmp (selector, "DEFAULT"))
    return true;
  if (0 == strcasecmp (selector, "HIGH"))
    return mbedtls_ssl_ciphersuite_get_cipher_key_bitlen (info) >= 128
           && !strstr (name, "-NULL-");
  if (0 == strcasecmp (selector, "aNULL"))
    return strstr (name, "-ANON-") != NULL;
  if (0 == strcasecmp (selector, "eNULL")
      || 0 == strcasecmp (selector, "NULL"))
    return strstr (name, "-NULL-") != NULL;
  if (0 == strcasecmp (selector, "kRSA"))
    return 0 == strncmp (name, "TLS-RSA-WITH-", 13);
  if (0 == strcasecmp (selector, "aRSA")
      || 0 == strcasecmp (selector, "RSA"))
    return strstr (name, "-RSA-") != NULL;
  if (0 == strcasecmp (selector, "PSK"))
    return strstr (name, "-PSK-") != NULL;
  if (0 == strcasecmp (selector, "SRP"))
    return strstr (name, "-SRP-") != NULL;
  if (0 == strcasecmp (selector, "RC4"))
    return strstr (name, "-RC4-") != NULL;
  if (0 == strcasecmp (selector, "MD5"))
    return strstr (name, "-MD5") != NULL;
  if (0 == strcasecmp (selector, "3DES"))
    return strstr (name, "-3DES-") != NULL;
  if (0 == strcasecmp (selector, "DES"))
    return strstr (name, "-DES-") != NULL
           || strstr (name, "-3DES-") != NULL;
  if (0 == strcasecmp (selector, "AES"))
    return strstr (name, "-AES-") != NULL;
  if (0 == strcasecmp (selector, "AESGCM"))
    return strstr (name, "-AES-") != NULL
           && strstr (name, "-GCM-") != NULL;
  if (0 == strcasecmp (selector, "CHACHA20"))
    return strstr (name, "-CHACHA20-") != NULL;
  return wasi_cipher_name_matches (name, selector);
}

static bool
wasi_configure_cipher_policy (struct wasi_ssl_context *ctx,
                              const char *policy)
{
  const int *available = mbedtls_ssl_list_ciphersuites ();
  size_t available_count = 0;
  size_t selected_count = 0;
  bool *disabled;
  char *copy;
  char *saveptr = NULL;
  char *token;

  while (available[available_count] != 0)
    ++available_count;
  ctx->ciphersuites = xnmalloc (available_count + 1,
                                sizeof *ctx->ciphersuites);
  disabled = xnmalloc (available_count, sizeof *disabled);
  memset (disabled, 0, available_count * sizeof *disabled);
  copy = xstrdup (policy);

  for (token = strtok_r (copy, ":, ", &saveptr);
       token;
       token = strtok_r (NULL, ":, ", &saveptr))
    {
      char operation = 0;
      bool matched = false;

      if (*token == '!' || *token == '-' || *token == '+')
        operation = *token++;
      if (*token == '\0' || *token == '@')
        goto unsupported;

      if (operation == '+')
        {
          /* OpenSSL's +selector moves already-enabled suites to the end. */
          size_t keep = 0;
          size_t moved = 0;
          int *move = xnmalloc (selected_count, sizeof *move);
          for (size_t i = 0; i < selected_count; ++i)
            if (wasi_cipher_matches_class (ctx->ciphersuites[i], token))
              {
                move[moved++] = ctx->ciphersuites[i];
                matched = true;
              }
            else
              ctx->ciphersuites[keep++] = ctx->ciphersuites[i];
          memcpy (ctx->ciphersuites + keep, move, moved * sizeof *move);
          selected_count = keep + moved;
          xfree (move);
          continue;
        }

      for (size_t i = 0; i < available_count; ++i)
        {
          bool already_selected = false;
          /* Native OpenSSL Wget applies --ciphers with
             SSL_CTX_set_cipher_list(), which intentionally controls only
             TLS 1.2-and-older suites. TLS 1.3 suites use a separate OpenSSL
             API and remain at their defaults. Preserve that split instead of
             allowing a TLS 1.2 policy to disable TLS 1.3 accidentally. */
          if (wasi_cipher_is_tls13 (available[i]))
            continue;
          if (!wasi_cipher_matches_class (available[i], token))
            continue;
          matched = true;

          if (operation == '!' || operation == '-')
            {
              size_t out = 0;
              if (operation == '!')
                disabled[i] = true;
              for (size_t j = 0; j < selected_count; ++j)
                if (ctx->ciphersuites[j] != available[i])
                  ctx->ciphersuites[out++] = ctx->ciphersuites[j];
              selected_count = out;
              continue;
            }

          if (disabled[i])
            continue;
          for (size_t j = 0; j < selected_count; ++j)
            if (ctx->ciphersuites[j] == available[i])
              already_selected = true;
          if (!already_selected)
            ctx->ciphersuites[selected_count++] = available[i];
        }

      /* OpenSSL accepts exclusions for algorithms absent from the compiled
         backend (for example !RC4 on a modern build). Unknown positive
         selectors are errors because otherwise the policy would broaden. */
      if (!matched && operation != '!' && operation != '-')
        goto unsupported;
    }

  xfree (copy);
  xfree (disabled);
  if (selected_count == 0)
    {
      logprintf (LOG_NOTQUIET,
                 _("Cipher policy %s selects no mbedTLS ciphersuites.\n"),
                 quote (policy));
      return false;
    }

  /* mbedTLS has one ciphersuite configuration API for every TLS version.
     Append its default TLS 1.3 suites after validating the OpenSSL-style
     pre-TLS-1.3 policy above, matching SSL_CTX_set_cipher_list semantics. */
  for (size_t i = 0; i < available_count; ++i)
    if (wasi_cipher_is_tls13 (available[i]))
      ctx->ciphersuites[selected_count++] = available[i];
  ctx->ciphersuites[selected_count] = 0;
  mbedtls_ssl_conf_ciphersuites (&ctx->conf, ctx->ciphersuites);
  return true;

 unsupported:
  logprintf (LOG_NOTQUIET,
             _("Unsupported mbedTLS cipher policy token '%s' in %s.\n"),
             token, quote (policy));
  xfree (copy);
  xfree (disabled);
  return false;
}

static bool
wasi_apply_secure_protocol (struct wasi_ssl_context *ctx)
{
  mbedtls_ssl_config *conf = &ctx->conf;
  bool cipher_policy_configured = opt.tls_ciphers_string != NULL;

  if (cipher_policy_configured
      && !wasi_configure_cipher_policy (ctx, opt.tls_ciphers_string))
    return false;

  switch (opt.secure_protocol)
    {
    case secure_protocol_tlsv1_3:
      mbedtls_ssl_conf_min_tls_version (conf, MBEDTLS_SSL_VERSION_TLS1_3);
      mbedtls_ssl_conf_max_tls_version (conf, MBEDTLS_SSL_VERSION_TLS1_3);
      return true;
    case secure_protocol_tlsv1_2:
      /* GNU Wget treats TLSv1_2 as a minimum, not an exact version: a
         TLS-1.3-only endpoint must remain reachable. */
      mbedtls_ssl_conf_min_tls_version (conf, MBEDTLS_SSL_VERSION_TLS1_2);
      return true;
    case secure_protocol_tlsv1:
    case secure_protocol_tlsv1_1:
      /* Not supported by mbedTLS 3.6; fall back to the lowest available. */
      mbedtls_ssl_conf_min_tls_version (conf, MBEDTLS_SSL_VERSION_TLS1_2);
      return true;
    case secure_protocol_sslv2:
    case secure_protocol_sslv3:
      /* A native modern Wget build rejects these unavailable protocols. Do
         not silently upgrade an explicitly requested SSLv2/SSLv3 connection
         to TLS, which could make a command succeed with different policy. */
      logprintf (LOG_NOTQUIET,
                 _("mbedTLS does not support requested protocol %s.\n"),
                 opt.secure_protocol_name);
      return false;
    case secure_protocol_auto:
    default:
      /* Library defaults: TLS 1.2 .. 1.3. */
      return true;
    case secure_protocol_pfs:
      {
        const int *available = mbedtls_ssl_list_ciphersuites ();
        size_t count = 0;
        size_t selected = 0;

        /* Like native OpenSSL Wget, an explicit --ciphers policy overrides
           the PFS preset rather than being intersected with it. */
        if (cipher_policy_configured)
          return true;

        while (available[count] != 0)
          ++count;
        ctx->ciphersuites = xnmalloc (count + 1, sizeof *ctx->ciphersuites);

        for (size_t i = 0; i < count; ++i)
          {
            const char *name =
              mbedtls_ssl_get_ciphersuite_name (available[i]);
            /* TLS 1.3 always uses (EC)DHE for certificate-authenticated
               handshakes. For TLS 1.2, retain only explicitly ephemeral
               DHE/ECDHE suites, excluding static RSA/ECDH and plain PSK. */
            if (name
                && (0 == strncmp (name, "TLS1-3-", 7)
                    || strstr (name, "-ECDHE-")
                    || strstr (name, "-DHE-")))
              ctx->ciphersuites[selected++] = available[i];
          }
        ctx->ciphersuites[selected] = 0;
        if (selected == 0)
          {
            logprintf (LOG_NOTQUIET,
                       _("mbedTLS has no forward-secret ciphersuites enabled.\n"));
            return false;
          }
        mbedtls_ssl_conf_ciphersuites (conf, ctx->ciphersuites);
        return true;
      }
    }
}

/* Configure the client certificate/key pair accepted by native Wget's
   --certificate and --private-key options. If only one path is provided,
   native OpenSSL Wget treats it as a combined PEM containing both objects. */
static int
wasi_load_client_credentials (struct wasi_ssl_context *ctx)
{
  const char *cert_path;
  const char *key_path;
  int ret;

  if (!opt.cert_file && !opt.private_key)
    return 0;

  cert_path = opt.cert_file ? opt.cert_file : opt.private_key;
  key_path = opt.private_key ? opt.private_key : opt.cert_file;

  ret = mbedtls_x509_crt_parse_file (&ctx->client_cert, cert_path);
  if (ret < 0)
    return ret;

  ret = mbedtls_pk_parse_keyfile (&ctx->client_key, key_path, NULL,
                                  mbedtls_ctr_drbg_random, &ctx->ctr_drbg);
  if (ret == MBEDTLS_ERR_PK_PASSWORD_REQUIRED)
    {
      char *password = getpass (_("Enter PEM pass phrase: "));
      if (!password)
        return ret;
      mbedtls_pk_free (&ctx->client_key);
      mbedtls_pk_init (&ctx->client_key);
      ret = mbedtls_pk_parse_keyfile (&ctx->client_key, key_path, password,
                                      mbedtls_ctr_drbg_random, &ctx->ctr_drbg);
    }
  if (ret < 0)
    return ret;

  ret = mbedtls_ssl_conf_own_cert (&ctx->conf, &ctx->client_cert,
                                   &ctx->client_key);
  return ret;
}

/* Load trust anchors in the same order as Linux Wget's GnuTLS backend: use the
   system trust store unless --ca-directory replaces it, then add the bundle
   supplied through --ca-certificate. Returns the number of sources parsed
   (>= 0) or a negative mbedTLS error. */
static int
wasi_load_trust (struct wasi_ssl_context *ctx)
{
  int loaded = 0;
  int ret;

  if (opt.ca_directory && opt.ca_directory[0] != '\0')
    {
      ret = mbedtls_x509_crt_parse_path (&ctx->cacert, opt.ca_directory);
      /* parse_path returns the number of files that failed to parse as a
         positive value; only a negative value is a hard error. */
      if (ret < 0)
        return ret;
      loaded += 1;
    }
  else
    {
      ret = mbedtls_x509_crt_parse_file (&ctx->cacert,
                                         WASI_TLS_DEFAULT_CA_BUNDLE);
      if (ret < 0)
        return ret;
      loaded += 1;
    }

  if (opt.ca_cert)
    {
      ret = mbedtls_x509_crt_parse_file (&ctx->cacert, opt.ca_cert);
      if (ret < 0)
        return ret;
      loaded += 1;
    }

  return loaded;
}

/* Perform the TLS handshake on FD and, on success, register the TLS transport
   so Wget's fd_* helpers use it. CONTINUE_SESSION requests the native Wget
   FTPS behavior: resume the control-channel session on a protected data
   connection. Returns true on success. */
bool
ssl_connect_wget (int fd, const char *hostname, int *continue_session)
{
  struct wasi_ssl_context *ctx;
  int original_flags = -1;
  bool nonblocking = false;
  double handshake_start;
  int ret;

  DEBUGP (("Initiating SSL handshake (mbedTLS).\n"));

  if (!hostname || hostname[0] == '\0')
    {
      errno = EINVAL;
      logprintf (LOG_NOTQUIET,
                 _("SSL handshake requires a non-empty server hostname.\n"));
      return false;
    }

  ctx = xnew0 (struct wasi_ssl_context);
  ctx->fd = fd;

  mbedtls_ssl_init (&ctx->ssl);
  mbedtls_ssl_config_init (&ctx->conf);
  mbedtls_ssl_session_init (&ctx->session);
  mbedtls_x509_crt_init (&ctx->cacert);
  mbedtls_x509_crl_init (&ctx->crl);
  mbedtls_x509_crt_init (&ctx->client_cert);
  mbedtls_pk_init (&ctx->client_key);
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

  if (!wasi_apply_secure_protocol (ctx))
    goto error;

  ret = wasi_load_client_credentials (ctx);
  if (ret != 0)
    {
      char errbuf[128];
      mbedtls_strerror (ret, errbuf, sizeof errbuf);
      logprintf (LOG_NOTQUIET,
                 _("Could not load TLS client certificate/key: %s\n"),
                 errbuf);
      goto error;
    }

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

  if (continue_session)
    {
      struct wasi_ssl_context *previous =
        fd_transport_context (*continue_session);
      if (previous && !previous->have_session)
        {
          /* Export lazily after Wget has finished using the control channel
             for commands. Exporting a TLS 1.3 session immediately after the
             handshake can precede the post-handshake NewSessionTicket and
             disturb application-record processing in mbedTLS 3.6. */
          mbedtls_ssl_session_free (&previous->session);
          mbedtls_ssl_session_init (&previous->session);
          if (mbedtls_ssl_get_session (&previous->ssl,
                                       &previous->session) == 0)
            previous->have_session = true;
        }
      if (!previous || !previous->have_session
          || mbedtls_ssl_set_session (&ctx->ssl, &previous->session) != 0)
        {
          logprintf (LOG_NOTQUIET,
                     _("Could not resume TLS session for socket %d.\n"), fd);
          goto error;
        }
    }

  /* SNI + the name checked during verification. Covers DNS and IP-address
     SANs (mbedTLS matches an IP literal against iPAddress SAN entries). */
  ret = mbedtls_ssl_set_hostname (&ctx->ssl, hostname);
  if (ret != 0)
    goto error;

  mbedtls_ssl_set_bio (&ctx->ssl, ctx, wasi_bio_send, wasi_bio_recv, NULL);

  original_flags = fcntl (fd, F_GETFL);
  if (original_flags < 0 || fcntl (fd, F_SETFL, original_flags | O_NONBLOCK) < 0)
    goto error;
  nonblocking = true;

  handshake_start = wasi_monotonic_seconds ();
  if (handshake_start < 0)
    goto error;

  do
    {
      ret = mbedtls_ssl_handshake (&ctx->ssl);
      if (ret == MBEDTLS_ERR_SSL_WANT_READ)
        {
          if (wasi_wait_fd (fd, opt.read_timeout, handshake_start,
                            WAIT_FOR_READ) <= 0)
            {
              ret = MBEDTLS_ERR_SSL_TIMEOUT;
              break;
            }
        }
      else if (ret == MBEDTLS_ERR_SSL_WANT_WRITE)
        {
          if (wasi_wait_fd (fd, opt.read_timeout, handshake_start,
                            WAIT_FOR_WRITE) <= 0)
            {
              ret = MBEDTLS_ERR_SSL_TIMEOUT;
              break;
            }
        }
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
  if (nonblocking)
    {
      int saved_errno = errno;
      if (fcntl (fd, F_SETFL, original_flags) < 0)
        logprintf (LOG_NOTQUIET,
                   _("Failed to restore socket flags after TLS handshake failure: %s\n"),
                   strerror (errno));
      errno = saved_errno;
    }
  mbedtls_ssl_free (&ctx->ssl);
  mbedtls_ssl_config_free (&ctx->conf);
  mbedtls_ssl_session_free (&ctx->session);
  mbedtls_x509_crt_free (&ctx->cacert);
  mbedtls_x509_crl_free (&ctx->crl);
  mbedtls_x509_crt_free (&ctx->client_cert);
  mbedtls_pk_free (&ctx->client_key);
  mbedtls_ctr_drbg_free (&ctx->ctr_drbg);
  mbedtls_entropy_free (&ctx->entropy);
  xfree (ctx->ciphersuites);
  xfree (ctx);
  return false;
}

/* --- Wget transport implementation over the TLS session --- */

static int
wasi_tls_read (int fd, char *buf, int bufsize, void *arg, double timeout)
{
  struct wasi_ssl_context *ctx = arg;
  double start;
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
  start = wasi_monotonic_seconds ();
  if (start < 0)
    return -1;

  do
    {
      ret = mbedtls_ssl_read (&ctx->ssl, (unsigned char *) buf, bufsize);
      if (ret == MBEDTLS_ERR_SSL_WANT_READ
          && wasi_wait_fd (fd, timeout, start, WAIT_FOR_READ) <= 0)
        return -1;
      if (ret == MBEDTLS_ERR_SSL_WANT_WRITE
          && wasi_wait_fd (fd, timeout, start, WAIT_FOR_WRITE) <= 0)
        return -1;
    }
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
wasi_tls_write (int fd, char *buf, int bufsize, void *arg)
{
  struct wasi_ssl_context *ctx = arg;
  int written = 0;

  while (written < bufsize)
    {
      int ret = mbedtls_ssl_write (&ctx->ssl,
                                   (const unsigned char *) buf + written,
                                   bufsize - written);
      if (ret == MBEDTLS_ERR_SSL_WANT_READ)
        {
          /* Linux Wget's OpenSSL/GnuTLS write callbacks block without applying
             the read timeout. Keep the socket nonblocking internally, but wait
             indefinitely for the readiness mbedTLS requested. */
          if (wasi_wait_fd (fd, 0, 0, WAIT_FOR_READ) <= 0)
            {
              ctx->last_err = MBEDTLS_ERR_SSL_TIMEOUT;
              return -1;
            }
          continue;
        }
      if (ret == MBEDTLS_ERR_SSL_WANT_WRITE)
        {
          if (wasi_wait_fd (fd, 0, 0, WAIT_FOR_WRITE) <= 0)
            {
              ctx->last_err = MBEDTLS_ERR_SSL_TIMEOUT;
              return -1;
            }
          continue;
        }
      if (ret < 0)
        {
          ctx->last_err = ret;
          return -1;
        }
      if (ret == 0)
        {
          errno = EIO;
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
  double start;
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
      start = wasi_monotonic_seconds ();
      if (start < 0)
        return -1;

      do
        {
          ret = mbedtls_ssl_read (&ctx->ssl, ctx->peekbuf, bufsize);
          if (ret == MBEDTLS_ERR_SSL_WANT_READ
              && wasi_wait_fd (fd, timeout, start, WAIT_FOR_READ) <= 0)
            return -1;
          if (ret == MBEDTLS_ERR_SSL_WANT_WRITE
              && wasi_wait_fd (fd, timeout, start, WAIT_FOR_WRITE) <= 0)
            return -1;
        }
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
  mbedtls_ssl_session_free (&ctx->session);
  mbedtls_x509_crt_free (&ctx->cacert);
  mbedtls_x509_crl_free (&ctx->crl);
  mbedtls_x509_crt_free (&ctx->client_cert);
  mbedtls_pk_free (&ctx->client_key);
  mbedtls_ctr_drbg_free (&ctx->ctr_drbg);
  mbedtls_entropy_free (&ctx->entropy);
  xfree (ctx->ciphersuites);
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

  if (!host || host[0] == '\0')
    {
      logprintf (LOG_NOTQUIET,
                 _("ERROR: cannot verify a certificate without a server hostname.\n"));
      return false;
    }

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
