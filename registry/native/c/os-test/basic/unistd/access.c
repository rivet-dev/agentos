/* Test whether a basic access invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( access(".", F_OK) < 0 )
		err(1, "access");
	return 0;
}
