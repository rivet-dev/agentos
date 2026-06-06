/* Test whether a basic difftime invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{	
	double diff = difftime(3, 2);
	double expected = 1.0;
	if ( diff != expected )
		errx(1, "difftime gave %f not %f", diff, expected);
	return 0;
}
