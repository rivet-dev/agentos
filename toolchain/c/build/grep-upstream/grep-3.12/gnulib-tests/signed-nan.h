/* Macros for quiet not-a-number.
   Copyright (C) 2023-2025 Free Software Foundation, Inc.

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

#ifndef _SIGNED_NAN_H
#define _SIGNED_NAN_H

#include <math.h>

#include "nan.h"

#ifdef __cplusplus
extern "C" {
#endif


/* Returns - x, implemented by inverting the sign bit,
   so that it works also on 'float' NaN values.  */
_GL_UNUSED static float
minus_NaNf (float x)
{
#if defined __mips__
  /* The mips instruction neg.s may have no effect on NaNs.
     Therefore, invert the sign bit using integer operations.  */
  union { unsigned int i; float value; } u;
  u.value = x;
  u.i ^= 1U << 31;
  return u.value;
#else
  return - x;
#endif
}

/* Returns a quiet 'float' NaN with sign bit == 0.  */
_GL_UNUSED static float
positive_NaNf ()
{
  /* 'volatile' works around a GCC bug:
     <https://gcc.gnu.org/bugzilla/show_bug.cgi?id=111655>  */
  float volatile nan = NaNf ();
  return (signbit (nan) ? minus_NaNf (nan) : nan);
}

/* Returns a quiet 'float' NaN with sign bit == 1.  */
_GL_UNUSED static float
negative_NaNf ()
{
  /* 'volatile' works around a GCC bug:
     <https://gcc.gnu.org/bugzilla/show_bug.cgi?id=111655>  */
  float volatile nan = NaNf ();
  return (signbit (nan) ? nan : minus_NaNf (nan));
}


/* Returns - x, implemented by inverting the sign bit,
   so that it works also on 'double' NaN values.  */
_GL_UNUSED static double
minus_NaNd (double x)
{
#if defined __mips__
  /* The mips instruction neg.d may have no effect on NaNs.
     Therefore, invert the sign bit using integer operations.  */
  union { unsigned long long i; double value; } u;
  u.value = x;
  u.i ^= 1ULL << 63;
  return u.value;
#else
  return - x;
#endif
}

/* Returns a quiet 'double' NaN with sign bit == 0.  */
_GL_UNUSED static double
positive_NaNd ()
{
  /* 'volatile' works around a GCC bug:
     <https://gcc.gnu.org/bugzilla/show_bug.cgi?id=111655>  */
  double volatile nan = NaNd ();
  return (signbit (nan) ? minus_NaNd (nan) : nan);
}

/* Returns a quiet 'double' NaN with sign bit == 1.  */
_GL_UNUSED static double
negative_NaNd ()
{
  /* 'volatile' works around a GCC bug:
     <https://gcc.gnu.org/bugzilla/show_bug.cgi?id=111655>  */
  double volatile nan = NaNd ();
  return (signbit (nan) ? nan : minus_NaNd (nan));
}


/* Returns - x, implemented by inverting the sign bit,
   so that it works also on 'long double' NaN values.  */
_GL_UNUSED static long double
minus_NaNl (long double x)
{
  return - x;
}

/* Returns a quiet 'long double' NaN with sign bit == 0.  */
_GL_UNUSED static long double
positive_NaNl ()
{
  /* 'volatile' works around a GCC bug:
     <https://gcc.gnu.org/bugzilla/show_bug.cgi?id=111655>  */
  long double volatile nan = NaNl ();
  return (signbit (nan) ? minus_NaNl (nan) : nan);
}

/* Returns a quiet 'long double' NaN with sign bit == 1.  */
_GL_UNUSED static long double
negative_NaNl ()
{
  /* 'volatile' works around a GCC bug:
     <https://gcc.gnu.org/bugzilla/show_bug.cgi?id=111655>  */
  long double volatile nan = NaNl ();
  return (signbit (nan) ? nan : minus_NaNl (nan));
}


#ifdef __cplusplus
}
#endif

#endif /* _SIGNED_NAN_H */
