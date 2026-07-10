/* Error reporting interface for xstrto* functions.

   Copyright (C) 1995-1996, 1998-1999, 2001-2004, 2006-2025 Free Software
   Foundation, Inc.

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

#ifndef XSTRTOL_ERROR_H_
# define XSTRTOL_ERROR_H_ 1

/* This file uses _Noreturn.  */
# if !_GL_CONFIG_H_INCLUDED
#  error "Please include config.h first."
# endif

# include "xstrtol.h"

# include <getopt.h>

/* Report an error for an invalid integer in an option argument.

   ERR is the error code returned by one of the xstrto* functions.

   Use OPT_IDX to decide whether to print the short option string "C"
   or "-C" or a long option string derived from LONG_OPTIONS.  OPT_IDX
   is -2 if the short option "C" was used, without any leading "-"; it
   is -1 if the short option "-C" was used; otherwise it is an index
   into LONG_OPTIONS, which should have a name preceded by two '-'
   characters.

   ARG is the option-argument containing the integer.

   After reporting an error, exit with a failure status.  */

_Noreturn void xstrtol_fatal (enum strtol_error /* err */,
                              int /* opt_idx */, char /* c */,
                              struct option const * /* long_options */,
                              char const * /* arg */);

#endif /* not XSTRTOL_ERROR_H_ */
