/*[XSI]*/
/* Test whether a basic ffs invocation works. */

#include <strings.h>

#include "../basic.h"

int main(void)
{
	int input = 42;
	int output = ffs(input);
	int expected = 2;
	if ( output != expected )
		errx(1, "ffs(%d) gave %d instead of %d", input, output, expected);
	return 0;
}
