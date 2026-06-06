/* Test whether a basic seteuid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( seteuid(geteuid()) < 0 )
		err(1, "seteuid");
	return 0;
}
