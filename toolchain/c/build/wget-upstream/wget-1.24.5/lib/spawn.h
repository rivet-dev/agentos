/* DO NOT EDIT! GENERATED AUTOMATICALLY! */
/* Definitions for POSIX spawn interface.
   Copyright (C) 2000, 2003-2004, 2008-2024 Free Software Foundation, Inc.
   This file is part of the GNU C Library.

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

#if __GNUC__ >= 3
#pragma GCC system_header
#endif


#if defined _GL_ALREADY_INCLUDING_SPAWN_H
/* Special invocation convention:
   On OS/2 kLIBC, <spawn.h> includes <signal.h>. Then <signal.h> ->
   <pthread.h> -> <sched.h> -> <spawn.h> are included by GNULIB.
   In this situation, struct sched_param is not yet defined.  */

#include_next <spawn.h>

#else

#ifndef _GL_SPAWN_H
/* Normal invocation convention.  */

/* The include_next requires a split double-inclusion guard.  */
#if 1

# define _GL_ALREADY_INCLUDING_SPAWN_H

# include_next <spawn.h>

# define _GL_ALREADY_INCLUDING_SPAWN_H

#endif

#ifndef _GL_SPAWN_H
#define _GL_SPAWN_H

/* This file uses GNULIB_POSIXCHECK, HAVE_RAW_DECL_*.  */
#if !_GL_CONFIG_H_INCLUDED
 #error "Please include config.h first."
#endif

/* Get definitions of 'struct sched_param' and 'sigset_t'.
   But avoid namespace pollution on glibc systems.  */
#if !(defined __GLIBC__ && !defined __UCLIBC__)
# include <sched.h>
# include <signal.h>
#endif

#include <sys/types.h>

#ifndef __THROW
# define __THROW
#endif

/* For plain 'restrict', use glibc's __restrict if defined.
   Otherwise, GCC 2.95 and later have "__restrict"; C99 compilers have
   "restrict", and "configure" may have defined "restrict".
   Other compilers use __restrict, __restrict__, and _Restrict, and
   'configure' might #define 'restrict' to those words, so pick a
   different name.  */
#ifndef _Restrict_
# if defined __restrict \
     || 2 < __GNUC__ + (95 <= __GNUC_MINOR__) \
     || __clang_major__ >= 3
#  define _Restrict_ __restrict
# elif 199901L <= __STDC_VERSION__ || defined restrict
#  define _Restrict_ restrict
# else
#  define _Restrict_
# endif
#endif
/* For the ISO C99 syntax
     array_name[restrict]
   use glibc's __restrict_arr if available.
   Otherwise, GCC 3.1 and clang support this syntax (but not in C++ mode).
   Other ISO C99 compilers support it as well.  */
#ifndef _Restrict_arr_
# ifdef __restrict_arr
#  define _Restrict_arr_ __restrict_arr
# elif ((199901L <= __STDC_VERSION__ \
         || 3 < __GNUC__ + (1 <= __GNUC_MINOR__) \
         || __clang_major__ >= 3) \
        && !defined __cplusplus)
#  define _Restrict_arr_ _Restrict_
# else
#  define _Restrict_arr_
# endif
#endif

/* The definitions of _GL_FUNCDECL_RPL etc. are copied here.  */
/* C++ compatible function declaration macros.
   Copyright (C) 2010-2024 Free Software Foundation, Inc.

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

/* _GL_FUNCDECL_RPL (func, rettype, parameters_and_attributes);
   declares a replacement function, named rpl_func, with the given prototype,
   consisting of return type, parameters, and attributes.
   Example:
     _GL_FUNCDECL_RPL (open, int, (const char *filename, int flags, ...)
                                  _GL_ARG_NONNULL ((1)));

   Note: Attributes, such as _GL_ATTRIBUTE_DEPRECATED, are supported in front
   of a _GL_FUNCDECL_RPL invocation only in C mode, not in C++ mode.  (That's
   because
     [[...]] extern "C" <declaration>;
   is invalid syntax in C++.)
 */
