/* Test whether a basic strfmon invocation works. */

#include <monetary.h>
#include <stdio.h>

#include "../basic.h"

int main(void)
{
	char output[256];
	// POSIX LC_MONETARY locale has mon_decimal_point = "", positive_sign = "",
	// and negative_sign = "", p_sign_posn = -1, n_sign_posn = -1,
	// int_p_sign_posn = -1, int_n_sign_posn = -1.
	// If neither + nor ( is specified, then p_sign_posn/n_sign_posn is used,
	// but since they're -1, localeconv() would return them as CHAR_MAX, and
	// as a special case, strfmon shall behave as if negative_sign = "-".
	// In other words, negative numbers work correctly, but the radix character
	// (mon_decimal_point) is empty, so decimals won't work properly.
	// Honestly this interface is basically unusable in the POSIX locale, but
	// the standard is very clear on the semantics of the POSIX locale.
	if ( strfmon(output, sizeof(output),
	             "%%foo%ibar%nqux%-^!=f11#3.3i", 901.42, -137.101, 42.010) < 0 )
		err(1, "strfmon");
	const char* expected = "%foo90142bar-13710quxf42010     ";
	if ( strcmp(output, expected) != 0 )
		errx(1, "got \"%s\" instead of \"%s\"", output, expected);
	return 0;
}
