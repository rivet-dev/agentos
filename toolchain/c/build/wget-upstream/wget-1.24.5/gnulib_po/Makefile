# Makefile for PO directory in any package using GNU gettext.
# Copyright (C) 1995-2000 Ulrich Drepper <drepper@gnu.ai.mit.edu>
# Copyright (C) 2000-2023 Free Software Foundation, Inc.
#
# Copying and distribution of this file, with or without modification,
# are permitted in any medium without royalty provided the copyright
# notice and this notice are preserved.  This file is offered as-is,
# without any warranty.
#
# Origin: gettext-0.22
GETTEXT_MACRO_VERSION = 0.20

PACKAGE = wget
VERSION = 1.24.5
PACKAGE_BUGREPORT = bug-wget@gnu.org

SED = /usr/bin/sed
SHELL = /bin/sh


srcdir = .
top_srcdir = ..


prefix = /usr/local
exec_prefix = ${prefix}
datarootdir = ${prefix}/share
datadir = ${datarootdir}
localedir = ${datarootdir}/locale
gettextsrcdir = $(datadir)/gettext/po

INSTALL = /usr/bin/install -c
INSTALL_DATA = ${INSTALL} -m 644

# We use $(mkdir_p).
# In automake <= 1.9.x, $(mkdir_p) is defined either as "mkdir -p --" or as
# "$(mkinstalldirs)" or as "$(install_sh) -d". For these automake versions,
# ${SHELL} /home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/wget-upstream/wget-1.24.5/build-aux/install-sh does not start with $(SHELL), so we add it.
# In automake >= 1.10, $(MKDIR_P) is derived from ${MKDIR_P}, which is defined
# either as "/path/to/mkdir -p" or ".../install-sh -c -d". For these automake
# versions, $(mkinstalldirs) and $(install_sh) are unused.
mkinstalldirs = $(SHELL) ${SHELL} /home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/wget-upstream/wget-1.24.5/build-aux/install-sh -d
install_sh = $(SHELL) ${SHELL} /home/nathan/.herdr/workspaces/agent-os/reg-tests/toolchain/c/build/wget-upstream/wget-1.24.5/build-aux/install-sh
MKDIR_P = /usr/bin/mkdir -p
mkdir_p = $(MKDIR_P)

# When building gettext-tools, we prefer to use the built programs
# rather than installed programs.  However, we can't do that when we
# are cross compiling.
CROSS_COMPILING = @CROSS_COMPILING@

GMSGFMT_ = :
GMSGFMT_no = :
GMSGFMT_yes = :
GMSGFMT = $(GMSGFMT_$(USE_MSGCTXT))
XGETTEXT_ = :
XGETTEXT_no = :
XGETTEXT_yes = :
XGETTEXT = $(XGETTEXT_$(USE_MSGCTXT))
MSGMERGE = :
MSGMERGE_UPDATE = : --update
MSGMERGE_FOR_MSGFMT_OPTION = --no-location --quiet
MSGINIT = msginit
MSGCONV = msgconv
MSGFILTER = msgfilter

POFILES =  af.po be.po bg.po ca.po cs.po da.po de.po el.po eo.po es.po et.po eu.po fi.po fr.po ga.po gl.po hu.po it.po ja.po ka.po ko.po ms.po nb.po nl.po pl.po pt.po pt_BR.po ro.po ru.po rw.po sk.po sl.po sr.po sv.po tr.po uk.po vi.po zh_CN.po zh_TW.po
GMOFILES =  af.gmo be.gmo bg.gmo ca.gmo cs.gmo da.gmo de.gmo el.gmo eo.gmo es.gmo et.gmo eu.gmo fi.gmo fr.gmo ga.gmo gl.gmo hu.gmo it.gmo ja.gmo ka.gmo ko.gmo ms.gmo nb.gmo nl.gmo pl.gmo pt.gmo pt_BR.gmo ro.gmo ru.gmo rw.gmo sk.gmo sl.gmo sr.gmo sv.gmo tr.gmo uk.gmo vi.gmo zh_CN.gmo zh_TW.gmo
UPDATEPOFILES =  af.po-update be.po-update bg.po-update ca.po-update cs.po-update da.po-update de.po-update el.po-update eo.po-update es.po-update et.po-update eu.po-update fi.po-update fr.po-update ga.po-update gl.po-update hu.po-update it.po-update ja.po-update ka.po-update ko.po-update ms.po-update nb.po-update nl.po-update pl.po-update pt.po-update pt_BR.po-update ro.po-update ru.po-update rw.po-update sk.po-update sl.po-update sr.po-update sv.po-update tr.po-update uk.po-update vi.po-update zh_CN.po-update zh_TW.po-update
DUMMYPOFILES =  af.nop be.nop bg.nop ca.nop cs.nop da.nop de.nop el.nop eo.nop es.nop et.nop eu.nop fi.nop fr.nop ga.nop gl.nop hu.nop it.nop ja.nop ka.nop ko.nop ms.nop nb.nop nl.nop pl.nop pt.nop pt_BR.nop ro.nop ru.nop rw.nop sk.nop sl.nop sr.nop sv.nop tr.nop uk.nop vi.nop zh_CN.nop zh_TW.nop
DISTFILES.common = Makefile.in.in remove-potcdate.sin \
$(DISTFILES.common.extra1) $(DISTFILES.common.extra2) $(DISTFILES.common.extra3)
DISTFILES = $(DISTFILES.common) Makevars POTFILES.in \
$(POFILES) $(GMOFILES) \
$(DISTFILES.extra1) $(DISTFILES.extra2) $(DISTFILES.extra3)

