/* Auxiliary program to detect support for a locale.
   Copyright 2010-2025 Free Software Foundation, Inc.

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

#include <config.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>

int
main (int argc, char **argv)
{
  if (1 < argc && setlocale (LC_ALL, argv[1]))
    {
      printf ("%d\n", (int) MB_CUR_MAX);
      exit (EXIT_SUCCESS);
    }

  exit (EXIT_FAILURE);
}
