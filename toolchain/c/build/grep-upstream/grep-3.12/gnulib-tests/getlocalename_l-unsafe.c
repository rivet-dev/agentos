/* Return name of a single locale category.
   Copyright (C) 1995-2025 Free Software Foundation, Inc.

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

#include <config.h>

/* Specification.  */
#include "getlocalename_l-unsafe.h"

#include <locale.h>
#include <limits.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

#if LC_MESSAGES == 1729 || defined __ANDROID__
# include "setlocale-fixes.h"
#endif
#include "setlocale_null.h"

#if (__GLIBC__ >= 2 && !defined __UCLIBC__) || (defined __linux__ && HAVE_LANGINFO_H) || defined __CYGWIN__
# include <langinfo.h>
#endif
#if defined __sun
# if HAVE_SOLARIS114_LOCALES
#  include <sys/localedef.h>
# endif
#endif
#if HAVE_NAMELESS_LOCALES
# include "localename-table.h"
#endif
#if defined __HAIKU__
# include <dlfcn.h>
#endif


#if LOCALENAME_ENHANCE_LOCALE_FUNCS

# include "flexmember.h"
# include "glthread/lock.h"
# include "thread-optim.h"

/* Define a local struniq() function.  */
# include "struniq.h"

/* The 'locale_t' object does not contain the names of the locale categories.
   We have to associate them with the object through a hash table.
   The hash table is defined in localename-table.[hc].  */

/* Returns the name of a given locale category in a given locale_t object.  */
static struct string_with_storage
get_locale_t_name_unsafe (int category, locale_t locale)
{
  if (category == LC_ALL)
    /* Invalid argument.  */
    abort ();
  if (locale == LC_GLOBAL_LOCALE)
    {
      /* Query the global locale.  */
      const char *name = setlocale_null (category);
      if (name != NULL)
        return (struct string_with_storage) { name, STORAGE_GLOBAL };
      else
        /* Should normally not happen.  */
        return (struct string_with_storage) { "", STORAGE_INDEFINITE };
    }
  else
    {
# if HAVE_AIX72_LOCALES
      if (category == LC_MESSAGES)
        {
          const char *name = ((__locale_t) locale)->locale_name;
          if (name != NULL)
            return (struct string_with_storage) { name, STORAGE_OBJECT };
        }
# endif
      /* Look up the names in the hash table.  */
      size_t hashcode = locale_hash_function (locale);
      size_t slot = hashcode % LOCALE_HASH_TABLE_SIZE;
      /* If the locale was not found in the table, return "".  This can
         happen if the application uses the original newlocale()/duplocale()
         functions instead of the overridden ones.  */
      const char *name = "";
      struct locale_hash_node *p;
      /* Lock while looking up the hash node.  */
      gl_rwlock_rdlock (locale_lock);
      for (p = locale_hash_table[slot]; p != NULL; p = p->next)
        if (p->locale == locale)
          {
            name = p->names.category_name[category - LCMIN];
            break;
          }
      gl_rwlock_unlock (locale_lock);
      return (struct string_with_storage) { name, STORAGE_INDEFINITE };
    }
}

/* Returns the name of a given locale category in a given locale_t object,
   allocated as a string with indefinite extent.  */
static const char *
get_locale_t_name (int category, locale_t locale)
{
  struct string_with_storage ret = get_locale_t_name_unsafe (category, locale);
  return (ret.storage != STORAGE_INDEFINITE
          ? struniq (ret.value)
          : ret.value);
}

# if !(defined newlocale && defined duplocale && defined freelocale)
#  error "newlocale, duplocale, freelocale not being replaced as expected!"
# endif

