/* Test whether a basic fpathconf invocation works. */

#include <fcntl.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int dir = open(".", O_RDONLY | O_DIRECTORY);
	if ( dir < 0 )
		err(1, "open: .");
	errno = 0;
	long bits = fpathconf(dir, _PC_FILESIZEBITS);
	if ( bits < 0L && errno )
		err(1, "fpathconf _PC_FILESIZEBITS");
	if ( bits < 32 )
		errx(1, "_PC_FILESIZEBITS < 32");
	return 0;
}
