/*[OB]*/
/* Test whether a basic ctime invocation works. */

#include <time.h>

#include "../basic.h"

int main(void)
{
	time_t epoch = 0;
	if ( !ctime(&epoch) )
		errx(1, "ctime returned NULL");
	return 0;
}
