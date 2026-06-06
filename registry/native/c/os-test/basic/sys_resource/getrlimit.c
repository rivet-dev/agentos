/* Test whether a basic getrlimit invocation works. */

#include <sys/resource.h>

#include "../basic.h"

int main(void)
{
	struct rlimit limit;
	if ( getrlimit(RLIMIT_NOFILE, &limit) < 0 )
		err(1, "getrlimit");
	return 0;
}
