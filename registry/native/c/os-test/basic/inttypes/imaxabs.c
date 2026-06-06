/* Test whether a basic imaxabs invocation works. */

#include <inttypes.h>

#include "../basic.h"

int main(void)
{
	intmax_t input = INTMAX_C(-4611686014132420609);
	intmax_t value = imaxabs(input);
	intmax_t expected = INTMAX_C(4611686014132420609);
	if ( value != expected )
		err(1, "imaxabs(%"PRIdMAX") was %"PRIdMAX" rather than %"PRIdMAX"",
		       input, value, expected);
	return 0;
}
