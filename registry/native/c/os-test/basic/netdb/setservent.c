/* Test whether a basic setservent invocation works. */

#include <errno.h>
#include <netdb.h>

#include "../basic.h"

int main(void)
{
	setservent(1);
	errno = 0;
	if ( !getservent() && errno )
		err(1, "getservent");
	return 0;
}
