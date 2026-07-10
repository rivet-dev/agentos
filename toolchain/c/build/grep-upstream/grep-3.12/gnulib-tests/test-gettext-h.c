/* Test of gettext.h convenience header.
   Copyright (C) 2024-2025 Free Software Foundation, Inc.

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

/* Written by Bruno Haible <bruno@clisp.org>, 2024.  */

#include <config.h>

#include "gettext.h"

#include <string.h>

int
main (void)
{
  const char *s;

  textdomain ("tzlof");

  s = gettext ("some text");
  if (strcmp (s, "some text") != 0)
    return 1;

  s = pgettext ("menu", "some other text");
  if (strcmp (s, "some other text") != 0)
    return 1;

  return 0;
}