#define _GL_FUNCDECL_RPL(func,rettype,parameters_and_attributes) \
  _GL_FUNCDECL_RPL_1 (rpl_##func, rettype, parameters_and_attributes)
#define _GL_FUNCDECL_RPL_1(rpl_func,rettype,parameters_and_attributes) \
  _GL_EXTERN_C rettype rpl_func parameters_and_attributes

/* _GL_FUNCDECL_SYS (func, rettype, parameters_and_attributes);
   declares the system function, named func, with the given prototype,
   consisting of return type, parameters, and attributes.
   Example:
     _GL_FUNCDECL_SYS (open, int, (const char *filename, int flags, ...)
                                  _GL_ARG_NONNULL ((1)));
 */
#define _GL_FUNCDECL_SYS(func,rettype,parameters_and_attributes) \
  _GL_EXTERN_C rettype func parameters_and_attributes

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
# elif __GNUC__ >= 3 && GNULIB_STRICT_CHECKING
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

/* The definition of _GL_ARG_NONNULL is copied here.  */
/* A C macro for declaring that specific arguments must not be NULL.
   Copyright (C) 2009-2024 Free Software Foundation, Inc.

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
   Copyright (C) 2010-2024 Free Software Foundation, Inc.

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

# if 4 < __GNUC__ || (__GNUC__ == 4 && 3 <= __GNUC_MINOR__)
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
# elif __GNUC__ >= 3 && GNULIB_STRICT_CHECKING
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
#  if 4 < __GNUC__ || (__GNUC__ == 4 && 3 <= __GNUC_MINOR__)
/* A compiler attribute is available in gcc versions 4.3.0 and later.  */
#   define _GL_WARN_ON_USE_CXX(function,rettype_gcc,rettype_clang,parameters_and_attributes,msg) \
extern rettype_gcc function parameters_and_attributes \
  __attribute__ ((__warning__ (msg)))
#  elif __clang_major__ >= 4
/* Another compiler attribute is available in clang.  */
#   define _GL_WARN_ON_USE_CXX(function,rettype_gcc,rettype_clang,parameters_and_attributes,msg) \
extern rettype_clang function parameters_and_attributes \
  __attribute__ ((__diagnose_if__ (1, msg, "warning")))
#  elif __GNUC__ >= 3 && GNULIB_STRICT_CHECKING
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


/* Data structure to contain attributes for thread creation.  */
#if 0 || (1 && !1)
# define posix_spawnattr_t rpl_posix_spawnattr_t
#endif
#if 0 || !1 || !1
# if !GNULIB_defined_posix_spawnattr_t
typedef struct
{
  short int _flags;
  pid_t _pgrp;
  sigset_t _sd;
  sigset_t _ss;
  struct sched_param _sp;
  int _policy;
  int __pad[16];
} posix_spawnattr_t;
#  define GNULIB_defined_posix_spawnattr_t 1
# endif
#endif


/* Data structure to contain information about the actions to be
   performed in the new process with respect to file descriptors.  */
#if 0 || (1 && !1)
# define posix_spawn_file_actions_t rpl_posix_spawn_file_actions_t
#endif
#if 0 || !1 || !1
# if !GNULIB_defined_posix_spawn_file_actions_t
typedef struct
{
  int _allocated;
  int _used;
  struct __spawn_action *_actions;
  int __pad[16];
} posix_spawn_file_actions_t;
#  define GNULIB_defined_posix_spawn_file_actions_t 1
# endif
#endif


/* Flags to be set in the 'posix_spawnattr_t'.  */
#if 1
# if 0
/* Use the values from the system, for better compatibility.  */
/* But this implementation does not support AIX extensions.  */
#   undef POSIX_SPAWN_FORK_HANDLERS
# endif
/* Provide the values that the system is lacking.  */
# ifndef POSIX_SPAWN_SETSCHEDPARAM
#  define POSIX_SPAWN_SETSCHEDPARAM 0
# endif
# ifndef POSIX_SPAWN_SETSCHEDULER
#  define POSIX_SPAWN_SETSCHEDULER 0
# endif
#else /* !1 */
# define POSIX_SPAWN_RESETIDS           0x01
# define POSIX_SPAWN_SETPGROUP          0x02
# define POSIX_SPAWN_SETSIGDEF          0x04
# define POSIX_SPAWN_SETSIGMASK         0x08
# define POSIX_SPAWN_SETSCHEDPARAM      0x10
# define POSIX_SPAWN_SETSCHEDULER       0x20
#endif
/* A GNU extension.  Use the next free bit position.  */
#ifndef POSIX_SPAWN_USEVFORK
# define POSIX_SPAWN_USEVFORK \
  ((POSIX_SPAWN_RESETIDS | (POSIX_SPAWN_RESETIDS - 1)                     \
    | POSIX_SPAWN_SETPGROUP | (POSIX_SPAWN_SETPGROUP - 1)                 \
    | POSIX_SPAWN_SETSIGDEF | (POSIX_SPAWN_SETSIGDEF - 1)                 \
    | POSIX_SPAWN_SETSIGMASK | (POSIX_SPAWN_SETSIGMASK - 1)               \
    | POSIX_SPAWN_SETSCHEDPARAM                                           \
    | (POSIX_SPAWN_SETSCHEDPARAM > 0 ? POSIX_SPAWN_SETSCHEDPARAM - 1 : 0) \
    | POSIX_SPAWN_SETSCHEDULER                                            \
    | (POSIX_SPAWN_SETSCHEDULER > 0 ? POSIX_SPAWN_SETSCHEDULER - 1 : 0))  \
   + 1)
#endif
#if !GNULIB_defined_verify_POSIX_SPAWN_USEVFORK_no_overlap
typedef int verify_POSIX_SPAWN_USEVFORK_no_overlap
            [(((POSIX_SPAWN_RESETIDS | POSIX_SPAWN_SETPGROUP
                | POSIX_SPAWN_SETSIGDEF | POSIX_SPAWN_SETSIGMASK
                | POSIX_SPAWN_SETSCHEDPARAM | POSIX_SPAWN_SETSCHEDULER)
               & POSIX_SPAWN_USEVFORK)
              == 0)
             ? 1 : -1];
# define GNULIB_defined_verify_POSIX_SPAWN_USEVFORK_no_overlap 1
#endif


#if 1
/* Spawn a new process executing PATH with the attributes describes in *ATTRP.
   Before running the process perform the actions described in FILE-ACTIONS.

   This function is a possible cancellation points and therefore not
   marked with __THROW. */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawn rpl_posix_spawn
#  endif
_GL_FUNCDECL_RPL (posix_spawn, int,
                  (pid_t *_Restrict_ __pid,
                   const char *_Restrict_ __path,
                   const posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   const posix_spawnattr_t *_Restrict_ __attrp,
                   char *const argv[_Restrict_arr_],
                   char *const envp[_Restrict_arr_])
                  _GL_ARG_NONNULL ((2, 5, 6)));
_GL_CXXALIAS_RPL (posix_spawn, int,
                  (pid_t *_Restrict_ __pid,
                   const char *_Restrict_ __path,
                   const posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   const posix_spawnattr_t *_Restrict_ __attrp,
                   char *const argv[_Restrict_arr_],
                   char *const envp[_Restrict_arr_]));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawn, int,
                  (pid_t *_Restrict_ __pid,
                   const char *_Restrict_ __path,
                   const posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   const posix_spawnattr_t *_Restrict_ __attrp,
                   char *const argv[_Restrict_arr_],
                   char *const envp[_Restrict_arr_])
                  _GL_ARG_NONNULL ((2, 5, 6)));
