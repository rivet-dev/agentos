/*[XSI]*/
/* Test whether a basic nice invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	if ( nice(0) < 0 && errno )
		err(1, "nice");
	return 0;
}
