function(edge_configure_icu)
  set(EDGE_ICU_ROOT "${PROJECT_ROOT}/deps/icu-small")
  set(EDGE_ICU_INCLUDE_DIRS
    "${EDGE_ICU_ROOT}/source/common"
    "${EDGE_ICU_ROOT}/source/i18n"
    "${EDGE_ICU_ROOT}/source/stubdata"
  )
  set(EDGE_ICU_LINK_LIBS "")
  file(GLOB_RECURSE EDGE_ICU_COMMON_SOURCES CONFIGURE_DEPENDS
    "${EDGE_ICU_ROOT}/source/common/*.cpp"
  )
  file(GLOB_RECURSE EDGE_ICU_I18N_SOURCES CONFIGURE_DEPENDS
    "${EDGE_ICU_ROOT}/source/i18n/*.cpp"
  )
  if(EDGE_IS_WASIX_TARGET)
    include("${PROJECT_ROOT}/wasix/cmake/icu_wasix.cmake")
  else()
    set(EDGE_ICU_BASE_DEFINES
      U_ATTRIBUTE_DEPRECATED=
      UCONFIG_NO_SERVICE=1
      U_ENABLE_DYLOAD=0
      U_STATIC_IMPLEMENTATION=1
      U_HAVE_STD_STRING=1
      UCONFIG_NO_BREAK_ITERATION=0
      U_DISABLE_RENAMING=1
    )

    add_library(edge_icu_stubdata STATIC
      "${EDGE_ICU_ROOT}/source/stubdata/stubdata.cpp"
    )
    target_include_directories(edge_icu_stubdata
      PUBLIC
        "${EDGE_ICU_ROOT}/source/common"
        "${EDGE_ICU_ROOT}/source/stubdata"
    )
    target_compile_definitions(edge_icu_stubdata
      PRIVATE
        ${EDGE_ICU_BASE_DEFINES}
        U_COMMON_IMPLEMENTATION=1
    )

    # Embed the real ICU common-data blob so Intl/ICU has locale data on native
    # too (WASIX embeds it separately in icu_wasix.cmake). The blob is registered
    # with ICU at runtime via udata_setCommonData(); see src/edge_icu_data.cc.
    # stubdata (linked below) remains as ICU's default entry point; the runtime
    # override takes precedence once activated.
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
    add_library(edge_icu_embedded_data STATIC "${EDGE_ICU_EMBED_C}")
    set_target_properties(edge_icu_embedded_data PROPERTIES LINKER_LANGUAGE C)

    add_library(edge_icu_common STATIC
      ${EDGE_ICU_COMMON_SOURCES}
    )
    target_include_directories(edge_icu_common
      PUBLIC
        "${EDGE_ICU_ROOT}/source/common"
    )
    target_compile_definitions(edge_icu_common
      PRIVATE
        ${EDGE_ICU_BASE_DEFINES}
        U_COMMON_IMPLEMENTATION=1
    )
    target_link_libraries(edge_icu_common
      PUBLIC
        edge_icu_stubdata
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
        ${EDGE_ICU_BASE_DEFINES}
        U_I18N_IMPLEMENTATION=1
    )
    target_link_libraries(edge_icu_i18n
      PUBLIC
        edge_icu_common
    )

    list(APPEND EDGE_ICU_LINK_LIBS
      edge_icu_i18n
      edge_icu_common
    )
  endif()

  set(EDGE_ICU_INCLUDE_DIRS "${EDGE_ICU_INCLUDE_DIRS}" PARENT_SCOPE)
  set(EDGE_ICU_LINK_LIBS "${EDGE_ICU_LINK_LIBS}" PARENT_SCOPE)
endfunction()
