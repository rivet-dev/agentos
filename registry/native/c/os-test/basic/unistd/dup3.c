/* Test whether a basic dup3 invocation works. */

#include <fcntl.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( dup3(2, 42, O_CLOEXEC) < 0 )
		err(1, "dup3");
	return 0;
}
