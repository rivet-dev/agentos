/* DO NOT EDIT! GENERATED AUTOMATICALLY! */
/* Implement the most essential subset of POSIX pthread.h.

   Copyright (C) 2009-2025 Free Software Foundation, Inc.

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

/* Written by Paul Eggert, Glen Lenker, and Bruno Haible.  */

#if __GNUC__ >= 3
#pragma GCC system_header
#endif


#if defined _GL_ALREADY_INCLUDING_PTHREAD_H
/* Special invocation convention:
   On Android, we have a sequence of nested includes
   <pthread.h> -> <time.h> -> <sys/time.h> -> <sys/select.h> ->
   <signal.h> -> <pthread.h>.
   In this situation, PTHREAD_COND_INITIALIZER is not yet defined,
   therefore we should not attempt to define PTHREAD_MUTEX_NORMAL etc.  */

#include_next <pthread.h>

#else
/* Normal invocation convention.  */

#ifndef _GL_PTHREAD_H_

#if 1

# define _GL_ALREADY_INCLUDING_PTHREAD_H

/* The include_next requires a split double-inclusion guard.  */
# include_next <pthread.h>

# undef _GL_ALREADY_INCLUDING_PTHREAD_H

#endif

#ifndef _GL_PTHREAD_H_
#define _GL_PTHREAD_H_

/* This file uses _Noreturn, _GL_ATTRIBUTE_PURE, GNULIB_POSIXCHECK,
   HAVE_RAW_DECL_*.  */
#if !_GL_CONFIG_H_INCLUDED
 #error "Please include config.h first."
#endif

#define __need_system_stdlib_h
#include <stdlib.h>
#undef __need_system_stdlib_h


/* The pthreads-win32 <pthread.h> defines a couple of broken macros.  */
#undef asctime_r
#undef ctime_r
#undef gmtime_r
#undef localtime_r
#undef rand_r
#undef strtok_r

#include <errno.h>
#include <sched.h>
#include <sys/types.h>
#include <time.h>

/* The __attribute__ feature is available in gcc versions 2.5 and later.
   The attribute __pure__ was added in gcc 2.96.  */
#ifndef _GL_ATTRIBUTE_PURE
# if __GNUC__ > 2 || (__GNUC__ == 2 && __GNUC_MINOR__ >= 96) || defined __clang__
#  define _GL_ATTRIBUTE_PURE __attribute__ ((__pure__))
# else
#  define _GL_ATTRIBUTE_PURE /* empty */
# endif
#endif

/* The definitions of _GL_FUNCDECL_RPL etc. are copied here.  */
/* C++ compatible function declaration macros.
   Copyright (C) 2010-2025 Free Software Foundation, Inc.

   This program is free software: you can redistribute it and/or modify it
   under the terms of the GNU Lesser General Public License as published
   by the Free Software Foundation; either version 2 of the License, or
   (at your option) any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
   Lesser General Public License for more details.

   You should have received a copy of the GNU Lesser General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

#ifndef _GL_CXXDEFS_H
#define _GL_CXXDEFS_H

/* Begin/end the GNULIB_NAMESPACE namespace.  */
#if defined __cplusplus && defined GNULIB_NAMESPACE
# define _GL_BEGIN_NAMESPACE namespace GNULIB_NAMESPACE {
# define _GL_END_NAMESPACE }
#else
# define _GL_BEGIN_NAMESPACE
# define _GL_END_NAMESPACE
#endif

/* The three most frequent use cases of these macros are:

   * For providing a substitute for a function that is missing on some
     platforms, but is declared and works fine on the platforms on which
     it exists:

       #if @GNULIB_FOO@
       # if !@HAVE_FOO@
       _GL_FUNCDECL_SYS (foo, ...);
       # endif
       _GL_CXXALIAS_SYS (foo, ...);
       _GL_CXXALIASWARN (foo);
       #elif defined GNULIB_POSIXCHECK
       ...
       #endif

   * For providing a replacement for a function that exists on all platforms,
     but is broken/insufficient and needs to be replaced on some platforms:

       #if @GNULIB_FOO@
       # if @REPLACE_FOO@
       #  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
       #   undef foo
       #   define foo rpl_foo
       #  endif
       _GL_FUNCDECL_RPL (foo, ...);
       _GL_CXXALIAS_RPL (foo, ...);
       # else
       _GL_CXXALIAS_SYS (foo, ...);
       # endif
       _GL_CXXALIASWARN (foo);
       #elif defined GNULIB_POSIXCHECK
       ...
       #endif

   * For providing a replacement for a function that exists on some platforms
     but is broken/insufficient and needs to be replaced on some of them and
     is additionally either missing or undeclared on some other platforms:

       #if @GNULIB_FOO@
       # if @REPLACE_FOO@
       #  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
       #   undef foo
       #   define foo rpl_foo
       #  endif
       _GL_FUNCDECL_RPL (foo, ...);
       _GL_CXXALIAS_RPL (foo, ...);
       # else
       #  if !@HAVE_FOO@   or   if !@HAVE_DECL_FOO@
       _GL_FUNCDECL_SYS (foo, ...);
       #  endif
       _GL_CXXALIAS_SYS (foo, ...);
       # endif
       _GL_CXXALIASWARN (foo);
       #elif defined GNULIB_POSIXCHECK
       ...
       #endif
*/

/* _GL_EXTERN_C declaration;
   performs the declaration with C linkage.  */
#if defined __cplusplus
# define _GL_EXTERN_C extern "C"
#else
# define _GL_EXTERN_C extern
#endif

/* _GL_EXTERN_C_FUNC declaration;
   performs the declaration of a function with C linkage.  */
#if defined __cplusplus
# define _GL_EXTERN_C_FUNC extern "C"
#else
/* In C mode, omit the 'extern' keyword, because attributes in bracket syntax
   are not allowed between 'extern' and the return type (see gnulib-common.m4).
 */
# define _GL_EXTERN_C_FUNC
#endif

/* _GL_FUNCDECL_RPL (func, rettype, parameters, [attributes]);
   declares a replacement function, named rpl_func, with the given prototype,
   consisting of return type, parameters, and attributes.
   Although attributes are optional, the comma before them is required
   for portability to C17 and earlier.  The attribute _GL_ATTRIBUTE_NOTHROW,
   if needed, must be placed after the _GL_FUNCDECL_RPL invocation,
   at the end of the declaration.
   Examples:
     _GL_FUNCDECL_RPL (free, void, (void *ptr), ) _GL_ATTRIBUTE_NOTHROW;
     _GL_FUNCDECL_RPL (open, int, (const char *filename, int flags, ...),
                                  _GL_ARG_NONNULL ((1)));

   Note: Attributes, such as _GL_ATTRIBUTE_DEPRECATED, are supported in front
   of a _GL_FUNCDECL_RPL invocation only in C mode, not in C++ mode.  (That's
   because
     [[...]] extern "C" <declaration>;
   is invalid syntax in C++.)
 */