#  endif
_GL_CXXALIAS_SYS (posix_spawn, int,
                  (pid_t *_Restrict_ __pid,
                   const char *_Restrict_ __path,
                   const posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   const posix_spawnattr_t *_Restrict_ __attrp,
                   char *const argv[_Restrict_arr_],
                   char *const envp[_Restrict_arr_]));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawn);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawn
# if HAVE_RAW_DECL_POSIX_SPAWN
_GL_WARN_ON_USE (posix_spawn, "posix_spawn is unportable - "
                 "use gnulib module posix_spawn for portability");
# endif
#endif

#if 1
/* Similar to 'posix_spawn' but search for FILE in the PATH.

   This function is a possible cancellation points and therefore not
   marked with __THROW.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnp rpl_posix_spawnp
#  endif
_GL_FUNCDECL_RPL (posix_spawnp, int,
                  (pid_t *__pid, const char *__file,
                   const posix_spawn_file_actions_t *__file_actions,
                   const posix_spawnattr_t *__attrp,
                   char *const argv[], char *const envp[])
                  _GL_ARG_NONNULL ((2, 5, 6)));
_GL_CXXALIAS_RPL (posix_spawnp, int,
                  (pid_t *__pid, const char *__file,
                   const posix_spawn_file_actions_t *__file_actions,
                   const posix_spawnattr_t *__attrp,
                   char *const argv[], char *const envp[]));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawnp, int,
                  (pid_t *__pid, const char *__file,
                   const posix_spawn_file_actions_t *__file_actions,
                   const posix_spawnattr_t *__attrp,
                   char *const argv[], char *const envp[])
                  _GL_ARG_NONNULL ((2, 5, 6)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnp, int,
                  (pid_t *__pid, const char *__file,
                   const posix_spawn_file_actions_t *__file_actions,
                   const posix_spawnattr_t *__attrp,
                   char *const argv[], char *const envp[]));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnp);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnp
# if HAVE_RAW_DECL_POSIX_SPAWNP
_GL_WARN_ON_USE (posix_spawnp, "posix_spawnp is unportable - "
                 "use gnulib module posix_spawnp for portability");
# endif
#endif


#if 1
/* Initialize data structure with attributes for 'spawn' to default values.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnattr_init rpl_posix_spawnattr_init
#  endif
_GL_FUNCDECL_RPL (posix_spawnattr_init, int, (posix_spawnattr_t *__attr)
                                             __THROW _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (posix_spawnattr_init, int, (posix_spawnattr_t *__attr));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawnattr_init, int, (posix_spawnattr_t *__attr)
                                             __THROW _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnattr_init, int, (posix_spawnattr_t *__attr));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnattr_init);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnattr_init
# if HAVE_RAW_DECL_POSIX_SPAWNATTR_INIT
_GL_WARN_ON_USE (posix_spawnattr_init, "posix_spawnattr_init is unportable - "
                 "use gnulib module posix_spawnattr_init for portability");
# endif
#endif

#if 1
/* Free resources associated with ATTR.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnattr_destroy rpl_posix_spawnattr_destroy
#  endif
_GL_FUNCDECL_RPL (posix_spawnattr_destroy, int, (posix_spawnattr_t *__attr)
                                                __THROW _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (posix_spawnattr_destroy, int, (posix_spawnattr_t *__attr));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawnattr_destroy, int, (posix_spawnattr_t *__attr)
                                                __THROW _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnattr_destroy, int, (posix_spawnattr_t *__attr));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnattr_destroy);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnattr_destroy
# if HAVE_RAW_DECL_POSIX_SPAWNATTR_DESTROY
_GL_WARN_ON_USE (posix_spawnattr_destroy,
                 "posix_spawnattr_destroy is unportable - "
                 "use gnulib module posix_spawnattr_destroy for portability");
# endif
#endif

#if 0
/* Store signal mask for signals with default handling from ATTR in
   SIGDEFAULT.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnattr_getsigdefault rpl_posix_spawnattr_getsigdefault
#  endif
_GL_FUNCDECL_RPL (posix_spawnattr_getsigdefault, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   sigset_t *_Restrict_ __sigdefault)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (posix_spawnattr_getsigdefault, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   sigset_t *_Restrict_ __sigdefault));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawnattr_getsigdefault, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   sigset_t *_Restrict_ __sigdefault)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnattr_getsigdefault, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   sigset_t *_Restrict_ __sigdefault));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnattr_getsigdefault);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnattr_getsigdefault
# if HAVE_RAW_DECL_POSIX_SPAWNATTR_GETSIGDEFAULT
_GL_WARN_ON_USE (posix_spawnattr_getsigdefault,
                 "posix_spawnattr_getsigdefault is unportable - "
                 "use gnulib module posix_spawnattr_getsigdefault for portability");
# endif
#endif

#if 0
/* Set signal mask for signals with default handling in ATTR to SIGDEFAULT.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnattr_setsigdefault rpl_posix_spawnattr_setsigdefault
#  endif
_GL_FUNCDECL_RPL (posix_spawnattr_setsigdefault, int,
                  (posix_spawnattr_t *_Restrict_ __attr,
                   const sigset_t *_Restrict_ __sigdefault)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (posix_spawnattr_setsigdefault, int,
                  (posix_spawnattr_t *_Restrict_ __attr,
                   const sigset_t *_Restrict_ __sigdefault));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawnattr_setsigdefault, int,
                  (posix_spawnattr_t *_Restrict_ __attr,
                   const sigset_t *_Restrict_ __sigdefault)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnattr_setsigdefault, int,
                  (posix_spawnattr_t *_Restrict_ __attr,
                   const sigset_t *_Restrict_ __sigdefault));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnattr_setsigdefault);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnattr_setsigdefault
# if HAVE_RAW_DECL_POSIX_SPAWNATTR_SETSIGDEFAULT
_GL_WARN_ON_USE (posix_spawnattr_setsigdefault,
                 "posix_spawnattr_setsigdefault is unportable - "
                 "use gnulib module posix_spawnattr_setsigdefault for portability");
# endif
#endif

#if 0
/* Store signal mask for the new process from ATTR in SIGMASK.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnattr_getsigmask rpl_posix_spawnattr_getsigmask
#  endif
_GL_FUNCDECL_RPL (posix_spawnattr_getsigmask, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   sigset_t *_Restrict_ __sigmask)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (posix_spawnattr_getsigmask, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   sigset_t *_Restrict_ __sigmask));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawnattr_getsigmask, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   sigset_t *_Restrict_ __sigmask)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnattr_getsigmask, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   sigset_t *_Restrict_ __sigmask));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnattr_getsigmask);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnattr_getsigmask
# if HAVE_RAW_DECL_POSIX_SPAWNATTR_GETSIGMASK
_GL_WARN_ON_USE (posix_spawnattr_getsigmask,
                 "posix_spawnattr_getsigmask is unportable - "
                 "use gnulib module posix_spawnattr_getsigmask for portability");
# endif
#endif

#if 1
/* Set signal mask for the new process in ATTR to SIGMASK.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnattr_setsigmask rpl_posix_spawnattr_setsigmask
#  endif
_GL_FUNCDECL_RPL (posix_spawnattr_setsigmask, int,
                  (posix_spawnattr_t *_Restrict_ __attr,
                   const sigset_t *_Restrict_ __sigmask)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (posix_spawnattr_setsigmask, int,
                  (posix_spawnattr_t *_Restrict_ __attr,
                   const sigset_t *_Restrict_ __sigmask));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawnattr_setsigmask, int,
                  (posix_spawnattr_t *_Restrict_ __attr,
                   const sigset_t *_Restrict_ __sigmask)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnattr_setsigmask, int,
                  (posix_spawnattr_t *_Restrict_ __attr,
                   const sigset_t *_Restrict_ __sigmask));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnattr_setsigmask);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnattr_setsigmask
# if HAVE_RAW_DECL_POSIX_SPAWNATTR_SETSIGMASK
_GL_WARN_ON_USE (posix_spawnattr_setsigmask,
                 "posix_spawnattr_setsigmask is unportable - "
                 "use gnulib module posix_spawnattr_setsigmask for portability");
# endif
#endif

#if 0
/* Get flag word from the attribute structure.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnattr_getflags rpl_posix_spawnattr_getflags
#  endif
_GL_FUNCDECL_RPL (posix_spawnattr_getflags, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   short int *_Restrict_ __flags)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (posix_spawnattr_getflags, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   short int *_Restrict_ __flags));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawnattr_getflags, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   short int *_Restrict_ __flags)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnattr_getflags, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   short int *_Restrict_ __flags));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnattr_getflags);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnattr_getflags
# if HAVE_RAW_DECL_POSIX_SPAWNATTR_GETFLAGS
_GL_WARN_ON_USE (posix_spawnattr_getflags,
                 "posix_spawnattr_getflags is unportable - "
                 "use gnulib module posix_spawnattr_getflags for portability");
# endif
#endif

#if 1
/* Store flags in the attribute structure.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnattr_setflags rpl_posix_spawnattr_setflags
#  endif
_GL_FUNCDECL_RPL (posix_spawnattr_setflags, int,
                  (posix_spawnattr_t *__attr, short int __flags)
                  __THROW _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (posix_spawnattr_setflags, int,
                  (posix_spawnattr_t *__attr, short int __flags));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawnattr_setflags, int,
                  (posix_spawnattr_t *__attr, short int __flags)
                  __THROW _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnattr_setflags, int,
                  (posix_spawnattr_t *__attr, short int __flags));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnattr_setflags);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnattr_setflags
# if HAVE_RAW_DECL_POSIX_SPAWNATTR_SETFLAGS
_GL_WARN_ON_USE (posix_spawnattr_setflags,
                 "posix_spawnattr_setflags is unportable - "
                 "use gnulib module posix_spawnattr_setflags for portability");
# endif
#endif

#if 0
/* Get process group ID from the attribute structure.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnattr_getpgroup rpl_posix_spawnattr_getpgroup
#  endif
_GL_FUNCDECL_RPL (posix_spawnattr_getpgroup, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   pid_t *_Restrict_ __pgroup)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (posix_spawnattr_getpgroup, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   pid_t *_Restrict_ __pgroup));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawnattr_getpgroup, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   pid_t *_Restrict_ __pgroup)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnattr_getpgroup, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   pid_t *_Restrict_ __pgroup));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnattr_getpgroup);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnattr_getpgroup
# if HAVE_RAW_DECL_POSIX_SPAWNATTR_GETPGROUP
_GL_WARN_ON_USE (posix_spawnattr_getpgroup,
                 "posix_spawnattr_getpgroup is unportable - "
                 "use gnulib module posix_spawnattr_getpgroup for portability");
# endif
#endif

#if 1
/* Store process group ID in the attribute structure.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnattr_setpgroup rpl_posix_spawnattr_setpgroup
#  endif
_GL_FUNCDECL_RPL (posix_spawnattr_setpgroup, int,
                  (posix_spawnattr_t *__attr, pid_t __pgroup)
                  __THROW _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (posix_spawnattr_setpgroup, int,
                  (posix_spawnattr_t *__attr, pid_t __pgroup));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawnattr_setpgroup, int,
                  (posix_spawnattr_t *__attr, pid_t __pgroup)
                  __THROW _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnattr_setpgroup, int,
                  (posix_spawnattr_t *__attr, pid_t __pgroup));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnattr_setpgroup);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnattr_setpgroup
# if HAVE_RAW_DECL_POSIX_SPAWNATTR_SETPGROUP
_GL_WARN_ON_USE (posix_spawnattr_setpgroup,
                 "posix_spawnattr_setpgroup is unportable - "
                 "use gnulib module posix_spawnattr_setpgroup for portability");
# endif
#endif

#if 0
/* Get scheduling policy from the attribute structure.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnattr_getschedpolicy rpl_posix_spawnattr_getschedpolicy
#  endif
_GL_FUNCDECL_RPL (posix_spawnattr_getschedpolicy, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   int *_Restrict_ __schedpolicy)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (posix_spawnattr_getschedpolicy, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   int *_Restrict_ __schedpolicy));
# else
#  if !1 || POSIX_SPAWN_SETSCHEDULER == 0
_GL_FUNCDECL_SYS (posix_spawnattr_getschedpolicy, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   int *_Restrict_ __schedpolicy)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnattr_getschedpolicy, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   int *_Restrict_ __schedpolicy));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnattr_getschedpolicy);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnattr_getschedpolicy
# if HAVE_RAW_DECL_POSIX_SPAWNATTR_GETSCHEDPOLICY
_GL_WARN_ON_USE (posix_spawnattr_getschedpolicy,
                 "posix_spawnattr_getschedpolicy is unportable - "
                 "use gnulib module posix_spawnattr_getschedpolicy for portability");
# endif
#endif

#if 0
/* Store scheduling policy in the attribute structure.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnattr_setschedpolicy rpl_posix_spawnattr_setschedpolicy
#  endif
_GL_FUNCDECL_RPL (posix_spawnattr_setschedpolicy, int,
                  (posix_spawnattr_t *__attr, int __schedpolicy)
                  __THROW _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (posix_spawnattr_setschedpolicy, int,
                  (posix_spawnattr_t *__attr, int __schedpolicy));
# else
#  if !1 || POSIX_SPAWN_SETSCHEDULER == 0
_GL_FUNCDECL_SYS (posix_spawnattr_setschedpolicy, int,
                  (posix_spawnattr_t *__attr, int __schedpolicy)
                  __THROW _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnattr_setschedpolicy, int,
                  (posix_spawnattr_t *__attr, int __schedpolicy));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnattr_setschedpolicy);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnattr_setschedpolicy
# if HAVE_RAW_DECL_POSIX_SPAWNATTR_SETSCHEDPOLICY
_GL_WARN_ON_USE (posix_spawnattr_setschedpolicy,
                 "posix_spawnattr_setschedpolicy is unportable - "
                 "use gnulib module posix_spawnattr_setschedpolicy for portability");
# endif
#endif

#if 0
/* Get scheduling parameters from the attribute structure.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnattr_getschedparam rpl_posix_spawnattr_getschedparam
#  endif
_GL_FUNCDECL_RPL (posix_spawnattr_getschedparam, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   struct sched_param *_Restrict_ __schedparam)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (posix_spawnattr_getschedparam, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   struct sched_param *_Restrict_ __schedparam));
# else
#  if !1 || POSIX_SPAWN_SETSCHEDPARAM == 0
_GL_FUNCDECL_SYS (posix_spawnattr_getschedparam, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   struct sched_param *_Restrict_ __schedparam)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnattr_getschedparam, int,
                  (const posix_spawnattr_t *_Restrict_ __attr,
                   struct sched_param *_Restrict_ __schedparam));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnattr_getschedparam);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnattr_getschedparam
# if HAVE_RAW_DECL_POSIX_SPAWNATTR_GETSCHEDPARAM
_GL_WARN_ON_USE (posix_spawnattr_getschedparam,
                 "posix_spawnattr_getschedparam is unportable - "
                 "use gnulib module posix_spawnattr_getschedparam for portability");
# endif
#endif

#if 0
/* Store scheduling parameters in the attribute structure.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawnattr_setschedparam rpl_posix_spawnattr_setschedparam
#  endif
_GL_FUNCDECL_RPL (posix_spawnattr_setschedparam, int,
                  (posix_spawnattr_t *_Restrict_ __attr,
                   const struct sched_param *_Restrict_ __schedparam)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (posix_spawnattr_setschedparam, int,
                  (posix_spawnattr_t *_Restrict_ __attr,
                   const struct sched_param *_Restrict_ __schedparam));
# else
#  if !1 || POSIX_SPAWN_SETSCHEDPARAM == 0
_GL_FUNCDECL_SYS (posix_spawnattr_setschedparam, int,
                  (posix_spawnattr_t *_Restrict_ __attr,
                   const struct sched_param *_Restrict_ __schedparam)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (posix_spawnattr_setschedparam, int,
                  (posix_spawnattr_t *_Restrict_ __attr,
                   const struct sched_param *_Restrict_ __schedparam));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawnattr_setschedparam);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawnattr_setschedparam
# if HAVE_RAW_DECL_POSIX_SPAWNATTR_SETSCHEDPARAM
_GL_WARN_ON_USE (posix_spawnattr_setschedparam,
                 "posix_spawnattr_setschedparam is unportable - "
                 "use gnulib module posix_spawnattr_setschedparam for portability");
# endif
#endif


#if 1
/* Initialize data structure for file attribute for 'spawn' call.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawn_file_actions_init rpl_posix_spawn_file_actions_init
#  endif
_GL_FUNCDECL_RPL (posix_spawn_file_actions_init, int,
                  (posix_spawn_file_actions_t *__file_actions)
                  __THROW _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (posix_spawn_file_actions_init, int,
                  (posix_spawn_file_actions_t *__file_actions));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawn_file_actions_init, int,
                  (posix_spawn_file_actions_t *__file_actions)
                  __THROW _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (posix_spawn_file_actions_init, int,
                  (posix_spawn_file_actions_t *__file_actions));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawn_file_actions_init);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawn_file_actions_init
# if HAVE_RAW_DECL_POSIX_SPAWN_FILE_ACTIONS_INIT
_GL_WARN_ON_USE (posix_spawn_file_actions_init,
                 "posix_spawn_file_actions_init is unportable - "
                 "use gnulib module posix_spawn_file_actions_init for portability");
# endif
#endif

#if 1
/* Free resources associated with FILE-ACTIONS.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawn_file_actions_destroy rpl_posix_spawn_file_actions_destroy
#  endif
_GL_FUNCDECL_RPL (posix_spawn_file_actions_destroy, int,
                  (posix_spawn_file_actions_t *__file_actions)
                  __THROW _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (posix_spawn_file_actions_destroy, int,
                  (posix_spawn_file_actions_t *__file_actions));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawn_file_actions_destroy, int,
                  (posix_spawn_file_actions_t *__file_actions)
                  __THROW _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (posix_spawn_file_actions_destroy, int,
                  (posix_spawn_file_actions_t *__file_actions));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawn_file_actions_destroy);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawn_file_actions_destroy
# if HAVE_RAW_DECL_POSIX_SPAWN_FILE_ACTIONS_DESTROY
_GL_WARN_ON_USE (posix_spawn_file_actions_destroy,
                 "posix_spawn_file_actions_destroy is unportable - "
                 "use gnulib module posix_spawn_file_actions_destroy for portability");
# endif
#endif

#if 1
/* Add an action to FILE-ACTIONS which tells the implementation to call
   'open' for the given file during the 'spawn' call.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawn_file_actions_addopen rpl_posix_spawn_file_actions_addopen
#  endif
_GL_FUNCDECL_RPL (posix_spawn_file_actions_addopen, int,
                  (posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   int __fd,
                   const char *_Restrict_ __path, int __oflag, mode_t __mode)
                  __THROW _GL_ARG_NONNULL ((1, 3)));
_GL_CXXALIAS_RPL (posix_spawn_file_actions_addopen, int,
                  (posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   int __fd,
                   const char *_Restrict_ __path, int __oflag, mode_t __mode));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawn_file_actions_addopen, int,
                  (posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   int __fd,
                   const char *_Restrict_ __path, int __oflag, mode_t __mode)
                  __THROW _GL_ARG_NONNULL ((1, 3)));
#  endif
_GL_CXXALIAS_SYS (posix_spawn_file_actions_addopen, int,
                  (posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   int __fd,
                   const char *_Restrict_ __path, int __oflag, mode_t __mode));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawn_file_actions_addopen);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawn_file_actions_addopen
# if HAVE_RAW_DECL_POSIX_SPAWN_FILE_ACTIONS_ADDOPEN
_GL_WARN_ON_USE (posix_spawn_file_actions_addopen,
                 "posix_spawn_file_actions_addopen is unportable - "
                 "use gnulib module posix_spawn_file_actions_addopen for portability");
# endif
#endif

#if 1
/* Add an action to FILE-ACTIONS which tells the implementation to call
   'close' for the given file descriptor during the 'spawn' call.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawn_file_actions_addclose rpl_posix_spawn_file_actions_addclose
#  endif
_GL_FUNCDECL_RPL (posix_spawn_file_actions_addclose, int,
                  (posix_spawn_file_actions_t *__file_actions, int __fd)
                  __THROW _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (posix_spawn_file_actions_addclose, int,
                  (posix_spawn_file_actions_t *__file_actions, int __fd));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawn_file_actions_addclose, int,
                  (posix_spawn_file_actions_t *__file_actions, int __fd)
                  __THROW _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (posix_spawn_file_actions_addclose, int,
                  (posix_spawn_file_actions_t *__file_actions, int __fd));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawn_file_actions_addclose);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawn_file_actions_addclose
# if HAVE_RAW_DECL_POSIX_SPAWN_FILE_ACTIONS_ADDCLOSE
_GL_WARN_ON_USE (posix_spawn_file_actions_addclose,
                 "posix_spawn_file_actions_addclose is unportable - "
                 "use gnulib module posix_spawn_file_actions_addclose for portability");
# endif
#endif

#if 1
/* Add an action to FILE-ACTIONS which tells the implementation to call
   'dup2' for the given file descriptors during the 'spawn' call.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawn_file_actions_adddup2 rpl_posix_spawn_file_actions_adddup2
#  endif
_GL_FUNCDECL_RPL (posix_spawn_file_actions_adddup2, int,
                  (posix_spawn_file_actions_t *__file_actions,
                   int __fd, int __newfd)
                  __THROW _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (posix_spawn_file_actions_adddup2, int,
                  (posix_spawn_file_actions_t *__file_actions,
                   int __fd, int __newfd));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawn_file_actions_adddup2, int,
                  (posix_spawn_file_actions_t *__file_actions,
                   int __fd, int __newfd)
                  __THROW _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (posix_spawn_file_actions_adddup2, int,
                  (posix_spawn_file_actions_t *__file_actions,
                   int __fd, int __newfd));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawn_file_actions_adddup2);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawn_file_actions_adddup2
# if HAVE_RAW_DECL_POSIX_SPAWN_FILE_ACTIONS_ADDDUP2
_GL_WARN_ON_USE (posix_spawn_file_actions_adddup2,
                 "posix_spawn_file_actions_adddup2 is unportable - "
                 "use gnulib module posix_spawn_file_actions_adddup2 for portability");
# endif
#endif

#if 1
/* Add an action to FILE-ACTIONS which tells the implementation to call
   'chdir' to the given directory during the 'spawn' call.  */
