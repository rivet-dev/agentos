/* Test whether a basic setgid invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( setgid(getgid()) < 0 )
		err(1, "setgid");
	return 0;
}
