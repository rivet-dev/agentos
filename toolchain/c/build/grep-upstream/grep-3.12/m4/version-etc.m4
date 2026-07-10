# version-etc.m4
# serial 2
dnl Copyright (C) 2009-2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

dnl $1 - configure flag and define name
dnl $2 - human readable description
m4_define([gl_VERSION_ETC_FLAG],
[dnl
  AC_ARG_WITH([$1], [AS_HELP_STRING([[--with-]$1], [$2])],
    [dnl
      case $withval in
        yes|no) ;;
        *) AC_DEFINE_UNQUOTED(AS_TR_CPP([PACKAGE_$1]), ["$withval"], [$2]) ;;
      esac
    ])
])

AC_DEFUN([gl_VERSION_ETC],
[dnl
  gl_VERSION_ETC_FLAG([packager],
                      [String identifying the packager of this software])
  gl_VERSION_ETC_FLAG([packager-version],
                      [Packager-specific version information])
  gl_VERSION_ETC_FLAG([packager-bug-reports],
                      [Packager info for bug reports (URL/e-mail/...)])
  if test "X$with_packager" = "X" && \
     test "X$with_packager_version$with_packager_bug_reports" != "X"
  then
    AC_MSG_ERROR([The --with-packager-{bug-reports,version} options require --with-packager])
  fi
])
