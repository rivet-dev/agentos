/* Test whether a basic abs invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	int input = -42;
	int value = abs(input);
	int expected = 42;
	if ( value != expected )
		err(1, "abs(%d) was %d rather than %d", input, value, expected);
	return 0;
}