#define _GL_FUNCDECL_RPL(func,rettype,parameters,...) \
  _GL_FUNCDECL_RPL_1 (rpl_##func, rettype, parameters, __VA_ARGS__)
#define _GL_FUNCDECL_RPL_1(rpl_func,rettype,parameters,...) \
  _GL_EXTERN_C_FUNC __VA_ARGS__ rettype rpl_func parameters

/* _GL_FUNCDECL_SYS (func, rettype, parameters, [attributes]);
   declares the system function, named func, with the given prototype,
   consisting of return type, parameters, and attributes.
   Although attributes are optional, the comma before them is required
   for portability to C17 and earlier.  The attribute _GL_ATTRIBUTE_NOTHROW,
   if needed, must be placed after the _GL_FUNCDECL_RPL invocation,
   at the end of the declaration.
   Examples:
     _GL_FUNCDECL_SYS (getumask, mode_t, (void), ) _GL_ATTRIBUTE_NOTHROW;
     _GL_FUNCDECL_SYS (posix_openpt, int, (int flags), _GL_ATTRIBUTE_NODISCARD);
 */
#define _GL_FUNCDECL_SYS(func,rettype,parameters,...) \
  _GL_EXTERN_C_FUNC __VA_ARGS__ rettype func parameters

/* _GL_CXXALIAS_RPL (func, rettype, parameters);
   declares a C++ alias called GNULIB_NAMESPACE::func
   that redirects to rpl_func, if GNULIB_NAMESPACE is defined.
   Example:
     _GL_CXXALIAS_RPL (open, int, (const char *filename, int flags, ...));

   Wrapping rpl_func in an object with an inline conversion operator
   avoids a reference to rpl_func unless GNULIB_NAMESPACE::func is
   actually used in the program.  */
#define _GL_CXXALIAS_RPL(func,rettype,parameters) \
  _GL_CXXALIAS_RPL_1 (func, rpl_##func, rettype, parameters)
#if defined __cplusplus && defined GNULIB_NAMESPACE
# define _GL_CXXALIAS_RPL_1(func,rpl_func,rettype,parameters) \
    namespace GNULIB_NAMESPACE                                \
    {                                                         \
      static const struct _gl_ ## func ## _wrapper            \
      {                                                       \
        typedef rettype (*type) parameters;                   \
                                                              \
        inline operator type () const                         \
        {                                                     \
          return ::rpl_func;                                  \
        }                                                     \
      } func = {};                                            \
    }                                                         \
    _GL_EXTERN_C int _gl_cxxalias_dummy
#else
# define _GL_CXXALIAS_RPL_1(func,rpl_func,rettype,parameters) \
    _GL_EXTERN_C int _gl_cxxalias_dummy
#endif

/* _GL_CXXALIAS_MDA (func, rettype, parameters);
   is to be used when func is a Microsoft deprecated alias, on native Windows.
   It declares a C++ alias called GNULIB_NAMESPACE::func
   that redirects to _func, if GNULIB_NAMESPACE is defined.
   Example:
     _GL_CXXALIAS_MDA (open, int, (const char *filename, int flags, ...));
 */
#define _GL_CXXALIAS_MDA(func,rettype,parameters) \
  _GL_CXXALIAS_RPL_1 (func, _##func, rettype, parameters)

/* _GL_CXXALIAS_RPL_CAST_1 (func, rpl_func, rettype, parameters);
   is like  _GL_CXXALIAS_RPL_1 (func, rpl_func, rettype, parameters);
   except that the C function rpl_func may have a slightly different
   declaration.  A cast is used to silence the "invalid conversion" error
   that would otherwise occur.  */
#if defined __cplusplus && defined GNULIB_NAMESPACE
# define _GL_CXXALIAS_RPL_CAST_1(func,rpl_func,rettype,parameters) \
    namespace GNULIB_NAMESPACE                                     \
    {                                                              \
      static const struct _gl_ ## func ## _wrapper                 \
      {                                                            \
        typedef rettype (*type) parameters;                        \
                                                                   \
        inline operator type () const                              \
        {                                                          \
          return reinterpret_cast<type>(::rpl_func);               \
        }                                                          \
      } func = {};                                                 \
    }                                                              \
    _GL_EXTERN_C int _gl_cxxalias_dummy
#else
# define _GL_CXXALIAS_RPL_CAST_1(func,rpl_func,rettype,parameters) \
    _GL_EXTERN_C int _gl_cxxalias_dummy
#endif

/* _GL_CXXALIAS_MDA_CAST (func, rettype, parameters);
   is like  _GL_CXXALIAS_MDA (func, rettype, parameters);
   except that the C function func may have a slightly different declaration.
   A cast is used to silence the "invalid conversion" error that would
   otherwise occur.  */
#define _GL_CXXALIAS_MDA_CAST(func,rettype,parameters) \
  _GL_CXXALIAS_RPL_CAST_1 (func, _##func, rettype, parameters)

/* _GL_CXXALIAS_SYS (func, rettype, parameters);
   declares a C++ alias called GNULIB_NAMESPACE::func
   that redirects to the system provided function func, if GNULIB_NAMESPACE
   is defined.
   Example:
     _GL_CXXALIAS_SYS (open, int, (const char *filename, int flags, ...));

   Wrapping func in an object with an inline conversion operator
   avoids a reference to func unless GNULIB_NAMESPACE::func is
   actually used in the program.  */
#if defined __cplusplus && defined GNULIB_NAMESPACE
# define _GL_CXXALIAS_SYS(func,rettype,parameters)            \
    namespace GNULIB_NAMESPACE                                \
    {                                                         \
      static const struct _gl_ ## func ## _wrapper            \
      {                                                       \
        typedef rettype (*type) parameters;                   \
                                                              \
        inline operator type () const                         \
        {                                                     \
          return ::func;                                      \
        }                                                     \
      } func = {};                                            \
    }                                                         \
    _GL_EXTERN_C int _gl_cxxalias_dummy
#else
# define _GL_CXXALIAS_SYS(func,rettype,parameters) \
    _GL_EXTERN_C int _gl_cxxalias_dummy
#endif

/* _GL_CXXALIAS_SYS_CAST (func, rettype, parameters);
   is like  _GL_CXXALIAS_SYS (func, rettype, parameters);
   except that the C function func may have a slightly different declaration.
   A cast is used to silence the "invalid conversion" error that would
   otherwise occur.  */
#if defined __cplusplus && defined GNULIB_NAMESPACE
# define _GL_CXXALIAS_SYS_CAST(func,rettype,parameters) \
    namespace GNULIB_NAMESPACE                          \
    {                                                   \
      static const struct _gl_ ## func ## _wrapper      \
      {                                                 \
        typedef rettype (*type) parameters;             \
                                                        \
        inline operator type () const                   \
        {                                               \
          return reinterpret_cast<type>(::func);        \
        }                                               \
      } func = {};                                      \
    }                                                   \
    _GL_EXTERN_C int _gl_cxxalias_dummy
#else
# define _GL_CXXALIAS_SYS_CAST(func,rettype,parameters) \
    _GL_EXTERN_C int _gl_cxxalias_dummy
#endif

/* _GL_CXXALIAS_SYS_CAST2 (func, rettype, parameters, rettype2, parameters2);
   is like  _GL_CXXALIAS_SYS (func, rettype, parameters);
   except that the C function is picked among a set of overloaded functions,
   namely the one with rettype2 and parameters2.  Two consecutive casts
   are used to silence the "cannot find a match" and "invalid conversion"
   errors that would otherwise occur.  */
#if defined __cplusplus && defined GNULIB_NAMESPACE
  /* The outer cast must be a reinterpret_cast.
     The inner cast: When the function is defined as a set of overloaded
     functions, it works as a static_cast<>, choosing the designated variant.
     When the function is defined as a single variant, it works as a
     reinterpret_cast<>. The parenthesized cast syntax works both ways.  */
# define _GL_CXXALIAS_SYS_CAST2(func,rettype,parameters,rettype2,parameters2) \
    namespace GNULIB_NAMESPACE                                                \
    {                                                                         \
      static const struct _gl_ ## func ## _wrapper                            \
      {                                                                       \
        typedef rettype (*type) parameters;                                   \
                                                                              \
        inline operator type () const                                         \
        {                                                                     \
          return reinterpret_cast<type>((rettype2 (*) parameters2)(::func));  \
        }                                                                     \
      } func = {};                                                            \
    }                                                                         \
    _GL_EXTERN_C int _gl_cxxalias_dummy
#else
# define _GL_CXXALIAS_SYS_CAST2(func,rettype,parameters,rettype2,parameters2) \
    _GL_EXTERN_C int _gl_cxxalias_dummy
#endif

/* _GL_CXXALIASWARN (func);
   causes a warning to be emitted when ::func is used but not when
   GNULIB_NAMESPACE::func is used.  func must be defined without overloaded
   variants.  */
#if defined __cplusplus && defined GNULIB_NAMESPACE
# define _GL_CXXALIASWARN(func) \
   _GL_CXXALIASWARN_1 (func, GNULIB_NAMESPACE)
# define _GL_CXXALIASWARN_1(func,namespace) \
   _GL_CXXALIASWARN_2 (func, namespace)
/* To work around GCC bug <https://gcc.gnu.org/bugzilla/show_bug.cgi?id=43881>,
   we enable the warning only when not optimizing.  */
# if !(defined __GNUC__ && !defined __clang__ && __OPTIMIZE__)
#  define _GL_CXXALIASWARN_2(func,namespace) \
    _GL_WARN_ON_USE (func, \
                     "The symbol ::" #func " refers to the system function. " \
                     "Use " #namespace "::" #func " instead.")
# elif (__GNUC__ >= 3 || defined __clang__) && GNULIB_STRICT_CHECKING
#  define _GL_CXXALIASWARN_2(func,namespace) \
     extern __typeof__ (func) func
# else
#  define _GL_CXXALIASWARN_2(func,namespace) \
     _GL_EXTERN_C int _gl_cxxalias_dummy
# endif
#else
# define _GL_CXXALIASWARN(func) \
    _GL_EXTERN_C int _gl_cxxalias_dummy
#endif

/* _GL_CXXALIASWARN1 (func, rettype, parameters_and_attributes);
   causes a warning to be emitted when the given overloaded variant of ::func
   is used but not when GNULIB_NAMESPACE::func is used.  */
#if defined __cplusplus && defined GNULIB_NAMESPACE
# define _GL_CXXALIASWARN1(func,rettype,parameters_and_attributes) \
   _GL_CXXALIASWARN1_1 (func, rettype, parameters_and_attributes, \
                        GNULIB_NAMESPACE)
# define _GL_CXXALIASWARN1_1(func,rettype,parameters_and_attributes,namespace) \
   _GL_CXXALIASWARN1_2 (func, rettype, parameters_and_attributes, namespace)
/* To work around GCC bug <https://gcc.gnu.org/bugzilla/show_bug.cgi?id=43881>,
   we enable the warning only when not optimizing.  */
# if !(defined __GNUC__ && !defined __clang__ && __OPTIMIZE__)
#  define _GL_CXXALIASWARN1_2(func,rettype,parameters_and_attributes,namespace) \
    _GL_WARN_ON_USE_CXX (func, rettype, rettype, parameters_and_attributes, \
                         "The symbol ::" #func " refers to the system function. " \
                         "Use " #namespace "::" #func " instead.")
# else
#  define _GL_CXXALIASWARN1_2(func,rettype,parameters_and_attributes,namespace) \
     _GL_EXTERN_C int _gl_cxxalias_dummy
# endif
#else
# define _GL_CXXALIASWARN1(func,rettype,parameters_and_attributes) \
    _GL_EXTERN_C int _gl_cxxalias_dummy
#endif

#endif /* _GL_CXXDEFS_H */

/* The definition of _Noreturn is copied here.  */
/* A C macro for declaring that a function does not return.
   Copyright (C) 2011-2025 Free Software Foundation, Inc.

   This program is free software: you can redistribute it and/or modify it
   under the terms of the GNU Lesser General Public License as published
   by the Free Software Foundation; either version 2 of the License, or
   (at your option) any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
   Lesser General Public License for more details.

   You should have received a copy of the GNU Lesser General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

#ifndef _Noreturn
# if (defined __cplusplus \
      && ((201103 <= __cplusplus && !(__GNUC__ == 4 && __GNUC_MINOR__ == 7)) \
          || (defined _MSC_VER && 1900 <= _MSC_VER)) \
      && 0)
    /* [[noreturn]] is not practically usable, because with it the syntax
         extern _Noreturn void func (...);
       would not be valid; such a declaration would only be valid with 'extern'
       and '_Noreturn' swapped, or without the 'extern' keyword.  However, some
       AIX system header files and several gnulib header files use precisely
       this syntax with 'extern'.  */
#  define _Noreturn [[noreturn]]
# elif (defined __clang__ && __clang_major__ < 16 \
        && defined _GL_WORK_AROUND_LLVM_BUG_59792)
   /* Compile with -D_GL_WORK_AROUND_LLVM_BUG_59792 to work around
      that rare LLVM bug, though you may get many false-alarm warnings.  */
#  define _Noreturn
# elif ((!defined __cplusplus || defined __clang__) \
        && (201112 <= (defined __STDC_VERSION__ ? __STDC_VERSION__ : 0) \
            || (!defined __STRICT_ANSI__ \
                && (4 < __GNUC__ + (7 <= __GNUC_MINOR__) && !defined __clang__ \
                    || (defined __apple_build_version__ \
                        ? 6000000 <= __apple_build_version__ \
                        : 3 < __clang_major__ + (5 <= __clang_minor__))))))
   /* _Noreturn works as-is.  */
# elif (2 < __GNUC__ + (8 <= __GNUC_MINOR__) || defined __clang__ \
        || 0x5110 <= __SUNPRO_C)
#  define _Noreturn __attribute__ ((__noreturn__))
# elif 1200 <= (defined _MSC_VER ? _MSC_VER : 0)
#  define _Noreturn __declspec (noreturn)
# else
#  define _Noreturn
# endif
#endif

/* The definition of _GL_ARG_NONNULL is copied here.  */
/* A C macro for declaring that specific arguments must not be NULL.
   Copyright (C) 2009-2025 Free Software Foundation, Inc.

   This program is free software: you can redistribute it and/or modify it
   under the terms of the GNU Lesser General Public License as published
   by the Free Software Foundation; either version 2 of the License, or
   (at your option) any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
   Lesser General Public License for more details.

   You should have received a copy of the GNU Lesser General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

/* _GL_ARG_NONNULL((n,...,m)) tells the compiler and static analyzer tools
   that the values passed as arguments n, ..., m must be non-NULL pointers.
   n = 1 stands for the first argument, n = 2 for the second argument etc.  */
#ifndef _GL_ARG_NONNULL
# if __GNUC__ > 3 || (__GNUC__ == 3 && __GNUC_MINOR__ >= 3) || defined __clang__
#  define _GL_ARG_NONNULL(params) __attribute__ ((__nonnull__ params))
# else
#  define _GL_ARG_NONNULL(params)
# endif
#endif

/* The definition of _GL_WARN_ON_USE is copied here.  */
/* A C macro for emitting warnings if a function is used.
   Copyright (C) 2010-2025 Free Software Foundation, Inc.

   This program is free software: you can redistribute it and/or modify it
   under the terms of the GNU Lesser General Public License as published
   by the Free Software Foundation; either version 2 of the License, or
   (at your option) any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
   Lesser General Public License for more details.

   You should have received a copy of the GNU Lesser General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

/* _GL_WARN_ON_USE (function, "literal string") issues a declaration
   for FUNCTION which will then trigger a compiler warning containing
   the text of "literal string" anywhere that function is called, if
   supported by the compiler.  If the compiler does not support this
   feature, the macro expands to an unused extern declaration.

   _GL_WARN_ON_USE_ATTRIBUTE ("literal string") expands to the
   attribute used in _GL_WARN_ON_USE.  If the compiler does not support
   this feature, it expands to empty.

   These macros are useful for marking a function as a potential
   portability trap, with the intent that "literal string" include
   instructions on the replacement function that should be used
   instead.
   _GL_WARN_ON_USE is for functions with 'extern' linkage.
   _GL_WARN_ON_USE_ATTRIBUTE is for functions with 'static' or 'inline'
   linkage.

   _GL_WARN_ON_USE should not be used more than once for a given function
   in a given compilation unit (because this may generate a warning even
   if the function is never called).

   However, one of the reasons that a function is a portability trap is
   if it has the wrong signature.  Declaring FUNCTION with a different
   signature in C is a compilation error, so this macro must use the
   same type as any existing declaration so that programs that avoid
   the problematic FUNCTION do not fail to compile merely because they
   included a header that poisoned the function.  But this implies that
   _GL_WARN_ON_USE is only safe to use if FUNCTION is known to already
   have a declaration.  Use of this macro implies that there must not
   be any other macro hiding the declaration of FUNCTION; but
   undefining FUNCTION first is part of the poisoning process anyway
   (although for symbols that are provided only via a macro, the result
   is a compilation error rather than a warning containing
   "literal string").  Also note that in C++, it is only safe to use if
   FUNCTION has no overloads.

   For an example, it is possible to poison 'getline' by:
   - adding a call to gl_WARN_ON_USE_PREPARE([[#include <stdio.h>]],
     [getline]) in configure.ac, which potentially defines
     HAVE_RAW_DECL_GETLINE
   - adding this code to a header that wraps the system <stdio.h>:
     #undef getline
     #if HAVE_RAW_DECL_GETLINE
     _GL_WARN_ON_USE (getline, "getline is required by POSIX 2008, but"
       "not universally present; use the gnulib module getline");
     #endif

   It is not possible to directly poison global variables.  But it is
   possible to write a wrapper accessor function, and poison that
   (less common usage, like &environ, will cause a compilation error
   rather than issue the nice warning, but the end result of informing
   the developer about their portability problem is still achieved):
     #if HAVE_RAW_DECL_ENVIRON
     static char ***
     rpl_environ (void) { return &environ; }
     _GL_WARN_ON_USE (rpl_environ, "environ is not always properly declared");
     # undef environ
     # define environ (*rpl_environ ())
     #endif
   or better (avoiding contradictory use of 'static' and 'extern'):
     #if HAVE_RAW_DECL_ENVIRON
     static char ***
     _GL_WARN_ON_USE_ATTRIBUTE ("environ is not always properly declared")
     rpl_environ (void) { return &environ; }
     # undef environ
     # define environ (*rpl_environ ())
     #endif
   */
#ifndef _GL_WARN_ON_USE

# if (4 < __GNUC__ || (__GNUC__ == 4 && 3 <= __GNUC_MINOR__)) && !defined __clang__
/* A compiler attribute is available in gcc versions 4.3.0 and later.  */
#  define _GL_WARN_ON_USE(function, message) \
_GL_WARN_EXTERN_C __typeof__ (function) function __attribute__ ((__warning__ (message)))
#  define _GL_WARN_ON_USE_ATTRIBUTE(message) \
  __attribute__ ((__warning__ (message)))
# elif __clang_major__ >= 4
/* Another compiler attribute is available in clang.  */
#  define _GL_WARN_ON_USE(function, message) \
_GL_WARN_EXTERN_C __typeof__ (function) function \
  __attribute__ ((__diagnose_if__ (1, message, "warning")))
#  define _GL_WARN_ON_USE_ATTRIBUTE(message) \
  __attribute__ ((__diagnose_if__ (1, message, "warning")))
# elif (__GNUC__ >= 3 || defined __clang__) && GNULIB_STRICT_CHECKING
/* Verify the existence of the function.  */
#  define _GL_WARN_ON_USE(function, message) \
_GL_WARN_EXTERN_C __typeof__ (function) function
#  define _GL_WARN_ON_USE_ATTRIBUTE(message)
# else /* Unsupported.  */
#  define _GL_WARN_ON_USE(function, message) \
_GL_WARN_EXTERN_C int _gl_warn_on_use
#  define _GL_WARN_ON_USE_ATTRIBUTE(message)
# endif
#endif

/* _GL_WARN_ON_USE_CXX (function, rettype_gcc, rettype_clang, parameters_and_attributes, "message")
   is like _GL_WARN_ON_USE (function, "message"), except that in C++ mode the
   function is declared with the given prototype, consisting of return type,
   parameters, and attributes.
   This variant is useful for overloaded functions in C++. _GL_WARN_ON_USE does
   not work in this case.  */
#ifndef _GL_WARN_ON_USE_CXX
# if !defined __cplusplus
#  define _GL_WARN_ON_USE_CXX(function,rettype_gcc,rettype_clang,parameters_and_attributes,msg) \
     _GL_WARN_ON_USE (function, msg)
# else
#  if (4 < __GNUC__ || (__GNUC__ == 4 && 3 <= __GNUC_MINOR__)) && !defined __clang__
/* A compiler attribute is available in gcc versions 4.3.0 and later.  */
#   define _GL_WARN_ON_USE_CXX(function,rettype_gcc,rettype_clang,parameters_and_attributes,msg) \
extern rettype_gcc function parameters_and_attributes \
  __attribute__ ((__warning__ (msg)))
#  elif __clang_major__ >= 4
/* Another compiler attribute is available in clang.  */
#   define _GL_WARN_ON_USE_CXX(function,rettype_gcc,rettype_clang,parameters_and_attributes,msg) \
extern rettype_clang function parameters_and_attributes \
  __attribute__ ((__diagnose_if__ (1, msg, "warning")))
#  elif (__GNUC__ >= 3 || defined __clang__) && GNULIB_STRICT_CHECKING
/* Verify the existence of the function.  */
#   define _GL_WARN_ON_USE_CXX(function,rettype_gcc,rettype_clang,parameters_and_attributes,msg) \
extern rettype_gcc function parameters_and_attributes
#  else /* Unsupported.  */
#   define _GL_WARN_ON_USE_CXX(function,rettype_gcc,rettype_clang,parameters_and_attributes,msg) \
_GL_WARN_EXTERN_C int _gl_warn_on_use
#  endif
# endif
#endif

/* _GL_WARN_EXTERN_C declaration;
   performs the declaration with C linkage.  */
#ifndef _GL_WARN_EXTERN_C
# if defined __cplusplus
#  define _GL_WARN_EXTERN_C extern "C"
# else
#  define _GL_WARN_EXTERN_C extern
# endif
#endif

/* =========== Thread types and macros =========== */

#if (defined _WIN32 && ! defined __CYGWIN__) && USE_WINDOWS_THREADS
# if IN_GREP_GNULIB_TESTS
#  include "windows-thread.h"
#  if 1
#   define pthread_t rpl_pthread_t
#   define pthread_attr_t rpl_pthread_attr_t
#  endif
#  if !GNULIB_defined_pthread_thread_types
typedef glwthread_thread_t pthread_t;
typedef unsigned int pthread_attr_t;
#   define GNULIB_defined_pthread_thread_types 1
#  endif
# else
#  if 1
#   define pthread_t rpl_pthread_t
#   define pthread_attr_t rpl_pthread_attr_t
#  endif
#  if !GNULIB_defined_pthread_thread_types
typedef int pthread_t;
typedef unsigned int pthread_attr_t;
#   define GNULIB_defined_pthread_thread_types 1
#  endif
# endif
# undef PTHREAD_CREATE_JOINABLE
# undef PTHREAD_CREATE_DETACHED
# define PTHREAD_CREATE_JOINABLE 0
# define PTHREAD_CREATE_DETACHED 1
#else
# if !1
#  if !GNULIB_defined_pthread_thread_types
typedef int pthread_t;
typedef unsigned int pthread_attr_t;
#   define GNULIB_defined_pthread_thread_types 1
#  endif
# endif
# if !1
#  define PTHREAD_CREATE_JOINABLE 0
#  define PTHREAD_CREATE_DETACHED 1
# endif
#endif

/* =========== Once-only control (initialization) types and macros ========== */

#if (defined _WIN32 && ! defined __CYGWIN__) && USE_WINDOWS_THREADS
# if 1
#  include "windows-once.h"
#  if 1
#   define pthread_once_t rpl_pthread_once_t
#  endif
#  if !GNULIB_defined_pthread_once_types
typedef glwthread_once_t pthread_once_t;
#   define GNULIB_defined_pthread_once_types 1
#  endif
#  undef PTHREAD_ONCE_INIT
#  define PTHREAD_ONCE_INIT GLWTHREAD_ONCE_INIT
# else
#  if 1
#   define pthread_once_t rpl_pthread_once_t
#  endif
#  if !GNULIB_defined_pthread_once_types
typedef int pthread_once_t;
#   define GNULIB_defined_pthread_once_types 1
#  endif
#  undef PTHREAD_ONCE_INIT
#  define PTHREAD_ONCE_INIT { 0 }
# endif
#else
# if !1
#  if !GNULIB_defined_pthread_once_types
typedef int pthread_once_t;
#   define GNULIB_defined_pthread_once_types 1
#  endif
#  undef PTHREAD_ONCE_INIT
#  define PTHREAD_ONCE_INIT { 0 }
# endif
#endif

/* =========== Mutex types and macros =========== */

#if (defined _WIN32 && ! defined __CYGWIN__) && USE_WINDOWS_THREADS
# if IN_GREP_GNULIB_TESTS
#  include "windows-timedmutex.h"
#  include "windows-timedrecmutex.h"
#  if 1
#   define pthread_mutex_t rpl_pthread_mutex_t
#   define pthread_mutexattr_t rpl_pthread_mutexattr_t
#  endif
#  if !GNULIB_defined_pthread_mutex_types
typedef struct
        {
          int type;
          union
            {
              glwthread_timedmutex_t    u_timedmutex;
              glwthread_timedrecmutex_t u_timedrecmutex;
            }
          u;
        }
        pthread_mutex_t;
typedef unsigned int pthread_mutexattr_t;
#   define GNULIB_defined_pthread_mutex_types 1
#  endif
#  undef PTHREAD_MUTEX_INITIALIZER
#  define PTHREAD_MUTEX_INITIALIZER { 1, { GLWTHREAD_TIMEDMUTEX_INIT } }
# else
#  if 1
#   define pthread_mutex_t rpl_pthread_mutex_t
#   define pthread_mutexattr_t rpl_pthread_mutexattr_t
#  endif
#  if !GNULIB_defined_pthread_mutex_types
typedef int pthread_mutex_t;
typedef unsigned int pthread_mutexattr_t;
#   define GNULIB_defined_pthread_mutex_types 1
#  endif
#  undef PTHREAD_MUTEX_INITIALIZER
#  define PTHREAD_MUTEX_INITIALIZER { 0 }
# endif
# undef PTHREAD_MUTEX_DEFAULT
# undef PTHREAD_MUTEX_NORMAL
# undef PTHREAD_MUTEX_ERRORCHECK
# undef PTHREAD_MUTEX_RECURSIVE
# define PTHREAD_MUTEX_DEFAULT PTHREAD_MUTEX_NORMAL
# define PTHREAD_MUTEX_NORMAL 0
# define PTHREAD_MUTEX_ERRORCHECK 1
# define PTHREAD_MUTEX_RECURSIVE 2
# undef PTHREAD_MUTEX_STALLED
# undef PTHREAD_MUTEX_ROBUST
# define PTHREAD_MUTEX_STALLED 0
# define PTHREAD_MUTEX_ROBUST 1
#else
# if !1
#  if !GNULIB_defined_pthread_mutex_types
typedef int pthread_mutex_t;
typedef unsigned int pthread_mutexattr_t;
#   define GNULIB_defined_pthread_mutex_types 1
#  endif
#  undef PTHREAD_MUTEX_INITIALIZER
#  define PTHREAD_MUTEX_INITIALIZER { 0 }
# endif
# if !1
#  define PTHREAD_MUTEX_DEFAULT PTHREAD_MUTEX_NORMAL
#  define PTHREAD_MUTEX_NORMAL 0
#  define PTHREAD_MUTEX_ERRORCHECK 1
#  define PTHREAD_MUTEX_RECURSIVE 2
# endif
# if !1
#  define PTHREAD_MUTEX_STALLED 0
#  define PTHREAD_MUTEX_ROBUST 1
# endif
#endif

/* =========== Read-write lock types and macros =========== */

#if (defined _WIN32 && ! defined __CYGWIN__) && USE_WINDOWS_THREADS
# if IN_GREP_GNULIB_TESTS
#  include "windows-timedrwlock.h"
#  if 1
#   define pthread_rwlock_t rpl_pthread_rwlock_t
#   define pthread_rwlockattr_t rpl_pthread_rwlockattr_t
#  endif
#  if !GNULIB_defined_pthread_rwlock_types
typedef glwthread_timedrwlock_t pthread_rwlock_t;
typedef unsigned int pthread_rwlockattr_t;
#   define GNULIB_defined_pthread_rwlock_types 1
#  endif
#  undef PTHREAD_RWLOCK_INITIALIZER
#  define PTHREAD_RWLOCK_INITIALIZER GLWTHREAD_TIMEDRWLOCK_INIT
# else
#  if 1
#   define pthread_rwlock_t rpl_pthread_rwlock_t
#   define pthread_rwlockattr_t rpl_pthread_rwlockattr_t
#  endif
#  if !GNULIB_defined_pthread_rwlock_types
typedef int pthread_rwlock_t;
typedef unsigned int pthread_rwlockattr_t;
#   define GNULIB_defined_pthread_rwlock_types 1
#  endif
#  undef PTHREAD_RWLOCK_INITIALIZER
#  define PTHREAD_RWLOCK_INITIALIZER { 0 }
# endif
#elif IN_GREP_GNULIB_TESTS && 0 /* i.e. PTHREAD_RWLOCK_UNIMPLEMENTED */
# if 1
#  define pthread_rwlock_t rpl_pthread_rwlock_t
#  define pthread_rwlockattr_t rpl_pthread_rwlockattr_t
# endif
# if !GNULIB_defined_pthread_rwlock_types
typedef struct
        {
          pthread_mutex_t lock; /* protects the remaining fields */
          pthread_cond_t waiting_readers; /* waiting readers */
          pthread_cond_t waiting_writers; /* waiting writers */
          unsigned int waiting_writers_count; /* number of waiting writers */
          int runcount; /* number of readers running, or -1 when a writer runs */
        }
        pthread_rwlock_t;
typedef unsigned int pthread_rwlockattr_t;
#  define GNULIB_defined_pthread_rwlock_types 1
# endif
# undef PTHREAD_RWLOCK_INITIALIZER
# define PTHREAD_RWLOCK_INITIALIZER \
   { PTHREAD_MUTEX_INITIALIZER, PTHREAD_COND_INITIALIZER, PTHREAD_COND_INITIALIZER, 0, 0 }
#elif IN_GREP_GNULIB_TESTS && 0 /* i.e. PTHREAD_RWLOCK_BAD_WAITQUEUE */
/* Use rwlocks of kind PREFER_WRITER or PREFER_WRITER_NONRECURSIVE instead of
   the DEFAULT.  */
# undef PTHREAD_RWLOCK_INITIALIZER
# define PTHREAD_RWLOCK_INITIALIZER PTHREAD_RWLOCK_WRITER_NONRECURSIVE_INITIALIZER_NP
#else
# if 1
#  if !defined PTHREAD_RWLOCK_INITIALIZER && defined PTHREAD_RWLOCK_INITIALIZER_NP /* z/OS */
#   define PTHREAD_RWLOCK_INITIALIZER PTHREAD_RWLOCK_INITIALIZER_NP
#  endif
# else
#  if !GNULIB_defined_pthread_rwlock_types
typedef int pthread_rwlock_t;
typedef unsigned int pthread_rwlockattr_t;
#   define GNULIB_defined_pthread_rwlock_types 1
#  endif
#  undef PTHREAD_RWLOCK_INITIALIZER
#  define PTHREAD_RWLOCK_INITIALIZER { 0 }
# endif
#endif

/* =========== Condition variable types and macros =========== */

#if (defined _WIN32 && ! defined __CYGWIN__) && USE_WINDOWS_THREADS
# if IN_GREP_GNULIB_TESTS
#  include "windows-cond.h"
#  if 1
#   define pthread_cond_t rpl_pthread_cond_t
#   define pthread_condattr_t rpl_pthread_condattr_t
#  endif
#  if !GNULIB_defined_pthread_cond_types
typedef glwthread_cond_t pthread_cond_t;
typedef unsigned int pthread_condattr_t;
#   define GNULIB_defined_pthread_cond_types 1
#  endif
#  undef PTHREAD_COND_INITIALIZER
#  define PTHREAD_COND_INITIALIZER GLWTHREAD_COND_INIT
# else
#  if 1
#   define pthread_cond_t rpl_pthread_cond_t
#   define pthread_condattr_t rpl_pthread_condattr_t
#  endif
#  if !GNULIB_defined_pthread_cond_types
typedef int pthread_cond_t;
typedef unsigned int pthread_condattr_t;
#   define GNULIB_defined_pthread_cond_types 1
#  endif
#  undef PTHREAD_COND_INITIALIZER
#  define PTHREAD_COND_INITIALIZER { 0 }
# endif
#else
# if !1
#  if !GNULIB_defined_pthread_cond_types
typedef int pthread_cond_t;
typedef unsigned int pthread_condattr_t;
#   define GNULIB_defined_pthread_cond_types 1
#  endif
#  undef PTHREAD_COND_INITIALIZER
#  define PTHREAD_COND_INITIALIZER { 0 }
# endif
#endif

/* =========== Thread-specific storage types and macros =========== */

#if (defined _WIN32 && ! defined __CYGWIN__) && USE_WINDOWS_THREADS
# if 0
#  include "windows-tls.h"
#  if 1
#   define pthread_key_t rpl_pthread_key_t
#  endif
#  if !GNULIB_defined_pthread_tss_types
typedef glwthread_tls_key_t pthread_key_t;
#   define GNULIB_defined_pthread_tss_types 1
#  endif
#  undef PTHREAD_DESTRUCTOR_ITERATIONS
#  define PTHREAD_DESTRUCTOR_ITERATIONS GLWTHREAD_DESTRUCTOR_ITERATIONS
# else
#  if 1
#   define pthread_key_t rpl_pthread_key_t
#  endif
#  if !GNULIB_defined_pthread_tss_types
typedef void ** pthread_key_t;
#   define GNULIB_defined_pthread_tss_types 1
#  endif
#  undef PTHREAD_DESTRUCTOR_ITERATIONS
#  define PTHREAD_DESTRUCTOR_ITERATIONS 0
# endif
#else
# if !1
#  if !GNULIB_defined_pthread_tss_types
typedef void ** pthread_key_t;
#   define GNULIB_defined_pthread_tss_types 1
#  endif
#  undef PTHREAD_DESTRUCTOR_ITERATIONS
#  define PTHREAD_DESTRUCTOR_ITERATIONS 0
# endif
#endif

/* =========== Spinlock types and macros =========== */

#if (defined _WIN32 && ! defined __CYGWIN__) && USE_WINDOWS_THREADS
# if 0
#  include "windows-spin.h"
#  if 1
#   define pthread_spinlock_t rpl_pthread_spinlock_t
#  endif
#  if !GNULIB_defined_pthread_spin_types
typedef glwthread_spinlock_t pthread_spinlock_t;
#   define GNULIB_defined_pthread_spin_types 1
#  endif
# else
#  if 1
#   define pthread_spinlock_t rpl_pthread_spinlock_t
#  endif
#  if !GNULIB_defined_pthread_spin_types
typedef pthread_mutex_t pthread_spinlock_t;
#   define GNULIB_defined_pthread_spin_types 1
#  endif
# endif
# undef PTHREAD_PROCESS_PRIVATE
# undef PTHREAD_PROCESS_SHARED
# define PTHREAD_PROCESS_PRIVATE 0
# define PTHREAD_PROCESS_SHARED 1
#else
# if 1
/* <pthread.h> exists and defines pthread_spinlock_t.  */
#  if !1 || 0
/* If the 'pthread-spin' module is in use, it defines all the pthread_spin*
   functions.  Prepare for it by overriding pthread_spinlock_t if that might
   be needed.  */
#   if !(((__GNUC__ > 4 || (__GNUC__ == 4 && __GNUC_MINOR__ >= 7) \
           || __clang_major__ > 3 || (__clang_major__ == 3 && __clang_minor__ >= 1)) \
          || (((__GNUC__ > 4 || (__GNUC__ == 4 && __GNUC_MINOR__ >= 1)) \
               && !defined __ANDROID__) \
              || __clang_major__ >= 3)) \
         && !defined __ibmxl__)