POTFILES = \
     ../lib/_Noreturn.h \
     ../lib/accept.c \
     ../lib/access.c \
     ../lib/af_alg.c \
     ../lib/af_alg.h \
     ../lib/alloca.c \
     ../lib/alloca.in.h \
     ../lib/arg-nonnull.h \
     ../lib/arpa_inet.in.h \
     ../lib/array-mergesort.h \
     ../lib/asnprintf.c \
     ../lib/asprintf.c \
     ../lib/assert.in.h \
     ../lib/assure.h \
     ../lib/at-func.c \
     ../lib/attribute.h \
     ../lib/base32.c \
     ../lib/base32.h \
     ../lib/basename-lgpl.c \
     ../lib/basename-lgpl.h \
     ../lib/basename.c \
     ../lib/binary-io.c \
     ../lib/binary-io.h \
     ../lib/bind.c \
     ../lib/bitrotate.c \
     ../lib/bitrotate.h \
     ../lib/btoc32.c \
     ../lib/btowc.c \
     ../lib/byteswap.in.h \
     ../lib/c++defs.h \
     ../lib/c-ctype.c \
     ../lib/c-ctype.h \
     ../lib/c-strcase.h \
     ../lib/c-strcasecmp.c \
     ../lib/c-strcaseeq.h \
     ../lib/c-strcasestr.c \
     ../lib/c-strcasestr.h \
     ../lib/c-strncasecmp.c \
     ../lib/c32_apply_type_test.c \
     ../lib/c32_get_type_test.c \
     ../lib/c32is-impl.h \
     ../lib/c32isalnum.c \
     ../lib/c32isalpha.c \
     ../lib/c32isblank.c \
     ../lib/c32iscntrl.c \
     ../lib/c32isdigit.c \
     ../lib/c32isgraph.c \
     ../lib/c32islower.c \
     ../lib/c32isprint.c \
     ../lib/c32ispunct.c \
     ../lib/c32isspace.c \
     ../lib/c32isupper.c \
     ../lib/c32isxdigit.c \
     ../lib/c32to-impl.h \
     ../lib/c32tolower.c \
     ../lib/c32width.c \
     ../lib/calloc.c \
     ../lib/canonicalize-lgpl.c \
     ../lib/canonicalize.c \
     ../lib/canonicalize.h \
     ../lib/cdefs.h \
     ../lib/chdir-long.c \
     ../lib/chdir-long.h \
     ../lib/cloexec.c \
     ../lib/cloexec.h \
     ../lib/close.c \
     ../lib/closedir.c \
     ../lib/concat-filename.c \
     ../lib/concat-filename.h \
     ../lib/connect.c \
     ../lib/ctype.in.h \
     ../lib/dirent-private.h \
     ../lib/dirent.in.h \
     ../lib/dirfd.c \
     ../lib/dirname-lgpl.c \
     ../lib/dirname.c \
     ../lib/dirname.h \
     ../lib/dup-safer-flag.c \
     ../lib/dup-safer.c \
     ../lib/dup.c \
     ../lib/dup2.c \
     ../lib/dynarray.h \
     ../lib/eloop-threshold.h \
     ../lib/errno.in.h \
     ../lib/error.c \
     ../lib/error.in.h \
     ../lib/exitfail.c \
     ../lib/exitfail.h \
     ../lib/fatal-signal.c \
     ../lib/fatal-signal.h \
     ../lib/fchdir.c \
     ../lib/fcntl.c \
     ../lib/fcntl.in.h \
     ../lib/fd-hook.c \
     ../lib/fd-hook.h \
     ../lib/fd-safer-flag.c \
     ../lib/fd-safer.c \
     ../lib/fdopendir.c \
     ../lib/fflush.c \
     ../lib/file-set.c \
     ../lib/file-set.h \
     ../lib/filename.h \
     ../lib/filenamecat-lgpl.c \
     ../lib/filenamecat.h \
     ../lib/findprog-in.c \
     ../lib/findprog.h \
     ../lib/flexmember.h \
     ../lib/float+.h \
     ../lib/float.c \
     ../lib/float.in.h \
     ../lib/flock.c \
     ../lib/fnmatch.c \
     ../lib/fnmatch.in.h \
     ../lib/fnmatch_loop.c \
     ../lib/fopen.c \
     ../lib/fpurge.c \
     ../lib/freading.c \
     ../lib/freading.h \
     ../lib/free.c \
     ../lib/fseek.c \
     ../lib/fseeko.c \
     ../lib/fstat.c \
     ../lib/fstatat.c \
     ../lib/ftell.c \
     ../lib/ftello.c \
     ../lib/futimens.c \
     ../lib/gai_strerror.c \
     ../lib/getaddrinfo.c \
     ../lib/getcwd-lgpl.c \
     ../lib/getcwd.c \
     ../lib/getdelim.c \
     ../lib/getdtablesize.c \
     ../lib/getgroups.c \
     ../lib/getline.c \
     ../lib/getopt-cdefs.in.h \
     ../lib/getopt-core.h \
     ../lib/getopt-ext.h \
     ../lib/getopt-pfx-core.h \
     ../lib/getopt-pfx-ext.h \
     ../lib/getopt.c \
     ../lib/getopt.in.h \
     ../lib/getopt1.c \
     ../lib/getopt_int.h \
     ../lib/getpass.c \
     ../lib/getpass.h \
     ../lib/getpeername.c \
     ../lib/getprogname.c \
     ../lib/getprogname.h \
     ../lib/getrandom.c \
     ../lib/getsockname.c \
     ../lib/gettext.h \
     ../lib/gettime.c \
     ../lib/gettimeofday.c \
     ../lib/gl_openssl.h \
     ../lib/glthread/lock.c \
     ../lib/glthread/lock.h \
     ../lib/glthread/threadlib.c \
     ../lib/group-member.c \
     ../lib/hard-locale.c \
     ../lib/hard-locale.h \
     ../lib/hash-pjw.c \
     ../lib/hash-pjw.h \
     ../lib/hash-triple-simple.c \
     ../lib/hash-triple.h \
     ../lib/hash.c \
     ../lib/hash.h \
     ../lib/ialloc.c \
     ../lib/ialloc.h \
     ../lib/iconv.in.h \
     ../lib/idx.h \
     ../lib/inet_ntop.c \
     ../lib/intprops-internal.h \
     ../lib/intprops.h \
     ../lib/inttypes.in.h \
     ../lib/ioctl.c \
     ../lib/isblank.c \
     ../lib/iswblank.c \
     ../lib/iswctype-impl.h \
     ../lib/iswctype.c \
     ../lib/iswdigit.c \
     ../lib/iswpunct.c \
     ../lib/iswxdigit.c \
     ../lib/itold.c \
     ../lib/langinfo.in.h \
     ../lib/lc-charset-dispatch.c \
     ../lib/lc-charset-dispatch.h \
     ../lib/libc-config.h \
     ../lib/libunistring.valgrind \
     ../lib/limits.in.h \
     ../lib/link.c \
     ../lib/listen.c \
     ../lib/localcharset.c \
     ../lib/localcharset.h \
     ../lib/locale.in.h \
     ../lib/localeconv.c \
     ../lib/lseek.c \
     ../lib/lstat.c \
     ../lib/malloc.c \
     ../lib/malloc/dynarray-skeleton.c \
     ../lib/malloc/dynarray.h \
     ../lib/malloc/dynarray_at_failure.c \
     ../lib/malloc/dynarray_emplace_enlarge.c \
     ../lib/malloc/dynarray_finalize.c \
     ../lib/malloc/dynarray_resize.c \
     ../lib/malloc/dynarray_resize_clear.c \
     ../lib/malloc/scratch_buffer.h \
     ../lib/malloc/scratch_buffer_grow.c \
     ../lib/malloc/scratch_buffer_grow_preserve.c \
     ../lib/malloc/scratch_buffer_set_array_size.c \
     ../lib/malloca.c \
     ../lib/malloca.h \
     ../lib/mbchar.c \
     ../lib/mbchar.h \
     ../lib/mbiter.c \
     ../lib/mbiter.h \
     ../lib/mbrtoc32.c \
     ../lib/mbrtowc-impl-utf8.h \
     ../lib/mbrtowc-impl.h \
     ../lib/mbrtowc.c \
     ../lib/mbsinit.c \
     ../lib/mbsrtoc32s-state.c \
     ../lib/mbsrtoc32s.c \
     ../lib/mbsrtowcs-impl.h \
     ../lib/mbsrtowcs-state.c \
     ../lib/mbsrtowcs.c \
     ../lib/mbszero.c \
     ../lib/mbtowc-impl.h \
     ../lib/mbtowc-lock.c \
     ../lib/mbtowc-lock.h \
     ../lib/mbtowc.c \
     ../lib/md2-stream.c \
     ../lib/md2.c \
     ../lib/md2.h \
     ../lib/md4-stream.c \
     ../lib/md4.c \
     ../lib/md4.h \
     ../lib/md5-stream.c \
     ../lib/md5.c \
     ../lib/md5.h \
     ../lib/memchr.c \
     ../lib/memchr.valgrind \
     ../lib/mempcpy.c \
     ../lib/memrchr.c \
     ../lib/minmax.h \
     ../lib/mkdir.c \
     ../lib/mkostemp.c \
     ../lib/mkstemp.c \
     ../lib/mktime-internal.h \
     ../lib/mktime.c \
     ../lib/msvc-inval.c \
     ../lib/msvc-inval.h \
     ../lib/msvc-nothrow.c \
     ../lib/msvc-nothrow.h \
     ../lib/nanosleep.c \
     ../lib/netdb.in.h \
     ../lib/netinet_in.in.h \
     ../lib/nl_langinfo-lock.c \
     ../lib/nl_langinfo.c \
     ../lib/open.c \
     ../lib/openat-die.c \
     ../lib/openat-priv.h \
     ../lib/openat-proc.c \
     ../lib/openat.c \
     ../lib/openat.h \
     ../lib/opendir.c \
     ../lib/os2-spawn.c \
     ../lib/os2-spawn.h \
     ../lib/pathmax.h \
     ../lib/pipe-safer.c \
     ../lib/pipe.c \
     ../lib/pipe2-safer.c \
     ../lib/pipe2.c \
     ../lib/printf-args.c \
     ../lib/printf-args.h \
     ../lib/printf-parse.c \
     ../lib/printf-parse.h \
     ../lib/pselect.c \
     ../lib/pthread_sigmask.c \
     ../lib/quote.h \
     ../lib/quotearg.c \
     ../lib/quotearg.h \
     ../lib/raise.c \
     ../lib/rawmemchr.c \
     ../lib/rawmemchr.valgrind \
     ../lib/readdir.c \
     ../lib/readlink.c \
     ../lib/realloc.c \
     ../lib/reallocarray.c \
     ../lib/recv.c \
     ../lib/regcomp.c \
     ../lib/regex.c \
     ../lib/regex.h \
     ../lib/regex_internal.c \
     ../lib/regex_internal.h \
     ../lib/regexec.c \
     ../lib/rename.c \
     ../lib/rewinddir.c \
     ../lib/rmdir.c \
     ../lib/same-inode.c \
     ../lib/same-inode.h \
     ../lib/save-cwd.c \
     ../lib/save-cwd.h \
     ../lib/sched.in.h \
     ../lib/scratch_buffer.h \
     ../lib/secure_getenv.c \
     ../lib/select.c \
     ../lib/send.c \
     ../lib/setlocale-lock.c \
     ../lib/setlocale_null-unlocked.c \
     ../lib/setlocale_null.c \
     ../lib/setlocale_null.h \
     ../lib/setsockopt.c \
     ../lib/sha1-stream.c \
     ../lib/sha1.c \
     ../lib/sha1.h \
     ../lib/sha256-stream.c \
     ../lib/sha256.c \
     ../lib/sha256.h \
     ../lib/sha512-stream.c \
     ../lib/sha512.c \
     ../lib/sha512.h \
     ../lib/sig-handler.c \
     ../lib/sig-handler.h \
     ../lib/sigaction.c \
     ../lib/signal.in.h \
     ../lib/sigprocmask.c \
     ../lib/size_max.h \
     ../lib/snprintf.c \
     ../lib/socket.c \
     ../lib/sockets.c \
     ../lib/sockets.h \
     ../lib/spawn-pipe.c \
     ../lib/spawn-pipe.h \
     ../lib/spawn.c \
     ../lib/spawn.in.h \
     ../lib/spawn_faction_addchdir.c \
     ../lib/spawn_faction_addclose.c \
     ../lib/spawn_faction_adddup2.c \
     ../lib/spawn_faction_addopen.c \
     ../lib/spawn_faction_destroy.c \
     ../lib/spawn_faction_init.c \
     ../lib/spawn_int.h \
     ../lib/spawnattr_destroy.c \
     ../lib/spawnattr_init.c \
     ../lib/spawnattr_setflags.c \
     ../lib/spawnattr_setpgroup.c \
     ../lib/spawnattr_setsigmask.c \
     ../lib/spawni.c \
     ../lib/spawnp.c \
     ../lib/stat-time.c \
     ../lib/stat-time.h \
     ../lib/stat-w32.c \
     ../lib/stat-w32.h \
     ../lib/stat.c \
     ../lib/stdckdint.in.h \
     ../lib/stddef.in.h \
     ../lib/stdint.in.h \
     ../lib/stdio-impl.h \
     ../lib/stdio-read.c \
     ../lib/stdio-write.c \
     ../lib/stdio.in.h \
     ../lib/stdlib.in.h \
     ../lib/stpcpy.c \
     ../lib/str-two-way.h \
     ../lib/strcasecmp.c \
     ../lib/strchrnul.c \
     ../lib/strchrnul.valgrind \
     ../lib/strdup.c \
     ../lib/streq.h \
     ../lib/strerror-override.c \
     ../lib/strerror-override.h \
     ../lib/strerror.c \
     ../lib/strerror_r.c \
     ../lib/string.in.h \
     ../lib/strings.in.h \
     ../lib/stripslash.c \
     ../lib/strncasecmp.c \
     ../lib/strndup.c \
     ../lib/strnlen.c \
     ../lib/strnlen1.c \
     ../lib/strnlen1.h \
     ../lib/strpbrk.c \
     ../lib/strptime.c \
     ../lib/strtok_r.c \
     ../lib/strtol.c \
     ../lib/strtoll.c \
     ../lib/symlink.c \
     ../lib/sys-limits.h \
     ../lib/sys_file.in.h \
     ../lib/sys_ioctl.in.h \
     ../lib/sys_random.in.h \
     ../lib/sys_select.in.h \
     ../lib/sys_socket.c \
     ../lib/sys_socket.in.h \
     ../lib/sys_stat.in.h \
     ../lib/sys_time.in.h \
     ../lib/sys_types.in.h \
     ../lib/sys_uio.in.h \
     ../lib/sys_wait.in.h \
     ../lib/tempname.c \
     ../lib/tempname.h \
     ../lib/thread-optim.h \
     ../lib/time.in.h \
     ../lib/time_r.c \
     ../lib/timegm.c \
     ../lib/timespec.c \
     ../lib/timespec.h \
     ../lib/tmpdir.c \
     ../lib/tmpdir.h \
     ../lib/u64.c \
     ../lib/u64.h \
     ../lib/uchar.in.h \
     ../lib/unicase.in.h \
     ../lib/unicase/cased.c \
     ../lib/unicase/cased.h \
     ../lib/unicase/caseprop.h \
     ../lib/unicase/context.h \
     ../lib/unicase/empty-prefix-context.c \
     ../lib/unicase/empty-suffix-context.c \
     ../lib/unicase/ignorable.c \
     ../lib/unicase/ignorable.h \
     ../lib/unicase/simple-mapping.h \
     ../lib/unicase/special-casing-table.gperf \
     ../lib/unicase/special-casing.c \
     ../lib/unicase/special-casing.in.h \
     ../lib/unicase/tolower.c \
     ../lib/unicase/tolower.h \
     ../lib/unicase/u-casemap.h \
     ../lib/unicase/u8-casemap.c \
     ../lib/unicase/u8-tolower.c \
     ../lib/unicase/unicasemap.h \
     ../lib/unictype.in.h \
     ../lib/unictype/bitmap.h \
     ../lib/unictype/combiningclass.c \
     ../lib/unictype/combiningclass.h \
     ../lib/unictype/ctype_alnum.c \
     ../lib/unictype/ctype_alnum.h \
     ../lib/unictype/ctype_alpha.c \
     ../lib/unictype/ctype_alpha.h \
     ../lib/unictype/ctype_blank.c \
     ../lib/unictype/ctype_blank.h \
     ../lib/unictype/ctype_cntrl.c \
     ../lib/unictype/ctype_cntrl.h \
     ../lib/unictype/ctype_digit.c \
     ../lib/unictype/ctype_digit.h \
     ../lib/unictype/ctype_graph.c \
     ../lib/unictype/ctype_graph.h \
     ../lib/unictype/ctype_lower.c \
     ../lib/unictype/ctype_lower.h \
     ../lib/unictype/ctype_print.c \
     ../lib/unictype/ctype_print.h \
     ../lib/unictype/ctype_punct.c \
     ../lib/unictype/ctype_punct.h \
     ../lib/unictype/ctype_space.c \
     ../lib/unictype/ctype_space.h \
     ../lib/unictype/ctype_upper.c \
     ../lib/unictype/ctype_upper.h \
     ../lib/unictype/ctype_xdigit.c \
     ../lib/unictype/ctype_xdigit.h \
     ../lib/unictype/pr_soft_dotted.c \
     ../lib/unictype/pr_soft_dotted.h \
     ../lib/uninorm.in.h \
     ../lib/uninorm/decompose-internal.c \
     ../lib/uninorm/decompose-internal.h \
     ../lib/uninorm/normalize-internal.h \
     ../lib/uninorm/u-normalize-internal.h \
     ../lib/uninorm/u8-normalize.c \
     ../lib/unistd--.h \
     ../lib/unistd-safer.h \
     ../lib/unistd.c \
     ../lib/unistd.in.h \
     ../lib/unistr.in.h \
     ../lib/unistr/u-cpy.h \
     ../lib/unistr/u-pcpy.h \
     ../lib/unistr/u-strcat.h \
     ../lib/unistr/u-strlen.h \
     ../lib/unistr/u32-chr.c \
     ../lib/unistr/u32-cpy.c \
     ../lib/unistr/u32-pcpy.c \
     ../lib/unistr/u32-strcat.c \
     ../lib/unistr/u32-strlen.c \
     ../lib/unistr/u8-cpy.c \
     ../lib/unistr/u8-mbtouc-unsafe-aux.c \
     ../lib/unistr/u8-mbtouc-unsafe.c \
     ../lib/unistr/u8-strlen.c \
     ../lib/unistr/u8-uctomb-aux.c \
     ../lib/unistr/u8-uctomb.c \
     ../lib/unitypes.in.h \
     ../lib/uniwidth.in.h \
     ../lib/uniwidth/cjk.h \
     ../lib/uniwidth/width.c \
     ../lib/uniwidth/width0.h \
     ../lib/uniwidth/width2.h \
     ../lib/unlink.c \
     ../lib/unlocked-io.h \
     ../lib/utime.c \
     ../lib/utime.in.h \
     ../lib/utimens.c \
     ../lib/utimens.h \
     ../lib/vasnprintf.c \
     ../lib/vasnprintf.h \
     ../lib/vasprintf.c \
     ../lib/verify.h \
     ../lib/vsnprintf.c \
     ../lib/w32sock.h \
     ../lib/wait-process.c \
     ../lib/wait-process.h \
     ../lib/waitpid.c \
     ../lib/warn-on-use.h \
     ../lib/wchar.in.h \
     ../lib/wcrtomb.c \
     ../lib/wctype-h.c \
     ../lib/wctype-impl.h \
     ../lib/wctype.c \
     ../lib/wctype.in.h \
     ../lib/wcwidth.c \
     ../lib/windows-initguard.h \
     ../lib/windows-mutex.c \
     ../lib/windows-mutex.h \
     ../lib/windows-once.c \
     ../lib/windows-once.h \
     ../lib/windows-recmutex.c \
     ../lib/windows-recmutex.h \
     ../lib/windows-rwlock.c \
     ../lib/windows-rwlock.h \
     ../lib/windows-spawn.c \
     ../lib/windows-spawn.h \
     ../lib/wmemchr-impl.h \
     ../lib/wmemchr.c \
     ../lib/wmempcpy.c \
     ../lib/write.c \
     ../lib/xalloc-die.c \
     ../lib/xalloc-oversized.h \
     ../lib/xalloc.h \
     ../lib/xmalloc.c \
     ../lib/xmemdup0.c \
     ../lib/xmemdup0.h \
     ../lib/xsize.c \
     ../lib/xsize.h \
     ../lib/xstrndup.c \
     ../lib/xstrndup.h

