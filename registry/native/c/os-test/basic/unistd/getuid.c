/* Test whether a basic getuid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( getuid() == (uid_t) -1 )
		err(1, "getuid");
	return 0;
}
