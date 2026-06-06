/* Test whether a basic dup invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( dup(2) < 0 )
		err(1, "dup");
	return 0;
}
