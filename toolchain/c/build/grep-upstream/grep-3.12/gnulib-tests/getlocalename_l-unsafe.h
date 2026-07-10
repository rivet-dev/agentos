/* Return name of a single locale category.
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

#ifndef _GETLOCALENAME_L_UNSAFE_H
#define _GETLOCALENAME_L_UNSAFE_H

#include <locale.h>

#include "arg-nonnull.h"

#ifdef __cplusplus
extern "C" {
#endif


/* This enumeration describes the storage of the return value.  */
enum storage
{
  /* Storage with indefinite extent.  */
  STORAGE_INDEFINITE,
  /* Storage in a thread-independent location, valid until the next setlocale()
     call.  */
  STORAGE_GLOBAL,
  /* Storage in a thread-local buffer, valid until the next
     getlocalename_l_unlocked() call in the same thread.  */
  STORAGE_THREAD,
  /* Storage in or attached to the locale_t object, valid until the next call
     to newlocale() with that object as base or until the next freelocale() call
     for that object.  */
  STORAGE_OBJECT
};

/* Return type of getlocalename_l_unlocked.  */
struct string_with_storage
{
  const char *value;
  enum storage storage;
};

/* Returns the name of the locale category CATEGORY in the given LOCALE object
   or, if LOCALE is LC_GLOBAL_LOCALE, in the global locale.
   CATEGORY must be != LC_ALL.  */
extern struct string_with_storage
       getlocalename_l_unsafe (int category, locale_t locale)
                              _GL_ARG_NONNULL ((2));


#ifdef __cplusplus
}
#endif

#endif /* _GETLOCALENAME_L_UNSAFE_H */
