#ifndef NAPI_V8_FACTORY_H_
#define NAPI_V8_FACTORY_H_

#include <type_traits>
#include <utility>

#include "napi_allocator.h"
#include "internal/napi_callback_payload.h"
#include "internal/napi_env_records.h"
#include "internal/napi_external_wrapper.h"
#include "internal/napi_module_wrap_record.h"
#include "internal/napi_ref.h"
#include "internal/napi_ref_with_data.h"
#include "internal/napi_ref_with_finalizer.h"
#include "internal/napi_serdes_context.h"

struct napi_env__;

namespace v8impl::detail
{

  template <typename T>
  inline constexpr bool napi_factory_unsupported__ = false;

  class napi_factory__
  {
  public:
    explicit napi_factory__(napi_env__ *env)
        : napi_ref_allocator_(env),
          napi_ref_with_data_allocator_(env),
          napi_ref_with_finalizer_allocator_(env),
          napi_callback_payload_allocator_(env),
          napi_external_wrapper_allocator_(env),
          napi_env_cleanup_hook_allocator_(env),
          napi_deferred_allocator_(env),
          napi_handle_scope_allocator_(env),
          napi_escapable_handle_scope_allocator_(env),
          napi_buffer_record_allocator_(env),
          module_wrap_record_allocator_(env),
          serializer_context_allocator_(env),
          deserializer_context_allocator_(env) {}

    napi_factory__(const napi_factory__ &) = delete;
    napi_factory__ &operator=(const napi_factory__ &) = delete;

    template <typename T, typename... Args>
    T *allocate(Args &&...args)
    {
      return allocator_for<T>().allocate(std::forward<Args>(args)...);
    }

    template <typename T>
    void release(T *value)
    {
      if (value == nullptr)
        return;
      allocator_for<T>().destroy(value);
    }

  private:
    template <typename T>
    auto &allocator_for()
    {
      if constexpr (std::is_same_v<T, napi_ref__>)
      {
        return napi_ref_allocator_;
      }
      else if constexpr (std::is_same_v<T, napi_ref_with_data__>)
      {
        return napi_ref_with_data_allocator_;
      }
      else if constexpr (std::is_same_v<T, napi_ref_with_finalizer__>)
      {
        return napi_ref_with_finalizer_allocator_;
      }
      else if constexpr (std::is_same_v<T, napi_deferred__>)
      {
        return napi_deferred_allocator_;
      }
      else if constexpr (std::is_same_v<T, napi_handle_scope__>)
      {
        return napi_handle_scope_allocator_;
      }
      else if constexpr (std::is_same_v<T, napi_escapable_handle_scope__>)
      {
        return napi_escapable_handle_scope_allocator_;
      }
      else if constexpr (std::is_same_v<T, napi_callback_payload__>)
      {
        return napi_callback_payload_allocator_;
      }
      else if constexpr (std::is_same_v<T, napi_external_wrapper__>)
      {
        return napi_external_wrapper_allocator_;
      }
      else if constexpr (std::is_same_v<T, napi_env_cleanup_hook__>)
      {
        return napi_env_cleanup_hook_allocator_;
      }
      else if constexpr (std::is_same_v<T, napi_buffer_record__>)
      {
        return napi_buffer_record_allocator_;
      }
      else if constexpr (std::is_same_v<T, ModuleWrapRecord>)
      {
        return module_wrap_record_allocator_;
      }
      else if constexpr (std::is_same_v<T, SerializerContext>)
      {
        return serializer_context_allocator_;
      }
      else if constexpr (std::is_same_v<T, DeserializerContext>)
      {
        return deserializer_context_allocator_;
      }
      else
      {
        static_assert(napi_factory_unsupported__<T>,
                      "Unsupported napi_factory__ allocation type");
      }
    }

    napi_allocator__<napi_ref__, napi_env__> napi_ref_allocator_;
    napi_allocator__<napi_ref_with_data__, napi_env__> napi_ref_with_data_allocator_;
    napi_allocator__<napi_ref_with_finalizer__, napi_env__> napi_ref_with_finalizer_allocator_;
    napi_allocator__<napi_deferred__, napi_env__> napi_deferred_allocator_;
    napi_allocator__<napi_handle_scope__, napi_env__> napi_handle_scope_allocator_;
    napi_allocator__<napi_escapable_handle_scope__, napi_env__> napi_escapable_handle_scope_allocator_;

    napi_allocator__<napi_callback_payload__, napi_env__> napi_callback_payload_allocator_;
    napi_allocator__<napi_external_wrapper__, napi_env__> napi_external_wrapper_allocator_;
    napi_allocator__<napi_env_cleanup_hook__, napi_env__> napi_env_cleanup_hook_allocator_;
    napi_allocator__<napi_buffer_record__, napi_env__> napi_buffer_record_allocator_;
    napi_allocator__<ModuleWrapRecord, napi_env__> module_wrap_record_allocator_;
    napi_allocator__<SerializerContext, napi_env__> serializer_context_allocator_;
    napi_allocator__<DeserializerContext, napi_env__> deserializer_context_allocator_;
  };

} // namespace v8impl::detail

#endif // NAPI_V8_FACTORY_H_
