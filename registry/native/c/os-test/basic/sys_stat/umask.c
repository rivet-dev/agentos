/* Test whether a basic umask invocation works. */

#include <sys/stat.h>

#include "../basic.h"

int main(void)
{
	umask(0777);
	if ( umask(0) != 0777 )
		errx(1, "umask did not apply");
	return 0;
}