/* newlocale() override.  */
locale_t
newlocale (int category_mask, const char *name, locale_t base)
#undef newlocale
{
  struct locale_categories_names names;
  struct locale_hash_node *node;
  locale_t result;

  /* Make sure name has indefinite extent.  */
  if (((LC_CTYPE_MASK | LC_NUMERIC_MASK | LC_TIME_MASK | LC_COLLATE_MASK
        | LC_MONETARY_MASK | LC_MESSAGES_MASK)
       & category_mask) != 0)
    name = struniq (name);

  /* Determine the category names of the result.  */
  if (((LC_CTYPE_MASK | LC_NUMERIC_MASK | LC_TIME_MASK | LC_COLLATE_MASK
        | LC_MONETARY_MASK | LC_MESSAGES_MASK)
       & ~category_mask) == 0)
    {
      /* Use name, ignore base.  */
      int i;

      name = struniq (name);
      for (i = 0; i < 6; i++)
        names.category_name[i] = name;
    }
  else
    {
      /* Use base, possibly also name.  */
      if (base == NULL)
        {
          int i;

          for (i = 0; i < 6; i++)
            {
              int category = i + LCMIN;
              int mask;

              switch (category)
                {
                case LC_CTYPE:
                  mask = LC_CTYPE_MASK;
                  break;
                case LC_NUMERIC:
                  mask = LC_NUMERIC_MASK;
                  break;
                case LC_TIME:
                  mask = LC_TIME_MASK;
                  break;
                case LC_COLLATE:
                  mask = LC_COLLATE_MASK;
                  break;
                case LC_MONETARY:
                  mask = LC_MONETARY_MASK;
                  break;
                case LC_MESSAGES:
                  mask = LC_MESSAGES_MASK;
                  break;
                default:
                  abort ();
                }
              names.category_name[i] =
                ((mask & category_mask) != 0 ? name : "C");
            }
        }
      else if (base == LC_GLOBAL_LOCALE)
        {
          int i;

          for (i = 0; i < 6; i++)
            {
              int category = i + LCMIN;
              int mask;

              switch (category)
                {
                case LC_CTYPE:
                  mask = LC_CTYPE_MASK;
                  break;
                case LC_NUMERIC:
                  mask = LC_NUMERIC_MASK;
                  break;
                case LC_TIME:
                  mask = LC_TIME_MASK;
                  break;
                case LC_COLLATE:
                  mask = LC_COLLATE_MASK;
                  break;
                case LC_MONETARY:
                  mask = LC_MONETARY_MASK;
                  break;
                case LC_MESSAGES:
                  mask = LC_MESSAGES_MASK;
                  break;
                default:
                  abort ();
                }
              names.category_name[i] =
                ((mask & category_mask) != 0
                 ? name
                 : get_locale_t_name (category, LC_GLOBAL_LOCALE));
            }
        }
      else
        {
          /* Look up the names of base in the hash table.  Like multiple calls
             of get_locale_t_name, but locking only once.  */
          struct locale_hash_node *p;

          /* Lock while looking up the hash node.  */
          gl_rwlock_rdlock (locale_lock);
          for (p = locale_hash_table[locale_hash_function (base) % LOCALE_HASH_TABLE_SIZE];
               p != NULL;
               p = p->next)
            if (p->locale == base)
              break;

          int i;
          for (i = 0; i < 6; i++)
            {
              int category = i + LCMIN;
              int mask;

              switch (category)
                {
                case LC_CTYPE:
                  mask = LC_CTYPE_MASK;
                  break;
                case LC_NUMERIC:
                  mask = LC_NUMERIC_MASK;
                  break;
                case LC_TIME:
                  mask = LC_TIME_MASK;
                  break;
                case LC_COLLATE:
                  mask = LC_COLLATE_MASK;
                  break;
                case LC_MONETARY:
                  mask = LC_MONETARY_MASK;
                  break;
                case LC_MESSAGES:
                  mask = LC_MESSAGES_MASK;
                  break;
                default:
                  abort ();
                }
              names.category_name[i] =
                ((mask & category_mask) != 0
                 ? name
                 : (p != NULL ? p->names.category_name[i] : ""));
            }

          gl_rwlock_unlock (locale_lock);
        }
    }

  node = (struct locale_hash_node *) malloc (sizeof (struct locale_hash_node));
  if (node == NULL)
    /* errno is set to ENOMEM.  */
    return NULL;

  result = newlocale (category_mask, name, base);
  if (result == NULL)
    {
      free (node);
      return NULL;
    }

  /* Fill the hash node.  */
  node->locale = result;
  node->names = names;

  /* Insert it in the hash table.  */
  {
    size_t hashcode = locale_hash_function (result);
    size_t slot = hashcode % LOCALE_HASH_TABLE_SIZE;
    struct locale_hash_node *p;

    /* Lock while inserting the new node.  */
    gl_rwlock_wrlock (locale_lock);
    for (p = locale_hash_table[slot]; p != NULL; p = p->next)
      if (p->locale == result)
        {
          /* This can happen if the application uses the original freelocale()
             function instead of the overridden one.  */
          p->names = node->names;
          break;
        }
    if (p == NULL)
      {
        node->next = locale_hash_table[slot];
        locale_hash_table[slot] = node;
      }

    gl_rwlock_unlock (locale_lock);

    if (p != NULL)
      free (node);
  }

  return result;
}

