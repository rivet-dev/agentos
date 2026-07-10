function(edge_add_vendored_deps)
  if(EDGE_IS_WASIX_TARGET)
    set(EDGE_LIBUV_ROOT "${PROJECT_ROOT}/deps/libuv-wasix")
  else()
    set(EDGE_LIBUV_ROOT "${PROJECT_ROOT}/deps/uv")
  endif()
  if(NOT EXISTS "${EDGE_LIBUV_ROOT}/CMakeLists.txt")
    message(FATAL_ERROR
      "libuv CMake project not found at ${EDGE_LIBUV_ROOT}. "
      "For WASIX, run wasix/setup-wasix-deps.sh first.")
  endif()

  set(LIBUV_BUILD_SHARED OFF CACHE BOOL "" FORCE)
  set(LIBUV_BUILD_TESTS OFF CACHE BOOL "" FORCE)
  set(LIBUV_BUILD_BENCH OFF CACHE BOOL "" FORCE)
  add_subdirectory("${EDGE_LIBUV_ROOT}" "${CMAKE_CURRENT_BINARY_DIR}/deps/uv")

  set(CARES_STATIC ON CACHE BOOL "" FORCE)
  set(CARES_SHARED OFF CACHE BOOL "" FORCE)
  set(CARES_INSTALL OFF CACHE BOOL "" FORCE)
  set(CARES_BUILD_TESTS OFF CACHE BOOL "" FORCE)
  set(CARES_BUILD_TOOLS OFF CACHE BOOL "" FORCE)
  add_subdirectory("${PROJECT_ROOT}/deps/cares" "${CMAKE_CURRENT_BINARY_DIR}/deps/cares")

  set(BUILD_UNITTESTS OFF CACHE BOOL "" FORCE)
  set(BUILD_MINIZIP_BIN OFF CACHE BOOL "" FORCE)
  set(BUILD_ZPIPE OFF CACHE BOOL "" FORCE)
  set(BUILD_MINIGZIP OFF CACHE BOOL "" FORCE)
  add_subdirectory("${PROJECT_ROOT}/deps/zlib" "${CMAKE_CURRENT_BINARY_DIR}/deps/zlib")
  if(EDGE_IS_WASIX_TARGET AND TARGET zlib_bench)
    # zlib_bench links against the shared zlib target, which fails under WASIX
    # static-linking constraints. Keep the libraries but skip the benchmark tool.
    set_target_properties(zlib_bench PROPERTIES EXCLUDE_FROM_ALL TRUE)
  endif()

  find_package(Threads REQUIRED)

  add_library(edge_brotli STATIC
    "${PROJECT_ROOT}/deps/brotli/c/common/constants.c"
    "${PROJECT_ROOT}/deps/brotli/c/common/context.c"
    "${PROJECT_ROOT}/deps/brotli/c/common/dictionary.c"
    "${PROJECT_ROOT}/deps/brotli/c/common/platform.c"
    "${PROJECT_ROOT}/deps/brotli/c/common/shared_dictionary.c"
    "${PROJECT_ROOT}/deps/brotli/c/common/transform.c"
    "${PROJECT_ROOT}/deps/brotli/c/dec/bit_reader.c"
    "${PROJECT_ROOT}/deps/brotli/c/dec/decode.c"
    "${PROJECT_ROOT}/deps/brotli/c/dec/huffman.c"
    "${PROJECT_ROOT}/deps/brotli/c/dec/prefix.c"
    "${PROJECT_ROOT}/deps/brotli/c/dec/state.c"
    "${PROJECT_ROOT}/deps/brotli/c/dec/static_init.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/backward_references.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/backward_references_hq.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/bit_cost.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/block_splitter.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/brotli_bit_stream.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/cluster.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/command.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/compound_dictionary.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/compress_fragment.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/compress_fragment_two_pass.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/dictionary_hash.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/encode.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/encoder_dict.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/entropy_encode.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/fast_log.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/histogram.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/literal_cost.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/memory.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/metablock.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/static_dict.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/static_dict_lut.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/static_init.c"
    "${PROJECT_ROOT}/deps/brotli/c/enc/utf8_util.c"
  )
  target_include_directories(edge_brotli
    PUBLIC
      "${PROJECT_ROOT}/deps/brotli/c/include"
  )
  if(APPLE)
    target_link_libraries(edge_brotli PUBLIC m)
  elseif(UNIX)
    target_link_libraries(edge_brotli PUBLIC m)
  endif()

  add_library(edge_zstd STATIC
    "${PROJECT_ROOT}/deps/zstd/lib/common/debug.c"
    "${PROJECT_ROOT}/deps/zstd/lib/common/entropy_common.c"
    "${PROJECT_ROOT}/deps/zstd/lib/common/fse_decompress.c"
    "${PROJECT_ROOT}/deps/zstd/lib/compress/fse_compress.c"
    "${PROJECT_ROOT}/deps/zstd/lib/compress/huf_compress.c"
    "${PROJECT_ROOT}/deps/zstd/lib/decompress/huf_decompress.c"
    "${PROJECT_ROOT}/deps/zstd/lib/common/pool.c"
    "${PROJECT_ROOT}/deps/zstd/lib/common/threading.c"
    "${PROJECT_ROOT}/deps/zstd/lib/common/xxhash.c"
    "${PROJECT_ROOT}/deps/zstd/lib/common/zstd_common.c"
    "${PROJECT_ROOT}/deps/zstd/lib/common/error_private.c"
    "${PROJECT_ROOT}/deps/zstd/lib/compress/hist.c"
    "${PROJECT_ROOT}/deps/zstd/lib/compress/zstd_compress.c"
    "${PROJECT_ROOT}/deps/zstd/lib/compress/zstd_compress_literals.c"
    "${PROJECT_ROOT}/deps/zstd/lib/compress/zstd_compress_sequences.c"
    "${PROJECT_ROOT}/deps/zstd/lib/compress/zstd_compress_superblock.c"
    "${PROJECT_ROOT}/deps/zstd/lib/compress/zstd_double_fast.c"
    "${PROJECT_ROOT}/deps/zstd/lib/compress/zstd_fast.c"
    "${PROJECT_ROOT}/deps/zstd/lib/compress/zstd_lazy.c"
    "${PROJECT_ROOT}/deps/zstd/lib/compress/zstd_ldm.c"
    "${PROJECT_ROOT}/deps/zstd/lib/compress/zstd_opt.c"
    "${PROJECT_ROOT}/deps/zstd/lib/compress/zstd_preSplit.c"
    "${PROJECT_ROOT}/deps/zstd/lib/compress/zstdmt_compress.c"
    "${PROJECT_ROOT}/deps/zstd/lib/decompress/zstd_ddict.c"
    "${PROJECT_ROOT}/deps/zstd/lib/decompress/zstd_decompress.c"
    "${PROJECT_ROOT}/deps/zstd/lib/decompress/zstd_decompress_block.c"
  )
  target_include_directories(edge_zstd
    PUBLIC
      "${PROJECT_ROOT}/deps/zstd/lib"
  )
  target_compile_definitions(edge_zstd
    PUBLIC
      XXH_NAMESPACE=ZSTD_
      ZSTD_MULTITHREAD
      ZSTD_DISABLE_ASM
  )
  target_link_libraries(edge_zstd
    PUBLIC
      Threads::Threads
  )

  add_library(edge_nghttp2 STATIC
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_buf.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_callbacks.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_debug.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_extpri.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_frame.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_hd.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_hd_huffman.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_hd_huffman_data.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_helper.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_http.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_map.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_mem.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_alpn.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_option.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_outbound_item.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_pq.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_priority_spec.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_queue.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_ratelim.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_rcbuf.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_session.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_stream.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_submit.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_time.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/nghttp2_version.c"
    "${PROJECT_ROOT}/deps/nghttp2/lib/sfparse.c"
  )
  target_include_directories(edge_nghttp2
    PUBLIC
      "${PROJECT_ROOT}/deps/nghttp2/lib/includes"
    PRIVATE
      "${PROJECT_ROOT}/deps/nghttp2/lib"
  )
  target_compile_definitions(edge_nghttp2
    PUBLIC
      NGHTTP2_STATICLIB
    PRIVATE
      BUILDING_NGHTTP2
      HAVE_CONFIG_H
      _U_=
  )
