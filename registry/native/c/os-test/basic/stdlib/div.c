/* Test whether a basic div invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	int numerator = 9001;
	int denominator = 37;
	div_t result = div(numerator, denominator);
	int expect_quot = 243;
	int expect_rem = 10;
	if ( result.quot != expect_quot || result.rem != expect_rem )
		errx(1, "div(%d, %d) gave (%d, %d) instead of (%d, %d)",
		     numerator, denominator, result.quot, result.rem,
		     expect_quot, expect_rem);
	return 0;
}