/* duplocale() override.  */
locale_t
duplocale (locale_t locale)
#undef duplocale
{
  struct locale_hash_node *node;
  locale_t result;

  if (locale == NULL)
    /* Invalid argument.  */
    abort ();

  node = (struct locale_hash_node *) malloc (sizeof (struct locale_hash_node));
  if (node == NULL)
    /* errno is set to ENOMEM.  */
    return NULL;

  result = duplocale (locale);
  if (result == NULL)
    {
      free (node);
      return NULL;
    }

  /* Fill the hash node.  */
  node->locale = result;
  if (locale == LC_GLOBAL_LOCALE)
    {
      int i;

      for (i = 0; i < 6; i++)
        {
          int category = i + LCMIN;
          node->names.category_name[i] =
            get_locale_t_name (category, LC_GLOBAL_LOCALE);
        }

      /* Lock before inserting the new node.  */
      gl_rwlock_wrlock (locale_lock);
    }
  else
    {
      struct locale_hash_node *p;

      /* Lock once, for the lookup and the insertion.  */
      gl_rwlock_wrlock (locale_lock);

      for (p = locale_hash_table[locale_hash_function (locale) % LOCALE_HASH_TABLE_SIZE];
           p != NULL;
           p = p->next)
        if (p->locale == locale)
          break;
      if (p != NULL)
        node->names = p->names;
      else
        {
          /* This can happen if the application uses the original
             newlocale()/duplocale() functions instead of the overridden
             ones.  */
          int i;

          for (i = 0; i < 6; i++)
            node->names.category_name[i] = "";
        }
    }

  /* Insert it in the hash table.  */
  {
    size_t hashcode = locale_hash_function (result);
    size_t slot = hashcode % LOCALE_HASH_TABLE_SIZE;
    struct locale_hash_node *p;

    for (p = locale_hash_table[slot]; p != NULL; p = p->next)
      if (p->locale == result)
        {
          /* This can happen if the application uses the original freelocale()
             function instead of the overridden one.  */
          p->names = node->names;
          break;
        }
    if (p == NULL)
      {
        node->next = locale_hash_table[slot];
        locale_hash_table[slot] = node;
      }

    gl_rwlock_unlock (locale_lock);

    if (p != NULL)
      free (node);
  }

  return result;
}

/* freelocale() override.  */
void
freelocale (locale_t locale)
#undef freelocale
{
  if (locale == NULL || locale == LC_GLOBAL_LOCALE)
    /* Invalid argument.  */
    abort ();

  {
    size_t hashcode = locale_hash_function (locale);
    size_t slot = hashcode % LOCALE_HASH_TABLE_SIZE;
    struct locale_hash_node *found;
    struct locale_hash_node **p;

    found = NULL;
    /* Lock while removing the hash node.  */
    gl_rwlock_wrlock (locale_lock);
    for (p = &locale_hash_table[slot]; *p != NULL; p = &(*p)->next)
      if ((*p)->locale == locale)
        {
          found = *p;
          *p = (*p)->next;
          break;
        }
    gl_rwlock_unlock (locale_lock);
    free (found);
  }

  freelocale (locale);
}

#endif