endfunction()

function(edge_add_runtime_support_libraries)
  add_library(edge_ncrypto STATIC
    "${PROJECT_ROOT}/deps/ncrypto/ncrypto.cc"
    "${PROJECT_ROOT}/deps/ncrypto/engine.cc"
  )
  target_include_directories(edge_ncrypto
    PUBLIC
      "${PROJECT_ROOT}/deps/ncrypto"
  )
  target_link_libraries(edge_ncrypto
    PUBLIC
      OpenSSL::Crypto
      OpenSSL::SSL
  )

  add_library(edge_simdutf STATIC
    "${PROJECT_ROOT}/deps/simdutf/simdutf.cpp"
  )
  add_library(edge_simdjson STATIC
    "${PROJECT_ROOT}/deps/simdjson/simdjson.cpp"
  )
  add_library(edge_ada STATIC
    "${PROJECT_ROOT}/deps/ada/ada.cpp"
  )
  target_compile_definitions(edge_ada PUBLIC ADA_USE_UNSAFE_STD_REGEX_PROVIDER)
  target_include_directories(edge_ada
    PUBLIC
      "${PROJECT_ROOT}/deps/ada"
  )

  target_include_directories(edge_simdjson
    PUBLIC
      "${PROJECT_ROOT}/deps"
  )

  target_include_directories(edge_simdutf
    PUBLIC
      "${PROJECT_ROOT}/deps/simdutf"
  )
endfunction()
