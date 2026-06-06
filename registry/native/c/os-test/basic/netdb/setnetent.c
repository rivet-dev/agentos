/* Test whether a basic setnetent invocation works. */

#include <errno.h>
#include <netdb.h>

#include "../basic.h"

int main(void)
{
	setnetent(1);
	errno = 0;
	if ( !getnetent() && errno )
		err(1, "getnetent");
	return 0;
}
