# configmake.m4
# serial 6
dnl Copyright (C) 2010-2025 Free Software Foundation, Inc.
dnl This file is free software; the Free Software Foundation
dnl gives unlimited permission to copy and/or distribute it,
dnl with or without modifications, as long as this notice is preserved.
dnl This file is offered as-is, without any warranty.

AC_PREREQ([2.60])

# gl_CONFIGMAKE_PREP
# ------------------
# Guarantee all of the standard directory variables, even when used with
# autoconf 2.64 (runstatedir wasn't supported before 2.70) or
# automake 1.11 (runstatedir isn't supported even in 1.16.1).
AC_DEFUN([gl_CONFIGMAKE_PREP],
[
  if test "x$lispdir" = x; then
    AC_SUBST([lispdir], ['${datarootdir}/emacs/site-lisp'])
  fi
  dnl Added in autoconf 2.70.
  if test "x$runstatedir" = x; then
    AC_SUBST([runstatedir], ['${localstatedir}/run'])
  fi

  dnl Automake 1.11 provides the pkg*dir variables merely without AC_SUBST,
  dnl that is, only at the Makefile.am level.  AC_SUBST them, so that
  dnl gl_CONFIGMAKE can compute the final values at configure time.
  dnl Blindly assigning the value at configure time is OK, since configure
  dnl does not have --pkg*dir=... options.
  AC_SUBST([pkgdatadir], ['${datadir}/${PACKAGE}'])
  AC_SUBST([pkgincludedir], ['${includedir}/${PACKAGE}'])
  AC_SUBST([pkglibdir], ['${libdir}/${PACKAGE}'])
  AC_SUBST([pkglibexecdir], ['${libexecdir}/${PACKAGE}'])
])

