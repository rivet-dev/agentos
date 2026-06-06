/* Test whether a basic sethostent invocation works. */

#include <errno.h>
#include <netdb.h>

#include "../basic.h"

int main(void)
{
	sethostent(1);
	errno = 0;
	if ( !gethostent() && errno )
		err(1, "gethostent");
	return 0;
}
