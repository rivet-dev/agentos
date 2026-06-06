/* Test whether a basic getprotoent invocation works. */

#include <errno.h>
#include <netdb.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	if ( !getprotoent() && errno )
		err(1, "getprotoent");
	return 0;
}
