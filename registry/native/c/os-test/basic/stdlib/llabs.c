/* Test whether a basic llabs invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	long long input = -4611686014132420609LL;
	long long value = llabs(input);
	long long expected = 4611686014132420609LL;
	if ( value != expected )
		err(1, "llabs(%lld) was %lld rather than %lld", input, value, expected);
	return 0;
}
