/* Test whether a basic geteuid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( geteuid() == (uid_t) -1 )
		err(1, "geteuid");
	return 0;
}
