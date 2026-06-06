/*[XSI]*/
/* Test whether a basic ffsl invocation works. */

#include <strings.h>

#include "../basic.h"

int main(void)
{
	long input = 0xF000000;
	int output = ffsl(input);
	int expected = 25;
	if ( output != expected )
		errx(1, "ffsl(%ld) gave %d instead of %d", input, output, expected);
	return 0;
}