# if 1
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawn_file_actions_addchdir rpl_posix_spawn_file_actions_addchdir
#  endif
_GL_FUNCDECL_RPL (posix_spawn_file_actions_addchdir, int,
                  (posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   const char *_Restrict_ __path)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
_GL_CXXALIAS_RPL (posix_spawn_file_actions_addchdir, int,
                  (posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   const char *_Restrict_ __path));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawn_file_actions_addchdir, int,
                  (posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   const char *_Restrict_ __path)
                  __THROW _GL_ARG_NONNULL ((1, 2)));
#  endif
_GL_CXXALIAS_SYS (posix_spawn_file_actions_addchdir, int,
                  (posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   const char *_Restrict_ __path));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawn_file_actions_addchdir);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawn_file_actions_addchdir
# if HAVE_RAW_DECL_POSIX_SPAWN_FILE_ACTIONS_ADDCHDIR
_GL_WARN_ON_USE (posix_spawn_file_actions_addchdir,
                 "posix_spawn_file_actions_addchdir is unportable - "
                 "use gnulib module posix_spawn_file_actions_addchdir for portability");
# endif
#endif

#if 0
/* Add an action to FILE-ACTIONS which tells the implementation to call
   'fchdir' to the given directory during the 'spawn' call.  */
