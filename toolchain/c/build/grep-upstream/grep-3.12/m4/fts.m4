# fts.m4
# serial 24
dnl Copyright (C) 2005-2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

AC_DEFUN([gl_FUNC_FTS],
[
  gl_FUNC_FTS_CORE
])

AC_DEFUN([gl_FUNC_FTS_CORE],
[
  dnl Prerequisites of lib/fts.c.
  gl_FUNC_OPENAT

  AC_CHECK_FUNCS_ONCE([fstatfs])
  AC_CHECK_HEADERS_ONCE([sys/param.h sys/vfs.h])dnl
  if test "$ac_cv_func_fstatfs,$ac_cv_header_sys_vfs_h" = yes,yes; then
    AC_CHECK_MEMBERS([struct statfs.f_type], [], [],
      [[$ac_includes_default
        #include <sys/vfs.h>
      ]])
    if test "$ac_cv_member_struct_statfs_f_type" = yes; then
      AC_CHECK_TYPES([__fsword_t], [], [],
        [[$ac_includes_default
          #include <sys/vfs.h>
        ]])
    fi
  fi

  gl_CHECK_FUNCS_ANDROID([fts_open], [[#include <fts.h>]])
  if case "$gl_cv_onwards_func_fts_open" in \
       yes | future*) true ;; \
       *) false ;; \
     esac; then
    dnl The system already has the symbols fts_open, etc. or will have them
    dnl in a future version.
    dnl Avoid conflicts between these symbols and ours at the linker level.
    AC_DEFINE([fts_open], [rpl_fts_open],
      [Define to the overridden function name])
    AC_DEFINE([fts_close], [rpl_fts_close],
      [Define to the overridden function name])
    AC_DEFINE([fts_read], [rpl_fts_read],
      [Define to the overridden function name])
    AC_DEFINE([fts_set], [rpl_fts_set],
      [Define to the overridden function name])
    AC_DEFINE([fts_children], [rpl_fts_children],
      [Define to the overridden function name])
    AC_DEFINE([fts_cross_check], [rpl_fts_cross_check],
      [Define to the overridden function name])
  fi
])
