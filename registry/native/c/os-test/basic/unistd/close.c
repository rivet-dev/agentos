/* Test whether a basic close invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( close(0) < 0 )
		err(1, "close");
	return 0;
}
