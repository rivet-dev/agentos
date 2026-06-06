/* Test whether a basic ldiv invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	long numerator = 524287L;
	long denominator = 1337L;
	ldiv_t result = ldiv(numerator, denominator);
	long expect_quot = 392L;
	long expect_rem = 183L;
	if ( result.quot != expect_quot || result.rem != expect_rem )
		errx(1, "ldiv(%ld, %ld) gave (%ld, %ld) instead of (%ld, %ld)",
		     numerator, denominator, result.quot, result.rem,
		     expect_quot, expect_rem);
	return 0;
}
