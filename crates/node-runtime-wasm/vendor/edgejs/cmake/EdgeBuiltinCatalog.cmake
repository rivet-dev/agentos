function(edge_add_builtin_catalog target_name)
  if(NOT TARGET edge_builtin_catalog_gen)
    file(GLOB_RECURSE EDGE_BUILTIN_LIB_SOURCES CONFIGURE_DEPENDS
      "${PROJECT_ROOT}/lib/*.js"
    )
    file(GLOB_RECURSE EDGE_BUILTIN_DEP_SOURCES CONFIGURE_DEPENDS
      "${PROJECT_ROOT}/deps/*.js"
      "${PROJECT_ROOT}/deps/*.mjs"
    )

    set(EDGE_BUILTIN_CATALOG_HEADER
      "${CMAKE_CURRENT_BINARY_DIR}/edge_builtin_catalog_data.h"
    )
    add_custom_command(
      OUTPUT "${EDGE_BUILTIN_CATALOG_HEADER}"
      COMMAND
        "${Python3_EXECUTABLE}"
        "${PROJECT_ROOT}/scripts/generate_builtin_catalog_header.py"
        --project-root "${PROJECT_ROOT}"
        --output "${EDGE_BUILTIN_CATALOG_HEADER}"
      DEPENDS
        "${PROJECT_ROOT}/scripts/generate_builtin_catalog_header.py"
        ${EDGE_BUILTIN_LIB_SOURCES}
        ${EDGE_BUILTIN_DEP_SOURCES}
      COMMENT "Generating Edge builtin catalog"
      VERBATIM
    )
    add_custom_target(edge_builtin_catalog_gen DEPENDS "${EDGE_BUILTIN_CATALOG_HEADER}")
  endif()
  add_dependencies("${target_name}" edge_builtin_catalog_gen)
  target_include_directories("${target_name}" PRIVATE "${CMAKE_CURRENT_BINARY_DIR}")
endfunction()
