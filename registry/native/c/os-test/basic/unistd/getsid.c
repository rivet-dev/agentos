/* Test whether a basic getsid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( getsid(0) == (pid_t) -1 )
		err(1, "getsid");
	return 0;
}
