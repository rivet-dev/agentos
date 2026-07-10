/* Make the global locale minimally POSIX compliant.
   Copyright (C) 2025 Free Software Foundation, Inc.

   This file is free software: you can redistribute it and/or modify
   it under the terms of the GNU Lesser General Public License as
   published by the Free Software Foundation; either version 2.1 of the
   License, or (at your option) any later version.

   This file is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU Lesser General Public License for more details.

   You should have received a copy of the GNU Lesser General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

/* Written by Bruno Haible <bruno@clisp.org>, 2025.  */

#ifndef _SETLOCALE_FIXES_H
#define _SETLOCALE_FIXES_H

#ifdef __cplusplus
extern "C" {
#endif


#if defined _WIN32 && !defined __CYGWIN__

/* This section is only relevant on platforms that lack LC_MESSAGES, namely
   native Windows.  */

/* setlocale_messages (NAME) is like setlocale (LC_MESSAGES, NAME).  */
extern const char *setlocale_messages (const char *name);

/* setlocale_messages_null () is like setlocale (LC_MESSAGES, NULL), except that
   it is guaranteed to be multithread-safe.  */
extern const char *setlocale_messages_null (void);

#endif


#if defined __ANDROID__

/* This section is only relevant on Android.
   While OpenBSD ≥ 6.2 and Android API level >= 21 have a fake locale_t type,
   regarding the global locale, OpenBSD at least stores the name of each
   category of the global locale.  Android doesn't do this, and additionally
   has a bug: setting any category != LC_CTYPE of the global locale affects
   the LC_CTYPE category as well.  */

/* Like setlocale (CATEGORY, NAME), but fixes these two Android bugs.  */
extern const char *setlocale_fixed (int category, const char *name);

/* Like setlocale_null (CATEGORY), but consistent with setlocale_fixed.  */
extern const char *setlocale_fixed_null (int category);

#endif


#ifdef __cplusplus
}
#endif

#endif /* _SETLOCALE_FIXES_H */
