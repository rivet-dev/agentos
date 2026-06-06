/* Test whether a basic getnetent invocation works. */

#include <errno.h>
#include <netdb.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	if ( !getnetent() && errno )
		err(1, "getnetent");
	return 0;
}
