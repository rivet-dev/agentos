/* Test of <math.h> substitute.
   Copyright (C) 2007-2025 Free Software Foundation, Inc.

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

/* Written by Bruno Haible <bruno@clisp.org>, 2007.  */

#include <config.h>

#include <math.h>

#ifndef INFINITY
# error INFINITY should be defined, added in ISO C 99
choke me
#endif

#ifndef NAN
# error NAN should be defined, added in ISO C 99
choke me
#endif

#ifndef HUGE_VALF
# error HUGE_VALF should be defined
choke me
#endif

#ifndef HUGE_VAL
# error HUGE_VAL should be defined
choke me
#endif

#ifndef HUGE_VALL
# error HUGE_VALL should be defined
choke me
#endif

#ifndef FP_ILOGB0
# error FP_ILOGB0 should be defined
choke me
#endif

#ifndef FP_ILOGBNAN
# error FP_ILOGBNAN should be defined
choke me
#endif

/* Check that INFINITY expands into a constant expression.  */
float in = INFINITY;

/* Check that NAN expands into a constant expression.  */
float na = NAN;

/* Check that HUGE_VALF expands into a constant expression.  */
float hf = HUGE_VALF;

/* Check that HUGE_VAL expands into a constant expression.  */
double hd = HUGE_VAL;

/* Check that HUGE_VALL expands into a constant expression.  */
long double hl = HUGE_VALL;

#include <limits.h>

#include "macros.h"

/* Compare two numbers with ==.
   This is a separate function because IRIX 6.5 "cc -O" miscompiles an
   'x == x' test.  */
static int
numeric_equalf (float x, float y)
{
  return x == y;
}
static int
numeric_equald (double x, double y)
{
  return x == y;
}
static int
numeric_equall (long double x, long double y)
{
  return x == y;
}

int
main (void)
{
  double d;
  double zero = 0.0;

  /* Check that INFINITY is a float.  */
  ASSERT (sizeof (INFINITY) == sizeof (float));

  /* Check that NAN is a float.  */
  ASSERT (sizeof (NAN) == sizeof (float));

  /* Check the value of NAN.  */
  d = NAN;
  ASSERT (!numeric_equald (d, d));

  /* Check the value of HUGE_VALF.  */
  ASSERT (numeric_equalf (HUGE_VALF, HUGE_VALF + HUGE_VALF));

  /* Check the value of HUGE_VAL.  */
  d = HUGE_VAL;
  ASSERT (numeric_equald (d, 1.0 / zero));
  ASSERT (numeric_equald (HUGE_VAL, HUGE_VAL + HUGE_VAL));

  /* Check the value of HUGE_VALL.  */
  ASSERT (numeric_equall (HUGE_VALL, HUGE_VALL + HUGE_VALL));

  /* Check the value of FP_ILOGB0.  */
  ASSERT (FP_ILOGB0 == INT_MIN || FP_ILOGB0 == - INT_MAX);

  /* Check the value of FP_ILOGBNAN.  */
  ASSERT (FP_ILOGBNAN == INT_MIN || FP_ILOGBNAN == INT_MAX);

  return test_exit_status;
}
