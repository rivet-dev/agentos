/* Test whether a basic setprotoent invocation works. */

#include <errno.h>
#include <netdb.h>

#include "../basic.h"

int main(void)
{
	setprotoent(1);
	errno = 0;
	if ( !getprotoent() && errno )
		err(1, "gethostent");
	return 0;
}
