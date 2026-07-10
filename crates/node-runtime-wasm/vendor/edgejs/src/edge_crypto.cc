#include "edge_crypto.h"

#include "crypto/edge_crypto_binding.h"

napi_value EdgeInstallCryptoBinding(napi_env env) {
  return edge::crypto::InstallCryptoBinding(env);
}