CATALOGS =  af.gmo be.gmo bg.gmo ca.gmo cs.gmo da.gmo de.gmo el.gmo eo.gmo es.gmo et.gmo eu.gmo fi.gmo fr.gmo ga.gmo gl.gmo hu.gmo it.gmo ja.gmo ka.gmo ko.gmo ms.gmo nb.gmo nl.gmo pl.gmo pt.gmo pt_BR.gmo ro.gmo ru.gmo rw.gmo sk.gmo sl.gmo sr.gmo sv.gmo tr.gmo uk.gmo vi.gmo zh_CN.gmo zh_TW.gmo

POFILESDEPS_ = $(srcdir)/$(DOMAIN).pot
POFILESDEPS_yes = $(POFILESDEPS_)
POFILESDEPS_no =
POFILESDEPS = $(POFILESDEPS_$(PO_DEPENDS_ON_POT))

DISTFILESDEPS_ = update-po
DISTFILESDEPS_yes = $(DISTFILESDEPS_)
DISTFILESDEPS_no =
DISTFILESDEPS = $(DISTFILESDEPS_$(DIST_DEPENDS_ON_UPDATE_PO))

# Makevars gets inserted here. (Don't remove this line!)
## DO NOT EDIT! GENERATED AUTOMATICALLY!
# Copyright (C) 2002-2024 Free Software Foundation, Inc.
#
# This file is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This file is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this file.  If not, see <https://www.gnu.org/licenses/>.
#
# As a special exception to the GNU General Public License,
# this file may be distributed as part of a program that
# contains a configuration script generated by Autoconf, under
# the same distribution terms as the rest of that program.
#
# Generated by gnulib-tool.

