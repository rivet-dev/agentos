/* Binary mode I/O with checking
   Copyright 2017-2025 Free Software Foundation, Inc.

   This file is free software: you can redistribute it and/or modify
   it under the terms of the GNU Lesser General Public License as
   published by the Free Software Foundation, either version 3 of the
   License, or (at your option) any later version.

   This file is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU Lesser General Public License for more details.

   You should have received a copy of the GNU Lesser General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

#include <config.h>

#define XBINARY_IO_INLINE _GL_EXTERN_INLINE
#include "xbinary-io.h"

#include <errno.h>
#include <error.h>
#include "exitfail.h"
#include "verify.h"

#include "gettext.h"
#define _(msgid) dgettext ("gnulib", msgid)

#if O_BINARY

_Noreturn void
xset_binary_mode_error (void)
{
  error (exit_failure, errno,
         _("failed to set file descriptor text/binary mode"));
  assume (false);
}

#endif
