/* Test whether a basic imaxdiv invocation works. */

#include <inttypes.h>

#include "../basic.h"

int main(void)
{
	intmax_t numerator = INTMAX_C(4611686014132420609);
	intmax_t denominator = INTMAX_C(524287);
	imaxdiv_t result = imaxdiv(numerator, denominator);
	intmax_t expect_quot = INTMAX_C(8796109791263);
	intmax_t expect_rem = INTMAX_C(516128);
	if ( result.quot != expect_quot || result.rem != expect_rem )
		errx(1, "imaxdiv(%"PRIdMAX", %"PRIdMAX") gave (%"PRIdMAX", %"PRIdMAX") "
		     "instead of (%"PRIdMAX", %"PRIdMAX")",
		     numerator, denominator, result.quot, result.rem,
		     expect_quot, expect_rem);
	return 0;
}
