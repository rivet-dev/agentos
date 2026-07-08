/* Output colorization on MS-Windows.
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

/* Written by Eli Zaretskii.  */

#include <config.h>

#include "colorize.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#undef DATADIR	/* conflicts with objidl.h, which is included by windows.h */
#include <windows.h>

static HANDLE hstdout = INVALID_HANDLE_VALUE;
static SHORT norm_attr;

/* Initialize the normal text attribute used by the console.  */
void
init_colorize (void)
{
  CONSOLE_SCREEN_BUFFER_INFO csbi;

  hstdout = GetStdHandle (STD_OUTPUT_HANDLE);
  if (hstdout != INVALID_HANDLE_VALUE
      && GetConsoleScreenBufferInfo (hstdout, &csbi))
     norm_attr = csbi.wAttributes;
  else
    hstdout = INVALID_HANDLE_VALUE;
}

/* Return non-zero if we should highlight matches in output.  */
int
should_colorize (void)
{
  /* $TERM is not normally defined on DOS/Windows, so don't require
     it for highlighting.  But some programs, like Emacs, do define
     it when running Grep as a subprocess, so make sure they don't
     set TERM=dumb.  */
  char const *t = getenv ("TERM");
  return ! (t && strcmp (t, "dumb") == 0);
}

/* Convert a color spec, a semi-colon separated list of the form
   "NN;MM;KK;...", where each number is a value of the SGR parameter,
   into the corresponding Windows console text attribute.

   This function supports a subset of the SGR rendition aspects that
   the Windows console can display.  */
static int
w32_sgr2attr (const char *sgr_seq)
{
  const char *s, *p;
  int code, fg = norm_attr & 15, bg = norm_attr & (15 << 4);
  int bright = 0, inverse = 0;
  static const int fg_color[] = {
    0,			/* black */
    FOREGROUND_RED,	/* red */
    FOREGROUND_GREEN,	/* green */
    FOREGROUND_GREEN | FOREGROUND_RED, /* yellow */
    FOREGROUND_BLUE,		       /* blue */
    FOREGROUND_BLUE | FOREGROUND_RED,  /* magenta */
    FOREGROUND_BLUE | FOREGROUND_GREEN, /* cyan */
    FOREGROUND_RED | FOREGROUND_GREEN | FOREGROUND_BLUE /* gray */
  };
  static const int bg_color[] = {
    0,			/* black */
    BACKGROUND_RED,	/* red */
    BACKGROUND_GREEN,	/* green */
    BACKGROUND_GREEN | BACKGROUND_RED, /* yellow */
    BACKGROUND_BLUE,		       /* blue */
    BACKGROUND_BLUE | BACKGROUND_RED,  /* magenta */
    BACKGROUND_BLUE | BACKGROUND_GREEN, /* cyan */
    BACKGROUND_RED | BACKGROUND_GREEN | BACKGROUND_BLUE /* gray */
  };

  for (s = p = sgr_seq; *s; p++)
    {
      if (*p == ';' || *p == '\0')
        {
          code = strtol (s, nullptr, 10);
          s = p + (*p != '\0');

          switch (code)
            {
            case 0:	/* all attributes off */
              fg = norm_attr & 15;
              bg = norm_attr & (15 << 4);
              bright = 0;
              inverse = 0;
              break;
            case 1:	/* intensity on */
              bright = 1;
              break;
            case 7:	/* inverse video */
              inverse = 1;
              break;
            case 22:	/* intensity off */
              bright = 0;
              break;
            case 27:	/* inverse off */
              inverse = 0;
              break;
            case 30: case 31: case 32: case 33: /* foreground color */
            case 34: case 35: case 36: case 37:
              fg = fg_color[code - 30];
              break;
            case 39:	/* default foreground */
              fg = norm_attr & 15;
              break;
            case 40: case 41: case 42: case 43: /* background color */
            case 44: case 45: case 46: case 47:
              bg = bg_color[code - 40];
              break;
            case 49:	/* default background */
              bg = norm_attr & (15 << 4);
              break;
            default:
              break;
            }
        }
    }
  if (inverse)
    {
      int t = fg;
      fg = (bg >> 4);
      bg = (t << 4);
    }
  if (bright)
    fg |= FOREGROUND_INTENSITY;

  return (bg & (15 << 4)) | (fg & 15);
}

/* Start a colorized text attribute on stdout using the SGR_START
   format; the attribute is specified by SGR_SEQ.  */
void
print_start_colorize (char const *sgr_start, char const *sgr_seq)
{
  /* If stdout is connected to a console, set the console text
     attribute directly instead of using SGR_START.  Otherwise, use
     SGR_START to emit the SGR escape sequence as on Posix platforms;
     this is needed when Grep is invoked as a subprocess of another
     program, such as Emacs, which will handle the display of the
     matches.  */
  if (hstdout != INVALID_HANDLE_VALUE)
    {
      SHORT attr = w32_sgr2attr (sgr_seq);
      SetConsoleTextAttribute (hstdout, attr);
    }
  else
    printf (sgr_start, sgr_seq);
}

/* Clear to the end of the current line with the default attribute.
   This is needed for reasons similar to those that require the "EL to
   Right after SGR" operation on Posix platforms: if we don't do this,
   setting the 'mt', 'ms', or 'mc' capabilities to use a non-default
   background color spills that color to the empty space at the end of
   the last screen line in a match whose line spans multiple screen
   lines.  */
static void
w32_clreol (void)
{
  DWORD nchars;
  COORD start_pos;
  DWORD written;
  CONSOLE_SCREEN_BUFFER_INFO csbi;

  GetConsoleScreenBufferInfo (hstdout, &csbi);
  start_pos = csbi.dwCursorPosition;
  nchars = csbi.dwSize.X - start_pos.X;

  FillConsoleOutputAttribute (hstdout, norm_attr, nchars, start_pos,
                              &written);
  FillConsoleOutputCharacter (hstdout, ' ', nchars, start_pos, &written);
}

/* Restore the normal text attribute using the SGR_END string.  */
void
print_end_colorize (char const *sgr_end)
{
  if (hstdout != INVALID_HANDLE_VALUE)
    {
      SetConsoleTextAttribute (hstdout, norm_attr);
      w32_clreol ();
    }
  else
    fputs (sgr_end, stdout);
}