# Usually the message domain is the same as the package name.
# But here it has a '-gnulib' suffix.
DOMAIN = wget-gnulib

# These two variables depend on the location of this directory.
subdir = gnulib_po
top_builddir = ..

# These options get passed to xgettext.
XGETTEXT_OPTIONS = \
  --keyword=_ --flag=_:1:pass-c-format \
  --keyword=N_ --flag=N_:1:pass-c-format \
  --keyword='proper_name:1,"This is a proper name. See the gettext manual, section Names."' \
  --keyword='proper_name_lite:1,"This is a proper name. See the gettext manual, section Names."' \
  --keyword='proper_name_utf8:1,"This is a proper name. See the gettext manual, section Names."' \
  --flag=error:3:c-format --flag=error_at_line:5:c-format

# This is the copyright holder that gets inserted into the header of the
# $(DOMAIN).pot file.  gnulib is copyrighted by the FSF.
COPYRIGHT_HOLDER = Free Software Foundation, Inc.

# This is the email address or URL to which the translators shall report
# bugs in the untranslated strings:
# - Strings which are not entire sentences, see the maintainer guidelines
#   in the GNU gettext documentation, section 'Preparing Strings'.
# - Strings which use unclear terms or require additional context to be
#   understood.
# - Strings which make invalid assumptions about notation of date, time or
#   money.
# - Pluralisation problems.
# - Incorrect English spelling.
# - Incorrect formatting.
# It can be your email address, or a mailing list address where translators
# can write to without being subscribed, or the URL of a web page through
# which the translators can contact you.
MSGID_BUGS_ADDRESS = bug-gnulib@gnu.org

