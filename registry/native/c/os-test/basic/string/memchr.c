/* Test whether a basic memchr invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	const char buf[] = "abcdefg";
	if ( memchr(buf, 'e', sizeof(buf)) != buf + 4 )
		errx(1, "memchr did not return 'e'");
	if ( memchr(buf, 'x', sizeof(buf)) )
		errx(1, "memchr found absent character");
	return 0;
}
