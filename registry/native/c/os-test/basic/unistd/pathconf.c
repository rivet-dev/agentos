/* Test whether a basic pathconf invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	long bits = pathconf(".", _PC_FILESIZEBITS);
	if ( bits < 0L && errno )
		err(1, "pathconf: .: _PC_FILESIZEBITS");
	if ( bits < 32 )
		errx(1, "_PC_FILESIZEBITS < 32");
	return 0;
}