/* We can't use GCC built-ins.  Approximate spinlocks with mutexes.  */
#    if !GNULIB_defined_pthread_spin_types
#     define pthread_spinlock_t pthread_mutex_t
#     define GNULIB_defined_pthread_spin_types 1
#    endif
#   endif
#  endif
# else
/* Approximate spinlocks with mutexes.  */
#  if !GNULIB_defined_pthread_spin_types
typedef pthread_mutex_t pthread_spinlock_t;
#   define GNULIB_defined_pthread_spin_types 1
#  endif
# endif
# if !1
#  define PTHREAD_PROCESS_PRIVATE 0
#  define PTHREAD_PROCESS_SHARED 1
# endif
#endif

/* =========== Other types and macros =========== */

#if !1
# if !GNULIB_defined_other_pthread_types
typedef int pthread_barrier_t;
typedef unsigned int pthread_barrierattr_t;
#  define GNULIB_defined_other_pthread_types 1
# endif
#endif

#if !defined PTHREAD_CANCELED

# define PTHREAD_BARRIER_SERIAL_THREAD (-1)

# define PTHREAD_CANCEL_DEFERRED 0
# define PTHREAD_CANCEL_ASYNCHRONOUS 1

# define PTHREAD_CANCEL_ENABLE 0
# define PTHREAD_CANCEL_DISABLE 1

