/* Auxiliary program to test a DFA code path that cannot be triggered
   by grep or gawk.
   Copyright 2014-2025 Free Software Foundation, Inc.

   This program is free software; you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published by
   the Free Software Foundation, either version 3, or (at your option)
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
#include <string.h>
#include <unistd.h>
#include <regex.h>
#include <dfa.h>
#include <localeinfo.h>

#include "binary-io.h"

_Noreturn void
dfaerror (char const *mesg)
{
  printf ("dfaerror: %s\n", mesg);
  exit (EXIT_FAILURE);
}

static int exit_status = EXIT_SUCCESS;

void
dfawarn (char const *mesg)
{
  printf ("dfawarn: %s\n", mesg);
  exit_status = EXIT_FAILURE;
}

int
main (int argc, char **argv)
{
  struct dfa *dfa;
  char *beg, *end, *p;
  int allow_nl;
  struct localeinfo localeinfo;

  if (argc < 3)
    exit (EXIT_FAILURE);

  /* This test's fixture needs to compare this program's output with an expected
     output.  On native Windows, the CR-LF newlines would cause this comparison
     to fail.  But we don't want to postprocess this program's output.  */
  set_binary_mode (STDOUT_FILENO, O_BINARY);

  setlocale (LC_ALL, "");
  init_localeinfo (&localeinfo);

  dfa = dfaalloc ();
  dfasyntax (dfa, &localeinfo, RE_SYNTAX_EGREP | RE_NO_EMPTY_RANGES, 0);
  dfacomp (argv[1], strlen (argv[1]), dfa, 0);

  beg = argv[2];
  end = argv[2] + strlen (argv[2]);
  allow_nl = argc > 3 && atoi (argv[3]);

  p = dfaexec (dfa, beg, end, allow_nl, NULL, NULL);

  if (p != NULL)
    printf ("%zd\n", p - beg);

  exit (exit_status);
}
