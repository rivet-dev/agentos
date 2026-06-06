/* Test whether a basic getgid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( getgid() == (gid_t) -1 )
		err(1, "getgid");
	return 0;
}