# define PTHREAD_CANCELED ((void *) -1)

# define PTHREAD_INHERIT_SCHED 0
# define PTHREAD_EXPLICIT_SCHED 1

# define PTHREAD_PRIO_NONE 0
# define PTHREAD_PRIO_INHERIT 1
# define PTHREAD_PRIO_PROTECT 2

# define PTHREAD_SCOPE_SYSTEM 0
# define PTHREAD_SCOPE_PROCESS 1

#endif

/* =========== Thread functions =========== */

#if IN_GREP_GNULIB_TESTS
/* The 'restrict' qualifier on ARG is nonsense, but POSIX specifies it this way.
   Sigh.  */
# if 1
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_create
#   define pthread_create rpl_pthread_create
#  endif
_GL_FUNCDECL_RPL (pthread_create, int,
                  (pthread_t *restrict threadp,
                   const pthread_attr_t *restrict attr,
                   void * (*mainfunc) (void *), void *restrict arg),
                  _GL_ARG_NONNULL ((1, 3)));
_GL_CXXALIAS_RPL (pthread_create, int,
                  (pthread_t *restrict threadp,
                   const pthread_attr_t *restrict attr,
                   void * (*mainfunc) (void *), void *restrict arg));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_create, int,
                  (pthread_t *restrict threadp,
                   const pthread_attr_t *restrict attr,
                   void * (*mainfunc) (void *), void *restrict arg),
                  _GL_ARG_NONNULL ((1, 3)));
