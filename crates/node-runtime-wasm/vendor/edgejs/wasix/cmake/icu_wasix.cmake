# WASIX ICU build/setup lives here to keep root CMakeLists focused on
# cross-platform wiring. This module centralizes the WASIX-specific ICU
# configuration (embedded data + compile flags) used by the WASIX target.

include_guard(GLOBAL)

find_package(Python3 COMPONENTS Interpreter REQUIRED)

set(EDGE_ICU_DATA_BZ2 "${EDGE_ICU_ROOT}/source/data/in/icudt78l.dat.bz2")
set(EDGE_ICU_EMBED_C "${CMAKE_CURRENT_BINARY_DIR}/generated/ubi_icudata.c")
add_custom_command(
  OUTPUT "${EDGE_ICU_EMBED_C}"
  COMMAND "${CMAKE_COMMAND}" -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/generated"
  COMMAND
    "${Python3_EXECUTABLE}" "${PROJECT_ROOT}/cmake/embed_binary.py"
    --input "${EDGE_ICU_DATA_BZ2}"
    --output "${EDGE_ICU_EMBED_C}"
    --symbol "ubi_icudt78l_dat"
    --compression bz2
  DEPENDS
    "${PROJECT_ROOT}/cmake/embed_binary.py"
    "${EDGE_ICU_DATA_BZ2}"
  VERBATIM
)

add_library(edge_icu_embedded_data STATIC
  "${EDGE_ICU_EMBED_C}"
)
set_target_properties(edge_icu_embedded_data PROPERTIES LINKER_LANGUAGE C)

add_library(edge_icu_common STATIC
  ${EDGE_ICU_COMMON_SOURCES}
  "${EDGE_ICU_ROOT}/source/stubdata/stubdata.cpp"
)
target_include_directories(edge_icu_common
  PUBLIC
    "${EDGE_ICU_ROOT}/source/common"
    "${EDGE_ICU_ROOT}/source/i18n"
)
target_compile_definitions(edge_icu_common
  PRIVATE
    HAVE_DLOPEN=0
    UCONFIG_ONLY_HTML_CONVERSION=1
    U_CHARSET_IS_UTF8=1
    U_USING_ICU_NAMESPACE=0
    U_ENABLE_DYLOAD=0
    USE_CHROMIUM_ICU=1
    U_ENABLE_TRACING=1
    U_ENABLE_RESOURCE_TRACING=0
    U_STATIC_IMPLEMENTATION
    U_COMMON_IMPLEMENTATION
    U_ICUDATAENTRY_IN_COMMON
    U_DISABLE_RENAMING=1
    UNISTR_FROM_STRING_EXPLICIT=
    UNISTR_FROM_CHAR_EXPLICIT=
)
target_compile_options(edge_icu_common PRIVATE -Wno-deprecated-declarations)
target_link_libraries(edge_icu_common
  PUBLIC
    edge_icu_embedded_data
)

add_library(edge_icu_i18n STATIC
  ${EDGE_ICU_I18N_SOURCES}
)
target_include_directories(edge_icu_i18n
  PUBLIC
    "${EDGE_ICU_ROOT}/source/common"
    "${EDGE_ICU_ROOT}/source/i18n"
)
target_compile_definitions(edge_icu_i18n
  PRIVATE
    HAVE_DLOPEN=0
    UCONFIG_ONLY_HTML_CONVERSION=1
    U_CHARSET_IS_UTF8=1
    U_USING_ICU_NAMESPACE=0
    U_ENABLE_DYLOAD=0
    USE_CHROMIUM_ICU=1
    U_ENABLE_TRACING=1
    U_ENABLE_RESOURCE_TRACING=0
    U_STATIC_IMPLEMENTATION
    U_I18N_IMPLEMENTATION
    U_DISABLE_RENAMING=1
    UNISTR_FROM_STRING_EXPLICIT=
    UNISTR_FROM_CHAR_EXPLICIT=
)
target_compile_options(edge_icu_i18n PRIVATE -Wno-deprecated-declarations)
target_link_libraries(edge_icu_i18n
  PUBLIC
    edge_icu_common
)

list(APPEND EDGE_ICU_LINK_LIBS
  edge_icu_i18n
  edge_icu_common
)
