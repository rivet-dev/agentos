if(NOT COMMAND xoption)
  macro(xoption OPTION_NAME OPTION_TEXT OPTION_DEFAULT)
    option(${OPTION_NAME} ${OPTION_TEXT} ${OPTION_DEFAULT})
    if(DEFINED ENV{${OPTION_NAME}})
      set(${OPTION_NAME} $ENV{${OPTION_NAME}})
    endif()
    if(${OPTION_NAME})
      add_definitions(-D${OPTION_NAME})
    endif()
    message(STATUS "  ${OPTION_NAME}: ${${OPTION_NAME}}")
  endmacro()
endif()

macro(napi_define_lifetime_options ENGINE_LABEL)
  xoption(NAPI_ENABLE_LIFETIME_TRACKER
    "Enable ${ENGINE_LABEL} N-API lifetime tracking diagnostics"
    OFF
  )
  xoption(NAPI_ENABLE_LIFETIME_PERIODIC_STATS
    "Print ${ENGINE_LABEL} N-API value/ref allocator stats every 10 seconds"
    OFF
  )
  xoption(NAPI_ENABLE_LIFETIME_TAG_STATS
    "Print ${ENGINE_LABEL} N-API active value/ref tag stats with periodic stats"
    OFF
  )
  xoption(NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP
    "Print active ${ENGINE_LABEL} N-API string and symbol values with periodic stats"
    OFF
  )

  if(NAPI_ENABLE_LIFETIME_PERIODIC_STATS AND NOT NAPI_ENABLE_LIFETIME_TRACKER)
    message(STATUS
      "NAPI_ENABLE_LIFETIME_PERIODIC_STATS requires "
      "NAPI_ENABLE_LIFETIME_TRACKER; enabling lifetime tracker"
    )
    set(NAPI_ENABLE_LIFETIME_TRACKER ON)
  endif()
  if(NAPI_ENABLE_LIFETIME_TAG_STATS AND NOT NAPI_ENABLE_LIFETIME_PERIODIC_STATS)
    message(STATUS
      "NAPI_ENABLE_LIFETIME_TAG_STATS requires "
      "NAPI_ENABLE_LIFETIME_PERIODIC_STATS; enabling periodic stats"
    )
    set(NAPI_ENABLE_LIFETIME_PERIODIC_STATS ON)
  endif()
  if(NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP AND NOT NAPI_ENABLE_LIFETIME_TAG_STATS)
    message(STATUS
      "NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP requires "
      "NAPI_ENABLE_LIFETIME_TAG_STATS; enabling tag stats"
    )
    set(NAPI_ENABLE_LIFETIME_TAG_STATS ON)
  endif()
  if(NAPI_ENABLE_LIFETIME_PERIODIC_STATS AND NOT NAPI_ENABLE_LIFETIME_TRACKER)
    set(NAPI_ENABLE_LIFETIME_TRACKER ON)
  endif()
endmacro()
