/* Test of <errno.h> substitute.
   Copyright (C) 2008-2025 Free Software Foundation, Inc.

   This program is free software: you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published by
   the Free Software Foundation, either version 3 of the License, or
   (at your option) any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

/* Written by Bruno Haible <bruno@clisp.org>, 2008.  */

#include <config.h>

#include <errno.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Check all POSIX-defined errno values, using M (v) to check value v.  */
#define CHECK_POSIX_ERRNOS(m) \
  m (E2BIG) \
  m (EACCES) \
  m (EADDRINUSE) \
  m (EADDRNOTAVAIL) \
  m (EAFNOSUPPORT) \
  m (EAGAIN) \
  m (EALREADY) \
  m (EBADF) \
  m (EBADMSG) \
  m (EBUSY) \
  m (ECANCELED) \
  m (ECHILD) \
  m (ECONNABORTED) \
  m (ECONNREFUSED) \
  m (ECONNRESET) \
  m (EDEADLK) \
  m (EDESTADDRREQ) \
  m (EDOM) \
  m (EDQUOT) \
  m (EEXIST) \
  m (EFAULT) \
  m (EFBIG) \
  m (EHOSTUNREACH) \
  m (EIDRM) \
  m (EILSEQ) \
  m (EINPROGRESS) \
  m (EINTR) \
  m (EINVAL) \
  m (EIO) \
  m (EISCONN) \
  m (EISDIR) \
  m (ELOOP) \
  m (EMFILE) \
  m (EMLINK) \
  m (EMSGSIZE) \
  m (EMULTIHOP) \
  m (ENAMETOOLONG) \
  m (ENETDOWN) \
  m (ENETRESET) \
  m (ENETUNREACH) \
  m (ENFILE) \
  m (ENOBUFS) \
  m (ENODEV) \
  m (ENOENT) \
  m (ENOEXEC) \
  m (ENOLCK) \
  m (ENOLINK) \
  m (ENOMEM) \
  m (ENOMSG) \
  m (ENOPROTOOPT) \
  m (ENOSPC) \
  m (ENOSYS) \
  m (ENOTCONN) \
  m (ENOTDIR) \
  m (ENOTEMPTY) \
  m (ENOTRECOVERABLE) \
  m (ENOTSOCK) \
  m (ENOTSUP) \
  m (ENOTTY) \
  m (ENXIO) \
  m (EOPNOTSUPP) \
  m (EOVERFLOW) \
  m (EOWNERDEAD) \
  m (EPERM) \
  m (EPIPE) \
  m (EPROTO) \
  m (EPROTONOSUPPORT) \
  m (EPROTOTYPE) \
  m (ERANGE) \
  m (EROFS) \
  m (ESOCKTNOSUPPORT) \
  m (ESPIPE) \
  m (ESRCH) \
  m (ESTALE) \
  m (ETIMEDOUT) \
  m (ETXTBSY) \
  m (EWOULDBLOCK) \
  m (EXDEV) \
  /* end of CHECK_POSIX_ERRNOS */

/* Verify that the POSIX mandated errno values can be used as integer
   constant expressions and are all positive (except on Haiku).  */
#if defined __HAIKU__
# define NONZERO_INTEGER_CONSTANT_EXPRESSION(e) static_assert (0 != (e) << 0);
CHECK_POSIX_ERRNOS (NONZERO_INTEGER_CONSTANT_EXPRESSION)
#else
# define POSITIVE_INTEGER_CONSTANT_EXPRESSION(e) static_assert (0 < (e) << 0);
CHECK_POSIX_ERRNOS (POSITIVE_INTEGER_CONSTANT_EXPRESSION)
#endif

/* Verify that errno values can all be used in #if.  */
#define USABLE_IN_IF(e) ^ e
#if 0 CHECK_POSIX_ERRNOS (USABLE_IN_IF)
#endif

/* Check that errno values all differ, except possibly for
   EWOULDBLOCK == EAGAIN and ENOTSUP == EOPNOTSUPP.  */
#define ERRTAB(e) { #e, e },
static struct nameval { char const *name; int value; }
  errtab[] = { CHECK_POSIX_ERRNOS (ERRTAB) };

static int
errtab_cmp (void const *va, void const *vb)
{
  struct nameval const *a = va, *b = vb;

  /* Sort by value first, then by name (to simplify later tests).
     Subtraction cannot overflow as both are positive.  */
  int diff = a->value - b->value;
  return diff ? diff : strcmp (a->name, b->name);
}

int
main ()
{
  int test_exit_status = EXIT_SUCCESS;

  /* Verify that errno can be assigned.  */
  errno = EOVERFLOW;

  /* Check that errno values all differ, except possibly for
     EAGAIN == EWOULDBLOCK and ENOTSUP == EOPNOTSUPP.  */
  int nerrtab = sizeof errtab / sizeof *errtab;
  qsort (errtab, nerrtab, sizeof *errtab, errtab_cmp);
  for (int i = 1; i < nerrtab; i++)
    if (errtab[i - 1].value == errtab[i].value)
      {
        fprintf (stderr, "%s == %s == %d\n",
                 errtab[i - 1].name, errtab[i].name, errtab[i].value);
        if (! ((strcmp ("EAGAIN", errtab[i - 1].name) == 0
                && strcmp ("EWOULDBLOCK", errtab[i].name) == 0)
               || (strcmp ("ENOTSUP", errtab[i - 1].name) == 0
                   && strcmp ("EOPNOTSUPP", errtab[i].name) == 0)))
          test_exit_status = EXIT_FAILURE;
      }

  return test_exit_status;
}