# This is the list of locale categories, beyond LC_MESSAGES, for which the
# message catalogs shall be used.  It is usually empty.
EXTRA_LOCALE_CATEGORIES =

# This tells whether the $(DOMAIN).pot file contains messages with an 'msgctxt'
# context.  Possible values are "yes" and "no".  Set this to yes if the
# package uses functions taking also a message context, like pgettext(), or
# if in $(XGETTEXT_OPTIONS) you define keywords with a context argument.
USE_MSGCTXT = no

all: all-no


.SUFFIXES:
.SUFFIXES: .po .gmo .sed .sin .nop .po-create .po-update

# The .pot file, stamp-po, .po files, and .gmo files appear in release tarballs.
# The GNU Coding Standards say in
# <https://www.gnu.org/prep/standards/html_node/Makefile-Basics.html>:
#   "GNU distributions usually contain some files which are not source files
#    ... . Since these files normally appear in the source directory, they
#    should always appear in the source directory, not in the build directory.
#    So Makefile rules to update them should put the updated files in the
#    source directory."
# Therefore we put these files in the source directory, not the build directory.

# During .po -> .gmo conversion, take into account the most recent changes to
# the .pot file. This eliminates the need to update the .po files when the
# .pot file has changed, which would be troublesome if the .po files are put
# under version control.
$(GMOFILES): $(srcdir)/$(DOMAIN).pot
.po.gmo:
	@lang=`echo $* | sed -e 's,.*/,,'`; \
	test "$(srcdir)" = . && cdcmd="" || cdcmd="cd $(srcdir) && "; \
	echo "$${cdcmd}rm -f $${lang}.gmo && $(MSGMERGE) $(MSGMERGE_FOR_MSGFMT_OPTION) -o $${lang}.1po $${lang}.po $(DOMAIN).pot && $(GMSGFMT) -c --statistics --verbose -o $${lang}.gmo $${lang}.1po && rm -f $${lang}.1po"; \
	cd $(srcdir) && \
	rm -f $${lang}.gmo && \
	$(MSGMERGE) $(MSGMERGE_FOR_MSGFMT_OPTION) -o $${lang}.1po $${lang}.po $(DOMAIN).pot && \
	$(GMSGFMT) -c --statistics --verbose -o t-$${lang}.gmo $${lang}.1po && \
	mv t-$${lang}.gmo $${lang}.gmo && \
	rm -f $${lang}.1po

.sin.sed:
	sed -e '/^#/d' $< > t-$@
	mv t-$@ $@


all-yes: $(srcdir)/stamp-po
all-no:

# Ensure that the gettext macros and this Makefile.in.in are in sync.
CHECK_MACRO_VERSION = \
	test "$(GETTEXT_MACRO_VERSION)" = "0.20" \
	  || { echo "*** error: gettext infrastructure mismatch: using a Makefile.in.in from gettext version $(GETTEXT_MACRO_VERSION) but the autoconf macros are from gettext version 0.20" 1>&2; \
	       exit 1; \
	     }

# $(srcdir)/$(DOMAIN).pot is only created when needed. When xgettext finds no
# internationalized messages, no $(srcdir)/$(DOMAIN).pot is created (because
# we don't want to bother translators with empty POT files). We assume that
# LINGUAS is empty in this case, i.e. $(POFILES) and $(GMOFILES) are empty.
# In this case, $(srcdir)/stamp-po is a nop (i.e. a phony target).

# $(srcdir)/stamp-po is a timestamp denoting the last time at which the CATALOGS
# have been loosely updated. Its purpose is that when a developer or translator
# checks out the package from a version control system, and the $(DOMAIN).pot
# file is not under version control, "make" will update the $(DOMAIN).pot and
# the $(CATALOGS), but subsequent invocations of "make" will do nothing. This
# timestamp would not be necessary if updating the $(CATALOGS) would always
# touch them; however, the rule for $(POFILES) has been designed to not touch
# files that don't need to be changed.
$(srcdir)/stamp-po: $(srcdir)/$(DOMAIN).pot
	@$(CHECK_MACRO_VERSION)
	test ! -f $(srcdir)/$(DOMAIN).pot || \
	  test -z "$(GMOFILES)" || $(MAKE) $(GMOFILES)
	@test ! -f $(srcdir)/$(DOMAIN).pot || { \
	  echo "touch $(srcdir)/stamp-po" && \
	  echo timestamp > $(srcdir)/stamp-poT && \
	  mv $(srcdir)/stamp-poT $(srcdir)/stamp-po; \
	}

# Note: Target 'all' must not depend on target '$(DOMAIN).pot-update',
# otherwise packages like GCC can not be built if only parts of the source
# have been downloaded.

