/* Test whether a basic dup2 invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( dup2(2, 42) < 0 )
		err(1, "dup2");
	return 0;
}
