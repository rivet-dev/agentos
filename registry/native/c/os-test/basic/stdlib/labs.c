/* Test whether a basic labs invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	long input = -2147483647L;
	long value = labs(input);
	long expected = 2147483647L;
	if ( value != expected )
		err(1, "labs(%lld) was %lld rather than %lld", input, value, expected);
	return 0;
}
