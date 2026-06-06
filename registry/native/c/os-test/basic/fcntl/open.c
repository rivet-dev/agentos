/* Test whether a basic open invocation works. */

#include <fcntl.h>

#include "../basic.h"

int main(void)
{
	int fd = open("fcntl/open", O_RDONLY);
	if ( fd < 0 )
		err(1, "open: fcntl/open");
	return 0;
}