#  endif
_GL_CXXALIAS_SYS_CAST (pthread_create, int,
                       (pthread_t *restrict threadp,
                        const pthread_attr_t *restrict attr,
                        void * (*mainfunc) (void *), void *restrict arg));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_create);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_create
# if HAVE_RAW_DECL_PTHREAD_CREATE
_GL_WARN_ON_USE (pthread_create, "pthread_create is not portable - "
                 "use gnulib module pthread-thread for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 1
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_attr_init
#   define pthread_attr_init rpl_pthread_attr_init
#  endif
_GL_FUNCDECL_RPL (pthread_attr_init, int, (pthread_attr_t *attr),
                                          _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_attr_init, int, (pthread_attr_t *attr));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_attr_init, int, (pthread_attr_t *attr),
                                          _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_attr_init, int, (pthread_attr_t *attr));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_attr_init);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_attr_init
# if HAVE_RAW_DECL_PTHREAD_ATTR_INIT
_GL_WARN_ON_USE (pthread_attr_init, "pthread_attr_init is not portable - "
                 "use gnulib module pthread-thread for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_attr_getdetachstate
#   define pthread_attr_getdetachstate rpl_pthread_attr_getdetachstate
#  endif
_GL_FUNCDECL_RPL (pthread_attr_getdetachstate, int,
                  (const pthread_attr_t *attr, int *detachstatep),
                  _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (pthread_attr_getdetachstate, int,
                  (const pthread_attr_t *attr, int *detachstatep));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_attr_getdetachstate, int,
                  (const pthread_attr_t *attr, int *detachstatep),
                  _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (pthread_attr_getdetachstate, int,
                  (const pthread_attr_t *attr, int *detachstatep));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_attr_getdetachstate);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_attr_getdetachstate
# if HAVE_RAW_DECL_PTHREAD_ATTR_GETDETACHSTATE
_GL_WARN_ON_USE (pthread_attr_getdetachstate, "pthread_attr_getdetachstate is not portable - "
                 "use gnulib module pthread-thread for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_attr_setdetachstate
#   define pthread_attr_setdetachstate rpl_pthread_attr_setdetachstate
#  endif
_GL_FUNCDECL_RPL (pthread_attr_setdetachstate, int,
                  (pthread_attr_t *attr, int detachstate),
                  _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_attr_setdetachstate, int,
                  (pthread_attr_t *attr, int detachstate));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_attr_setdetachstate, int,
                  (pthread_attr_t *attr, int detachstate),
                  _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_attr_setdetachstate, int,
                  (pthread_attr_t *attr, int detachstate));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_attr_setdetachstate);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_attr_setdetachstate
# if HAVE_RAW_DECL_PTHREAD_ATTR_SETDETACHSTATE
_GL_WARN_ON_USE (pthread_attr_setdetachstate, "pthread_attr_setdetachstate is not portable - "
                 "use gnulib module pthread-thread for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_attr_destroy
#   define pthread_attr_destroy rpl_pthread_attr_destroy
#  endif
_GL_FUNCDECL_RPL (pthread_attr_destroy, int, (pthread_attr_t *attr),
                                             _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_attr_destroy, int, (pthread_attr_t *attr));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_attr_destroy, int, (pthread_attr_t *attr),
                                             _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_attr_destroy, int, (pthread_attr_t *attr));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_attr_destroy);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_attr_destroy
# if HAVE_RAW_DECL_PTHREAD_ATTR_DESTROY
_GL_WARN_ON_USE (pthread_attr_destroy, "pthread_attr_destroy is not portable - "
                 "use gnulib module pthread-thread for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_self
#   define pthread_self rpl_pthread_self
#  endif
_GL_FUNCDECL_RPL (pthread_self, pthread_t, (void), _GL_ATTRIBUTE_PURE);
_GL_CXXALIAS_RPL (pthread_self, pthread_t, (void));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_self, pthread_t, (void), _GL_ATTRIBUTE_PURE);
#  endif
_GL_CXXALIAS_SYS (pthread_self, pthread_t, (void));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_self);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_self
# if HAVE_RAW_DECL_PTHREAD_SELF
_GL_WARN_ON_USE (pthread_self, "pthread_self is not portable - "
                 "use gnulib module pthread-thread for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_equal
#   define pthread_equal rpl_pthread_equal
#  endif
_GL_FUNCDECL_RPL (pthread_equal, int, (pthread_t thread1, pthread_t thread2), );
_GL_CXXALIAS_RPL (pthread_equal, int, (pthread_t thread1, pthread_t thread2));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_equal, int, (pthread_t thread1, pthread_t thread2), );
#  endif
_GL_CXXALIAS_SYS (pthread_equal, int, (pthread_t thread1, pthread_t thread2));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_equal);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_equal
# if HAVE_RAW_DECL_PTHREAD_EQUAL
_GL_WARN_ON_USE (pthread_equal, "pthread_equal is not portable - "
                 "use gnulib module pthread-thread for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_detach
#   define pthread_detach rpl_pthread_detach
#  endif
_GL_FUNCDECL_RPL (pthread_detach, int, (pthread_t thread), );
_GL_CXXALIAS_RPL (pthread_detach, int, (pthread_t thread));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_detach, int, (pthread_t thread), );
#  endif
_GL_CXXALIAS_SYS (pthread_detach, int, (pthread_t thread));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_detach);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_detach
# if HAVE_RAW_DECL_PTHREAD_DETACH
_GL_WARN_ON_USE (pthread_detach, "pthread_detach is not portable - "
                 "use gnulib module pthread-thread for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_join
#   define pthread_join rpl_pthread_join
#  endif
_GL_FUNCDECL_RPL (pthread_join, int, (pthread_t thread, void **valuep), );
_GL_CXXALIAS_RPL (pthread_join, int, (pthread_t thread, void **valuep));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_join, int, (pthread_t thread, void **valuep), );
#  endif
_GL_CXXALIAS_SYS (pthread_join, int, (pthread_t thread, void **valuep));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_join);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_join
# if HAVE_RAW_DECL_PTHREAD_JOIN
_GL_WARN_ON_USE (pthread_join, "pthread_join is not portable - "
                 "use gnulib module pthread-thread for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_exit
#   define pthread_exit rpl_pthread_exit
#  endif
_GL_FUNCDECL_RPL (pthread_exit, _Noreturn void, (void *value), );
_GL_CXXALIAS_RPL (pthread_exit, void, (void *value));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_exit, _Noreturn void, (void *value), );
#  endif
/* Need to cast because of AIX with xlclang++.  */
_GL_CXXALIAS_SYS_CAST (pthread_exit, void, (void *value));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_exit);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_exit
# if HAVE_RAW_DECL_PTHREAD_EXIT
_GL_WARN_ON_USE (pthread_exit, "pthread_exit is not portable - "
                 "use gnulib module pthread-thread for portability");
# endif
#endif

/* =========== Once-only control (initialization) functions =========== */

#if 1
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_once
#   define pthread_once rpl_pthread_once
#  endif
_GL_FUNCDECL_RPL (pthread_once, int,
                  (pthread_once_t *once_control, void (*initfunction) (void)),
                  _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (pthread_once, int,
                  (pthread_once_t *once_control, void (*initfunction) (void)));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_once, int,
                  (pthread_once_t *once_control, void (*initfunction) (void)),
                   _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS_CAST (pthread_once, int,
                       (pthread_once_t *once_control,
                        void (*initfunction) (void)));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_once);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_once
# if HAVE_RAW_DECL_PTHREAD_ONCE
_GL_WARN_ON_USE (pthread_once, "pthread_once is not portable - "
                 "use gnulib module pthread-once for portability");
# endif
#endif

/* =========== Mutex functions =========== */

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_mutex_init
#   define pthread_mutex_init rpl_pthread_mutex_init
#  endif
_GL_FUNCDECL_RPL (pthread_mutex_init, int,
                  (pthread_mutex_t *restrict mutex,
                   const pthread_mutexattr_t *restrict attr),
                  _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_mutex_init, int,
                  (pthread_mutex_t *restrict mutex,
                   const pthread_mutexattr_t *restrict attr));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_mutex_init, int,
                  (pthread_mutex_t *restrict mutex,
                   const pthread_mutexattr_t *restrict attr),
                  _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_mutex_init, int,
                  (pthread_mutex_t *restrict mutex,
                   const pthread_mutexattr_t *restrict attr));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_mutex_init);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_mutex_init
# if HAVE_RAW_DECL_PTHREAD_MUTEX_INIT
_GL_WARN_ON_USE (pthread_mutex_init, "pthread_mutex_init is not portable - "
                 "use gnulib module pthread-mutex for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_mutexattr_init
#   define pthread_mutexattr_init rpl_pthread_mutexattr_init
#  endif
_GL_FUNCDECL_RPL (pthread_mutexattr_init, int, (pthread_mutexattr_t *attr),
                                               _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_mutexattr_init, int, (pthread_mutexattr_t *attr));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_mutexattr_init, int, (pthread_mutexattr_t *attr),
                                               _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_mutexattr_init, int, (pthread_mutexattr_t *attr));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_mutexattr_init);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_mutexattr_init
# if HAVE_RAW_DECL_PTHREAD_MUTEXATTR_INIT
_GL_WARN_ON_USE (pthread_mutexattr_init, "pthread_mutexattr_init is not portable - "
                 "use gnulib module pthread-mutex for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_mutexattr_gettype
#   define pthread_mutexattr_gettype rpl_pthread_mutexattr_gettype
#  endif
_GL_FUNCDECL_RPL (pthread_mutexattr_gettype, int,
                  (const pthread_mutexattr_t *restrict attr,
                   int *restrict typep),
                  _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (pthread_mutexattr_gettype, int,
                  (const pthread_mutexattr_t *restrict attr,
                   int *restrict typep));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_mutexattr_gettype, int,
                  (const pthread_mutexattr_t *restrict attr,
                   int *restrict typep),
                  _GL_ARG_NONNULL ((1, 2)));
#  endif
/* Need to cast, because on FreeBSD the first parameter is
                        pthread_mutexattr_t *attr.  */
_GL_CXXALIAS_SYS_CAST (pthread_mutexattr_gettype, int,
                       (const pthread_mutexattr_t *restrict attr,
                        int *restrict typep));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_mutexattr_gettype);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_mutexattr_gettype
