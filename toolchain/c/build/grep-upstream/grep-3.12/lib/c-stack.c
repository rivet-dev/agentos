/* Stack overflow handling.

   Copyright (C) 2002, 2004, 2006, 2008-2025 Free Software Foundation, Inc.

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

/* Written by Paul Eggert.  */

/* NOTES:

   A program that uses alloca, dynamic arrays, or large local
   variables may extend the stack by more than a page at a time.  If
   so, when the stack overflows the operating system may not detect
   the overflow until the program uses the array, and this module may
   incorrectly report a program error instead of a stack overflow.

   To avoid this problem, allocate only small objects on the stack; a
   program should be OK if it limits single allocations to a page or
   less.  Allocate larger arrays in static storage, or on the heap
   (e.g., with malloc).  Yes, this is a pain, but we don't know of any
   better solution that is portable.

   No attempt has been made to deal with multithreaded applications.  */

#include <config.h>

#include "c-stack.h"

#include <errno.h>
#include <inttypes.h>
#include <signal.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#if DEBUG
# include <stdio.h>
#endif

#include <sigsegv.h>

#include "exitfail.h"
#include "idx.h"
#include "ignore-value.h"

#include "gettext.h"
#define _(msgid) dgettext ("gnulib", msgid)

/* Here we need the original abort() function.  (Printing a stack trace
   from within a signal handler is not going to work in most cases anyway.)  */
#undef abort

#if HAVE_STACK_OVERFLOW_RECOVERY

/* Storage for the alternate signal stack.
   64 KiB is not too large for Gnulib-using apps, and is large enough
   for all known platforms.  Smaller sizes may run into trouble.
   For example, libsigsegv 2.6 through 2.8 have a bug where some
   architectures use more than the Linux default of an 8 KiB alternate
   stack when deciding if a fault was caused by stack overflow.  */
static max_align_t alternate_signal_stack[(64 * 1024
                                           + sizeof (max_align_t) - 1)
                                          / sizeof (max_align_t)];

/* The user-specified action to take when a SEGV-related program error
   or stack overflow occurs.  */
static _GL_ASYNC_SAFE void (* volatile segv_action) (int);

/* Translated messages for program errors and stack overflow.  Do not
   translate them in the signal handler, since gettext is not
   async-signal-safe.  */
static char const * volatile program_error_message;
static char const * volatile stack_overflow_message;

/* Output an error message, then exit with status EXIT_FAILURE if it
   appears to have been a stack overflow, or with a core dump
   otherwise.  This function is async-signal-safe.  */

static char const * volatile progname;

static _GL_ASYNC_SAFE _Noreturn void
die (int signo)
{
  segv_action (signo);
  char const *message = signo ? program_error_message : stack_overflow_message;

  /* If the message is short, write it all at once to avoid
     interleaving with other messages.  Avoid writev as it is not
     documented to be async-signal-safe.  */
  size_t prognamelen = strlen (progname);
  size_t messagelen = strlen (message);
  static char const separator[] = {':', ' '};
  char buf[sizeof alternate_signal_stack / 16 + sizeof separator];
  idx_t buflen;
  if (prognamelen + messagelen < sizeof buf - sizeof separator)
    {
      char *p = mempcpy (buf, progname, prognamelen);
      p = mempcpy (p, separator, sizeof separator);
      p = mempcpy (p, message, messagelen);
      *p++ = '\n';
      buflen = p - buf;
    }
  else
    {
      ignore_value (write (STDERR_FILENO, progname, prognamelen));
      ignore_value (write (STDERR_FILENO, separator, sizeof separator));
      ignore_value (write (STDERR_FILENO, message, messagelen));
      buf[0] = '\n';
      buflen = 1;
    }
  ignore_value (write (STDERR_FILENO, buf, buflen));

  if (! signo)
    _exit (exit_failure);
  raise (signo);
  abort ();
}

static _GL_ASYNC_SAFE void
null_action (_GL_UNUSED int signo)
{
}

/* Pacify GCC 9.3.1, which otherwise would complain about segv_handler.  */
# if _GL_GNUC_PREREQ (4, 6)
#  pragma GCC diagnostic ignored "-Wsuggest-attribute=pure"
# endif

/* Nonzero if general segv handler could not be installed.  */
static volatile int segv_handler_missing;

/* Handle a segmentation violation and exit if it cannot be stack
   overflow.  This function is async-signal-safe.  */

static _GL_ASYNC_SAFE int
segv_handler (_GL_UNUSED void *address, int serious)
{
# if DEBUG
  {
    char buf[1024];
    int saved_errno = errno;
    ignore_value (write (STDERR_FILENO, buf,
                         sprintf (buf, "segv_handler serious=%d\n", serious)));
    errno = saved_errno;
  }
# endif

  /* If this fault is not serious, return 0 to let the stack overflow
     handler take a shot at it.  */
  if (!serious)
    return 0;
  die (SIGSEGV);
}

/* Handle a segmentation violation that is likely to be a stack
   overflow and exit.  This function is async-signal-safe.  */

static _GL_ASYNC_SAFE _Noreturn void
overflow_handler (int emergency, _GL_UNUSED stackoverflow_context_t context)
{
# if DEBUG
  {
    char buf[1024];
    ignore_value (write (STDERR_FILENO, buf,
                         sprintf (buf, ("overflow_handler emergency=%d"
                                        " segv_handler_missing=%d\n"),
                                  emergency, segv_handler_missing)));
  }
# endif

  die ((!emergency || segv_handler_missing) ? 0 : SIGSEGV);
}

int
c_stack_action (_GL_ASYNC_SAFE void (*action) (int))
{
  segv_action = action ? action : null_action;
  program_error_message = _("program error");
  stack_overflow_message = _("stack overflow");
  progname = getprogname ();

  /* Always install the overflow handler.  */
  if (stackoverflow_install_handler (overflow_handler,
                                     alternate_signal_stack,
                                     sizeof alternate_signal_stack))
    {
      errno = ENOTSUP;
      return -1;
    }
  /* Try installing a general handler; if it fails, then treat all
     segv as stack overflow.  */
  segv_handler_missing = sigsegv_install_handler (segv_handler);
  return 0;
}

#else /* !HAVE_STACK_OVERFLOW_RECOVERY */

int
c_stack_action (_GL_ASYNC_SAFE void (*action) (_GL_UNUSED int) )
{
  errno = ENOTSUP;
  return -1;
}

#endif
