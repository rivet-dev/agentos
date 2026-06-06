/* Test whether a basic setuid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( setuid(getuid()) < 0 )
		err(1, "setuid");
	return 0;
}