# This target rebuilds $(DOMAIN).pot; it is an expensive operation.
# Note that $(DOMAIN).pot is not touched if it doesn't need to be changed.
# The determination of whether the package xyz is a GNU one is based on the
# heuristic whether some file in the top level directory mentions "GNU xyz".
# If GNU 'find' is available, we avoid grepping through monster files.
$(DOMAIN).pot-update: $(POTFILES) $(srcdir)/POTFILES.in remove-potcdate.sed
	package_gnu="$(PACKAGE_GNU)"; \
	test -n "$$package_gnu" || { \
	  if { if (LC_ALL=C find --version) 2>/dev/null | grep GNU >/dev/null; then \
	         LC_ALL=C find -L $(top_srcdir) -maxdepth 1 -type f -size -10000000c -exec grep -i 'GNU wget' /dev/null '{}' ';' 2>/dev/null; \
	       else \
	         LC_ALL=C grep -i 'GNU wget' $(top_srcdir)/* 2>/dev/null; \
	       fi; \
	     } | grep -v 'libtool:' >/dev/null; then \
	     package_gnu=yes; \
	   else \
	     package_gnu=no; \
	   fi; \
	}; \
	if test "$$package_gnu" = "yes"; then \
	  package_prefix='GNU '; \
	else \
	  package_prefix=''; \
	fi; \
	if test -n '$(MSGID_BUGS_ADDRESS)' || test '$(PACKAGE_BUGREPORT)' = '@'PACKAGE_BUGREPORT'@'; then \
	  msgid_bugs_address='$(MSGID_BUGS_ADDRESS)'; \
	else \
	  msgid_bugs_address='$(PACKAGE_BUGREPORT)'; \
	fi; \
	case `$(XGETTEXT) --version | sed 1q | sed -e 's,^[^0-9]*,,'` in \
	  '' | 0.[0-9] | 0.[0-9].* | 0.1[0-5] | 0.1[0-5].* | 0.16 | 0.16.[0-1]*) \
	    $(XGETTEXT) --default-domain=$(DOMAIN) --directory=$(top_srcdir) \
	      --add-comments=TRANSLATORS: \
	      --files-from=$(srcdir)/POTFILES.in \
	      --copyright-holder='$(COPYRIGHT_HOLDER)' \
	      --msgid-bugs-address="$$msgid_bugs_address" \
	      $(XGETTEXT_OPTIONS)  --flag=error:3:c-format --flag=error_at_line:5:c-format --flag=asprintf:2:c-format --flag=vasprintf:2:c-format \
	    ;; \
	  *) \
	    $(XGETTEXT) --default-domain=$(DOMAIN) --directory=$(top_srcdir) \
	      --add-comments=TRANSLATORS: \
	      --files-from=$(srcdir)/POTFILES.in \
	      --copyright-holder='$(COPYRIGHT_HOLDER)' \
	      --package-name="$${package_prefix}wget" \
	      --package-version='1.24.5' \
	      --msgid-bugs-address="$$msgid_bugs_address" \
	      $(XGETTEXT_OPTIONS)  --flag=error:3:c-format --flag=error_at_line:5:c-format --flag=asprintf:2:c-format --flag=vasprintf:2:c-format \
	    ;; \
	esac
	test ! -f $(DOMAIN).po || { \
	  if test -f $(srcdir)/$(DOMAIN).pot-header; then \
	    sed -e '1,/^#$$/d' < $(DOMAIN).po > $(DOMAIN).1po && \
	    cat $(srcdir)/$(DOMAIN).pot-header $(DOMAIN).1po > $(DOMAIN).po && \
	    rm -f $(DOMAIN).1po \
	    || exit 1; \
	  fi; \
	  if test -f $(srcdir)/$(DOMAIN).pot; then \
	    sed -f remove-potcdate.sed < $(srcdir)/$(DOMAIN).pot > $(DOMAIN).1po && \
	    sed -f remove-potcdate.sed < $(DOMAIN).po > $(DOMAIN).2po && \
	    if cmp $(DOMAIN).1po $(DOMAIN).2po >/dev/null 2>&1; then \
	      rm -f $(DOMAIN).1po $(DOMAIN).2po $(DOMAIN).po; \
	    else \
	      rm -f $(DOMAIN).1po $(DOMAIN).2po $(srcdir)/$(DOMAIN).pot && \
	      mv $(DOMAIN).po $(srcdir)/$(DOMAIN).pot; \
	    fi; \
	  else \
	    mv $(DOMAIN).po $(srcdir)/$(DOMAIN).pot; \
	  fi; \
	}

# This rule has no dependencies: we don't need to update $(DOMAIN).pot at
# every "make" invocation, only create it when it is missing.
# Only "make $(DOMAIN).pot-update" or "make dist" will force an update.
$(srcdir)/$(DOMAIN).pot:
	$(MAKE) $(DOMAIN).pot-update

# This target rebuilds a PO file if $(DOMAIN).pot has changed.
# Note that a PO file is not touched if it doesn't need to be changed.
$(POFILES): $(POFILESDEPS)
	@test -f $(srcdir)/$(DOMAIN).pot || $(MAKE) $(srcdir)/$(DOMAIN).pot
	@lang=`echo $@ | sed -e 's,.*/,,' -e 's/\.po$$//'`; \
	if test -f "$(srcdir)/$${lang}.po"; then \
	  test "$(srcdir)" = . && cdcmd="" || cdcmd="cd $(srcdir) && "; \
	  echo "$${cdcmd}$(MSGMERGE_UPDATE) --quiet $(MSGMERGE_OPTIONS) --lang=$${lang} --previous $${lang}.po $(DOMAIN).pot"; \
	  cd $(srcdir) \
	    && { case `$(MSGMERGE_UPDATE) --version | sed 1q | sed -e 's,^[^0-9]*,,'` in \
	           '' | 0.[0-9] | 0.[0-9].* | 0.10 | 0.10.*) \
	             $(MSGMERGE_UPDATE) $(MSGMERGE_OPTIONS) $${lang}.po $(DOMAIN).pot;; \
	           0.1[1-5] | 0.1[1-5].*) \
	             $(MSGMERGE_UPDATE) --quiet $(MSGMERGE_OPTIONS) $${lang}.po $(DOMAIN).pot;; \
	           0.1[6-7] | 0.1[6-7].*) \
	             $(MSGMERGE_UPDATE) --quiet $(MSGMERGE_OPTIONS) --previous $${lang}.po $(DOMAIN).pot;; \
	           *) \
	             $(MSGMERGE_UPDATE) --quiet $(MSGMERGE_OPTIONS) --lang=$${lang} --previous $${lang}.po $(DOMAIN).pot;; \
	         esac; \
	       }; \
	else \
	  $(MAKE) $${lang}.po-create; \
	fi


install: install-exec install-data
install-exec:
install-data: install-data-no
	if test "$(PACKAGE)" = "gettext-tools"; then \
	  $(mkdir_p) $(DESTDIR)$(gettextsrcdir); \
	  for file in $(DISTFILES.common) Makevars.template; do \
	    $(INSTALL_DATA) $(srcdir)/$$file \
			    $(DESTDIR)$(gettextsrcdir)/$$file; \
	  done; \
	  for file in Makevars; do \
	    rm -f $(DESTDIR)$(gettextsrcdir)/$$file; \
	  done; \
	else \
	  : ; \
	fi
