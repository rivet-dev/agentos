/* Test whether a basic clock invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{
	clock_t result = clock();
	if ( result == (clock_t) -1 )
		err(1, "clock");
	return 0;
}
