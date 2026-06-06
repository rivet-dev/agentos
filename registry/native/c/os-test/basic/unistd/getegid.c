/* Test whether a basic getegid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( getegid() == (gid_t) -1 )
		err(1, "getegid");
	return 0;
}