install-data-no: all
install-data-yes: all
	@catalogs='$(CATALOGS)'; \
	for cat in $$catalogs; do \
	  cat=`basename $$cat`; \
	  lang=`echo $$cat | sed -e 's/\.gmo$$//'`; \
	  dir=$(localedir)/$$lang/LC_MESSAGES; \
	  $(mkdir_p) $(DESTDIR)$$dir; \
	  if test -r $$cat; then realcat=$$cat; else realcat=$(srcdir)/$$cat; fi; \
	  $(INSTALL_DATA) $$realcat $(DESTDIR)$$dir/$(DOMAIN).mo; \
	  echo "installing $$realcat as $(DESTDIR)$$dir/$(DOMAIN).mo"; \
	  for lc in '' $(EXTRA_LOCALE_CATEGORIES); do \
	    if test -n "$$lc"; then \
	      if (cd $(DESTDIR)$(localedir)/$$lang && LC_ALL=C ls -l -d $$lc 2>/dev/null) | grep ' -> ' >/dev/null; then \
	        link=`cd $(DESTDIR)$(localedir)/$$lang && LC_ALL=C ls -l -d $$lc | sed -e 's/^.* -> //'`; \
	        mv $(DESTDIR)$(localedir)/$$lang/$$lc $(DESTDIR)$(localedir)/$$lang/$$lc.old; \
	        mkdir $(DESTDIR)$(localedir)/$$lang/$$lc; \
	        (cd $(DESTDIR)$(localedir)/$$lang/$$lc.old && \
	         for file in *; do \
	           if test -f $$file; then \
	             ln -s ../$$link/$$file $(DESTDIR)$(localedir)/$$lang/$$lc/$$file; \
	           fi; \
	         done); \
	        rm -f $(DESTDIR)$(localedir)/$$lang/$$lc.old; \
	      else \
	        if test -d $(DESTDIR)$(localedir)/$$lang/$$lc; then \
	          :; \
	        else \
	          rm -f $(DESTDIR)$(localedir)/$$lang/$$lc; \
	          mkdir $(DESTDIR)$(localedir)/$$lang/$$lc; \
	        fi; \
	      fi; \
	      rm -f $(DESTDIR)$(localedir)/$$lang/$$lc/$(DOMAIN).mo; \
	      ln -s ../LC_MESSAGES/$(DOMAIN).mo $(DESTDIR)$(localedir)/$$lang/$$lc/$(DOMAIN).mo 2>/dev/null || \
	      ln $(DESTDIR)$(localedir)/$$lang/LC_MESSAGES/$(DOMAIN).mo $(DESTDIR)$(localedir)/$$lang/$$lc/$(DOMAIN).mo 2>/dev/null || \
	      cp -p $(DESTDIR)$(localedir)/$$lang/LC_MESSAGES/$(DOMAIN).mo $(DESTDIR)$(localedir)/$$lang/$$lc/$(DOMAIN).mo; \
	      echo "installing $$realcat link as $(DESTDIR)$(localedir)/$$lang/$$lc/$(DOMAIN).mo"; \
	    fi; \
	  done; \
	done

install-strip: install

installdirs: installdirs-exec installdirs-data
installdirs-exec:
installdirs-data: installdirs-data-no
	if test "$(PACKAGE)" = "gettext-tools"; then \
	  $(mkdir_p) $(DESTDIR)$(gettextsrcdir); \
	else \
	  : ; \
	fi
installdirs-data-no:
installdirs-data-yes:
	@catalogs='$(CATALOGS)'; \
	for cat in $$catalogs; do \
	  cat=`basename $$cat`; \
	  lang=`echo $$cat | sed -e 's/\.gmo$$//'`; \
	  dir=$(localedir)/$$lang/LC_MESSAGES; \
	  $(mkdir_p) $(DESTDIR)$$dir; \
	  for lc in '' $(EXTRA_LOCALE_CATEGORIES); do \
	    if test -n "$$lc"; then \
	      if (cd $(DESTDIR)$(localedir)/$$lang && LC_ALL=C ls -l -d $$lc 2>/dev/null) | grep ' -> ' >/dev/null; then \
	        link=`cd $(DESTDIR)$(localedir)/$$lang && LC_ALL=C ls -l -d $$lc | sed -e 's/^.* -> //'`; \
	        mv $(DESTDIR)$(localedir)/$$lang/$$lc $(DESTDIR)$(localedir)/$$lang/$$lc.old; \
	        mkdir $(DESTDIR)$(localedir)/$$lang/$$lc; \
	        (cd $(DESTDIR)$(localedir)/$$lang/$$lc.old && \
	         for file in *; do \
	           if test -f $$file; then \
	             ln -s ../$$link/$$file $(DESTDIR)$(localedir)/$$lang/$$lc/$$file; \
	           fi; \
	         done); \
	        rm -f $(DESTDIR)$(localedir)/$$lang/$$lc.old; \
	      else \
	        if test -d $(DESTDIR)$(localedir)/$$lang/$$lc; then \
	          :; \
	        else \
	          rm -f $(DESTDIR)$(localedir)/$$lang/$$lc; \
	          mkdir $(DESTDIR)$(localedir)/$$lang/$$lc; \
	        fi; \
	      fi; \
	    fi; \
	  done; \
	done

# Define this as empty until I found a useful application.
installcheck:

uninstall: uninstall-exec uninstall-data
uninstall-exec:
uninstall-data: uninstall-data-no
	if test "$(PACKAGE)" = "gettext-tools"; then \
	  for file in $(DISTFILES.common) Makevars.template; do \
	    rm -f $(DESTDIR)$(gettextsrcdir)/$$file; \
	  done; \
	else \
	  : ; \
	fi
uninstall-data-no:
uninstall-data-yes:
	catalogs='$(CATALOGS)'; \
	for cat in $$catalogs; do \
	  cat=`basename $$cat`; \
	  lang=`echo $$cat | sed -e 's/\.gmo$$//'`; \
	  for lc in LC_MESSAGES $(EXTRA_LOCALE_CATEGORIES); do \
	    rm -f $(DESTDIR)$(localedir)/$$lang/$$lc/$(DOMAIN).mo; \
	  done; \
	done

check: all

info dvi ps pdf html tags TAGS ctags CTAGS ID:

install-dvi install-ps install-pdf install-html:

mostlyclean:
	rm -f remove-potcdate.sed
	rm -f $(srcdir)/stamp-poT
	rm -f core core.* $(DOMAIN).po $(DOMAIN).1po $(DOMAIN).2po *.new.po
	rm -fr *.o

clean: mostlyclean

distclean: clean
	rm -f Makefile Makefile.in POTFILES

maintainer-clean: distclean
	@echo "This command is intended for maintainers to use;"
	@echo "it deletes files that may require special tools to rebuild."
	rm -f $(srcdir)/$(DOMAIN).pot $(srcdir)/stamp-po $(GMOFILES)

distdir = $(top_builddir)/$(PACKAGE)-$(VERSION)/$(subdir)
dist distdir:
	test -z "$(DISTFILESDEPS)" || $(MAKE) $(DISTFILESDEPS)
	@$(MAKE) dist2
# This is a separate target because 'update-po' must be executed before.
dist2: $(srcdir)/stamp-po $(DISTFILES)
	@dists="$(DISTFILES)"; \
	if test "$(PACKAGE)" = "gettext-tools"; then \
	  dists="$$dists Makevars.template"; \
	fi; \
	if test -f $(srcdir)/$(DOMAIN).pot; then \
	  dists="$$dists $(DOMAIN).pot stamp-po"; \
	else \
	  case $(XGETTEXT) in \
	    :) echo "Warning: Creating a tarball without '$(DOMAIN).pot', because a suitable 'xgettext' program was not found in PATH." 1>&2;; \
	    *) echo "Warning: Creating a tarball without '$(DOMAIN).pot', because 'xgettext' found no strings to extract. Check the contents of the POTFILES.in file and the XGETTEXT_OPTIONS in the Makevars file." 1>&2;; \
	  esac; \
	fi; \
	if test -f $(srcdir)/ChangeLog; then \
	  dists="$$dists ChangeLog"; \
	fi; \
	for i in 0 1 2 3 4 5 6 7 8 9; do \
	  if test -f $(srcdir)/ChangeLog.$$i; then \
	    dists="$$dists ChangeLog.$$i"; \
	  fi; \
	done; \
	if test -f $(srcdir)/LINGUAS; then dists="$$dists LINGUAS"; fi; \
	for file in $$dists; do \
	  if test -f $$file; then \
	    cp -p $$file $(distdir) || exit 1; \
	  else \
	    cp -p $(srcdir)/$$file $(distdir) || exit 1; \
	  fi; \
	done

