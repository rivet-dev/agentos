/* Test whether a basic lchown invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( lchown(".", (uid_t) -1, (gid_t) -1) < 0 && errno != EPERM )
		err(1, "lchown");
	return 0;
}