# gl_CONFIGMAKE
# -------------
# Find the final values of the standard directory variables, and create
# AC_SUBSTed *_c and *_c_make variables with the corresponding values in
# target runtime environment ($host_os) syntax.
AC_DEFUN([gl_CONFIGMAKE],
[
  AC_REQUIRE([gl_CONFIGMAKE_PREP])

  dnl Save the values.
  gl_saved_prefix="${prefix}"
  gl_saved_exec_prefix="${exec_prefix}"
  gl_saved_bindir="${bindir}"
  gl_saved_sbindir="${sbindir}"
  gl_saved_libexecdir="${libexecdir}"
  gl_saved_datarootdir="${datarootdir}"
  gl_saved_datadir="${datadir}"
  gl_saved_sysconfdir="${sysconfdir}"
  gl_saved_sharedstatedir="${sharedstatedir}"
  gl_saved_localstatedir="${localstatedir}"
  gl_saved_runstatedir="${runstatedir}"
  gl_saved_includedir="${includedir}"
  gl_saved_oldincludedir="${oldincludedir}"
  gl_saved_docdir="${docdir}"
  gl_saved_infodir="${infodir}"
  gl_saved_htmldir="${htmldir}"
  gl_saved_dvidir="${dvidir}"
  gl_saved_pdfdir="${pdfdir}"
  gl_saved_psdir="${psdir}"
  gl_saved_libdir="${libdir}"
  gl_saved_lispdir="${lispdir}"
  gl_saved_localedir="${localedir}"
  gl_saved_mandir="${mandir}"
  gl_saved_pkgdatadir="${pkgdatadir}"
  gl_saved_pkgincludedir="${pkgincludedir}"
  gl_saved_pkglibdir="${pkglibdir}"
  gl_saved_pkglibexecdir="${pkglibexecdir}"

  dnl Find the final values.
  dnl Unfortunately, prefix gets only finally determined at the end of
  dnl configure.
  if test "X$prefix" = "XNONE"; then
    prefix="$ac_default_prefix"
  fi
  dnl Unfortunately, exec_prefix gets only finally determined at the end of
  dnl configure.
  if test "X$exec_prefix" = "XNONE"; then
    exec_prefix='${prefix}'
  fi
  eval exec_prefix="$exec_prefix"
  eval bindir="$bindir"
  eval sbindir="$sbindir"
  eval libexecdir="$libexecdir"
  eval datarootdir="$datarootdir"
  eval datadir="$datadir"
  eval sysconfdir="$sysconfdir"
  eval sharedstatedir="$sharedstatedir"
  eval localstatedir="$localstatedir"
  eval runstatedir="$runstatedir"
  eval includedir="$includedir"
  eval oldincludedir="$oldincludedir"
  eval docdir="$docdir"
  eval infodir="$infodir"
  eval htmldir="$htmldir"
  eval dvidir="$dvidir"
  eval pdfdir="$pdfdir"
  eval psdir="$psdir"
  eval libdir="$libdir"
  eval lispdir="$lispdir"
  eval localedir="$localedir"
  eval mandir="$mandir"
  eval pkgdatadir="$pkgdatadir"
  eval pkgincludedir="$pkgincludedir"
  eval pkglibdir="$pkglibdir"
  eval pkglibexecdir="$pkglibexecdir"

  dnl Transform the final values.
  gl_BUILD_TO_HOST([prefix])
  gl_BUILD_TO_HOST([exec_prefix])
  gl_BUILD_TO_HOST([bindir])
  gl_BUILD_TO_HOST([sbindir])
  gl_BUILD_TO_HOST([libexecdir])
  gl_BUILD_TO_HOST([datarootdir])
  gl_BUILD_TO_HOST([datadir])
  gl_BUILD_TO_HOST([sysconfdir])
  gl_BUILD_TO_HOST([sharedstatedir])
  gl_BUILD_TO_HOST([localstatedir])
  gl_BUILD_TO_HOST([runstatedir])
  gl_BUILD_TO_HOST([includedir])
  gl_BUILD_TO_HOST([oldincludedir])
  gl_BUILD_TO_HOST([docdir])
  gl_BUILD_TO_HOST([infodir])
  gl_BUILD_TO_HOST([htmldir])
  gl_BUILD_TO_HOST([dvidir])
  gl_BUILD_TO_HOST([pdfdir])
  gl_BUILD_TO_HOST([psdir])
  gl_BUILD_TO_HOST([libdir])
  gl_BUILD_TO_HOST([lispdir])
  gl_BUILD_TO_HOST([localedir])
  gl_BUILD_TO_HOST([mandir])
  gl_BUILD_TO_HOST([pkgdatadir])
  gl_BUILD_TO_HOST([pkgincludedir])
  gl_BUILD_TO_HOST([pkglibdir])
  gl_BUILD_TO_HOST([pkglibexecdir])

  dnl Restore the values.
  pkglibexecdir="${gl_saved_pkglibexecdir}"
  pkglibdir="${gl_saved_pkglibdir}"
  pkgincludedir="${gl_saved_pkgincludedir}"
  pkgdatadir="${gl_saved_pkgdatadir}"
  mandir="${gl_saved_mandir}"
  localedir="${gl_saved_localedir}"
  lispdir="${gl_saved_lispdir}"
  libdir="${gl_saved_libdir}"
  psdir="${gl_saved_psdir}"
  pdfdir="${gl_saved_pdfdir}"
  dvidir="${gl_saved_dvidir}"
  htmldir="${gl_saved_htmldir}"
  infodir="${gl_saved_infodir}"
  docdir="${gl_saved_docdir}"
  oldincludedir="${gl_saved_oldincludedir}"
  includedir="${gl_saved_includedir}"
  runstatedir="${gl_saved_runstatedir}"
  localstatedir="${gl_saved_localstatedir}"
  sharedstatedir="${gl_saved_sharedstatedir}"
  sysconfdir="${gl_saved_sysconfdir}"
  datadir="${gl_saved_datadir}"
  datarootdir="${gl_saved_datarootdir}"
  libexecdir="${gl_saved_libexecdir}"
  sbindir="${gl_saved_sbindir}"
  bindir="${gl_saved_bindir}"
  exec_prefix="${gl_saved_exec_prefix}"
  prefix="${gl_saved_prefix}"
])