update-po: Makefile
	$(MAKE) $(DOMAIN).pot-update
	test -z "$(UPDATEPOFILES)" || $(MAKE) $(UPDATEPOFILES)
	$(MAKE) update-gmo

# General rule for creating PO files.

.nop.po-create:
	@lang=`echo $@ | sed -e 's/\.po-create$$//'`; \
	echo "File $$lang.po does not exist. If you are a translator, you can create it through 'msginit'." 1>&2; \
	exit 1

# General rule for updating PO files.

.nop.po-update:
	@lang=`echo $@ | sed -e 's/\.po-update$$//'`; \
	if test "$(PACKAGE)" = "gettext-tools" && test "$(CROSS_COMPILING)" != "yes"; then PATH=`pwd`/../src:$$PATH; fi; \
	tmpdir=`pwd`; \
	echo "$$lang:"; \
	test "$(srcdir)" = . && cdcmd="" || cdcmd="cd $(srcdir) && "; \
	echo "$${cdcmd}$(MSGMERGE) --quiet $(MSGMERGE_OPTIONS) --lang=$$lang --previous $$lang.po $(DOMAIN).pot -o $$lang.new.po"; \
	cd $(srcdir); \
	if { case `$(MSGMERGE) --version | sed 1q | sed -e 's,^[^0-9]*,,'` in \
	       '' | 0.[0-9] | 0.[0-9].* | 0.10 | 0.10.*) \
	         $(MSGMERGE) $(MSGMERGE_OPTIONS) -o $$tmpdir/$$lang.new.po $$lang.po $(DOMAIN).pot;; \
	       0.1[1-5] | 0.1[1-5].*) \
	         $(MSGMERGE) --quiet $(MSGMERGE_OPTIONS) -o $$tmpdir/$$lang.new.po $$lang.po $(DOMAIN).pot;; \
	       0.1[6-7] | 0.1[6-7].*) \
	         $(MSGMERGE) --quiet $(MSGMERGE_OPTIONS) --previous -o $$tmpdir/$$lang.new.po $$lang.po $(DOMAIN).pot;; \
	       *) \
	         $(MSGMERGE) --quiet $(MSGMERGE_OPTIONS) --lang=$$lang --previous -o $$tmpdir/$$lang.new.po $$lang.po $(DOMAIN).pot;; \
	     esac; \
	   }; then \
	  if cmp $$lang.po $$tmpdir/$$lang.new.po >/dev/null 2>&1; then \
	    rm -f $$tmpdir/$$lang.new.po; \
	  else \
	    if mv -f $$tmpdir/$$lang.new.po $$lang.po; then \
	      :; \
	    else \
	      echo "msgmerge for $$lang.po failed: cannot move $$tmpdir/$$lang.new.po to $$lang.po" 1>&2; \
	      exit 1; \
	    fi; \
	  fi; \
	else \
	  echo "msgmerge for $$lang.po failed!" 1>&2; \
	  rm -f $$tmpdir/$$lang.new.po; \
	fi

$(DUMMYPOFILES):

update-gmo: Makefile $(GMOFILES)
	@:

# Recreate Makefile by invoking config.status. Explicitly invoke the shell,
# because execution permission bits may not work on the current file system.
# Use /bin/bash, which is the shell determined by autoconf for the use by its
# scripts, not $(SHELL) which is hardwired to /bin/sh and may be deficient.
Makefile: Makefile.in.in Makevars $(top_builddir)/config.status POTFILES.in LINGUAS
	cd $(top_builddir) \
	  && /bin/bash ./config.status $(subdir)/$@.in po-directories

force:

# Tell versions [3.59,3.63) of GNU make not to export all variables.
# Otherwise a system limit (for SysV at least) may be exceeded.
.NOEXPORT:
# Special Makefile rules for English message catalogs with quotation marks.
#
# Copyright (C) 2001-2017 Free Software Foundation, Inc.
# This file, Rules-quot, and its auxiliary files (listed under
# DISTFILES.common.extra1) are free software; the Free Software Foundation
# gives unlimited permission to use, copy, distribute, and modify them.

DISTFILES.common.extra1 = quot.sed boldquot.sed en@quot.header en@boldquot.header insert-header.sin Rules-quot

.SUFFIXES: .insert-header .po-update-en

en@quot.po-create:
	$(MAKE) en@quot.po-update
en@boldquot.po-create:
	$(MAKE) en@boldquot.po-update

en@quot.po-update: en@quot.po-update-en
en@boldquot.po-update: en@boldquot.po-update-en

.insert-header.po-update-en:
	@lang=`echo $@ | sed -e 's/\.po-update-en$$//'`; \
	if test "$(PACKAGE)" = "gettext-tools" && test "$(CROSS_COMPILING)" != "yes"; then PATH=`pwd`/../src:$$PATH; GETTEXTLIBDIR=`cd $(top_srcdir)/src && pwd`; export GETTEXTLIBDIR; fi; \
	tmpdir=`pwd`; \
	echo "$$lang:"; \
	ll=`echo $$lang | sed -e 's/@.*//'`; \
	LC_ALL=C; export LC_ALL; \
	cd $(srcdir); \
	if $(MSGINIT) $(MSGINIT_OPTIONS) -i $(DOMAIN).pot --no-translator -l $$lang -o - 2>/dev/null \
	   | $(SED) -f $$tmpdir/$$lang.insert-header | $(MSGCONV) -t UTF-8 | \
	   { case `$(MSGFILTER) --version | sed 1q | sed -e 's,^[^0-9]*,,'` in \
	     '' | 0.[0-9] | 0.[0-9].* | 0.1[0-8] | 0.1[0-8].*) \
	       $(MSGFILTER) $(SED) -f `echo $$lang | sed -e 's/.*@//'`.sed \
	       ;; \
	     *) \
	       $(MSGFILTER) `echo $$lang | sed -e 's/.*@//'` \
	       ;; \
	     esac } 2>/dev/null > $$tmpdir/$$lang.new.po \
	     ; then \
	  if cmp $$lang.po $$tmpdir/$$lang.new.po >/dev/null 2>&1; then \
	    rm -f $$tmpdir/$$lang.new.po; \
	  else \
	    if mv -f $$tmpdir/$$lang.new.po $$lang.po; then \
	      :; \
	    else \
	      echo "creation of $$lang.po failed: cannot move $$tmpdir/$$lang.new.po to $$lang.po" 1>&2; \
	      exit 1; \
	    fi; \
	  fi; \
	else \
	  echo "creation of $$lang.po failed!" 1>&2; \
	  rm -f $$tmpdir/$$lang.new.po; \
	fi

en@quot.insert-header: insert-header.sin
	sed -e '/^#/d' -e 's/HEADER/en@quot.header/g' $(srcdir)/insert-header.sin > en@quot.insert-header

en@boldquot.insert-header: insert-header.sin
	sed -e '/^#/d' -e 's/HEADER/en@boldquot.header/g' $(srcdir)/insert-header.sin > en@boldquot.insert-header

mostlyclean: mostlyclean-quot
mostlyclean-quot:
	rm -f *.insert-header
