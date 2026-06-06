/* Test whether a basic time invocation works. */

#include <stdint.h>
#include <time.h>

#include "../basic.h"

int main(void)
{
	time_t value;
	time_t result = time(&value);
	if ( result == (time_t) -1 )
		err(1, "time");
	if ( result != value )
		err(1, "time returned %jd but saved %jd",
		    (intmax_t) result, (intmax_t) value);
	return 0;
}
