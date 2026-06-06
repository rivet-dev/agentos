/* Test whether a basic chdir invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	if ( chdir(".") < 0 )
		err(1, "chdir");
	return 0;
}