# if HAVE_RAW_DECL_PTHREAD_MUTEXATTR_GETTYPE
_GL_WARN_ON_USE (pthread_mutexattr_gettype, "pthread_mutexattr_gettype is not portable - "
                 "use gnulib module pthread-mutex for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_mutexattr_settype
#   define pthread_mutexattr_settype rpl_pthread_mutexattr_settype
#  endif
_GL_FUNCDECL_RPL (pthread_mutexattr_settype, int,
                  (pthread_mutexattr_t *attr, int type), _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_mutexattr_settype, int,
                  (pthread_mutexattr_t *attr, int type));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_mutexattr_settype, int,
                  (pthread_mutexattr_t *attr, int type), _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_mutexattr_settype, int,
                  (pthread_mutexattr_t *attr, int type));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_mutexattr_settype);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_mutexattr_settype
# if HAVE_RAW_DECL_PTHREAD_MUTEXATTR_SETTYPE
_GL_WARN_ON_USE (pthread_mutexattr_settype, "pthread_mutexattr_settype is not portable - "
                 "use gnulib module pthread-mutex for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_mutexattr_getrobust
#   define pthread_mutexattr_getrobust rpl_pthread_mutexattr_getrobust
#  endif
_GL_FUNCDECL_RPL (pthread_mutexattr_getrobust, int,
                  (const pthread_mutexattr_t *restrict attr,
                   int *restrict robustp),
                  _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (pthread_mutexattr_getrobust, int,
                  (const pthread_mutexattr_t *restrict attr,
                   int *restrict robustp));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_mutexattr_getrobust, int,
                  (const pthread_mutexattr_t *restrict attr,
                   int *restrict robustp),
                  _GL_ARG_NONNULL ((1, 2)));
#  endif
/* Need to cast, because on FreeBSD the first parameter is
                        pthread_mutexattr_t *attr.  */
_GL_CXXALIAS_SYS_CAST (pthread_mutexattr_getrobust, int,
                       (const pthread_mutexattr_t *restrict attr,
                        int *restrict robustp));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_mutexattr_getrobust);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_mutexattr_getrobust
# if HAVE_RAW_DECL_PTHREAD_MUTEXATTR_GETROBUST
_GL_WARN_ON_USE (pthread_mutexattr_getrobust, "pthread_mutexattr_getrobust is not portable - "
                 "use gnulib module pthread-mutex for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_mutexattr_setrobust
#   define pthread_mutexattr_setrobust rpl_pthread_mutexattr_setrobust
#  endif
_GL_FUNCDECL_RPL (pthread_mutexattr_setrobust, int,
                  (pthread_mutexattr_t *attr, int robust),
                  _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_mutexattr_setrobust, int,
                  (pthread_mutexattr_t *attr, int robust));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_mutexattr_setrobust, int,
                  (pthread_mutexattr_t *attr, int robust),
                  _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_mutexattr_setrobust, int,
                  (pthread_mutexattr_t *attr, int robust));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_mutexattr_setrobust);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_mutexattr_setrobust
# if HAVE_RAW_DECL_PTHREAD_MUTEXATTR_SETROBUST
_GL_WARN_ON_USE (pthread_mutexattr_setrobust, "pthread_mutexattr_setrobust is not portable - "
                 "use gnulib module pthread-mutex for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_mutexattr_destroy
#   define pthread_mutexattr_destroy rpl_pthread_mutexattr_destroy
#  endif
_GL_FUNCDECL_RPL (pthread_mutexattr_destroy, int, (pthread_mutexattr_t *attr),
                                                  _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_mutexattr_destroy, int, (pthread_mutexattr_t *attr));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_mutexattr_destroy, int, (pthread_mutexattr_t *attr),
                                                  _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_mutexattr_destroy, int, (pthread_mutexattr_t *attr));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_mutexattr_destroy);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_mutexattr_destroy
# if HAVE_RAW_DECL_PTHREAD_MUTEXATTR_DESTROY
_GL_WARN_ON_USE (pthread_mutexattr_destroy, "pthread_mutexattr_destroy is not portable - "
                 "use gnulib module pthread-mutex for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_mutex_lock
#   define pthread_mutex_lock rpl_pthread_mutex_lock
#  endif
_GL_FUNCDECL_RPL (pthread_mutex_lock, int, (pthread_mutex_t *mutex),
                                           _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_mutex_lock, int, (pthread_mutex_t *mutex));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_mutex_lock, int, (pthread_mutex_t *mutex),
                                           _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_mutex_lock, int, (pthread_mutex_t *mutex));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_mutex_lock);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_mutex_lock
# if HAVE_RAW_DECL_PTHREAD_MUTEX_LOCK
_GL_WARN_ON_USE (pthread_mutex_lock, "pthread_mutex_lock is not portable - "
                 "use gnulib module pthread-mutex for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_mutex_trylock
#   define pthread_mutex_trylock rpl_pthread_mutex_trylock
#  endif
_GL_FUNCDECL_RPL (pthread_mutex_trylock, int, (pthread_mutex_t *mutex),
                                              _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_mutex_trylock, int, (pthread_mutex_t *mutex));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_mutex_trylock, int, (pthread_mutex_t *mutex),
                                              _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_mutex_trylock, int, (pthread_mutex_t *mutex));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_mutex_trylock);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_mutex_trylock
# if HAVE_RAW_DECL_PTHREAD_MUTEX_TRYLOCK
_GL_WARN_ON_USE (pthread_mutex_trylock, "pthread_mutex_trylock is not portable - "
                 "use gnulib module pthread-mutex for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_mutex_timedlock
#   define pthread_mutex_timedlock rpl_pthread_mutex_timedlock
#  endif
_GL_FUNCDECL_RPL (pthread_mutex_timedlock, int,
                  (pthread_mutex_t *restrict mutex,
                   const struct timespec *restrict abstime),
                  _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (pthread_mutex_timedlock, int,
                  (pthread_mutex_t *restrict mutex,
                   const struct timespec *restrict abstime));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_mutex_timedlock, int,
                  (pthread_mutex_t *restrict mutex,
                   const struct timespec *restrict abstime),
                  _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (pthread_mutex_timedlock, int,
                  (pthread_mutex_t *restrict mutex,
                   const struct timespec *restrict abstime));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_mutex_timedlock);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_mutex_timedlock
# if HAVE_RAW_DECL_PTHREAD_MUTEX_TIMEDLOCK
_GL_WARN_ON_USE (pthread_mutex_timedlock, "pthread_mutex_timedlock is not portable - "
                 "use gnulib module pthread_mutex_timedlock for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_mutex_unlock
#   define pthread_mutex_unlock rpl_pthread_mutex_unlock
#  endif
_GL_FUNCDECL_RPL (pthread_mutex_unlock, int, (pthread_mutex_t *mutex),
                                             _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_mutex_unlock, int, (pthread_mutex_t *mutex));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_mutex_unlock, int, (pthread_mutex_t *mutex),
                                             _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_mutex_unlock, int, (pthread_mutex_t *mutex));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_mutex_unlock);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_mutex_unlock
# if HAVE_RAW_DECL_PTHREAD_MUTEX_UNLOCK
_GL_WARN_ON_USE (pthread_mutex_unlock, "pthread_mutex_unlock is not portable - "
                 "use gnulib module pthread-mutex for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_mutex_destroy
#   define pthread_mutex_destroy rpl_pthread_mutex_destroy
#  endif
_GL_FUNCDECL_RPL (pthread_mutex_destroy, int, (pthread_mutex_t *mutex),
                                              _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_mutex_destroy, int, (pthread_mutex_t *mutex));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_mutex_destroy, int, (pthread_mutex_t *mutex),
                                              _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_mutex_destroy, int, (pthread_mutex_t *mutex));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_mutex_destroy);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_mutex_destroy
# if HAVE_RAW_DECL_PTHREAD_MUTEX_DESTROY
_GL_WARN_ON_USE (pthread_mutex_destroy, "pthread_mutex_destroy is not portable - "
                 "use gnulib module pthread-mutex for portability");
# endif
#endif

/* =========== Read-write lock functions =========== */

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_rwlock_init
#   define pthread_rwlock_init rpl_pthread_rwlock_init
#  endif
_GL_FUNCDECL_RPL (pthread_rwlock_init, int,
                  (pthread_rwlock_t *restrict lock,
                   const pthread_rwlockattr_t *restrict attr),
                  _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_rwlock_init, int,
                  (pthread_rwlock_t *restrict lock,
                   const pthread_rwlockattr_t *restrict attr));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_rwlock_init, int,
                  (pthread_rwlock_t *restrict lock,
                   const pthread_rwlockattr_t *restrict attr),
                  _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_rwlock_init, int,
                  (pthread_rwlock_t *restrict lock,
                   const pthread_rwlockattr_t *restrict attr));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_rwlock_init);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_rwlock_init
# if HAVE_RAW_DECL_PTHREAD_RWLOCK_INIT
_GL_WARN_ON_USE (pthread_rwlock_init, "pthread_rwlock_init is not portable - "
                 "use gnulib module pthread-rwlock for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_rwlockattr_init
#   define pthread_rwlockattr_init rpl_pthread_rwlockattr_init
#  endif
_GL_FUNCDECL_RPL (pthread_rwlockattr_init, int, (pthread_rwlockattr_t *attr),
                                                _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_rwlockattr_init, int, (pthread_rwlockattr_t *attr));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_rwlockattr_init, int, (pthread_rwlockattr_t *attr),
                                                _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_rwlockattr_init, int, (pthread_rwlockattr_t *attr));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_rwlockattr_init);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_rwlockattr_init
# if HAVE_RAW_DECL_PTHREAD_RWLOCKATTR_INIT
_GL_WARN_ON_USE (pthread_rwlockattr_init, "pthread_rwlockattr_init is not portable - "
                 "use gnulib module pthread-rwlock for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_rwlockattr_destroy
#   define pthread_rwlockattr_destroy rpl_pthread_rwlockattr_destroy
#  endif
_GL_FUNCDECL_RPL (pthread_rwlockattr_destroy, int,
                  (pthread_rwlockattr_t *attr), _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_rwlockattr_destroy, int,
                  (pthread_rwlockattr_t *attr));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_rwlockattr_destroy, int,
                  (pthread_rwlockattr_t *attr), _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_rwlockattr_destroy, int,
                  (pthread_rwlockattr_t *attr));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_rwlockattr_destroy);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_rwlockattr_destroy