struct string_with_storage
getlocalename_l_unsafe (int category, locale_t locale)
{
  if (category == LC_ALL)
    /* Unsupported in this simple implementation.  */
    abort ();

  if (locale != LC_GLOBAL_LOCALE)
    {
#if GNULIB_defined_locale_t
      struct gl_locale_category_t *plc =
        &locale->category[gl_log2_lcmask_to_index (gl_log2_lc_mask (category))];
      return (struct string_with_storage) { plc->name, STORAGE_OBJECT };
#elif __GLIBC__ >= 2 && !defined __UCLIBC__
      /* Work around an incorrect definition of the _NL_LOCALE_NAME macro in
         glibc < 2.12.
         See <https://sourceware.org/bugzilla/show_bug.cgi?id=10968>.  */
      const char *name =
        nl_langinfo_l (_NL_ITEM ((category), _NL_ITEM_INDEX (-1)), locale);
      if (name[0] == '\0')
        /* Fallback code for glibc < 2.4, which did not implement
           nl_langinfo_l (_NL_LOCALE_NAME (category), locale).  */
        name = locale->__names[category];
      return (struct string_with_storage) { name, STORAGE_OBJECT };
#elif defined __linux__ && HAVE_LANGINFO_H && defined NL_LOCALE_NAME
      /* musl libc */
      const char *name = nl_langinfo_l (NL_LOCALE_NAME (category), locale);
      return (struct string_with_storage) { name, STORAGE_OBJECT };
#elif (defined __FreeBSD__ || defined __DragonFly__) || (defined __APPLE__ && defined __MACH__)
      /* FreeBSD >= 9.1, Mac OS X */
      int mask;

      switch (category)
        {
        case LC_CTYPE:
          mask = LC_CTYPE_MASK;
          break;
        case LC_NUMERIC:
          mask = LC_NUMERIC_MASK;
          break;
        case LC_TIME:
          mask = LC_TIME_MASK;
          break;
        case LC_COLLATE:
          mask = LC_COLLATE_MASK;
          break;
        case LC_MONETARY:
          mask = LC_MONETARY_MASK;
          break;
        case LC_MESSAGES:
          mask = LC_MESSAGES_MASK;
          break;
        default: /* We shouldn't get here.  */
          return (struct string_with_storage) { "", STORAGE_INDEFINITE };
        }
      const char *name = querylocale (mask, locale);
      return (struct string_with_storage) { name, STORAGE_OBJECT };
#elif defined __NetBSD__
      /* NetBSD >= 7.0 */
      #define _LOCALENAME_LEN_MAX 33
      struct _locale {
        void *cache;
        char query[_LOCALENAME_LEN_MAX * 6];
        const char *part_name[7];
      };
      const char *name = ((struct _locale *) locale)->part_name[category];
      return (struct string_with_storage) { name, STORAGE_OBJECT };
#elif defined __sun
# if HAVE_SOLARIS114_LOCALES
      /* Solaris >= 11.4.  */
      void *lcp = (*locale)->core.data->lcp;
      if (lcp != NULL)
        switch (category)
          {
          case LC_CTYPE:
          case LC_NUMERIC:
          case LC_TIME:
          case LC_COLLATE:
          case LC_MONETARY:
          case LC_MESSAGES:
            {
              const char *name = ((const char * const *) lcp)[category];
              return (struct string_with_storage) { name, STORAGE_OBJECT };
            }
          default: /* We shouldn't get here.  */
            return (struct string_with_storage) { "", STORAGE_INDEFINITE };
          }
      /* We shouldn't get here.  */
      return (struct string_with_storage) { "", STORAGE_INDEFINITE };
# else
      /* Solaris 11 OpenIndiana.
         For the internal structure of locale objects, see
         https://github.com/OpenIndiana/illumos-gate/blob/master/usr/src/lib/libc/port/locale/localeimpl.h  */
      switch (category)
        {
        case LC_CTYPE:
        case LC_NUMERIC:
        case LC_TIME:
        case LC_COLLATE:
        case LC_MONETARY:
        case LC_MESSAGES:
          {
            const char *name = ((const char * const *) locale)[category];
            return (struct string_with_storage) { name, STORAGE_OBJECT };
          }
        default: /* We shouldn't get here.  */
          return (struct string_with_storage) { "", STORAGE_INDEFINITE };
        }
# endif
#elif HAVE_NAMELESS_LOCALES
      /* OpenBSD >= 6.2, AIX >= 7.1 */
      return get_locale_t_name_unsafe (category, locale);
#elif defined __OpenBSD__ && HAVE_FAKE_LOCALES
      /* OpenBSD >= 6.2 has only fake locales.  */
      if (locale == (locale_t) 2)
        return (struct string_with_storage) { "C.UTF-8", STORAGE_INDEFINITE };
      return (struct string_with_storage) { "C", STORAGE_INDEFINITE };
#elif defined __CYGWIN__
      /* Cygwin >= 2.6.
         Cygwin <= 2.6.1 lacks NL_LOCALE_NAME, requiring peeking inside
         an opaque struct.  */
# ifdef NL_LOCALE_NAME
      const char *name = nl_langinfo_l (NL_LOCALE_NAME (category), locale);
# else
      /* FIXME: Remove when we can assume new-enough Cygwin.  */
      struct __locale_t {
        char categories[7][32];
      };
      const char *name = ((struct __locale_t *) locale)->categories[category];
# endif
      return (struct string_with_storage) { name, STORAGE_OBJECT };
#elif defined __HAIKU__
      /* Since 2022, Haiku has per-thread locales.  locale_t is 'void *',
         but in fact a 'LocaleBackendData *'.  */
      struct LocaleBackendData {
        int magic;
        void /*BPrivate::Libroot::LocaleBackend*/ *backend;
        void /*BPrivate::Libroot::LocaleDataBridge*/ *databridge;
      };
      void *locale_backend =
        ((struct LocaleBackendData *) locale)->backend;
      if (locale_backend != NULL)
        {
          /* The only existing concrete subclass of
             BPrivate::Libroot::LocaleBackend is
             BPrivate::Libroot::ICULocaleBackend.
             Invoke the (non-virtual) method
             BPrivate::Libroot::ICULocaleBackend::_QueryLocale on it.
             This method is located in a separate shared library,
             libroot-addon-icu.so.  */
          static void * volatile querylocale_method /* = NULL */;
          static int volatile querylocale_found /* = 0 */;
          /* Attempt to open this shared library, the first time we get
             here.  */
          if (querylocale_found == 0)
            {
              void *handle =
                dlopen ("/boot/system/lib/libroot-addon-icu.so", 0);
              if (handle != NULL)
                {
                  void *sym =
                    dlsym (handle, "_ZN8BPrivate7Libroot16ICULocaleBackend12_QueryLocaleEi");
                  if (sym != NULL)
                    {
                      querylocale_method = sym;
                      querylocale_found = 1;
                    }
                  else
                    /* Could not find the symbol.  */
                    querylocale_found = -1;
                }
              else
                /* Could not open the separate shared library.  */
                querylocale_found = -1;
            }
          if (querylocale_found > 0)
            {
              /* The _QueryLocale method is a non-static C++ method with
                 parameters (int category) and return type 'const char *'.
                 See
                   haiku/headers/private/libroot/locale/ICULocaleBackend.h
                   haiku/src/system/libroot/add-ons/icu/ICULocaleBackend.cpp
                 This is the same as a C function with parameters
                   (BPrivate::Libroot::LocaleBackend* this, int category)
                 and return type 'const char *'.  Invoke it.  */
              const char * (*querylocale_func) (void *, int) =
                (const char * (*) (void *, int)) querylocale_method;
              const char *name = querylocale_func (locale_backend, category);
              return (struct string_with_storage) { name, STORAGE_OBJECT };
            }
        }
      else
        /* It's the "C" or "POSIX" locale.  */
        return (struct string_with_storage) { "C", STORAGE_INDEFINITE };
#elif defined __ANDROID__
      /* Android API level >= 21 */
      struct __locale_t {
        size_t mb_cur_max;
      };
      const char *name = ((struct __locale_t *) locale)->mb_cur_max == 4 ? "C.UTF-8" : "C";
      return (struct string_with_storage) { name, STORAGE_INDEFINITE };
#else
 #error "Please port gnulib getlocalename_l-unsafe.c to your platform! Report this to bug-gnulib."
#endif
    }
  else
    {
      /* Query the global locale.  */
      const char *name;
#if LC_MESSAGES == 1729
      if (category == LC_MESSAGES)
        name = setlocale_messages_null ();
      else
#endif
#if defined __ANDROID__
        name = setlocale_fixed_null (category);
#else
        name = setlocale_null (category);
#endif
      if (name != NULL)
        return (struct string_with_storage) { name, STORAGE_GLOBAL };
      else
        /* Should normally not happen.  */
        return (struct string_with_storage) { "", STORAGE_INDEFINITE };
    }
}
