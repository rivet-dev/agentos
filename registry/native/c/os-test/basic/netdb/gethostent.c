/* Test whether a basic gethostent invocation works. */

#include <errno.h>
#include <netdb.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	if ( !gethostent() && errno )
		err(1, "gethostent");
	return 0;
}