# if HAVE_RAW_DECL_PTHREAD_RWLOCKATTR_DESTROY
_GL_WARN_ON_USE (pthread_rwlockattr_destroy, "pthread_rwlockattr_destroy is not portable - "
                 "use gnulib module pthread-rwlock for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_rwlock_rdlock
#   define pthread_rwlock_rdlock rpl_pthread_rwlock_rdlock
#  endif
_GL_FUNCDECL_RPL (pthread_rwlock_rdlock, int, (pthread_rwlock_t *lock),
                                              _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_rwlock_rdlock, int, (pthread_rwlock_t *lock));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_rwlock_rdlock, int, (pthread_rwlock_t *lock),
                                              _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_rwlock_rdlock, int, (pthread_rwlock_t *lock));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_rwlock_rdlock);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_rwlock_rdlock
# if HAVE_RAW_DECL_PTHREAD_RWLOCK_RDLOCK
_GL_WARN_ON_USE (pthread_rwlock_rdlock, "pthread_rwlock_rdlock is not portable - "
                 "use gnulib module pthread-rwlock for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_rwlock_wrlock
#   define pthread_rwlock_wrlock rpl_pthread_rwlock_wrlock
#  endif
_GL_FUNCDECL_RPL (pthread_rwlock_wrlock, int, (pthread_rwlock_t *lock),
                                              _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_rwlock_wrlock, int, (pthread_rwlock_t *lock));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_rwlock_wrlock, int, (pthread_rwlock_t *lock),
                                              _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_rwlock_wrlock, int, (pthread_rwlock_t *lock));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_rwlock_wrlock);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_rwlock_wrlock
# if HAVE_RAW_DECL_PTHREAD_RWLOCK_WRLOCK
_GL_WARN_ON_USE (pthread_rwlock_wrlock, "pthread_rwlock_wrlock is not portable - "
                 "use gnulib module pthread-rwlock for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_rwlock_tryrdlock
#   define pthread_rwlock_tryrdlock rpl_pthread_rwlock_tryrdlock
#  endif
_GL_FUNCDECL_RPL (pthread_rwlock_tryrdlock, int, (pthread_rwlock_t *lock),
                                                 _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_rwlock_tryrdlock, int, (pthread_rwlock_t *lock));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_rwlock_tryrdlock, int, (pthread_rwlock_t *lock),
                                                 _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_rwlock_tryrdlock, int, (pthread_rwlock_t *lock));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_rwlock_tryrdlock);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_rwlock_tryrdlock
# if HAVE_RAW_DECL_PTHREAD_RWLOCK_TRYRDLOCK
_GL_WARN_ON_USE (pthread_rwlock_tryrdlock, "pthread_rwlock_tryrdlock is not portable - "
                 "use gnulib module pthread-rwlock for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_rwlock_trywrlock
#   define pthread_rwlock_trywrlock rpl_pthread_rwlock_trywrlock
#  endif
_GL_FUNCDECL_RPL (pthread_rwlock_trywrlock, int, (pthread_rwlock_t *lock),
                                                 _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_rwlock_trywrlock, int, (pthread_rwlock_t *lock));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_rwlock_trywrlock, int, (pthread_rwlock_t *lock),
                                                 _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_rwlock_trywrlock, int, (pthread_rwlock_t *lock));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_rwlock_trywrlock);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_rwlock_trywrlock
# if HAVE_RAW_DECL_PTHREAD_RWLOCK_TRYWRLOCK
_GL_WARN_ON_USE (pthread_rwlock_trywrlock, "pthread_rwlock_trywrlock is not portable - "
                 "use gnulib module pthread-rwlock for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_rwlock_timedrdlock
#   define pthread_rwlock_timedrdlock rpl_pthread_rwlock_timedrdlock
#  endif
_GL_FUNCDECL_RPL (pthread_rwlock_timedrdlock, int,
                  (pthread_rwlock_t *restrict lock,
                   const struct timespec *restrict abstime),
                  _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (pthread_rwlock_timedrdlock, int,
                  (pthread_rwlock_t *restrict lock,
                   const struct timespec *restrict abstime));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_rwlock_timedrdlock, int,
                  (pthread_rwlock_t *restrict lock,
                   const struct timespec *restrict abstime),
                  _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (pthread_rwlock_timedrdlock, int,
                  (pthread_rwlock_t *restrict lock,
                   const struct timespec *restrict abstime));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_rwlock_timedrdlock);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_rwlock_timedrdlock
# if HAVE_RAW_DECL_PTHREAD_RWLOCK_TIMEDRDLOCK
_GL_WARN_ON_USE (pthread_rwlock_timedrdlock, "pthread_rwlock_timedrdlock is not portable - "
                 "use gnulib module pthread-rwlock for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_rwlock_timedwrlock
#   define pthread_rwlock_timedwrlock rpl_pthread_rwlock_timedwrlock
#  endif
_GL_FUNCDECL_RPL (pthread_rwlock_timedwrlock, int,
                  (pthread_rwlock_t *restrict lock,
                   const struct timespec *restrict abstime),
                  _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (pthread_rwlock_timedwrlock, int,
                  (pthread_rwlock_t *restrict lock,
                   const struct timespec *restrict abstime));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_rwlock_timedwrlock, int,
                  (pthread_rwlock_t *restrict lock,
                   const struct timespec *restrict abstime),
                  _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (pthread_rwlock_timedwrlock, int,
                  (pthread_rwlock_t *restrict lock,
                   const struct timespec *restrict abstime));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_rwlock_timedwrlock);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_rwlock_timedwrlock
# if HAVE_RAW_DECL_PTHREAD_RWLOCK_TIMEDWRLOCK
_GL_WARN_ON_USE (pthread_rwlock_timedwrlock, "pthread_rwlock_timedwrlock is not portable - "
                 "use gnulib module pthread-rwlock for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_rwlock_unlock
#   define pthread_rwlock_unlock rpl_pthread_rwlock_unlock
#  endif
_GL_FUNCDECL_RPL (pthread_rwlock_unlock, int, (pthread_rwlock_t *lock),
                                              _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_rwlock_unlock, int, (pthread_rwlock_t *lock));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_rwlock_unlock, int, (pthread_rwlock_t *lock),
                                              _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_rwlock_unlock, int, (pthread_rwlock_t *lock));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_rwlock_unlock);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_rwlock_unlock
# if HAVE_RAW_DECL_PTHREAD_RWLOCK_UNLOCK
_GL_WARN_ON_USE (pthread_rwlock_unlock, "pthread_rwlock_unlock is not portable - "
                 "use gnulib module pthread-rwlock for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_rwlock_destroy
#   define pthread_rwlock_destroy rpl_pthread_rwlock_destroy
#  endif
_GL_FUNCDECL_RPL (pthread_rwlock_destroy, int, (pthread_rwlock_t *lock),
                                               _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_rwlock_destroy, int, (pthread_rwlock_t *lock));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_rwlock_destroy, int, (pthread_rwlock_t *lock),
                                               _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_rwlock_destroy, int, (pthread_rwlock_t *lock));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_rwlock_destroy);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_rwlock_destroy
# if HAVE_RAW_DECL_PTHREAD_RWLOCK_DESTROY
_GL_WARN_ON_USE (pthread_rwlock_destroy, "pthread_rwlock_destroy is not portable - "
                 "use gnulib module pthread-rwlock for portability");
# endif
#endif

/* =========== Condition variable functions =========== */

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_cond_init
#   define pthread_cond_init rpl_pthread_cond_init
#  endif
_GL_FUNCDECL_RPL (pthread_cond_init, int,
                  (pthread_cond_t *restrict cond,
                   const pthread_condattr_t *restrict attr),
                  _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_cond_init, int,
                  (pthread_cond_t *restrict cond,
                   const pthread_condattr_t *restrict attr));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_cond_init, int,
                  (pthread_cond_t *restrict cond,
                   const pthread_condattr_t *restrict attr),
                  _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_cond_init, int,
                  (pthread_cond_t *restrict cond,
                   const pthread_condattr_t *restrict attr));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_cond_init);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_cond_init
# if HAVE_RAW_DECL_PTHREAD_COND_INIT
_GL_WARN_ON_USE (pthread_cond_init, "pthread_cond_init is not portable - "
                 "use gnulib module pthread-cond for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_condattr_init
#   define pthread_condattr_init rpl_pthread_condattr_init
#  endif
_GL_FUNCDECL_RPL (pthread_condattr_init, int, (pthread_condattr_t *attr),
                                              _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_condattr_init, int, (pthread_condattr_t *attr));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_condattr_init, int, (pthread_condattr_t *attr),
                                              _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_condattr_init, int, (pthread_condattr_t *attr));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_condattr_init);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_condattr_init
# if HAVE_RAW_DECL_PTHREAD_CONDATTR_INIT
_GL_WARN_ON_USE (pthread_condattr_init, "pthread_condattr_init is not portable - "
                 "use gnulib module pthread-cond for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_condattr_destroy
#   define pthread_condattr_destroy rpl_pthread_condattr_destroy
#  endif
_GL_FUNCDECL_RPL (pthread_condattr_destroy, int, (pthread_condattr_t *attr),
                                                 _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_condattr_destroy, int, (pthread_condattr_t *attr));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_condattr_destroy, int, (pthread_condattr_t *attr),
                                                 _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_condattr_destroy, int, (pthread_condattr_t *attr));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_condattr_destroy);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_condattr_destroy
# if HAVE_RAW_DECL_PTHREAD_CONDATTR_DESTROY
_GL_WARN_ON_USE (pthread_condattr_destroy, "pthread_condattr_destroy is not portable - "
                 "use gnulib module pthread-cond for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_cond_wait
#   define pthread_cond_wait rpl_pthread_cond_wait
#  endif
_GL_FUNCDECL_RPL (pthread_cond_wait, int,
                  (pthread_cond_t *restrict cond,
                   pthread_mutex_t *restrict mutex),
                  _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (pthread_cond_wait, int,
                  (pthread_cond_t *restrict cond,
                   pthread_mutex_t *restrict mutex));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_cond_wait, int,
                  (pthread_cond_t *restrict cond,
                   pthread_mutex_t *restrict mutex),
                  _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (pthread_cond_wait, int,
                  (pthread_cond_t *restrict cond,
                   pthread_mutex_t *restrict mutex));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_cond_wait);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_cond_wait
# if HAVE_RAW_DECL_PTHREAD_COND_WAIT
_GL_WARN_ON_USE (pthread_cond_wait, "pthread_cond_wait is not portable - "
                 "use gnulib module pthread-cond for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_cond_timedwait
#   define pthread_cond_timedwait rpl_pthread_cond_timedwait
#  endif
_GL_FUNCDECL_RPL (pthread_cond_timedwait, int,
                  (pthread_cond_t *restrict cond,
                   pthread_mutex_t *restrict mutex,
                   const struct timespec *restrict abstime),
                  _GL_ARG_NONNULL ((1, 2, 3)));
_GL_CXXALIAS_RPL (pthread_cond_timedwait, int,
                  (pthread_cond_t *restrict cond,
                   pthread_mutex_t *restrict mutex,
                   const struct timespec *restrict abstime));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_cond_timedwait, int,
                  (pthread_cond_t *restrict cond,
                   pthread_mutex_t *restrict mutex,
                   const struct timespec *restrict abstime),
                  _GL_ARG_NONNULL ((1, 2, 3)));
#  endif
_GL_CXXALIAS_SYS (pthread_cond_timedwait, int,
                  (pthread_cond_t *restrict cond,
                   pthread_mutex_t *restrict mutex,
                   const struct timespec *restrict abstime));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_cond_timedwait);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_cond_timedwait
