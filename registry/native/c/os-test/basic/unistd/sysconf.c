/* Test whether a basic sysconf invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( sysconf(_SC_PAGE_SIZE) < 0 )
		err(1, "sysconf");
	return 0;
}
