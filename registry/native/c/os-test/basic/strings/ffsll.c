/*[XSI]*/
/* Test whether a basic ffsll invocation works. */

#include <strings.h>

#include "../basic.h"

int main(void)
{
	long input = 0xF0000000000000;
	int output = ffsll(input);
	int expected = 53;
	if ( output != expected )
		errx(1, "ffsll(%lld) gave %d instead of %d", input, output, expected);
	return 0;
}