# if HAVE_RAW_DECL_PTHREAD_COND_TIMEDWAIT
_GL_WARN_ON_USE (pthread_cond_timedwait, "pthread_cond_timedwait is not portable - "
                 "use gnulib module pthread-cond for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_cond_signal
#   define pthread_cond_signal rpl_pthread_cond_signal
#  endif
_GL_FUNCDECL_RPL (pthread_cond_signal, int, (pthread_cond_t *cond),
                                            _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_cond_signal, int, (pthread_cond_t *cond));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_cond_signal, int, (pthread_cond_t *cond),
                                            _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_cond_signal, int, (pthread_cond_t *cond));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_cond_signal);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_cond_signal
# if HAVE_RAW_DECL_PTHREAD_COND_SIGNAL
_GL_WARN_ON_USE (pthread_cond_signal, "pthread_cond_signal is not portable - "
                 "use gnulib module pthread-cond for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_cond_broadcast
#   define pthread_cond_broadcast rpl_pthread_cond_broadcast
#  endif
_GL_FUNCDECL_RPL (pthread_cond_broadcast, int, (pthread_cond_t *cond),
                                               _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_cond_broadcast, int, (pthread_cond_t *cond));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_cond_broadcast, int, (pthread_cond_t *cond),
                                               _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_cond_broadcast, int, (pthread_cond_t *cond));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_cond_broadcast);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_cond_broadcast
# if HAVE_RAW_DECL_PTHREAD_COND_BROADCAST
_GL_WARN_ON_USE (pthread_cond_broadcast, "pthread_cond_broadcast is not portable - "
                 "use gnulib module pthread-cond for portability");
# endif
#endif

#if IN_GREP_GNULIB_TESTS
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_cond_destroy
#   define pthread_cond_destroy rpl_pthread_cond_destroy
#  endif
_GL_FUNCDECL_RPL (pthread_cond_destroy, int, (pthread_cond_t *cond),
                                             _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_cond_destroy, int, (pthread_cond_t *cond));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_cond_destroy, int, (pthread_cond_t *cond),
                                             _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_cond_destroy, int, (pthread_cond_t *cond));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_cond_destroy);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_cond_destroy
# if HAVE_RAW_DECL_PTHREAD_COND_DESTROY
_GL_WARN_ON_USE (pthread_cond_destroy, "pthread_cond_destroy is not portable - "
                 "use gnulib module pthread-cond for portability");
# endif
#endif

/* =========== Thread-specific storage functions =========== */

#if 0
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_key_create
#   define pthread_key_create rpl_pthread_key_create
#  endif
_GL_FUNCDECL_RPL (pthread_key_create, int,
                  (pthread_key_t *keyp, void (*destructor) (void *)),
                  _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_key_create, int,
                  (pthread_key_t *keyp, void (*destructor) (void *)));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_key_create, int,
                  (pthread_key_t *keyp, void (*destructor) (void *)),
                  _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS_CAST (pthread_key_create, int,
                       (pthread_key_t *keyp, void (*destructor) (void *)));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_key_create);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_key_create
# if HAVE_RAW_DECL_PTHREAD_KEY_CREATE
_GL_WARN_ON_USE (pthread_key_create, "pthread_key_create is not portable - "
                 "use gnulib module pthread-tss for portability");
# endif
#endif

#if 0
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_setspecific
#   define pthread_setspecific rpl_pthread_setspecific
#  endif
_GL_FUNCDECL_RPL (pthread_setspecific, int,
                  (pthread_key_t key, const void *value), );
_GL_CXXALIAS_RPL (pthread_setspecific, int,
                  (pthread_key_t key, const void *value));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_setspecific, int,
                  (pthread_key_t key, const void *value), );
#  endif
_GL_CXXALIAS_SYS (pthread_setspecific, int,
                  (pthread_key_t key, const void *value));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_setspecific);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_setspecific
# if HAVE_RAW_DECL_PTHREAD_SETSPECIFIC
_GL_WARN_ON_USE (pthread_setspecific, "pthread_setspecific is not portable - "
                 "use gnulib module pthread-tss for portability");
# endif
#endif

#if 0
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_getspecific
#   define pthread_getspecific rpl_pthread_getspecific
#  endif
_GL_FUNCDECL_RPL (pthread_getspecific, void *, (pthread_key_t key), );
_GL_CXXALIAS_RPL (pthread_getspecific, void *, (pthread_key_t key));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_getspecific, void *, (pthread_key_t key), );
#  endif
_GL_CXXALIAS_SYS (pthread_getspecific, void *, (pthread_key_t key));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_getspecific);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_getspecific
# if HAVE_RAW_DECL_PTHREAD_GETSPECIFIC
_GL_WARN_ON_USE (pthread_getspecific, "pthread_getspecific is not portable - "
                 "use gnulib module pthread-tss for portability");
# endif
#endif

#if 0
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_key_delete
#   define pthread_key_delete rpl_pthread_key_delete
#  endif
_GL_FUNCDECL_RPL (pthread_key_delete, int, (pthread_key_t key), );
_GL_CXXALIAS_RPL (pthread_key_delete, int, (pthread_key_t key));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_key_delete, int, (pthread_key_t key), );
#  endif
_GL_CXXALIAS_SYS (pthread_key_delete, int, (pthread_key_t key));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_key_delete);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_key_delete
# if HAVE_RAW_DECL_PTHREAD_KEY_DELETE
_GL_WARN_ON_USE (pthread_key_delete, "pthread_key_delete is not portable - "
                 "use gnulib module pthread-tss for portability");
# endif
#endif

/* =========== Spinlock functions =========== */

#if 0
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_spin_init
#   define pthread_spin_init rpl_pthread_spin_init
#  endif
_GL_FUNCDECL_RPL (pthread_spin_init, int,
                  (pthread_spinlock_t *lock, int shared_across_processes),
                  _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_spin_init, int,
                  (pthread_spinlock_t *lock, int shared_across_processes));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_spin_init, int,
                  (pthread_spinlock_t *lock, int shared_across_processes),
                  _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_spin_init, int,
                  (pthread_spinlock_t *lock, int shared_across_processes));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_spin_init);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_spin_init
# if HAVE_RAW_DECL_PTHREAD_SPIN_INIT
_GL_WARN_ON_USE (pthread_spin_init, "pthread_spin_init is not portable - "
                 "use gnulib module pthread-spin for portability");
# endif
#endif

#if 0
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_spin_lock
#   define pthread_spin_lock rpl_pthread_spin_lock
#  endif
_GL_FUNCDECL_RPL (pthread_spin_lock, int, (pthread_spinlock_t *lock),
                                          _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_spin_lock, int, (pthread_spinlock_t *lock));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_spin_lock, int, (pthread_spinlock_t *lock),
                                          _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_spin_lock, int, (pthread_spinlock_t *lock));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_spin_lock);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_spin_lock
# if HAVE_RAW_DECL_PTHREAD_SPIN_LOCK
_GL_WARN_ON_USE (pthread_spin_lock, "pthread_spin_lock is not portable - "
                 "use gnulib module pthread-spin for portability");
# endif
#endif

#if 0
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_spin_trylock
#   define pthread_spin_trylock rpl_pthread_spin_trylock
#  endif
_GL_FUNCDECL_RPL (pthread_spin_trylock, int, (pthread_spinlock_t *lock),
                                             _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_spin_trylock, int, (pthread_spinlock_t *lock));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_spin_trylock, int, (pthread_spinlock_t *lock),
                                             _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_spin_trylock, int, (pthread_spinlock_t *lock));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_spin_trylock);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_spin_trylock
# if HAVE_RAW_DECL_PTHREAD_SPIN_TRYLOCK
_GL_WARN_ON_USE (pthread_spin_trylock, "pthread_spin_trylock is not portable - "
                 "use gnulib module pthread-spin for portability");
# endif
#endif

#if 0
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_spin_unlock
#   define pthread_spin_unlock rpl_pthread_spin_unlock
#  endif
_GL_FUNCDECL_RPL (pthread_spin_unlock, int, (pthread_spinlock_t *lock),
                                            _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_spin_unlock, int, (pthread_spinlock_t *lock));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_spin_unlock, int, (pthread_spinlock_t *lock),
                                            _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_spin_unlock, int, (pthread_spinlock_t *lock));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_spin_unlock);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_spin_unlock
# if HAVE_RAW_DECL_PTHREAD_SPIN_UNLOCK
_GL_WARN_ON_USE (pthread_spin_unlock, "pthread_spin_unlock is not portable - "
                 "use gnulib module pthread-spin for portability");
# endif
#endif

#if 0
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   undef pthread_spin_destroy
#   define pthread_spin_destroy rpl_pthread_spin_destroy
#  endif
_GL_FUNCDECL_RPL (pthread_spin_destroy, int, (pthread_spinlock_t *lock),
                                             _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (pthread_spin_destroy, int, (pthread_spinlock_t *lock));
# else
#  if !1
_GL_FUNCDECL_SYS (pthread_spin_destroy, int, (pthread_spinlock_t *lock),
                                             _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (pthread_spin_destroy, int, (pthread_spinlock_t *lock));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (pthread_spin_destroy);
# endif
#elif defined GNULIB_POSIXCHECK
# undef pthread_spin_destroy
# if HAVE_RAW_DECL_PTHREAD_SPIN_DESTROY
_GL_WARN_ON_USE (pthread_spin_destroy, "pthread_spin_destroy is not portable - "
                 "use gnulib module pthread-spin for portability");
# endif
#endif


#if defined __cplusplus && defined GNULIB_NAMESPACE && !1 && defined __MINGW32__
/* Provide the symbols required by mingw's <bits/gthr-default.h>.  */
using GNULIB_NAMESPACE::pthread_create;
using GNULIB_NAMESPACE::pthread_self;
using GNULIB_NAMESPACE::pthread_equal;
using GNULIB_NAMESPACE::pthread_detach;
using GNULIB_NAMESPACE::pthread_join;
using GNULIB_NAMESPACE::pthread_once;
using GNULIB_NAMESPACE::pthread_mutex_init;
using GNULIB_NAMESPACE::pthread_mutexattr_init;
using GNULIB_NAMESPACE::pthread_mutexattr_settype;
using GNULIB_NAMESPACE::pthread_mutexattr_destroy;
using GNULIB_NAMESPACE::pthread_mutex_lock;
using GNULIB_NAMESPACE::pthread_mutex_trylock;
using GNULIB_NAMESPACE::pthread_mutex_timedlock;
using GNULIB_NAMESPACE::pthread_mutex_unlock;
using GNULIB_NAMESPACE::pthread_mutex_destroy;
using GNULIB_NAMESPACE::pthread_cond_wait;
using GNULIB_NAMESPACE::pthread_cond_timedwait;
using GNULIB_NAMESPACE::pthread_cond_signal;
using GNULIB_NAMESPACE::pthread_cond_broadcast;
using GNULIB_NAMESPACE::pthread_cond_destroy;
using GNULIB_NAMESPACE::pthread_key_create;
using GNULIB_NAMESPACE::pthread_setspecific;
using GNULIB_NAMESPACE::pthread_getspecific;
using GNULIB_NAMESPACE::pthread_key_delete;
#endif


#endif /* _GL_PTHREAD_H_ */
#endif /* _GL_PTHREAD_H_ */
#endif
