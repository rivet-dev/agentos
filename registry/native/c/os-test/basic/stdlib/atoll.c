/* Test whether a basic atoll invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	long long value = atoll("4611686014132420609");
	if ( value != 4611686014132420609 )
		errx(1, "atol() was %lld, not 4611686014132420609", value);
	return 0;
}
