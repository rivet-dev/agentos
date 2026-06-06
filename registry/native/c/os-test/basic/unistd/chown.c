/* Test whether a basic chown invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( chown(".", (uid_t) -1, (uid_t) -1) < 0 && errno != EPERM )
		err(1, "chown");
	return 0;
}
