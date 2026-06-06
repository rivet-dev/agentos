#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// Fix math_errhandling missing on Minix. Use INFINITY to detect math.h.
#if defined(__minix__) && defined(INFINITY)
#ifndef MATH_ERRNO
#define MATH_ERRNO (1 << 0)
#define MATH_ERREXCEPT (1 << 1)
#define math_errhandling MATH_ERREXCEPT
#endif
#endif

// Fix CMPLX macros missing on some systems. Use complex to detect complex.h.
#ifdef complex

// Minix is shipping clang 3.6 which doesn't support __builtin_complex,
// introduced in clang 19.1.0
#if (defined(__GNUC__) && !defined(__clang__)) || (defined(__clang_major__) && 19 < __clang_major__) || defined(__open_xl__) || __has_builtin(__builtin_complex)

#ifndef CMPLXF
#define CMPLXF(x, y) (__builtin_complex((float)(x), (float)(y)))
#endif
#ifndef CMPLX
#define CMPLX(x, y) (__builtin_complex((double)(x), (double)(y)))
#endif
#ifndef CMPLXL
#define CMPLXL(x, y) (__builtin_complex((long double)(x), (long double)(y)))
#endif

#else

#ifndef CMPLXF
union float_complex { float complex c; float parts[2]; };
float complex CMPLXF(float real, float imag)
{
	union float_complex u = { .parts = { real, imag } };
	return u.c;
}
#endif
#ifndef CMPLX
union double_complex { double complex c; double parts[2]; };
double complex CMPLX(double real, double imag)
{
	union double_complex u = { .parts = { real, imag } };
	return u.c;
}
#endif
#ifndef CMPLXL
union long_double_complex { long double complex c; long double parts[2]; };
long double complex CMPLXL(long double real, long double imag)
{
	union long_double_complex u = { .parts = { real, imag } };
	return u.c;
}
#endif

#endif

#endif

#include "../misc/errors.h"
