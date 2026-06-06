/* Test whether a basic strchr invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	const char buf[] = "abcdefg";
	if ( strchr(buf, 'e') != buf + 4 )
		errx(1, "strchr did not return 'e'");
	if ( strchr(buf, 'x') )
		errx(1, "strchr found absent character");
	return 0;
}