# if 0
#  if !(defined __cplusplus && defined GNULIB_NAMESPACE)
#   define posix_spawn_file_actions_addfchdir rpl_posix_spawn_file_actions_addfchdir
#  endif
_GL_FUNCDECL_RPL (posix_spawn_file_actions_addfchdir, int,
                  (posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   int __fd)
                  __THROW _GL_ARG_NONNULL ((1)));
_GL_CXXALIAS_RPL (posix_spawn_file_actions_addfchdir, int,
                  (posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   int __fd));
# else
#  if !1
_GL_FUNCDECL_SYS (posix_spawn_file_actions_addfchdir, int,
                  (posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   int __fd)
                  __THROW _GL_ARG_NONNULL ((1)));
#  endif
_GL_CXXALIAS_SYS (posix_spawn_file_actions_addfchdir, int,
                  (posix_spawn_file_actions_t *_Restrict_ __file_actions,
                   int __fd));
# endif
# if __GLIBC__ >= 2
_GL_CXXALIASWARN (posix_spawn_file_actions_addfchdir);
# endif
#elif defined GNULIB_POSIXCHECK
# undef posix_spawn_file_actions_addfchdir
# if HAVE_RAW_DECL_POSIX_SPAWN_FILE_ACTIONS_ADDFCHDIR
_GL_WARN_ON_USE (posix_spawn_file_actions_addfchdir,
                 "posix_spawn_file_actions_addfchdir is unportable - "
                 "use gnulib module posix_spawn_file_actions_addfchdir for portability");
# endif
#endif


#endif /* _GL_SPAWN_H */
#endif /* _GL_SPAWN_H */
#endif
