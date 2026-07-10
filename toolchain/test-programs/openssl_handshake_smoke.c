#include <openssl/err.h>
#include <openssl/pem.h>
#include <openssl/ssl.h>
#include <stdio.h>
#include <string.h>

static int fail(const char *message) {
  fprintf(stderr, "openssl handshake smoke: %s\n", message);
  ERR_print_errors_fp(stderr);
  return 1;
}

static int handshake_step(SSL *ssl, int *complete) {
  int result = SSL_do_handshake(ssl);
  if (result == 1) {
    *complete = 1;
    return 0;
  }

  int error = SSL_get_error(ssl, result);
  if (error == SSL_ERROR_WANT_READ || error == SSL_ERROR_WANT_WRITE) {
    return 0;
  }
  return -1;
}

int main(int argc, char **argv) {
  if (argc != 3) {
    fprintf(stderr, "usage: openssl_handshake_smoke CERT.pem KEY.pem\n");
    return 2;
  }

  SSL_CTX *client_context = NULL;
  SSL_CTX *server_context = NULL;
  SSL *client = NULL;
  SSL *server = NULL;
  int exit_code = 1;

  if (OPENSSL_init_ssl(0, NULL) != 1) {
    return fail("OPENSSL_init_ssl failed");
  }
  client_context = SSL_CTX_new(TLS_client_method());
  server_context = SSL_CTX_new(TLS_server_method());
  if (client_context == NULL || server_context == NULL) {
    fail("SSL_CTX_new failed");
    goto cleanup;
  }

  SSL_CTX_set_verify(client_context, SSL_VERIFY_NONE, NULL);
  if (SSL_CTX_use_certificate_file(server_context, argv[1], SSL_FILETYPE_PEM) != 1 ||
      SSL_CTX_use_PrivateKey_file(server_context, argv[2], SSL_FILETYPE_PEM) != 1 ||
      SSL_CTX_check_private_key(server_context) != 1) {
    fail("loading the TLS fixture failed");
    goto cleanup;
  }

  client = SSL_new(client_context);
  server = SSL_new(server_context);
  if (client == NULL || server == NULL) {
    fail("SSL_new failed");
    goto cleanup;
  }

  BIO *client_bio = NULL;
  BIO *server_bio = NULL;
  if (BIO_new_bio_pair(&client_bio, 0, &server_bio, 0) != 1) {
    fail("BIO_new_bio_pair failed");
    goto cleanup;
  }
  SSL_set_bio(client, client_bio, client_bio);
  SSL_set_bio(server, server_bio, server_bio);
  SSL_set_connect_state(client);
  SSL_set_accept_state(server);

  // TLS 1.3 handshake state transitions are defined by RFC 8446 section 4.
  // https://www.rfc-editor.org/rfc/rfc8446#section-4
  int client_complete = 0;
  int server_complete = 0;
  for (int step = 0; step < 1000 && (!client_complete || !server_complete); step++) {
    if (!client_complete && handshake_step(client, &client_complete) != 0) {
      fail("client handshake failed");
      goto cleanup;
    }
    if (!server_complete && handshake_step(server, &server_complete) != 0) {
      fail("server handshake failed");
      goto cleanup;
    }
  }
  if (!client_complete || !server_complete) {
    fail("handshake did not converge within 1000 steps");
    goto cleanup;
  }

  static const char payload[] = "agentos-openssl-wasm";
  char received[sizeof(payload)] = {0};
  if (SSL_write(client, payload, (int)sizeof(payload)) != (int)sizeof(payload)) {
    fail("encrypted client write failed");
    goto cleanup;
  }
  if (SSL_read(server, received, (int)sizeof(received)) != (int)sizeof(payload) ||
      memcmp(received, payload, sizeof(payload)) != 0) {
    fail("encrypted server read did not match");
    goto cleanup;
  }

  printf("openssl=%s protocol=%s cipher=%s encrypted-bytes=%zu\n",
         OpenSSL_version(OPENSSL_VERSION), SSL_get_version(client),
         SSL_get_cipher_name(client), sizeof(payload));
  exit_code = 0;

cleanup:
  SSL_free(client);
  SSL_free(server);
  SSL_CTX_free(client_context);
  SSL_CTX_free(server_context);
  return exit_code;
}
