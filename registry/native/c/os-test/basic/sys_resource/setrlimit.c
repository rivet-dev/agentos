/* Test whether a basic setrlimit invocation works. */

#include <sys/resource.h>

#include "../basic.h"

int main(void)
{
	struct rlimit limit;
	if ( getrlimit(RLIMIT_NOFILE, &limit) < 0 )
		err(1, "getrlimit");
	if ( setrlimit(RLIMIT_NOFILE, &limit) < 0 )
		err(1, "setrlimit");
	return 0;
}
