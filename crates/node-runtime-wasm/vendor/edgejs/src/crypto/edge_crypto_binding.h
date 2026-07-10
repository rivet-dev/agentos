#ifndef NAPI_EXPERIMENTAL
#define NAPI_EXPERIMENTAL
#endif

#ifndef EDGE_CRYPTO_BINDING_H_
#define EDGE_CRYPTO_BINDING_H_

#include <cstddef>
#include <cstdint>

#include "js_native_api.h"

typedef struct evp_pkey_st EVP_PKEY;

namespace edge::crypto {

napi_value InstallCryptoBinding(napi_env env);
napi_value CryptoGetBundledRootCertificates(napi_env env, napi_callback_info info);
napi_value CryptoGetExtraCACertificates(napi_env env, napi_callback_info info);
napi_value CryptoGetSystemCACertificates(napi_env env, napi_callback_info info);
napi_value CryptoGetUserRootCertificates(napi_env env, napi_callback_info info);
napi_value CryptoResetRootCertStore(napi_env env, napi_callback_info info);
napi_value CryptoStartLoadingCertificatesOffThread(napi_env env, napi_callback_info info);
bool GetAnyBufferSourceBytes(napi_env env, napi_value value, uint8_t** data, size_t* len);
EVP_PKEY* ParsePrivateKeyWithPassphrase(const uint8_t* data,
                                        size_t len,
                                        const uint8_t* passphrase,
                                        size_t passphrase_len,
                                        bool has_passphrase);

}  // namespace edge::crypto

#endif  // EDGE_CRYPTO_BINDING_H_
