/*[XSI]*/
/* Test whether a basic getrusage invocation works. */

#include <sys/resource.h>

#include "../basic.h"

int main(void)
{
	struct rusage usage;
	if ( getrusage(RUSAGE_SELF, &usage) < 0 )
		err(1, "getrusage: RUSAGE_SELF");
	if ( getrusage(RUSAGE_CHILDREN, &usage) < 0 )
		err(1, "getrusage: RUSAGE_CHILDREN");
	return 0;
}
