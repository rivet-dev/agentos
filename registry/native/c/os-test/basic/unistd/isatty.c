/* Test whether a basic isatty invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	if ( !isatty(0) && errno && errno != ENOTTY )
		err(1, "isatty");
	return 0;
}
