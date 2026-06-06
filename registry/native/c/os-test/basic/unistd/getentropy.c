/* Test whether a basic getentropy invocation works. */

#include <limits.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
#ifdef GETENTROPY_MAX
	char buf[GETENTROPY_MAX];
#else
	char buf[256];
#endif
	if ( getentropy(buf, sizeof(buf)) < 0 )
		err(1, "getentropy");
	return 0;
}
