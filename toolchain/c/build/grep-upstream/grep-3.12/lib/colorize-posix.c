/* Output colorization.
   Copyright 2011-2025 Free Software Foundation, Inc.

   This program is free software; you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published by
   the Free Software Foundation; either version 3, or (at your option)
   any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

/* Without this pragma, gcc 4.7.0 20120102 suggests that the
   init_colorize function might be candidate for attribute 'const'  */
#if (__GNUC__ == 4 && 6 <= __GNUC_MINOR__) || 4 < __GNUC__
# pragma GCC diagnostic ignored "-Wsuggest-attribute=const"
#endif

#include <config.h>

#include "colorize.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* Return non-zero if we should highlight matches in output to file
   descriptor FD.  */
int
should_colorize (void)
{
  char const *t = getenv ("TERM");
  return t && strcmp (t, "dumb") != 0;
}

void init_colorize (void) { }

/* Start a colorized text attribute on stdout using the SGR_START
   format; the attribute is specified by SGR_SEQ.  */
void
print_start_colorize (char const *sgr_start, char const *sgr_seq)
{
  printf (sgr_start, sgr_seq);
}

/* Restore the normal text attribute using the SGR_END string.  */
void
print_end_colorize (char const *sgr_end)
{
  fputs (sgr_end, stdout);
}
