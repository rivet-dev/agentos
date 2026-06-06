/* Test whether a basic lldiv invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	long long numerator = 4611686014132420609LL;
	long long denominator = 524287LL;
	lldiv_t result = lldiv(numerator, denominator);
	long long expect_quot = 8796109791263LL;
	long long expect_rem = 516128LL;
	if ( result.quot != expect_quot || result.rem != expect_rem )
		errx(1, "lldiv(%lld, %lld) gave (%lld, %lld) instead of (%lld, %lld)",
		     numerator, denominator, result.quot, result.rem,
		     expect_quot, expect_rem);
	return 0;
}
