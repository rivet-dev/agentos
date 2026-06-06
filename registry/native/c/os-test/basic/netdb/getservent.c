/* Test whether a basic getservent invocation works. */

#include <netdb.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	if ( !getservent() && errno )
		err(1, "getservent");
	return 0;
}
